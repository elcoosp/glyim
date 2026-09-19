use glyim_core::Interner;
use glyim_core::primitives::Mutability;
use glyim_hir::{Body, CrateHir, Expr, ExprId, ItemKind, Pat};
use glyim_span::{FileId, Span};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
/// Reference.
pub struct Reference {
/// Struct.
    pub file_id: FileId,
/// Struct.
    pub span: Span,
/// Struct.
    pub is_definition: bool,
/// Struct.
    pub kind: ReferenceKind,
    /// Read/write access. A reference is `Write` when it is the direct LHS of an
    /// `Expr::Assign` or the operand of a `&mut` borrow; everything else is a
    /// `Read`. Mirrors Tier 1.1's `is_mut_use` classification.
    pub access: AccessKind,
/// Struct.
    pub def_id: Option<glyim_core::def_id::DefId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// AccessKind.
pub enum AccessKind {
/// Variant.
    Read,
/// Variant.
    Write,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// ReferenceKind.
pub enum ReferenceKind {
/// Variant.
    Call,
/// Variant.
    TypeReference,
/// Variant.
    FieldAccess,
/// Variant.
    Constructor,
/// Variant.
    Pattern,
/// Variant.
    Definition,
/// Variant.
    Variable,
}

/// ReferenceGraph.
pub struct ReferenceGraph {
    references: HashMap<String, Vec<Reference>>,
    function_names: HashSet<String>,
}

impl Default for ReferenceGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl ReferenceGraph {
/// new.
    pub fn new() -> Self {
        Self {
            references: HashMap::new(),
            function_names: HashSet::new(),
        }
    }

/// build_from_hir.
    pub fn build_from_hir(&mut self, file_id: FileId, hir: &CrateHir, interner: &Interner) {
        self.references
            .retain(|_, refs| refs.iter().all(|r| r.file_id != file_id));

        for item in hir.items.iter() {
            if let ItemKind::Fn(_) = &item.kind {
                let name = interner.resolve(item.name).to_string();
                self.function_names.insert(name);
            }
        }

        let mut seen = HashSet::new();
        let function_names = &self.function_names;

        let mut add_ref =
            |name: &str, span: Span, is_def: bool, kind: ReferenceKind, access: AccessKind| {
                let key = (
                    name.to_string(),
                    file_id,
                    span.lo.to_usize(),
                    span.hi.to_usize(),
                    kind,
                    access,
                );
                if seen.insert(key) {
                    self.references
                        .entry(name.to_string())
                        .or_default()
                        .push(Reference {
                            file_id,
                            span,
                            is_definition: is_def,
                            kind,
                            access,
                            def_id: None,
                        });
                }
            };

        for item in hir.items.iter() {
            let name = interner.resolve(item.name).to_string();
            add_ref(
                &name,
                item.span,
                true,
                ReferenceKind::Definition,
                AccessKind::Read,
            );

            if let ItemKind::Fn(fn_item) = &item.kind {
                for param in &fn_item.params {
                    let param_name = interner.resolve(param.name).to_string();
                    add_ref(
                        &param_name,
                        param.span,
                        true,
                        ReferenceKind::Definition,
                        AccessKind::Read,
                    );
                }
            }
            if let ItemKind::Struct(struct_item) = &item.kind {
                for field in &struct_item.fields {
                    let field_name = interner.resolve(field.name).to_string();
                    add_ref(
                        &field_name,
                        field.span,
                        true,
                        ReferenceKind::Definition,
                        AccessKind::Read,
                    );
                }
            }
            if let ItemKind::Enum(enum_item) = &item.kind {
                for variant in &enum_item.variants {
                    let variant_name = interner.resolve(variant.name).to_string();
                    add_ref(
                        &variant_name,
                        variant.span,
                        true,
                        ReferenceKind::Definition,
                        AccessKind::Read,
                    );
                }
            }
        }

        fn walk_pattern(
            pat_id: glyim_hir::PatId,
            body: &Body,
            interner: &Interner,
            add_ref: &mut impl FnMut(&str, Span, bool, ReferenceKind, AccessKind),
            access: AccessKind,
        ) {
            let pat = &body.pats[pat_id];
            match pat {
                Pat::Binding { name, .. } => {
                    let name_str = interner.resolve(*name).to_string();
                    add_ref(
                        &name_str,
                        Span::DUMMY,
                        true,
                        ReferenceKind::Definition,
                        access,
                    );
                }
                Pat::Struct { fields, .. } => {
                    for (_, pat_id) in fields {
                        walk_pattern(*pat_id, body, interner, add_ref, access);
                    }
                }
                Pat::Tuple(pats) | Pat::Or(pats) => {
                    for p in pats {
                        walk_pattern(*p, body, interner, add_ref, access);
                    }
                }
                _ => {}
            }
        }

        fn extract_path_name(expr_id: ExprId, body: &Body, interner: &Interner) -> Option<String> {
            let expr = &body.exprs[expr_id];
            match expr {
                Expr::Path(path) => path
                    .as_name()
                    .map(|name| interner.resolve(name).to_string()),
                Expr::Ref { expr, .. } => extract_path_name(*expr, body, interner),
                Expr::Unary { expr, .. } => extract_path_name(*expr, body, interner),
                Expr::Field { receiver, .. } => extract_path_name(*receiver, body, interner),
                _ => None,
            }
        }

        fn walk_expr(
            expr_id: ExprId,
            body: &Body,
            interner: &Interner,
            _file_id: FileId,
            add_ref: &mut impl FnMut(&str, Span, bool, ReferenceKind, AccessKind),
            function_names: &HashSet<String>,
            in_call_func: bool,
            access: AccessKind,
        ) {
            let expr = &body.exprs[expr_id];
            let span = body.expr_spans.get(expr_id).copied().unwrap_or(Span::DUMMY);

            match expr {
                Expr::Path(path) => {
                    if let Some(name) = path.as_name() {
                        let name_str = interner.resolve(name).to_string();
                        if !in_call_func && !function_names.contains(&name_str) {
                            add_ref(&name_str, span, false, ReferenceKind::Variable, access);
                        }
                    }
                }
                Expr::Call { func, args } => {
                    if let Some(name) = extract_path_name(*func, body, interner) {
                        add_ref(&name, span, false, ReferenceKind::Call, AccessKind::Read);
                    }
                    walk_expr(
                        *func,
                        body,
                        interner,
                        _file_id,
                        add_ref,
                        function_names,
                        true,
                        AccessKind::Read,
                    );
                    for arg in args {
                        walk_expr(
                            *arg,
                            body,
                            interner,
                            _file_id,
                            add_ref,
                            function_names,
                            false,
                            AccessKind::Read,
                        );
                    }
                }
                Expr::MethodCall {
                    receiver,
                    method,
                    args,
                    ..
                } => {
                    walk_expr(
                        *receiver,
                        body,
                        interner,
                        _file_id,
                        add_ref,
                        function_names,
                        false,
                        AccessKind::Read,
                    );
                    let method_str = interner.resolve(*method).to_string();
                    add_ref(
                        &method_str,
                        span,
                        false,
                        ReferenceKind::Call,
                        AccessKind::Read,
                    );
                    for arg in args {
                        walk_expr(
                            *arg,
                            body,
                            interner,
                            _file_id,
                            add_ref,
                            function_names,
                            false,
                            AccessKind::Read,
                        );
                    }
                }
                Expr::Field { receiver, field } => {
                    walk_expr(
                        *receiver,
                        body,
                        interner,
                        _file_id,
                        add_ref,
                        function_names,
                        false,
                        AccessKind::Read,
                    );
                    let field_str = interner.resolve(*field).to_string();
                    add_ref(
                        &field_str,
                        span,
                        false,
                        ReferenceKind::FieldAccess,
                        AccessKind::Read,
                    );
                }
                Expr::Binary { lhs, rhs, .. } => {
                    walk_expr(
                        *lhs,
                        body,
                        interner,
                        _file_id,
                        add_ref,
                        function_names,
                        false,
                        AccessKind::Read,
                    );
                    walk_expr(
                        *rhs,
                        body,
                        interner,
                        _file_id,
                        add_ref,
                        function_names,
                        false,
                        AccessKind::Read,
                    );
                }
                Expr::Unary { expr, .. } => {
                    walk_expr(
                        *expr,
                        body,
                        interner,
                        _file_id,
                        add_ref,
                        function_names,
                        false,
                        AccessKind::Read,
                    );
                }
                Expr::Block { stmts, tail } => {
                    for stmt in stmts {
                        walk_expr(
                            *stmt,
                            body,
                            interner,
                            _file_id,
                            add_ref,
                            function_names,
                            false,
                            AccessKind::Read,
                        );
                    }
                    if let Some(tail_expr) = tail {
                        walk_expr(
                            *tail_expr,
                            body,
                            interner,
                            _file_id,
                            add_ref,
                            function_names,
                            false,
                            AccessKind::Read,
                        );
                    }
                }
                Expr::If {
                    cond,
                    then_branch,
                    else_branch,
                } => {
                    walk_expr(
                        *cond,
                        body,
                        interner,
                        _file_id,
                        add_ref,
                        function_names,
                        false,
                        AccessKind::Read,
                    );
                    walk_expr(
                        *then_branch,
                        body,
                        interner,
                        _file_id,
                        add_ref,
                        function_names,
                        false,
                        AccessKind::Read,
                    );
                    if let Some(else_expr) = else_branch {
                        walk_expr(
                            *else_expr,
                            body,
                            interner,
                            _file_id,
                            add_ref,
                            function_names,
                            false,
                            AccessKind::Read,
                        );
                    }
                }
                Expr::Match { scrutinee, arms } => {
                    walk_expr(
                        *scrutinee,
                        body,
                        interner,
                        _file_id,
                        add_ref,
                        function_names,
                        false,
                        AccessKind::Read,
                    );
                    for arm in arms {
                        walk_pattern(arm.pat, body, interner, add_ref, AccessKind::Read);
                        if let Some(guard) = arm.guard {
                            walk_expr(
                                guard,
                                body,
                                interner,
                                _file_id,
                                add_ref,
                                function_names,
                                false,
                                AccessKind::Read,
                            );
                        }
                        walk_expr(
                            arm.body,
                            body,
                            interner,
                            _file_id,
                            add_ref,
                            function_names,
                            false,
                            AccessKind::Read,
                        );
                    }
                }
                Expr::Return { value: Some(val) } => {
                    walk_expr(
                        *val,
                        body,
                        interner,
                        _file_id,
                        add_ref,
                        function_names,
                        false,
                        AccessKind::Read,
                    );
                }
                Expr::Return { value: None } => {}
                Expr::Break { value: Some(val) } => {
                    walk_expr(
                        *val,
                        body,
                        interner,
                        _file_id,
                        add_ref,
                        function_names,
                        false,
                        AccessKind::Read,
                    );
                }
                Expr::Break { value: None } => {}
                Expr::Let { pat, value, .. } => {
                    // The pattern of a `let` introduces a *write* to the bound
                    // variable (mirrors the `Expr::Assign` LHS write tracking).
                    if let Pat::Binding { name, .. } = &body.pats[*pat] {
                        let name_str = interner.resolve(*name).to_string();
                        add_ref(
                            &name_str,
                            span,
                            true,
                            ReferenceKind::Variable,
                            AccessKind::Write,
                        );
                    }
                    // Walk the initializer only (not the pattern to avoid
                    // duplicate uses of the bound name).
                    walk_expr(
                        *value,
                        body,
                        interner,
                        _file_id,
                        add_ref,
                        function_names,
                        false,
                        AccessKind::Read,
                    );
                }
                Expr::Assign { lhs, rhs } => {
                    if let Expr::Path(path) = &body.exprs[*lhs]
                        && let Some(name) = path.as_name()
                    {
                        let name_str = interner.resolve(name).to_string();
                        // The direct LHS of an assignment is a *write* to that
                        // variable (mirrors Tier 1.1's is_mut_use write tracking).
                        add_ref(
                            &name_str,
                            span,
                            true,
                            ReferenceKind::Variable,
                            AccessKind::Write,
                        );
                    }
                    // Walk RHS only (not LHS to avoid duplicate use)
                    walk_expr(
                        *rhs,
                        body,
                        interner,
                        _file_id,
                        add_ref,
                        function_names,
                        false,
                        AccessKind::Read,
                    );
                }
                Expr::Loop { body: loop_body } => {
                    walk_expr(
                        *loop_body,
                        body,
                        interner,
                        _file_id,
                        add_ref,
                        function_names,
                        false,
                        AccessKind::Read,
                    );
                }
                Expr::While {
                    cond,
                    body: loop_body,
                } => {
                    walk_expr(
                        *cond,
                        body,
                        interner,
                        _file_id,
                        add_ref,
                        function_names,
                        false,
                        AccessKind::Read,
                    );
                    walk_expr(
                        *loop_body,
                        body,
                        interner,
                        _file_id,
                        add_ref,
                        function_names,
                        false,
                        AccessKind::Read,
                    );
                }
                Expr::For {
                    pat,
                    iterable,
                    body: loop_body,
                } => {
                    walk_pattern(*pat, body, interner, add_ref, AccessKind::Read);
                    walk_expr(
                        *iterable,
                        body,
                        interner,
                        _file_id,
                        add_ref,
                        function_names,
                        false,
                        AccessKind::Read,
                    );
                    walk_expr(
                        *loop_body,
                        body,
                        interner,
                        _file_id,
                        add_ref,
                        function_names,
                        false,
                        AccessKind::Read,
                    );
                }
                Expr::Struct { fields, spread, .. } => {
                    for (_, field_expr) in fields {
                        walk_expr(
                            *field_expr,
                            body,
                            interner,
                            _file_id,
                            add_ref,
                            function_names,
                            false,
                            AccessKind::Read,
                        );
                    }
                    if let Some(spread_expr) = spread {
                        walk_expr(
                            *spread_expr,
                            body,
                            interner,
                            _file_id,
                            add_ref,
                            function_names,
                            false,
                            AccessKind::Read,
                        );
                    }
                }
                Expr::Array(elems) | Expr::Tuple(elems) => {
                    for elem in elems {
                        walk_expr(
                            *elem,
                            body,
                            interner,
                            _file_id,
                            add_ref,
                            function_names,
                            false,
                            AccessKind::Read,
                        );
                    }
                }
                Expr::Closure {
                    params,
                    body: closure_body,
                    is_move: _,
                } => {
                    for param in params {
                        walk_pattern(*param, body, interner, add_ref, AccessKind::Read);
                    }
                    walk_expr(
                        *closure_body,
                        body,
                        interner,
                        _file_id,
                        add_ref,
                        function_names,
                        false,
                        AccessKind::Read,
                    );
                }
                Expr::Index { base, index } => {
                    walk_expr(
                        *base,
                        body,
                        interner,
                        _file_id,
                        add_ref,
                        function_names,
                        false,
                        AccessKind::Read,
                    );
                    walk_expr(
                        *index,
                        body,
                        interner,
                        _file_id,
                        add_ref,
                        function_names,
                        false,
                        AccessKind::Read,
                    );
                }
                Expr::Range { start, end, .. } => {
                    if let Some(start_expr) = start {
                        walk_expr(
                            *start_expr,
                            body,
                            interner,
                            _file_id,
                            add_ref,
                            function_names,
                            false,
                            AccessKind::Read,
                        );
                    }
                    if let Some(end_expr) = end {
                        walk_expr(
                            *end_expr,
                            body,
                            interner,
                            _file_id,
                            add_ref,
                            function_names,
                            false,
                            AccessKind::Read,
                        );
                    }
                }
                Expr::Cast { expr, .. } => {
                    walk_expr(
                        *expr,
                        body,
                        interner,
                        _file_id,
                        add_ref,
                        function_names,
                        false,
                        AccessKind::Read,
                    );
                }
                Expr::Ref { expr, mutability } => {
                    // A `&mut x` borrow is a write to `x` (mirrors Tier 1.1's
                    // is_mut_use classification); `&x` is a read borrow.
                    let operand_access = if *mutability == Mutability::Mut {
                        AccessKind::Write
                    } else {
                        AccessKind::Read
                    };
                    walk_expr(
                        *expr,
                        body,
                        interner,
                        _file_id,
                        add_ref,
                        function_names,
                        false,
                        operand_access,
                    );
                }
                // These variants carry no sub-expressions to recurse into, so a
                // no-op traversal is the correct (exhaustive) handling. Listing
                // them explicitly — instead of a `_ => {}` wildcard — means a
                // future `Expr` variant that DOES carry children will be a
                // compile error here, forcing a real traversal to be written
                // (Phase 8.1, unstub-5).
                Expr::Missing | Expr::Literal(_) | Expr::Continue | Expr::Err => {}
            Expr::Await { expr } | Expr::Try { expr } => {
                walk_expr(*expr, body, interner, _file_id, add_ref, function_names, false, AccessKind::Read);
            }
            }
        }

        for (_, body) in hir.bodies.iter_enumerated() {
            for param in &body.params {
                walk_pattern(*param, body, interner, &mut add_ref, AccessKind::Read);
            }
            for (expr_id, _) in body.exprs.iter_enumerated() {
                walk_expr(
                    expr_id,
                    body,
                    interner,
                    file_id,
                    &mut add_ref,
                    function_names,
                    false,
                    AccessKind::Read,
                );
            }
        }
    }

/// find_references.
    pub fn find_references(&self, symbol_name: &str) -> &[Reference] {
        self.references
            .get(symbol_name)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Every symbol name that has at least one recorded reference (definition
    /// or use). Since every `Reference` is either a `Read` or a `Write`
    /// (see `AccessKind`), a name absent from this set has no reads and no
    /// writes anywhere in the indexed HIR — i.e. it is unused. Used by the
    /// unused-import code action (Tier 6.5) to replace the old text-substring
    /// heuristic, which false-positived on shadowed names and false-negatived
    /// on names that appear only inside strings/comments.
    pub fn used_symbols(&self) -> HashSet<String> {
        self.references.keys().cloned().collect()
    }

    #[doc(hidden)]
    pub fn insert_test_reference(&mut self, name: &str, reference: Reference) {
        self.references
            .entry(name.to_string())
            .or_default()
            .push(reference);
    }
}
