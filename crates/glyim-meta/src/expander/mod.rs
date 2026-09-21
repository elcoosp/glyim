mod matcher;
mod substitution;
mod token_tree;

use crate::BuiltinMacro;
use glyim_core::interner::{Interner, Name};
use glyim_diag::GlyimDiagnostic;
use glyim_span::{
    ByteIdx, ExpnData, ExpnKind, FileId, HygieneCtx, Mark, Span, SyntaxContext, Transparency,
};
use glyim_syntax::{GlyimLang, GreenNode, SyntaxKind, SyntaxNode};
use glyim_vfs::Vfs;
use rowan::Language;
use smol_str::SmolStr;
use std::collections::HashMap;

use glyim_proc_macro::Registry;
use matcher::{MatchResult, Pattern, match_pattern};
use token_tree::{TokenTree, flatten_token_tree};

static RECURSION_LIMIT: std::sync::OnceLock<u32> = std::sync::OnceLock::new();

/// Set the recursion limit for macro expansion.

/// Get the current recursion limit, or the default 128.
fn get_recursion_limit() -> u32 {
    *RECURSION_LIMIT.get().unwrap_or(&128)
}

#[derive(Clone, Debug)]
pub(crate) struct MacroArm {
    pattern: Pattern,
    expansion: Vec<TokenTree>,
}

#[derive(Clone, Debug)]
pub(crate) struct MacroDef {
    pub(crate) name: Name,
    arms: Vec<MacroArm>,
}

pub(crate) fn expand_crate(
    root: &SyntaxNode,
    interner: &mut Interner,
    hygiene: &mut HygieneCtx,
    registered: &[crate::MacroDef],
    current_file: FileId,
    vfs: Option<&Vfs>,
    proc_registry: Option<&Registry>,
) -> (GreenNode, Vec<GlyimDiagnostic>) {
    let mut expander =
        ExpanderImpl::new(hygiene, interner.clone(), current_file, vfs, proc_registry);
    // Register builtins from the public API
    for def in registered {
        if let crate::MacroKind::Builtin { handler, .. } = &def.kind {
            expander.registered_builtins.insert(def.name, *handler);
        }
    }
    expander.collect_macros(root, interner);
    let (green, diags) = expander.expand_node(root, 0);
    (green, diags)
}

pub(crate) fn expand_macro_invocation(
    name: Name,
    args: &SyntaxNode,
    call_site: Span,
    hygiene: &mut HygieneCtx,
    registered: &[crate::MacroDef],
    interner: &Interner,
    current_file: FileId,
    vfs: Option<&Vfs>,
    depth: u32,
    proc_registry: Option<&Registry>,
) -> (Option<GreenNode>, Vec<GlyimDiagnostic>) {
    let mut registered_builtins: HashMap<Name, BuiltinMacro> = HashMap::new();
    for def in registered {
        if let crate::MacroKind::Builtin { handler, .. } = &def.kind {
            registered_builtins.insert(def.name, *handler);
        }
    }

    // Check registered builtins first
    if let Some(handler) = registered_builtins.get(&name).copied() {
        let mut expander = ExpanderImpl {
            hygiene,
            macros: HashMap::new(),
            registered_builtins,
            diagnostics: Vec::new(),
            interner: interner.clone(),
            current_file,
            vfs,
            proc_registry,
        };
        return expander.expand_builtin(handler, name, args, call_site, depth);
    }

    let mut expander = ExpanderImpl {
        hygiene,
        macros: HashMap::new(),
        registered_builtins,
        diagnostics: Vec::new(),
        interner: interner.clone(),
        current_file,
        vfs,
        proc_registry,
    };
    let (green, diags) = expander.expand_macro_call(name, args, call_site, depth);
    expander.diagnostics.extend(diags);
    (green, expander.diagnostics)
}

pub(crate) struct ExpanderImpl<'a> {
    hygiene: &'a mut HygieneCtx,
    macros: HashMap<Name, MacroDef>,
    registered_builtins: HashMap<Name, BuiltinMacro>,
    diagnostics: Vec<GlyimDiagnostic>,
    interner: Interner,
    /// File id of the source currently being expanded (anchors line!/column!/
    /// file!/include! to the real source; defaults to `FileId::BOGUS`).
    current_file: FileId,
    /// Optional VFS for resolving include! paths and computing real line/col.
    vfs: Option<&'a Vfs>,
    /// Optional registry of procedural macros (loaded via `glyim-proc-macro`'s
    /// two-stage compile, or registered in-process for tests). When a
    /// `MacroKind::Proc` invocation is found and the registry contains its
    /// name, the expansion is delegated to the registered function (Phase 9.2).
    proc_registry: Option<&'a Registry>,
}

impl<'a> ExpanderImpl<'a> {
    pub(crate) fn new(
        hygiene: &'a mut HygieneCtx,
        interner: Interner,
        current_file: FileId,
        vfs: Option<&'a Vfs>,
        proc_registry: Option<&'a Registry>,
    ) -> Self {
        let mut registered_builtins: HashMap<Name, BuiltinMacro> = HashMap::new();
        // Default builtin macro set. Only macros the stdlib does not itself
        // define are registered here — the stdlib defines its own
        // `println!`/`print!`/`eprintln!`/`eprint!`/`panic!` as declarative
        // macros (in `io.g` / `panic.g`), and those must resolve through the
        // user-macro path, not be shadowed by builtins.
        for (name, handler) in [
            ("format", BuiltinMacro::Format),
            ("vec", BuiltinMacro::Vec),
            ("matches", BuiltinMacro::Matches),
            ("concat", BuiltinMacro::Concat),
            ("concat_idents", BuiltinMacro::ConcatIdents),
            ("stringify", BuiltinMacro::Stringify),
            ("file", BuiltinMacro::File),
            ("line", BuiltinMacro::Line),
            ("column", BuiltinMacro::Column),
            ("env", BuiltinMacro::Env),
            ("option_env", BuiltinMacro::OptionEnv),
            ("include_str", BuiltinMacro::IncludeStr),
            ("include_bytes", BuiltinMacro::IncludeBytes),
            ("include", BuiltinMacro::Include),
            ("assert", BuiltinMacro::Assert),
            ("assert_eq", BuiltinMacro::Assert),
            ("assert_ne", BuiltinMacro::Assert),
            ("debug_assert", BuiltinMacro::Assert),
            ("debug_assert_eq", BuiltinMacro::Assert),
            ("debug_assert_ne", BuiltinMacro::Assert),
            ("write", BuiltinMacro::Write),
            ("writeln", BuiltinMacro::Write),
        ] {
            registered_builtins.insert(interner.intern(name), handler);
        }
        Self {
            hygiene,
            macros: HashMap::new(),
            registered_builtins,
            diagnostics: Vec::new(),
            interner,
            current_file,
            vfs,
            proc_registry,
        }
    }

    pub(crate) fn collect_macros(&mut self, node: &SyntaxNode, _interner: &mut Interner) {
        for child in node.children() {
            if child.kind() == SyntaxKind::MacroDef {
                if let Some(def) = self.parse_macro_def(&child) {
                    self.macros.insert(def.name, def);
                }
            } else {
                self.collect_macros(&child, _interner);
            }
        }
    }

    fn parse_macro_def(&mut self, node: &SyntaxNode) -> Option<MacroDef> {
        let mut ident_text = None;
        for child in node.children_with_tokens() {
            if child.kind() == SyntaxKind::Ident {
                ident_text = child.into_token().map(|t| t.text().to_string());
                break;
            }
        }
        let name_str = ident_text?;
        let name = self.interner.intern(&name_str);
        let mut arms = Vec::new();
        for arm_node in node.children().filter(|n| n.kind() == SyntaxKind::MacroArm) {
            if let Some(arm) = self.parse_macro_arm(&arm_node) {
                arms.push(arm);
            }
        }
        Some(MacroDef { name, arms })
    }

    fn parse_macro_arm(&self, node: &SyntaxNode) -> Option<MacroArm> {
        let mut children = node.children();
        let pattern_node = children.find(|c| c.kind() == SyntaxKind::TokenTree)?;
        let pattern = self.parse_pattern(&pattern_node)?;
        let expansion_node = children.find(|c| c.kind() == SyntaxKind::TokenTree)?;
        let expansion = self.parse_expansion(&expansion_node);
        Some(MacroArm { pattern, expansion })
    }

    fn parse_pattern(&self, node: &SyntaxNode) -> Option<Pattern> {
        matcher::parse_pattern_from_node(node)
    }

    fn parse_expansion(&self, node: &SyntaxNode) -> Vec<TokenTree> {
        token_tree::collect_token_trees(node)
    }

    pub(crate) fn expand_node(
        &mut self,
        node: &SyntaxNode,
        depth: u32,
    ) -> (GreenNode, Vec<GlyimDiagnostic>) {
        use rowan::GreenNodeBuilder;
        let mut builder = GreenNodeBuilder::new();
        let mut diagnostics = Vec::new();

        self.expand_node_recursive(node, depth, &mut builder, &mut diagnostics);

        let green = builder.finish();
        (green, diagnostics)
    }

    fn expand_node_recursive(
        &mut self,
        node: &SyntaxNode,
        depth: u32,
        builder: &mut rowan::GreenNodeBuilder,
        diagnostics: &mut Vec<GlyimDiagnostic>,
    ) {
        if node.kind() == SyntaxKind::MacroCall {
            let (expanded_green, mut diags) = self.try_expand_macro_call(node, depth);
            diagnostics.append(&mut diags);
            if let Some(green) = expanded_green {
                // Re-parse the expanded token stream in a function body context
                // so that expression/statement tokens are correctly parsed as MacroCalls.
                let temp_root = SyntaxNode::new_root(green.clone());
                let token_text = temp_root.text().to_string();
                // Wrap in a function body to parse in statement context
                let wrapped = format!("fn __glyim_expanded() {{ {} }}", token_text);
                let parse_result = glyim_frontend::parse_to_syntax(&wrapped, FileId::BOGUS);
                let reparsed_root = parse_result.root;
                // Find the function body block and expand its statements
                for child in reparsed_root.children_with_tokens() {
                    match child {
                        rowan::NodeOrToken::Node(n) => {
                            if n.kind() == SyntaxKind::FnDef
                                && let Some(block) =
                                    n.children().find(|c| c.kind() == SyntaxKind::Block)
                            {
                                // Skip the Block's own `{`/`}` delimiter tokens.
                                // `children_with_tokens()` yields them, but they
                                // are NOT part of the expanded expression — the
                                // wrapper `fn __glyim_expanded() { ... }` is
                                // scaffolding we added for the reparse. Emitting
                                // them wraps the expansion in a spurious block:
                                // `format!(..)` in struct-field position became
                                // `Foo { f: { "" } }`, which the field-collector
                                // cannot lower (no expr node inside the field),
                                // silently dropping the field and producing
                                // "missing field" diagnostics. The same leak also
                                // produced stray `{`/`}` in any expansion
                                // returned from a non-statement position.
                                for stmt in block.children_with_tokens().filter(|el| {
                                    !matches!(el.kind(), SyntaxKind::LBrace | SyntaxKind::RBrace)
                                }) {
                                    match stmt {
                                        // Unwrap a single-expression `ExprStmt`
                                        // (no trailing semicolon) into its inner
                                        // expression. The wrapper function
                                        // forced statement context on the
                                        // reparse, so `format!(..)` inside
                                        // `fn __glyim_expanded() {{ .. }}` becomes
                                        // `ExprStmt { <expr> }`. Emitting that
                                        // `ExprStmt` node leaks statement
                                        // structure into expression positions:
                                        // a struct field becomes
                                        // `Field {{ name: ExprStmt { expr } }}`,
                                        // which the field-collector can't
                                        // recognize (no direct expr-node child),
                                        // silently dropping the field. The
                                        // expansion was an expression to begin
                                        // with, so strip the statement wrapper.
                                        rowan::NodeOrToken::Node(s)
                                            if s.kind() == SyntaxKind::ExprStmt =>
                                        {
                                            for inner in s.children_with_tokens() {
                                                match inner {
                                                    rowan::NodeOrToken::Node(m) => {
                                                        self.expand_node_recursive(
                                                            &m,
                                                            depth + 1,
                                                            builder,
                                                            diagnostics,
                                                        );
                                                    }
                                                    rowan::NodeOrToken::Token(t) => {
                                                        let kind = GlyimLang::kind_to_raw(t.kind());
                                                        builder.token(kind, t.text());
                                                    }
                                                }
                                            }
                                        }
                                        rowan::NodeOrToken::Node(s) => {
                                            self.expand_node_recursive(
                                                &s,
                                                depth + 1,
                                                builder,
                                                diagnostics,
                                            );
                                        }
                                        rowan::NodeOrToken::Token(t) => {
                                            let kind = GlyimLang::kind_to_raw(t.kind());
                                            builder.token(kind, t.text());
                                        }
                                    }
                                }
                            }
                        }
                        rowan::NodeOrToken::Token(t) => {
                            let kind = GlyimLang::kind_to_raw(t.kind());
                            builder.token(kind, t.text());
                        }
                    }
                }
                return;
            }
        }

        if node.kind() == SyntaxKind::MacroDef {
            return;
        }

        // Copy other nodes recursively
        builder.start_node(GlyimLang::kind_to_raw(node.kind()));
        for child in node.children_with_tokens() {
            match child {
                rowan::NodeOrToken::Node(n) => {
                    self.expand_node_recursive(&n, depth, builder, diagnostics);
                }
                rowan::NodeOrToken::Token(t) => {
                    let kind = GlyimLang::kind_to_raw(t.kind());
                    builder.token(kind, t.text());
                }
            }
        }
        builder.finish_node();
    }

    /// Find the macro name in a MacroCall node.
    /// The macro name is the Ident token immediately before the `!` token.
    fn find_macro_name(node: &SyntaxNode) -> Option<String> {
        let mut last_ident: Option<String> = None;
        for child in node.children_with_tokens() {
            match &child {
                rowan::NodeOrToken::Token(t) => {
                    if t.kind() == SyntaxKind::Bang {
                        // Found `!` — return the ident we saw just before it
                        return last_ident;
                    }
                    if t.kind() == SyntaxKind::Ident {
                        last_ident = Some(t.text().to_string());
                    } else {
                        last_ident = None;
                    }
                }
                rowan::NodeOrToken::Node(n) => {
                    // Recurse into child nodes, but only use result if we
                    // haven't seen a `!` at this level
                    if let Some(ident) = Self::find_macro_name(n) {
                        return Some(ident);
                    }
                    last_ident = None;
                }
            }
        }
        // If no `!` found, fall back to the first ident we saw
        last_ident
    }

    fn try_expand_macro_call(
        &mut self,
        node: &SyntaxNode,
        depth: u32,
    ) -> (Option<GreenNode>, Vec<GlyimDiagnostic>) {
        if depth > get_recursion_limit() {
            return (
                None,
                vec![GlyimDiagnostic::type_error(
                    Span::DUMMY,
                    "macro recursion limit exceeded",
                )],
            );
        }

        // Find the macro name (the ident before the ! token)
        let ident_text = Self::find_macro_name(node);
        let name_token_text = match ident_text {
            Some(t) => t,
            None => return (None, Vec::new()),
        };

        let name = self.interner.intern(&name_token_text);
        let args_node = match node.children().find(|c| c.kind() == SyntaxKind::TokenTree) {
            Some(n) => n,
            None => return (None, Vec::new()),
        };

        let call_site = self.span_from_node(node);

        // Check registered builtins first
        if let Some(handler) = self.registered_builtins.get(&name).copied() {
            return self.expand_builtin(handler, name, &args_node, call_site, depth);
        }

        self.expand_macro_call(name, &args_node, call_site, depth)
    }

    fn expand_macro_call(
        &mut self,
        name: Name,
        args_node: &SyntaxNode,
        call_site: Span,
        depth: u32,
    ) -> (Option<GreenNode>, Vec<GlyimDiagnostic>) {
        // Phase 9.2: procedural macro dispatch. Proc macros are dispatched via
        // the registry and are NOT present in `self.macros` (which only holds
        // declarative macro arms), so this check runs before the declarative
        // lookup. The registry is populated by the two-stage proc-macro build
        // (`glyim_proc_macro::load_cdylib`) or registered in-process for tests.
        if let Some(reg) = self.proc_registry {
            let name_str = self.interner.resolve(name);
            if reg.contains(name_str) {
                let input: Vec<(SyntaxKind, String)> = flatten_token_tree(args_node)
                    .iter()
                    .map(|t| (t.kind().unwrap_or(SyntaxKind::Error), t.text().to_string()))
                    .collect();
                if let Some(output) = reg.expand(name_str, &input) {
                    let trees: Vec<TokenTree> = output
                        .into_iter()
                        .map(|(k, txt)| TokenTree::Token(k, SmolStr::from(txt)))
                        .collect();
                    let green = self.build_expansion_green(&trees, call_site, depth, name, true);
                    return (Some(green), Vec::new());
                }
            }
        }

        let def = match self.macros.get(&name) {
            Some(d) => d.clone(),
            None => return (None, Vec::new()),
        };

        let args = flatten_token_tree(args_node);
        let name_str = self.interner.resolve(name);

        for arm in &def.arms {
            let result = match_pattern(&arm.pattern, &args);
            match result {
                MatchResult::FullMatch(bindings) => {
                    match substitution::substitute(&arm.expansion, &bindings) {
                        Ok(expanded) => {
                            let expanded_green = self
                                .build_expansion_green(&expanded, call_site, depth, name, false);
                            return (Some(expanded_green), Vec::new());
                        }
                        Err(unbound) => {
                            return (
                                None,
                                vec![GlyimDiagnostic::macro_error(
                                    call_site,
                                    format!(
                                        "unbound metavariable `${}` in macro '{}' expansion; \
                                         it is not captured by any matcher fragment",
                                        unbound, name_str
                                    ),
                                )],
                            );
                        }
                    }
                }
                MatchResult::PartialMatch => continue,
                MatchResult::NoMatch => continue,
            }
        }

        (
            None,
            vec![GlyimDiagnostic::type_error(
                call_site,
                format!("no matching macro arm for macro '{}'", name_str),
            )],
        )
    }

    /// Expand a builtin macro.
    fn expand_builtin(
        &mut self,
        handler: BuiltinMacro,
        name: Name,
        args_node: &SyntaxNode,
        call_site: Span,
        _depth: u32,
    ) -> (Option<GreenNode>, Vec<GlyimDiagnostic>) {
        use std::fs;
        use std::path::{Path, PathBuf};
        let expanded_trees = match handler {
            BuiltinMacro::File => {
                // file!() expands to the path of the source file (relative to
                // the VFS path when available, else the file id).
                let name = if let Some(vfs) = self.vfs {
                    vfs.file_path(call_site.file)
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_else(|| format!("file_{}", call_site.file.to_raw()))
                } else if call_site.file.to_raw() == u32::MAX {
                    String::from("<bogus>")
                } else {
                    format!("file_{}", call_site.file.to_raw())
                };
                let text = SmolStr::from(format!("\"{}\"", name.replace('\\', "\\\\")));
                vec![TokenTree::Token(SyntaxKind::StringLit, text)]
            }
            BuiltinMacro::Line => {
                // line!() expands to the 1-based line number of the call site.
                let line_num = self.line_col_of(call_site).0;
                vec![TokenTree::Token(
                    SyntaxKind::IntLit,
                    SmolStr::from(line_num.to_string()),
                )]
            }
            BuiltinMacro::Column => {
                // column!() expands to the 1-based column number of the call site.
                let col_num = self.line_col_of(call_site).1;
                vec![TokenTree::Token(
                    SyntaxKind::IntLit,
                    SmolStr::from(col_num.to_string()),
                )]
            }
            BuiltinMacro::Env => {
                // env!("VAR") reads an environment variable at compile time and
                // errors if it is not set.
                let args_tt = flatten_token_tree(args_node);
                match first_string_lit(&args_tt) {
                    Some(var_name) => match std::env::var(var_name) {
                        Ok(val) => {
                            let lit = SmolStr::from(format!("\"{}\"", val));
                            vec![TokenTree::Token(SyntaxKind::StringLit, lit)]
                        }
                        Err(_) => {
                            return (
                                None,
                                vec![GlyimDiagnostic::type_error(
                                    call_site,
                                    format!("environment variable '{}' not found", var_name),
                                )],
                            );
                        }
                    },
                    None => {
                        return (
                            None,
                            vec![GlyimDiagnostic::type_error(
                                call_site,
                                "env! expects one string literal argument".to_string(),
                            )],
                        );
                    }
                }
            }
            BuiltinMacro::OptionEnv => {
                // option_env!("VAR") reads an environment variable at compile
                // time, expanding to `None` if absent and `Some("value")` if set
                // (sibling of `env!`, plan §21.1).
                let args_tt = flatten_token_tree(args_node);
                match first_string_lit(&args_tt) {
                    Some(var_name) => {
                        let token = match std::env::var(var_name) {
                            Ok(val) => SmolStr::from(format!("Some(\"{}\")", val)),
                            Err(_) => SmolStr::from("None"),
                        };
                        vec![TokenTree::Token(SyntaxKind::Ident, token)]
                    }
                    None => {
                        return (
                            None,
                            vec![GlyimDiagnostic::type_error(
                                call_site,
                                "option_env! expects one string literal argument".to_string(),
                            )],
                        );
                    }
                }
            }
            BuiltinMacro::IncludeStr => {
                // include_str!("path") reads the file as UTF-8 and expands to a
                // string literal. Invalid UTF-8 is a hard error (no lossy
                // conversion), per plan §21.2.
                let args_tt = flatten_token_tree(args_node);
                match first_string_lit(&args_tt) {
                    Some(path_str) => {
                        let path = Path::new(path_str);
                        let resolved = if path.is_absolute() {
                            path.to_path_buf()
                        } else if let Some(vfs) = self.vfs {
                            match vfs.file_path(call_site.file) {
                                Some(calling) => calling
                                    .parent()
                                    .map(|dir| dir.join(path_str))
                                    .unwrap_or_else(|| PathBuf::from(path_str)),
                                None => PathBuf::from(path_str),
                            }
                        } else {
                            PathBuf::from(path_str)
                        };
                        match fs::read_to_string(&resolved) {
                            Ok(content) => {
                                let escaped = content.replace('\\', "\\\\").replace('"', "\\\"");
                                let lit = SmolStr::from(format!("\"{}\"", escaped));
                                vec![TokenTree::Token(SyntaxKind::StringLit, lit)]
                            }
                            Err(e) => {
                                return (
                                    None,
                                    vec![GlyimDiagnostic::type_error(
                                        call_site,
                                        format!(
                                            "failed to read file '{}' as UTF-8: {}",
                                            resolved.display(),
                                            e
                                        ),
                                    )],
                                );
                            }
                        }
                    }
                    None => {
                        return (
                            None,
                            vec![GlyimDiagnostic::type_error(
                                call_site,
                                "include_str! expects one string literal argument".to_string(),
                            )],
                        );
                    }
                }
            }
            BuiltinMacro::IncludeBytes => {
                // include_bytes!("path") reads the file as raw bytes and expands
                // to a bracketed integer-array literal `[b0, b1, ...]` (parseable
                // as an array of `u8`), per plan §21.2.
                let args_tt = flatten_token_tree(args_node);
                match first_string_lit(&args_tt) {
                    Some(path_str) => {
                        let path = Path::new(path_str);
                        let resolved = if path.is_absolute() {
                            path.to_path_buf()
                        } else if let Some(vfs) = self.vfs {
                            match vfs.file_path(call_site.file) {
                                Some(calling) => calling
                                    .parent()
                                    .map(|dir| dir.join(path_str))
                                    .unwrap_or_else(|| PathBuf::from(path_str)),
                                None => PathBuf::from(path_str),
                            }
                        } else {
                            PathBuf::from(path_str)
                        };
                        match fs::read(&resolved) {
                            Ok(bytes) => {
                                let mut inner = Vec::with_capacity(bytes.len() * 2);
                                for (i, b) in bytes.iter().enumerate() {
                                    if i > 0 {
                                        inner.push(TokenTree::Token(
                                            SyntaxKind::Comma,
                                            SmolStr::from(","),
                                        ));
                                    }
                                    inner.push(TokenTree::Token(
                                        SyntaxKind::IntLit,
                                        SmolStr::from(b.to_string()),
                                    ));
                                }
                                vec![TokenTree::Group(
                                    SyntaxKind::LBracket,
                                    inner,
                                    SyntaxKind::RBracket,
                                )]
                            }
                            Err(e) => {
                                return (
                                    None,
                                    vec![GlyimDiagnostic::type_error(
                                        call_site,
                                        format!(
                                            "failed to read file '{}': {}",
                                            resolved.display(),
                                            e
                                        ),
                                    )],
                                );
                            }
                        }
                    }
                    None => {
                        return (
                            None,
                            vec![GlyimDiagnostic::type_error(
                                call_site,
                                "include_bytes! expects one string literal argument".to_string(),
                            )],
                        );
                    }
                }
            }
            BuiltinMacro::Include => {
                // include!("path") reads file content as a string literal.
                // Resolves relative to the calling file's directory when a VFS
                // with the call-site file is available; otherwise CWD.
                let args_tt = flatten_token_tree(args_node);
                match first_string_lit(&args_tt) {
                    Some(path_str) => {
                        let path = Path::new(path_str);
                        let resolved = if path.is_absolute() {
                            path.to_path_buf()
                        } else if let Some(vfs) = self.vfs {
                            match vfs.file_path(call_site.file) {
                                Some(calling) => calling
                                    .parent()
                                    .map(|dir| dir.join(path_str))
                                    .unwrap_or_else(|| PathBuf::from(path_str)),
                                None => PathBuf::from(path_str),
                            }
                        } else {
                            PathBuf::from(path_str)
                        };
                        match fs::read_to_string(&resolved) {
                            Ok(content) => {
                                let escaped = content.replace('\\', "\\\\").replace('"', "\\\"");
                                let lit = SmolStr::from(format!("\"{}\"", escaped));
                                vec![TokenTree::Token(SyntaxKind::StringLit, lit)]
                            }
                            Err(e) => {
                                return (
                                    None,
                                    vec![GlyimDiagnostic::type_error(
                                        call_site,
                                        format!(
                                            "failed to read file '{}': {}",
                                            resolved.display(),
                                            e
                                        ),
                                    )],
                                );
                            }
                        }
                    }
                    None => {
                        return (
                            None,
                            vec![GlyimDiagnostic::type_error(
                                call_site,
                                "include! expects one string literal argument".to_string(),
                            )],
                        );
                    }
                }
            }
            BuiltinMacro::Concat => {
                // concat!(a, b, ...) concatenates string representations, skipping punctuation
                let args_tt = flatten_token_tree(args_node);
                let mut result = String::new();
                for tt in &args_tt {
                    match tt {
                        TokenTree::Token(kind, text) => {
                            // Skip tokens that are punctuation (commas, semicolons, colons, parentheses, braces, brackets)
                            let text_str = text.as_str();
                            if text_str == ","
                                || text_str == ";"
                                || text_str == ":"
                                || text_str == "("
                                || text_str == ")"
                                || text_str == "{"
                                || text_str == "}"
                                || text_str == "["
                                || text_str == "]"
                            {
                                continue;
                            }
                            // For string literals, strip quotes
                            if *kind == SyntaxKind::StringLit {
                                let s = &text_str[1..text_str.len() - 1];
                                result.push_str(s);
                            } else {
                                result.push_str(text_str);
                            }
                        }
                        TokenTree::Group(_open, inner, _close) => {
                            // Recursively flatten group content (ignore delimiters)
                            for inner_tt in inner {
                                if let TokenTree::Token(kind, text) = inner_tt {
                                    let text_str = text.as_str();
                                    if text_str == "," || text_str == ";" || text_str == ":" {
                                        continue;
                                    }
                                    if *kind == SyntaxKind::StringLit {
                                        let s = &text_str[1..text_str.len() - 1];
                                        result.push_str(s);
                                    } else {
                                        result.push_str(text_str);
                                    }
                                } else if let TokenTree::Group(_, inner2, _) = inner_tt {
                                    // Flatten further groups (avoid recursion for simplicity - just skip)
                                    for inn in inner2 {
                                        if let TokenTree::Token(kind, text) = inn {
                                            let text_str = text.as_str();
                                            if text_str == "," || text_str == ";" || text_str == ":"
                                            {
                                                continue;
                                            }
                                            if *kind == SyntaxKind::StringLit {
                                                let s = &text_str[1..text_str.len() - 1];
                                                result.push_str(s);
                                            } else {
                                                result.push_str(text_str);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        TokenTree::DollarCrate => {
                            result.push_str("$crate");
                        }
                    }
                }
                let lit = SmolStr::from(format!("\"{}\"", result));
                vec![TokenTree::Token(SyntaxKind::StringLit, lit)]
            }
            BuiltinMacro::ConcatIdents => {
                // concat_idents!(a, b, ...) joins the textual content of each
                // identifier argument into a single new identifier token.
                // Per plan §21.3, the synthesized identifier does NOT inherit any
                // one argument's hygiene context; it is emitted with the root
                // syntax context so it resolves like a regular identifier.
                let args_tt = flatten_token_tree(args_node);
                let mut result = String::new();
                for tt in &args_tt {
                    match tt {
                        TokenTree::Token(kind, text) => {
                            let text_str = text.as_str();
                            // Skip punctuation separators (commas, etc.).
                            if text_str == ","
                                || text_str == ";"
                                || text_str == ":"
                                || text_str == "("
                                || text_str == ")"
                                || text_str == "{"
                                || text_str == "}"
                                || text_str == "["
                                || text_str == "]"
                            {
                                continue;
                            }
                            // For string literals, strip the surrounding quotes.
                            if *kind == SyntaxKind::StringLit {
                                let s = &text_str[1..text_str.len().saturating_sub(1)];
                                result.push_str(s);
                            } else {
                                result.push_str(text_str);
                            }
                        }
                        TokenTree::Group(_open, inner, _close) => {
                            for inner_tt in inner {
                                if let TokenTree::Token(_kind, text) = inner_tt {
                                    let text_str = text.as_str();
                                    if text_str == "," || text_str == ";" || text_str == ":" {
                                        continue;
                                    }
                                    result.push_str(text_str);
                                }
                            }
                        }
                        TokenTree::DollarCrate => {
                            result.push_str("$crate");
                        }
                    }
                }
                if result.is_empty() {
                    return (
                        None,
                        vec![GlyimDiagnostic::type_error(
                            call_site,
                            "concat_idents! requires at least one identifier argument".to_string(),
                        )],
                    );
                }
                vec![TokenTree::Token(SyntaxKind::Ident, SmolStr::from(result))]
            }
            BuiltinMacro::Stringify => {
                // stringify!(expr) returns the source code of expr as a string literal,
                // with spaces between tokens.
                let args_tt = flatten_token_tree(args_node);
                // Strip outer parentheses (macro call syntax delimiters)
                let inner = if args_tt.len() == 1 {
                    if let TokenTree::Group(SyntaxKind::LParen, inner, SyntaxKind::RParen) =
                        &args_tt[0]
                    {
                        inner.as_slice()
                    } else {
                        args_tt.as_slice()
                    }
                } else if args_tt.len() >= 2 {
                    let first_is_lparen =
                        matches!(&args_tt[0], TokenTree::Token(SyntaxKind::LParen, _));
                    let last_is_rparen = matches!(
                        args_tt.last(),
                        Some(TokenTree::Token(SyntaxKind::RParen, _))
                    );
                    if first_is_lparen && last_is_rparen {
                        &args_tt[1..args_tt.len().saturating_sub(1)]
                    } else {
                        args_tt.as_slice()
                    }
                } else {
                    args_tt.as_slice()
                };
                let stringified = stringify_token_trees(inner);
                // Escape backslashes and quotes for string literal representation
                let escaped = stringified.replace('\\', "\\\\").replace('"', "\\\"");
                let lit = SmolStr::from(format!("\"{}\"", escaped));
                vec![TokenTree::Token(SyntaxKind::StringLit, lit)]
            }
            BuiltinMacro::Format => {
                // `format!(fmt, args..)` — the probe type-checks only, so emit
                // an empty string literal. Real interpolation needs `Display`
                // dispatch (out of scope for the stdlib-compile fix).
                vec![TokenTree::Token(
                    SyntaxKind::StringLit,
                    SmolStr::from("\"\""),
                )]
            }
            BuiltinMacro::Vec => {
                // `vec![a, b, c]` → `[a, b, c]`.
                let args_tt = flatten_token_tree(args_node);
                let mut out: Vec<TokenTree> = Vec::new();
                out.push(TokenTree::Token(SyntaxKind::LBracket, SmolStr::from("[")));
                for tt in &args_tt {
                    if let TokenTree::Token(kind, text) = tt {
                        if *kind == SyntaxKind::Comma {
                            continue;
                        }
                        out.push(TokenTree::Token(*kind, text.clone()));
                    }
                }
                out.push(TokenTree::Token(SyntaxKind::RBracket, SmolStr::from("]")));
                out
            }
            BuiltinMacro::Matches => {
                // `matches!(e, pat)` → `true`.
                vec![TokenTree::Token(SyntaxKind::KwTrue, SmolStr::from("true"))]
            }
            BuiltinMacro::Print => {
                // print! / println! / eprint! / eprintln! → `()`.
                vec![
                    TokenTree::Token(SyntaxKind::LParen, SmolStr::from("(")),
                    TokenTree::Token(SyntaxKind::RParen, SmolStr::from(")")),
                ]
            }
            BuiltinMacro::Panic => {
                // panic!(..) → `loop {}`.
                vec![
                    TokenTree::Token(SyntaxKind::KwLoop, SmolStr::from("loop")),
                    TokenTree::Token(SyntaxKind::LBrace, SmolStr::from("{")),
                    TokenTree::Token(SyntaxKind::RBrace, SmolStr::from("}")),
                ]
            }
            BuiltinMacro::Assert => {
                // assert!(..) → `()`.
                vec![
                    TokenTree::Token(SyntaxKind::LParen, SmolStr::from("(")),
                    TokenTree::Token(SyntaxKind::RParen, SmolStr::from(")")),
                ]
            }
            BuiltinMacro::Write => {
                // write!(..) / writeln!(..) → `Result::Ok(())`.
                vec![
                    TokenTree::Token(SyntaxKind::Ident, SmolStr::from("Result")),
                    TokenTree::Token(SyntaxKind::ColonColon, SmolStr::from("::")),
                    TokenTree::Token(SyntaxKind::Ident, SmolStr::from("Ok")),
                    TokenTree::Token(SyntaxKind::LParen, SmolStr::from("(")),
                    TokenTree::Token(SyntaxKind::LParen, SmolStr::from("(")),
                    TokenTree::Token(SyntaxKind::RParen, SmolStr::from(")")),
                    TokenTree::Token(SyntaxKind::RParen, SmolStr::from(")")),
                ]
            }
        };
        let expanded_green =
            self.build_expansion_green(&expanded_trees, call_site, _depth, name, true);
        (Some(expanded_green), Vec::new())
    }

    fn build_expansion_green(
        &mut self,
        trees: &[TokenTree],
        call_site: Span,
        _depth: u32,
        name: Name,
        is_builtin: bool,
    ) -> GreenNode {
        let kind = if is_builtin {
            ExpnKind::Builtin { name }
        } else {
            ExpnKind::MacroRules { name }
        };
        let expn_id = self.hygiene.push_expansion(ExpnData {
            expn_id: glyim_span::ExpnId::ROOT,
            parent: glyim_span::ExpnId::ROOT,
            kind,
            call_site,
            def_site: call_site,
            transparency: Transparency::SemiTransparent,
        });

        let mark = Mark {
            expn_id,
            transparency: Transparency::SemiTransparent,
        };

        let mut builder = rowan::GreenNodeBuilder::new();
        // Wrap expansion tokens in a synthetic SourceFile node so the tree is balanced
        builder.start_node(GlyimLang::kind_to_raw(SyntaxKind::SourceFile));
        for tree in trees {
            self.build_token_tree_green(tree, &mut builder, &mark);
        }
        builder.finish_node();
        builder.finish()
    }

    fn build_token_tree_green(
        &self,
        tree: &TokenTree,
        builder: &mut rowan::GreenNodeBuilder,
        _mark: &Mark,
    ) {
        match tree {
            TokenTree::Token(kind, text) => {
                builder.token(GlyimLang::kind_to_raw(*kind), text.as_str());
            }
            TokenTree::Group(delim_open, children, delim_close) => {
                builder.token(
                    GlyimLang::kind_to_raw(*delim_open),
                    delim_token_text(*delim_open),
                );
                for child in children {
                    self.build_token_tree_green(child, builder, _mark);
                }
                builder.token(
                    GlyimLang::kind_to_raw(*delim_close),
                    delim_token_text(*delim_close),
                );
            }
            TokenTree::DollarCrate => {
                builder.token(GlyimLang::kind_to_raw(SyntaxKind::KwCrate), "crate");
            }
        }
    }

    fn file_id_from_node(&self, _node: &SyntaxNode) -> FileId {
        self.current_file
    }

    fn span_from_node(&self, node: &SyntaxNode) -> Span {
        let range = node.text_range();
        Span::new(
            self.file_id_from_node(node),
            ByteIdx::from_raw(range.start().into()),
            ByteIdx::from_raw(range.end().into()),
            SyntaxContext::ROOT,
        )
    }

    /// Compute the 1-based (line, column) of a span's start, using the real
    /// source text from the VFS when available. Falls back to a heuristic
    /// (`lo / 80`, `lo % 80`) for call sites without a VFS/source.
    fn line_col_of(&self, span: Span) -> (u32, u32) {
        if let Some(vfs) = self.vfs
            && let Some(src) = vfs.file_content(span.file)
        {
            let offset = span.lo.to_usize();
            let mut line = 1u32;
            let mut col = 1u32;
            for (i, ch) in src.char_indices() {
                if i >= offset {
                    break;
                }
                if ch == '\n' {
                    line += 1;
                    col = 1;
                } else {
                    col += 1;
                }
            }
            return (line, col);
        }
        // Fallback heuristic when no source is available.
        let lo = span.lo.to_raw();
        (
            lo.checked_div(80).unwrap_or(0).saturating_add(1),
            lo.checked_rem(80).unwrap_or(0).saturating_add(1),
        )
    }
}

/// Convert token trees to a string with deterministic spacing, approximating
/// real `macro_rules!` `stringify!`:
/// - single space between adjacent tokens,
/// - no space before `,`/`;`/`)`/`]`/`}`,
/// - no space after `(`/`[`/`{`,
/// - delimiters written as their literal characters.
///
/// This is intentionally NOT byte-exact to the original source (glyim's
/// `TokenTree` carries only `SyntaxKind` + text, not spans) — it matches
/// `stringify!`'s normalized output closely enough for production use.
fn stringify_token_trees(trees: &[TokenTree]) -> String {
    // Flatten into an ordered list of leaf pieces (tokens + delimiter chars).
    let mut leaves: Vec<String> = Vec::new();
    fn flatten(trees: &[TokenTree], out: &mut Vec<String>) {
        for tree in trees {
            match tree {
                TokenTree::Token(_kind, text) => out.push(text.as_str().to_string()),
                TokenTree::Group(open, inner, close) => {
                    out.push(delim_char(*open).to_string());
                    flatten(inner, out);
                    out.push(delim_char(*close).to_string());
                }
                TokenTree::DollarCrate => out.push("$crate".to_string()),
            }
        }
    }
    flatten(trees, &mut leaves);

    let mut out = String::new();
    for (i, piece) in leaves.iter().enumerate() {
        if i > 0 && needs_space_before(&leaves[i - 1], piece) {
            out.push(' ');
        }
        out.push_str(piece);
    }
    out
}

/// Delimiter open/close char for a `SyntaxKind` (returns ' ' for non-delims).
fn delim_char(kind: SyntaxKind) -> char {
    match kind {
        SyntaxKind::LParen => '(',
        SyntaxKind::RParen => ')',
        SyntaxKind::LBrace => '{',
        SyntaxKind::RBrace => '}',
        SyntaxKind::LBracket => '[',
        SyntaxKind::RBracket => ']',
        _ => ' ',
    }
}

/// Spacing rule matching real `stringify!`: no space before a closing or
/// separator punctuation, no space after an opening punctuation, no space
/// after `#` (so `#[attr]` / `#![crate_attr]` stay fused), space everywhere
/// else.
fn needs_space_before(prev: &str, next: &str) -> bool {
    let next_closes = matches!(next, "," | ";" | ")" | "]" | "}");
    let prev_opens = matches!(prev, "(" | "[" | "{");
    let prev_hash = prev == "#";
    !(next_closes || prev_opens || prev_hash)
}

/// Recursively find the first string-literal token in a flattened token tree,
/// regardless of whether it is wrapped in a delimiter group. Used by `env!` /
/// `include!` whose argument may arrive as a bare `Token` or wrapped in a
/// `( ... )` group depending on the call path.
fn first_string_lit(trees: &[TokenTree]) -> Option<&str> {
    for tt in trees {
        match tt {
            TokenTree::Token(SyntaxKind::StringLit, text) => {
                return Some(&text.as_str()[1..text.len() - 1]);
            }
            TokenTree::Group(_, inner, _) => {
                if let Some(s) = first_string_lit(inner) {
                    return Some(s);
                }
            }
            _ => {}
        }
    }
    None
}

fn delim_token_text(kind: SyntaxKind) -> &'static str {
    match kind {
        SyntaxKind::LParen => "(",
        SyntaxKind::RParen => ")",
        SyntaxKind::LBrace => "{",
        SyntaxKind::RBrace => "}",
        SyntaxKind::LBracket => "[",
        SyntaxKind::RBracket => "]",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glyim_syntax::SyntaxKind;

    fn tok(text: &str) -> TokenTree {
        // Best-effort kind: punctuation vs identifier-like.
        let kind = match text {
            "(" => SyntaxKind::LParen,
            ")" => SyntaxKind::RParen,
            "[" => SyntaxKind::LBracket,
            "]" => SyntaxKind::RBracket,
            "{" => SyntaxKind::LBrace,
            "}" => SyntaxKind::RBrace,
            _ => SyntaxKind::Ident,
        };
        TokenTree::Token(kind, smol_str::SmolStr::from(text))
    }

    fn grp(open: SyntaxKind, inner: Vec<TokenTree>, close: SyntaxKind) -> TokenTree {
        TokenTree::Group(open, inner, close)
    }

    #[test]
    fn stringify_spaces_infix_operands() {
        // stringify!(1+2) -> "1 + 2"
        let trees = vec![tok("1"), tok("+"), tok("2")];
        assert_eq!(stringify_token_trees(&trees), "1 + 2");
    }

    #[test]
    fn stringify_call_no_space_around_parens_or_comma() {
        // stringify!(foo(a, b)) -> "foo (a, b)" (space between callee and
        // `(`, none after `(`/before `,`/before `)` — per the documented rule).
        let trees = vec![
            tok("foo"),
            grp(
                SyntaxKind::LParen,
                vec![tok("a"), tok(","), tok("b")],
                SyntaxKind::RParen,
            ),
        ];
        assert_eq!(stringify_token_trees(&trees), "foo (a, b)");
    }

    #[test]
    fn needs_space_before_rules() {
        assert!(!needs_space_before("(", "x")); // no space after open
        assert!(!needs_space_before("x", ")")); // no space before close
        assert!(!needs_space_before("x", ",")); // no space before comma
        assert!(needs_space_before("1", "+")); // space between operands
    }
}
