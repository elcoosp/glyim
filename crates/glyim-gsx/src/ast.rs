//! GSX AST: the structural view tree extracted from the `Gsx*` CST nodes
//! (design doc §3).
//!
//! The CST keeps every source byte (lossless, rowan); this AST is the
//! *structural* projection the pipeline consumes: elements, fragments,
//! attributes (with their value kinds), and children where text is already
//! split into static pieces + holes at parse time (§3.2: "The GSX parser
//! splits the literal into static pieces + typed holes at parse time, so each
//! hole's read-set is syntactically visible and escaping is type-driven").
//!
//! Tag names distinguish the three resolution classes of §3.2:
//! - lowercase intrinsic (`div`) → resolved through the active dialect schema,
//! - dialect-qualified intrinsic (`web:canvas`) → host-specific escape hatch,
//! - uppercase / path component (`Counter`, `ui::Button`) → resolved through
//!   the def-map like any item.

use crate::diag::{GsxCode, GsxDiagnostic};
use crate::expr::{Expr, expr_from_cst};
use glyim_syntax::{SyntaxKind, SyntaxNode};

/// A tag name as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagName {
    /// Lowercase intrinsic: resolved through the dialect schema (§3.2).
    Intrinsic(String),
    /// Dialect-qualified: `<web:canvas/>`, `<native:window/>` (§3.2).
    Dialect(String, String),
    /// Component: uppercase head or multi-segment path (`Counter`,
    /// `ui::Button`) — resolved through the def-map (§3.2).
    Component(Vec<String>),
}

impl TagName {
    /// Classification helper used by the VIR builder.
    pub fn is_component(&self) -> bool {
        matches!(self, TagName::Component(_))
    }
}

/// The value of one attribute.
#[derive(Debug, Clone, PartialEq)]
pub enum AttrValue {
    /// `name="lit"` — constant (G2 folds these into the blueprint).
    Lit(String),
    /// `name={expr}` — dynamic slot (§6.1).
    Expr(Expr),
    /// Bare `disabled` — constant `true` (§3.2).
    Bare,
}

/// One attribute, classified.
#[derive(Debug, Clone, PartialEq)]
pub enum GsxAttr {
    /// Plain attribute `name=value`.
    Plain(String, AttrValue),
    /// Directive attribute `ns:name=value` — `on:` events (§7.3),
    /// `class:`/`style:` toggles (§6.1), `bind:` two-way binding (§7.9).
    Directive(String, String, AttrValue),
}

impl GsxAttr {
    /// Attribute (or directive) name for schema lookups.
    pub fn schema_name(&self) -> String {
        match self {
            GsxAttr::Plain(n, _) => n.clone(),
            // `class:warn` checks the `class` prop family; `on:click` the
            // event family; `bind:value` the `value` prop family.
            GsxAttr::Directive(ns, _, _) => ns.clone(),
        }
    }

    /// The directive sub-name (`click` of `on:click`), when a directive.
    pub fn directive_sub(&self) -> Option<&str> {
        match self {
            GsxAttr::Plain(..) => None,
            GsxAttr::Directive(_, sub, _) => Some(sub),
        }
    }
}

/// One interpolated-text piece: static run or hole (§3.2).
#[derive(Debug, Clone, PartialEq)]
pub enum TextPiece {
    /// Literal text (escaped per hole position at emission, G6/G7).
    Static(String),
    /// `{expr}` hole.
    Hole(Expr),
}

/// A child of an element or fragment.
#[derive(Debug, Clone, PartialEq)]
pub enum GsxChild {
    /// A quoted text child — one or more pieces (holes split, §3.2).
    Text(Vec<TextPiece>),
    /// `{expr}` dynamic child (§3.2) — any expression; the VIR builder
    /// recognizes region forms (`if`/`match`/`for`, components).
    Block(Expr),
    /// A nested element node.
    Node(GsxNode),
}

/// An element or fragment node.
#[derive(Debug, Clone, PartialEq)]
pub enum GsxNode {
    /// `<tag attrs> children </tag>` or `<tag attrs/>`.
    Element {
        /// Tag name (classification in [`TagName`]).
        tag: TagName,
        /// Attributes in source order.
        attrs: Vec<GsxAttr>,
        /// Children; empty for self-closing.
        children: Vec<GsxChild>,
        /// Whether written as `<tag/>`.
        self_closing: bool,
    },
    /// `<> children </>` (§3.2).
    Fragment {
        /// `Children`.
        children: Vec<GsxChild>,
    },
}

impl GsxNode {
    /// All attributes (elements only; fragments have none).
    pub fn attrs(&self) -> &[GsxAttr] {
        match self {
            GsxNode::Element { attrs, .. } => attrs,
            GsxNode::Fragment { .. } => &[],
        }
    }

    /// Children of this node.
    pub fn children(&self) -> &[GsxChild] {
        match self {
            GsxNode::Element { children, .. } | GsxNode::Fragment { children } => children,
        }
    }
}

/// The result of converting one GSX CST root: an AST plus any structural
/// diagnostics (duplicate attrs, unknown attr-value shapes).
#[derive(Debug, Clone, Default)]
pub struct GsxResult {
    /// The extracted node, when the source contained one.
    pub root: Option<GsxNode>,
    /// Structural diagnostics from conversion.
    pub diagnostics: Vec<GsxDiagnostic>,
}

/// Convert the top-level `Gsx*` nodes of a parse tree into the GSX AST.
///
/// A view body is a sequence of elements; e.g. a view fn whose body is
/// `<section>…</section>`. When the body holds several sibling elements
/// they are wrapped in a synthetic fragment (§3.2: fragments group children
/// without a host node).
pub fn gsx_from_parse(parsed: &glyim_frontend::ParseResult) -> GsxResult {
    let mut out = GsxResult::default();
    // Top-level GSX nodes: no Gsx ancestor.
    let top: Vec<SyntaxNode> = parsed
        .root
        .descendants()
        .filter(|n| {
            matches!(
                n.kind(),
                SyntaxKind::GsxElement | SyntaxKind::GsxSelfClosing | SyntaxKind::GsxFragment
            )
        })
        .filter(|n| {
            !n.ancestors().skip(1).any(|a| {
                matches!(
                    a.kind(),
                    SyntaxKind::GsxElement | SyntaxKind::GsxSelfClosing | SyntaxKind::GsxFragment
                )
            })
        })
        .collect();
    let mut ds = vec![];
    let roots: Vec<GsxNode> = top
        .iter()
        .filter_map(|n| {
            let (g, more) = node_from_cst(n);
            ds.extend(more);
            g
        })
        .collect();
    out.root = match roots.len() {
        0 => None,
        1 => Some(roots.into_iter().next().unwrap()),
        _ => Some(GsxNode::Fragment {
            children: roots.into_iter().map(GsxChild::Node).collect(),
        }),
    };
    out.diagnostics = ds;
    out
}

/// Public re-export: convert one GSX CST node into the AST (used by the
/// expression model for region arms).
pub fn node_from_cst_pub(node: &SyntaxNode) -> (Option<GsxNode>, Vec<GsxDiagnostic>) {
    node_from_cst(node)
}

fn node_from_cst(node: &SyntaxNode) -> (Option<GsxNode>, Vec<GsxDiagnostic>) {
    match node.kind() {
        SyntaxKind::GsxFragment => {
            let mut ds = vec![];
            let children = children_from_cst(node, &mut ds);
            (Some(GsxNode::Fragment { children }), ds)
        }
        SyntaxKind::GsxElement | SyntaxKind::GsxSelfClosing => {
            let mut ds = vec![];
            let open = node
                .children()
                .find(|n| n.kind() == SyntaxKind::GsxOpenTag)
                .expect("GsxElement/GsxSelfClosing always has an open tag");
            let tag = tag_from_open(&open);
            let mut attrs = vec![];
            let mut seen: Vec<String> = vec![];
            for a in open.children() {
                match a.kind() {
                    SyntaxKind::GsxAttr | SyntaxKind::GsxDirectiveAttr | SyntaxKind::GsxSpread => {
                        if let Some(attr) = attr_from_cst(&a) {
                            let key = match &attr {
                                // G2: constant `class` values concatenate, so
                                // repeated class attrs are legal (folded later).
                                GsxAttr::Plain(n, _) if n == "class" => String::new(),
                                GsxAttr::Plain(n, _) => format!("attr:{n}"),
                                GsxAttr::Directive(ns, sub, _) => format!("dir:{ns}:{sub}"),
                            };
                            if !key.is_empty() {
                                if seen.contains(&key) {
                                    ds.push(GsxDiagnostic::new(
                                        GsxCode::DuplicateAttr,
                                        format!("duplicate attribute `{}`", attr_name_text(&attr)),
                                    ));
                                } else {
                                    seen.push(key);
                                }
                            }
                            attrs.push(attr);
                        }
                    }
                    _ => {}
                }
            }
            let self_closing = node.kind() == SyntaxKind::GsxSelfClosing;
            let children = if self_closing {
                vec![]
            } else {
                children_from_cst(node, &mut ds)
            };
            (
                Some(GsxNode::Element {
                    tag,
                    attrs,
                    children,
                    self_closing,
                }),
                ds,
            )
        }
        _ => (None, vec![]),
    }
}

fn attr_name_text(attr: &GsxAttr) -> String {
    match attr {
        GsxAttr::Plain(n, _) => n.clone(),
        GsxAttr::Directive(ns, sub, _) => format!("{ns}:{sub}"),
    }
}

/// Classify a tag name per §3.2.
fn tag_from_open(open: &SyntaxNode) -> TagName {
    let mut toks: Vec<String> = open
        .children_with_tokens()
        .filter_map(|e| e.into_token())
        .filter(|t| {
            matches!(
                t.kind(),
                SyntaxKind::Ident | SyntaxKind::Colon | SyntaxKind::ColonColon
            )
        })
        .map(|t| t.text().to_string())
        .collect();
    // Drop tokens that belong to attributes: attribute names follow the tag
    // name; stop at the first token that is followed by `=`, Minus, or is a
    // known directive head. Simpler and robust: the tag name is the longest
    // prefix of Ident/(:|::)Ident tokens that appears before any Eq token in
    // the raw token stream.
    let raw: Vec<(SyntaxKind, String)> = open
        .children_with_tokens()
        .filter_map(|e| e.into_token())
        .filter(|t| !t.kind().is_trivia())
        .map(|t| (t.kind(), t.text().to_string()))
        .collect();
    let mut name_toks: Vec<String> = vec![];
    for (i, (k, text)) in raw.iter().enumerate() {
        match k {
            SyntaxKind::Lt => continue,
            SyntaxKind::Ident if name_toks.is_empty() => name_toks.push(text.clone()),
            SyntaxKind::Colon | SyntaxKind::ColonColon => {
                if matches!(raw.get(i + 1), Some((SyntaxKind::Ident, _))) {
                    name_toks.push(text.clone());
                } else {
                    break;
                }
            }
            SyntaxKind::Ident if !name_toks.is_empty() => {
                // Only part of the name if directly preceded by : or ::
                let prev = raw.get(i - 1).map(|(k, _)| *k);
                if prev == Some(SyntaxKind::Colon) || prev == Some(SyntaxKind::ColonColon) {
                    name_toks.push(text.clone());
                } else {
                    break;
                }
            }
            _ => break,
        }
    }
    let _ = &mut toks;
    classify_name(&name_toks)
}

fn classify_name(name_toks: &[String]) -> TagName {
    if name_toks.is_empty() {
        return TagName::Intrinsic(String::new());
    }
    // Dialect namespace: exactly `ns : name` (single colon, two segments).
    if name_toks.len() == 3 && name_toks[1] == ":" {
        return TagName::Dialect(name_toks[0].clone(), name_toks[2].clone());
    }
    let head = &name_toks[0];
    let starts_upper = head
        .chars()
        .next()
        .map(|c| c.is_ascii_uppercase())
        .unwrap_or(false);
    if starts_upper || name_toks.len() > 1 {
        // Component: `Counter` or `ui::Button` (path segments).
        let mut segs = vec![];
        let mut i = 0;
        while i < name_toks.len() {
            match &name_toks[i] {
                t if t == ":" || t == "::" => {
                    i += 1;
                }
                t => {
                    segs.push(t.clone());
                    i += 1;
                }
            }
        }
        return TagName::Component(segs);
    }
    TagName::Intrinsic(head.clone())
}

fn attr_from_cst(node: &SyntaxNode) -> Option<GsxAttr> {
    let raw: Vec<(SyntaxKind, String)> = node
        .children_with_tokens()
        .filter(|e| {
            let k = e.kind();
            !k.is_trivia()
                && k != SyntaxKind::GsxAttr
                && k != SyntaxKind::GsxDirectiveAttr
                && k != SyntaxKind::GsxSpread
        })
        .map(|e| match e {
            glyim_syntax::SyntaxElement::Token(t) => (t.kind(), t.text().to_string()),
            glyim_syntax::SyntaxElement::Node(n) => (n.kind(), n.text().to_string()),
        })
        .collect();
    // Split leading name tokens from the value part.
    match node.kind() {
        SyntaxKind::GsxSpread => {
            // `{..expr}` — extract the inner expression node.
            let inner = node
                .children()
                .find(|n| crate::expr::is_expr_node(n.kind()))?;
            let e = expr_from_cst(&inner);
            Some(GsxAttr::Plain("..".to_string(), AttrValue::Expr(e)))
        }
        SyntaxKind::GsxAttr => {
            if raw.first().map(|(k, _)| *k) == Some(SyntaxKind::LBrace) {
                // Shorthand `{name}`: the attr's value is the brace expr.
                let inner = node
                    .children()
                    .find(|n| crate::expr::is_expr_node(n.kind()))?;
                let e = expr_from_cst(&inner);
                return Some(GsxAttr::Plain(shorthand_name(&e), AttrValue::Expr(e)));
            }
            let (name, value_start) = attr_name_and_value(&raw)?;
            Some(GsxAttr::Plain(name, value_from(value_start, node)))
        }
        SyntaxKind::GsxDirectiveAttr => {
            // tokens: Ident `:` Ident [Eq?] value…  — `on:click={h}` has no
            // `=`; `on:click={h}` with `=` is accepted for uniformity.
            let ns = raw.first()?.1.clone();
            let colon = raw.iter().position(|(k, _)| *k == SyntaxKind::Colon)?;
            let sub = raw.get(colon + 1)?.1.clone();
            let mut value_start = colon + 2;
            if raw.get(value_start).map(|(k, _)| *k) == Some(SyntaxKind::Eq) {
                value_start += 1;
            }
            Some(GsxAttr::Directive(ns, sub, value_from(value_start, node)))
        }
        _ => None,
    }
}

/// `{name}` shorthand: the attribute name is the trailing field/path name of
/// the hole expression (§3.2: `{name}` = `name={name}`).
fn shorthand_name(e: &Expr) -> String {
    match e {
        Expr::Field(_, name) => name.clone(),
        Expr::Var(v) => v.rsplit("::").next().unwrap_or(v).to_string(),
        _ => String::new(),
    }
}

/// Extract `name` from raw attribute tokens and the index just past any `=`.
fn attr_name_and_value(raw: &[(SyntaxKind, String)]) -> Option<(String, usize)> {
    let mut name = String::new();
    let mut i = 0;
    if i < raw.len() {
        match raw[i].0 {
            SyntaxKind::Ident | SyntaxKind::KwType | SyntaxKind::KwSelf => {
                name.push_str(&raw[i].1);
                i += 1;
                // hyphenated: `-Ident`
                if i + 1 < raw.len()
                    && raw[i].0 == SyntaxKind::Minus
                    && raw[i + 1].0 == SyntaxKind::Ident
                {
                    name.push('-');
                    name.push_str(&raw[i + 1].1);
                    i += 2;
                }
            }
            _ => return None,
        }
    }
    if i < raw.len() && raw[i].0 == SyntaxKind::Eq {
        i += 1;
    }
    Some((name, i))
}

fn value_from(start: usize, node: &SyntaxNode) -> AttrValue {
    let raw: Vec<(SyntaxKind, String)> = node
        .children_with_tokens()
        .filter(|e| {
            let k = e.kind();
            !k.is_trivia()
                && k != SyntaxKind::GsxAttr
                && k != SyntaxKind::GsxDirectiveAttr
                && k != SyntaxKind::GsxSpread
        })
        .map(|e| match e {
            glyim_syntax::SyntaxElement::Token(t) => (t.kind(), t.text().to_string()),
            glyim_syntax::SyntaxElement::Node(n) => (n.kind(), n.text().to_string()),
        })
        .collect();
    if start >= raw.len() {
        return AttrValue::Bare;
    }
    match raw[start].0 {
        SyntaxKind::StringLit => {
            let text = &raw[start].1;
            let inner = text
                .strip_prefix('"')
                .and_then(|x| x.strip_suffix('"'))
                .unwrap_or(text);
            AttrValue::Lit(crate::expr::unescape(inner))
        }
        SyntaxKind::LBrace => {
            let inner = node
                .children()
                .find(|n| crate::expr::is_expr_node(n.kind()));
            match inner {
                Some(n) => AttrValue::Expr(expr_from_cst(&n)),
                None => AttrValue::Bare,
            }
        }
        _ => AttrValue::Bare,
    }
}

fn children_from_cst(node: &SyntaxNode, ds: &mut Vec<GsxDiagnostic>) -> Vec<GsxChild> {
    let mut out = vec![];
    for c in node.children() {
        match c.kind() {
            SyntaxKind::GsxTextLit => {
                let raw = c.text().to_string();
                let inner = raw
                    .strip_prefix('"')
                    .and_then(|x| x.strip_suffix('"'))
                    .unwrap_or(&raw);
                out.push(GsxChild::Text(split_text_holes(inner)));
            }
            SyntaxKind::GsxBlockChild => {
                let e = c
                    .children()
                    .find(|n| crate::expr::is_expr_node(n.kind()))
                    .map(|n| expr_from_cst(&n));
                match e {
                    Some(e) => out.push(GsxChild::Block(e)),
                    None => ds.push(GsxDiagnostic::new(
                        GsxCode::ContentModel,
                        "empty block child",
                    )),
                }
            }
            SyntaxKind::GsxElement | SyntaxKind::GsxSelfClosing | SyntaxKind::GsxFragment => {
                let (n, more) = node_from_cst(&c);
                ds.extend(more);
                if let Some(n) = n {
                    out.push(GsxChild::Node(n));
                }
            }
            _ => {}
        }
    }
    out
}

/// §3.2: split `"Count: {s.count}"` into static pieces + holes at parse time.
///
/// Braces inside a hole (closures, blocks) are honored by brace counting so
/// `"Click {on:click={..}}"`-shaped nonsense still splits sanely; an
/// unterminated hole falls back to static text (structural diagnostics are
/// the parser's job, not the splitter's).
pub fn split_text_holes(text: &str) -> Vec<TextPiece> {
    let mut pieces = vec![];
    let mut static_run = String::new();
    let mut chars = text.char_indices().peekable();
    while let Some((i, ch)) = chars.next() {
        if ch == '{' {
            // Find the matching close brace, honoring nesting and quotes.
            let mut depth = 1usize;
            let mut j = i + 1;
            let bytes = text.as_bytes();
            let mut in_str = false;
            while j < bytes.len() {
                let b = bytes[j];
                if in_str {
                    if b == b'\\' {
                        j += 1;
                    } else if b == b'"' {
                        in_str = false;
                    }
                } else if b == b'"' {
                    in_str = true;
                } else if b == b'{' {
                    depth += 1;
                } else if b == b'}' {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                j += 1;
            }
            if depth != 0 {
                // Unterminated: treat as static text.
                static_run.push('{');
                continue;
            }
            let hole_src = &text[i + 1..j];
            if !static_run.is_empty() {
                pieces.push(TextPiece::Static(std::mem::take(&mut static_run)));
            }
            let hole = parse_hole_expr(hole_src);
            pieces.push(TextPiece::Hole(hole));
            // Advance past the hole.
            let skip = j + 1 - i - 1;
            for _ in 0..skip {
                chars.next();
            }
        } else {
            static_run.push(ch);
        }
    }
    if !static_run.is_empty() {
        pieces.push(TextPiece::Static(static_run));
    }
    if pieces.is_empty() {
        pieces.push(TextPiece::Static(String::new()));
    }
    pieces
}

/// Parse one hole's source text through the frontend and extract the model.
fn parse_hole_expr(src: &str) -> Expr {
    let wrapped = format!("fn __g() {{ let _h = {src}; }}");
    let parsed = glyim_frontend::parse_to_syntax(&wrapped, glyim_span::FileId::from_raw(0));
    parsed
        .root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::LetStmt)
        .and_then(|l| l.children().find(|n| crate::expr::is_expr_node(n.kind())))
        .map(|n| expr_from_cst(&n))
        .unwrap_or(Expr::Opaque(src.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::{Expr, Lit};

    /// Parse GSX source and return (root, diagnostics).
    fn parse_gsx(src: &str) -> (Option<GsxNode>, Vec<GsxDiagnostic>) {
        let wrapped = format!("fn __g() {{ {src} }}");
        let parsed = glyim_frontend::parse_to_syntax(&wrapped, glyim_span::FileId::from_raw(0));
        let g = gsx_from_parse(&parsed);
        (g.root, g.diagnostics)
    }

    /// §3.2: attributes carry their value kind: lit / expr / bare.
    #[test]
    fn attr_value_kinds() {
        let (root, ds) = parse_gsx("<input type=\"text\" value={x} disabled/>");
        assert!(ds.is_empty(), "{ds:?}");
        let n = root.unwrap();
        let attrs = n.attrs();
        assert_eq!(attrs.len(), 3);
        assert_eq!(
            attrs[0],
            GsxAttr::Plain("type".to_string(), AttrValue::Lit("text".to_string()))
        );
        assert!(matches!(
            &attrs[1],
            GsxAttr::Plain(n, AttrValue::Expr(Expr::Var(v))) if n == "value" && v == "x"
        ));
        assert_eq!(
            attrs[2],
            GsxAttr::Plain("disabled".to_string(), AttrValue::Bare)
        );
    }

    /// §3.2: text is always a string literal and holes are split at parse
    /// time (`"Count: {s.count}"` → static + hole).
    #[test]
    fn text_hole_splitting() {
        let (root, ds) = parse_gsx("<output>\"Count: {s.count}!\"</output>");
        assert!(ds.is_empty(), "{ds:?}");
        let n = root.unwrap();
        let kids = n.children();
        assert_eq!(kids.len(), 1);
        match &kids[0] {
            GsxChild::Text(pieces) => {
                assert_eq!(
                    pieces,
                    &vec![
                        TextPiece::Static("Count: ".to_string()),
                        TextPiece::Hole(Expr::field("s", "count")),
                        TextPiece::Static("!".to_string()),
                    ]
                );
            }
            other => panic!("unexpected child: {other:?}"),
        }
    }

    /// Nested holes with closures/braces split correctly (brace counting).
    #[test]
    fn text_hole_with_closure_braces() {
        let (root, _) = parse_gsx("<p>\"a {if s.count > 0 { 1 } else { 2 }} b\"</p>");
        let n = root.unwrap();
        match &n.children()[0] {
            GsxChild::Text(pieces) => {
                assert_eq!(pieces.len(), 3);
                assert!(matches!(&pieces[1], TextPiece::Hole(Expr::If { .. })));
                assert_eq!(pieces[2], TextPiece::Static(" b".to_string()));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    /// §3.2 tag classification: intrinsic / dialect / component / path.
    #[test]
    fn tag_classification() {
        let (r, _) = parse_gsx("<div/>");
        assert_eq!(tag_of(&r), TagName::Intrinsic("div".to_string()));
        let (r, _) = parse_gsx("<web:canvas/>");
        assert_eq!(
            tag_of(&r),
            TagName::Dialect("web".to_string(), "canvas".to_string())
        );
        let (r, _) = parse_gsx("<Counter x={1}/>");
        assert_eq!(tag_of(&r), TagName::Component(vec!["Counter".to_string()]));
        let (r, _) = parse_gsx("<ui::Button/>");
        assert_eq!(
            tag_of(&r),
            TagName::Component(vec!["ui".to_string(), "Button".to_string()])
        );
    }

    fn tag_of(r: &Option<GsxNode>) -> TagName {
        match r.as_ref().unwrap() {
            GsxNode::Element { tag, .. } => tag.clone(),
            GsxNode::Fragment { .. } => panic!("fragment"),
        }
    }

    /// §7.3: `on:click={handler}` survives as a directive with a closure.
    #[test]
    fn on_click_directive() {
        let (root, ds) = parse_gsx("<button on:click={|s, _| s.count += 1}>\"+\"</button>");
        assert!(ds.is_empty(), "{ds:?}");
        let n = root.unwrap();
        let attrs = n.attrs();
        assert_eq!(attrs.len(), 1);
        match &attrs[0] {
            GsxAttr::Directive(ns, sub, AttrValue::Expr(Expr::Closure { params, .. })) => {
                assert_eq!((ns.as_str(), sub.as_str()), ("on", "click"));
                assert_eq!(params, &vec!["s".to_string(), "_".to_string()]);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    /// Duplicate attributes are diagnosed (§5.2 schema lint family). Repeated
    /// `class` is exempt — G2 folds constant class strings.
    #[test]
    fn duplicate_attr_diagnosed() {
        let (root, ds) = parse_gsx("<input value={x} value={y}/>");
        assert!(root.is_some());
        assert!(ds.iter().any(|d| d.code == GsxCode::DuplicateAttr));
        let (_, ds2) = parse_gsx("<div class=\"a\" class=\"b\"/>");
        assert!(ds2.is_empty(), "class folding is legal: {ds2:?}");
    }

    /// Fragments and self-closing flags round-trip.
    #[test]
    fn fragment_and_self_closing() {
        let (r, _) = parse_gsx("<>\"a\"</>");
        assert!(matches!(r.unwrap(), GsxNode::Fragment { .. }));
        let (r, _) = parse_gsx("<hr/>");
        match r.unwrap() {
            GsxNode::Element {
                self_closing,
                children,
                ..
            } => {
                assert!(self_closing);
                assert!(children.is_empty());
            }
            _ => panic!(),
        }
    }

    /// The §3.1 Counter example converts completely.
    #[test]
    fn counter_example_converts() {
        let src = r#"<section class="counter" aria-label="Counter">
    <h1>"Hello, {s.name}!"</h1>
    <button on:click={|s, _| s.count -= s.step}>"−"</button>
    <output class:warn={s.count < 0}>"{s.count}"</output>
    <input bind:value={s.name} placeholder="Your name" />
    {if s.count > 10 { <p class="big">"That's a lot."</p> } else { <></> }}
    <ul>
        {for item in s.history.iter() by item.id { <li>"{item.label}"</li> }}
    </ul>
</section>"#;
        let (root, ds) = parse_gsx(src);
        assert!(ds.is_empty(), "{ds:?}");
        let n = root.expect("section");
        let kids = n.children();
        // h1, button, output, input, if-block, ul
        assert_eq!(kids.len(), 6, "children: {kids:?}");
    }

    /// G6 seeds: a `trusted(...)` hole keeps its call visible.
    #[test]
    fn trusted_html_hole() {
        let (root, _) = parse_gsx("<div>\"{trusted(s.html)}\"</div>");
        let n = root.unwrap();
        match &n.children()[0] {
            GsxChild::Text(pieces) => {
                assert!(
                    matches!(&pieces[0], TextPiece::Hole(Expr::Call(name, _)) if name == "trusted")
                );
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    /// Spread `{..props}` becomes a `..`-named attribute with an expression.
    #[test]
    fn spread_attr() {
        let (root, _) = parse_gsx("<Row {..props}/>");
        let n = root.unwrap();
        match &n.attrs()[0] {
            GsxAttr::Plain(name, AttrValue::Expr(Expr::Var(v))) => {
                assert_eq!(name, "..");
                assert_eq!(v, "props");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    /// Shorthand `{name}` = `name={name}` (§3.2).
    #[test]
    fn shorthand_attr() {
        let (root, _) = parse_gsx("<Row {name}/>");
        let n = root.unwrap();
        match &n.attrs()[0] {
            GsxAttr::Plain(name, AttrValue::Expr(e)) => {
                assert_eq!(name, "name");
                assert_eq!(*e, Expr::Var("name".to_string()));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    /// A static-only literal has exactly one Static piece (G7 chunking).
    #[test]
    fn pure_static_text() {
        assert_eq!(
            split_text_holes("Hello"),
            vec![TextPiece::Static("Hello".to_string())]
        );
        let (root, _) = parse_gsx("<h1>\"plain\"</h1>");
        let n = root.unwrap();
        assert_eq!(
            n.children(),
            &[GsxChild::Text(vec![TextPiece::Static("plain".to_string())])]
        );
        let _ = Lit::Unit; // keep Lit import used
    }
}
