//! Helper module for constructing MIR bodies in borrowck tests.

use glyim_core::{CrateId, DefId, IndexVec, LocalDefId, Mutability};
use glyim_mir::*;
use glyim_span::Span;
use glyim_type::Ty;

/// Create a dummy `DefId` for test bodies.
pub fn dummy_def_id() -> DefId {
    DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0))
}

/// Builder for constructing MIR bodies for borrowck tests.
pub struct MirBodyBuilder {
    locals: Vec<LocalDecl>,
    basic_blocks: Vec<BasicBlockData>,
    arg_count: usize,
    return_ty: Ty,
}

impl MirBodyBuilder {
    /// Create a new builder. The return type is used for local 0 (the return place).
    pub fn new(return_ty: Ty) -> Self {
        Self {
            locals: vec![LocalDecl {
                ty: return_ty,
                mutability: Mutability::Not,
                source_info: SourceInfo::new(Span::DUMMY),
            }],
            basic_blocks: Vec::new(),
            arg_count: 0,
            return_ty,
        }
    }

    /// Add a new local variable. Returns its `LocalIdx`.
    pub fn add_local(&mut self, ty: Ty, mutability: Mutability) -> LocalIdx {
        let idx = LocalIdx::from_raw(self.locals.len() as u32);
        self.locals.push(LocalDecl {
            ty,
            mutability,
            source_info: SourceInfo::new(Span::DUMMY),
        });
        idx
    }

    /// Add a new basic block with the given terminator. Returns its `BasicBlockIdx`.
    pub fn push_block(&mut self, terminator: Terminator) -> BasicBlockIdx {
        let idx = BasicBlockIdx::from_raw(self.basic_blocks.len() as u32);
        self.basic_blocks.push(BasicBlockData::new(terminator));
        idx
    }

    /// Push a statement onto the given basic block.
    pub fn push_stmt(&mut self, block: BasicBlockIdx, kind: StatementKind) {
        self.basic_blocks[block.to_raw() as usize]
            .statements
            .push(Statement {
                kind,
                source_info: SourceInfo::new(Span::DUMMY),
            });
    }

    /// Consume the builder and produce the final MIR Body.
    pub fn build(self) -> Body {
        Body {
            owner: dummy_def_id(),
            basic_blocks: IndexVec::<BasicBlockIdx, _>::from_raw(self.basic_blocks),
            locals: IndexVec::<LocalIdx, _>::from_raw(self.locals),
            arg_count: self.arg_count,
            return_ty: self.return_ty,
            span: Span::DUMMY,
            var_debug_info: Vec::new(),
        }
    }
}

/// Helper: create a Goto terminator targeting the given block index.
pub fn goto(target: u32) -> Terminator {
    Terminator {
        kind: TerminatorKind::Goto {
            target: BasicBlockIdx::from_raw(target),
        },
        source_info: SourceInfo::new(Span::DUMMY),
    }
}

/// Helper: create a Return terminator.
pub fn ret() -> Terminator {
    Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    }
}

/// Helper: create a `SwitchInt` terminator for an if-else branch.
pub fn if_switch(discr: LocalIdx, switch_ty: Ty, then_bb: u32, else_bb: u32) -> Terminator {
    Terminator {
        kind: TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(discr)),
            switch_ty,
            targets: SwitchTargets::if_switch(
                BasicBlockIdx::from_raw(then_bb),
                BasicBlockIdx::from_raw(else_bb),
            ),
        },
        source_info: SourceInfo::new(Span::DUMMY),
    }
}

/// Helper: create an Assign statement that creates a borrow.
pub fn assign_borrow(dest: LocalIdx, borrowed: LocalIdx, kind: BorrowKind) -> StatementKind {
    StatementKind::Assign(Place::new(dest), Rvalue::Ref(Place::new(borrowed), kind))
}

/// Helper: create an Assign statement that copies a value.
pub fn assign_copy(dest: LocalIdx, src: LocalIdx) -> StatementKind {
    StatementKind::Assign(
        Place::new(dest),
        Rvalue::Use(Operand::Copy(Place::new(src))),
    )
}

/// A local mock implementing `BorrowckCtx` for tests.
///
/// We implement this inside `glyim-borrowck` itself to avoid the
/// "multiple versions of crate" problem that occurs when `glyim-test`
/// depends on a separate copy of `glyim-borrowck`.
pub struct TestBorrowckCtx<'a> {
    ty_ctx: &'a glyim_type::TyCtx,
    body: &'a Body,
}

impl<'a> TestBorrowckCtx<'a> {
    pub fn new(ty_ctx: &'a glyim_type::TyCtx, body: &'a Body) -> Self {
        Self { ty_ctx, body }
    }
}

impl crate::BorrowckCtx for TestBorrowckCtx<'_> {
    fn ty_ctx(&self) -> &glyim_type::TyCtx {
        self.ty_ctx
    }

    fn local_decl(&self, local: glyim_mir::LocalIdx) -> &glyim_mir::LocalDecl {
        &self.body.locals[local]
    }

    fn is_copy(&self, _ty: glyim_type::Ty) -> bool {
        false
    }
    fn local_name(&self, idx: LocalIdx) -> String {
        format!("_{}", idx.to_raw())
    }
}
