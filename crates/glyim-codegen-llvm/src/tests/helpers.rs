//! Test helpers for constructing MIR bodies.

use glyim_core::arena::IndexVec;
use glyim_core::primitives::*;
use glyim_core::{CrateId, DefId, LocalDefId};
use glyim_mir::*;
use glyim_span::Span;
use glyim_type::Ty;

/// Builder for constructing MIR bodies in tests.
pub struct BodyBuilder {
    locals: IndexVec<LocalIdx, LocalDecl>,
    basic_blocks: IndexVec<BasicBlockIdx, BasicBlockData>,
    arg_count: usize,
    return_ty: Ty,
}

impl BodyBuilder {
    /// Create a new body builder with the given return type.
    pub fn new(return_ty: Ty) -> Self {
        let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
        locals.push(LocalDecl {
            ty: return_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        });
        let mut basic_blocks: IndexVec<BasicBlockIdx, BasicBlockData> = IndexVec::new();
        basic_blocks.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo::new(Span::DUMMY),
            },
            is_cleanup: false,
        });
        Self {
            locals,
            basic_blocks,
            arg_count: 0,
            return_ty,
        }
    }

    pub fn add_local(&mut self, ty: Ty) -> LocalIdx {
        let idx = LocalIdx::from_raw(self.locals.len() as u32);
        self.locals.push(LocalDecl {
            ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        });
        idx
    }

    pub fn add_arg(&mut self, ty: Ty) -> LocalIdx {
        let idx = self.add_local(ty);
        self.arg_count += 1;
        idx
    }

    pub fn add_statement(&mut self, stmt: Statement) {
        self.basic_blocks[BasicBlockIdx::from_raw(0)]
            .statements
            .push(stmt);
    }

    pub fn set_terminator(&mut self, term: Terminator) {
        self.basic_blocks[BasicBlockIdx::from_raw(0)].terminator = term;
    }

    pub fn add_block(&mut self, term: Terminator) -> BasicBlockIdx {
        let idx = BasicBlockIdx::from_raw(self.basic_blocks.len() as u32);
        self.basic_blocks.push(BasicBlockData {
            statements: vec![],
            terminator: term,
            is_cleanup: false,
        });
        idx
    }

    pub fn build(self) -> Body {
        Body {
            owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
            basic_blocks: self.basic_blocks,
            locals: self.locals,
            arg_count: self.arg_count,
            return_ty: self.return_ty,
            span: Span::DUMMY,
            var_debug_info: vec![],
        }
    }
}

pub fn make_string_const(name: glyim_core::Name, ty: Ty) -> MirConst {
    MirConst {
        kind: MirConstKind::String(name),
        ty,
        span: Span::DUMMY,
    }
}

pub fn make_assign(place: Place, rvalue: Rvalue) -> Statement {
    Statement {
        kind: StatementKind::Assign(place, rvalue),
        source_info: SourceInfo::new(Span::DUMMY),
    }
}

/// Create a constant i32/i64 operand.
pub fn const_operand_i32(value: impl Into<i128>, ty: Ty) -> Operand {
    Operand::Constant(MirConst {
        kind: MirConstKind::Int(value.into()),
        ty,
        span: Span::DUMMY,
    })
}

/// Create a constant u32 operand.
pub fn const_operand_u32(value: impl Into<u128>, ty: Ty) -> Operand {
    Operand::Constant(MirConst {
        kind: MirConstKind::Uint(value.into()),
        ty,
        span: Span::DUMMY,
    })
}

/// Create a constant f64 operand.
pub fn const_operand_f64(value: f64, ty: Ty) -> Operand {
    Operand::Constant(MirConst {
        kind: MirConstKind::FloatBits(value.to_bits()),
        ty,
        span: Span::DUMMY,
    })
}

/// Create a constant bool operand.
pub fn const_operand_bool(value: bool) -> Operand {
    Operand::Constant(MirConst {
        kind: MirConstKind::Bool(value),
        ty: Ty::BOOL,
        span: Span::DUMMY,
    })
}

/// Box a pair of operands for `BinaryOp`.
pub fn box_operands(lhs: Operand, rhs: Operand) -> Box<(Operand, Operand)> {
    Box::new((lhs, rhs))
}

/// Create a simple MIR body that computes a single rvalue and returns it.
pub fn simple_mir_body(return_ty: Ty, rvalue: Rvalue) -> Body {
    let mut builder = BodyBuilder::new(return_ty);
    let result_local = builder.add_local(return_ty);
    builder.add_statement(make_assign(Place::new(result_local), rvalue));
    builder.build()
}
