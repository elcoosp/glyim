/// format.
pub mod format;

static SNAPSHOT_FILE_ID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1000);

fn next_snapshot_file_id() -> glyim_span::FileId {
    glyim_span::FileId::from_raw(
        SNAPSHOT_FILE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
    )
}

/// snapshot_cst.
pub fn snapshot_cst(name: &str, source: &str) {
    let file_id = next_snapshot_file_id();
    let result = glyim_frontend::parse_to_syntax(source, file_id);
    let tree = format!("{:#?}", result.root);
    insta::with_settings!({ snapshot_suffix => name }, {
        insta::assert_snapshot!(tree);
    });
}

/// snapshot_mir.
pub fn snapshot_mir(name: &str, ctx: &glyim_type::TyCtx, body: &glyim_mir::Body) {
    let formatted = format::format_mir_body(ctx, body);
    insta::with_settings!({ snapshot_suffix => name }, {
        insta::assert_snapshot!(formatted);
    });
}

/// Verbose variant (plan §24.3): annotates each place with its resolved type
/// and each local with its debug-info variable name. The default
/// `snapshot_mir` keeps the terse format for readability in the common case.
pub fn snapshot_mir_verbose(name: &str, ctx: &glyim_type::TyCtx, body: &glyim_mir::Body) {
    let formatted = format::format_mir_body_verbose(ctx, body);
    insta::with_settings!({ snapshot_suffix => name }, {
        insta::assert_snapshot!(formatted);
    });
}

/// snapshot_def_map.
pub fn snapshot_def_map(name: &str, def_map: &glyim_def_map::CrateDefMap) {
    let formatted = format::format_def_map(def_map);
    insta::with_settings!({ snapshot_suffix => name }, {
        insta::assert_snapshot!(formatted);
    });
}
// Snapshot path normalization: ensure tests use relative paths instead of absolute.

#[cfg(test)]
mod tests {
    use glyim_core::primitives::{IntTy, Mutability};
    use glyim_core::{CrateId, DefId, IndexVec, LocalDefId};
    use glyim_mir::{
        BasicBlockData, Body, LocalDecl, LocalIdx, MirConst, MirConstKind, Operand, Place,
        SourceInfo, Statement, StatementKind, Terminator, TerminatorKind, VarDebugInfo,
        VarDebugInfoValue,
    };
    use glyim_span::Span;
    use glyim_type::TyKind;

    fn def_id() -> DefId {
        DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0))
    }

    #[test]
    fn verbose_mir_includes_resolved_types_and_debug_names() {
        let (ctx, i32_ty) = crate::with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Int(IntTy::I32)));

        let mut body = Body::dummy(def_id());
        let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
        locals.push(LocalDecl {
            ty: i32_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        });
        body.locals = locals;
        // Debug info: variable named "x" bound to local 0.
        let name = super::format::intern_name("x");
        body.var_debug_info.push(VarDebugInfo {
            name,
            value: VarDebugInfoValue::Place(Place::new(LocalIdx::from_raw(0))),
        });
        // A statement assigning a constant to local 0, so verbose output
        // annotates the place with its resolved type.
        let bb = BasicBlockData {
            statements: vec![Statement {
                kind: StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(0)),
                    glyim_mir::Rvalue::Use(Operand::Constant(MirConst {
                        kind: MirConstKind::Uint(0),
                        ty: i32_ty,
                        span: Span::DUMMY,
                    })),
                ),
                source_info: SourceInfo::new(Span::DUMMY),
            }],
            terminator: Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo::new(Span::DUMMY),
            },
            is_cleanup: false,
        };
        body.basic_blocks = glyim_core::IndexVec::from_raw(vec![bb]);

        let terse = super::format::format_mir_body(&ctx, &body);
        let verbose = super::format::format_mir_body_verbose(&ctx, &body);

        // Default format has no debug variable names and no inline place-type
        // annotations on statements/terminators.
        assert!(
            !terse.contains("/* x */"),
            "terse format must not show debug names"
        );
        assert!(
            !terse.contains("$0: i32 ="),
            "terse format must not annotate places inline with types"
        );
        assert!(
            terse.contains("$0 = Use(Const(uint(0)))"),
            "terse statement shape"
        );

        // Verbose format annotates the debug name, the local type, and inline
        // place types on the statement.
        assert!(
            verbose.contains("/* x */"),
            "verbose must show debug name /* x */"
        );
        assert!(
            verbose.contains("$0: i32 = Use(Const(uint(0)))"),
            "verbose must annotate the place with its resolved type"
        );
        assert!(verbose.contains("bb0:"));
    }
}
