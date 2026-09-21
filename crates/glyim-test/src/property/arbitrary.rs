use glyim_core::primitives::*;
use glyim_solve::InferenceTable;
use glyim_type::*;
use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};

/// Generator.
pub struct Generator {
    rng: StdRng,
    max_depth: u32,
}

impl Generator {
    /// new.
    pub fn new(seed: u64) -> Self {
        Self {
            rng: StdRng::seed_from_u64(seed),
            max_depth: 4,
        }
    }
    /// with_max_depth.
    pub fn with_max_depth(mut self, depth: u32) -> Self {
        self.max_depth = depth;
        self
    }

    /// generate_ty.
    pub fn generate_ty(&mut self, ctx: &mut TyCtxMut, depth: u32) -> Ty {
        if depth >= self.max_depth {
            return self.leaf_ty(ctx);
        }
        match self.rng.random_range(0..8) {
            0 => ctx.bool_ty(),
            1 => ctx.never_ty(),
            2 => ctx.unit_ty(),
            3 => ctx.mk_ty(TyKind::Int(self.int_ty())),
            4 => ctx.mk_ty(TyKind::Uint(self.uint_ty())),
            5 => ctx.mk_ty(TyKind::Float(self.float_ty())),
            6 => {
                let inner = self.generate_ty(ctx, depth + 1);
                ctx.mk_ref(Region::Erased, inner, self.mutability())
            }
            7 => {
                let inner = self.generate_ty(ctx, depth + 1);
                ctx.mk_ty(TyKind::Slice(inner))
            }
            _ => self.leaf_ty(ctx),
        }
    }

    /// generate_ty_with_infer.
    pub fn generate_ty_with_infer(
        &mut self,
        ctx: &mut TyCtxMut,
        infer: &mut InferenceTable,
        depth: u32,
    ) -> Ty {
        if depth >= self.max_depth {
            return self.leaf_ty_with_infer(ctx, infer);
        }
        match self.rng.random_range(0..11) {
            0..=7 => self.generate_ty(ctx, depth),
            8 => {
                let var = infer.new_ty_var(ctx);
                ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)))
            }
            9 => {
                let var = infer.new_int_var(ctx);
                ctx.mk_ty(TyKind::Infer(InferVar::Int(var)))
            }
            10 => {
                let var = infer.new_float_var(ctx);
                ctx.mk_ty(TyKind::Infer(InferVar::Float(var)))
            }
            _ => unreachable!(),
        }
    }

    fn leaf_ty(&mut self, ctx: &mut TyCtxMut) -> Ty {
        match self.rng.random_range(0..4) {
            0 => ctx.bool_ty(),
            1 => ctx.unit_ty(),
            2 => ctx.mk_ty(TyKind::Int(self.int_ty())),
            _ => ctx.mk_ty(TyKind::Uint(self.uint_ty())),
        }
    }

    fn leaf_ty_with_infer(&mut self, ctx: &mut TyCtxMut, infer: &mut InferenceTable) -> Ty {
        match self.rng.random_range(0..6) {
            0 => ctx.bool_ty(),
            1 => ctx.unit_ty(),
            2 => ctx.mk_ty(TyKind::Int(self.int_ty())),
            3 => {
                let var = infer.new_ty_var(ctx);
                ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)))
            }
            4 => {
                let var = infer.new_int_var(ctx);
                ctx.mk_ty(TyKind::Infer(InferVar::Int(var)))
            }
            5 => {
                let var = infer.new_float_var(ctx);
                ctx.mk_ty(TyKind::Infer(InferVar::Float(var)))
            }
            _ => unreachable!(),
        }
    }

    fn int_ty(&mut self) -> IntTy {
        match self.rng.random_range(0..5) {
            0 => IntTy::I8,
            1 => IntTy::I16,
            2 => IntTy::I32,
            3 => IntTy::I64,
            _ => IntTy::Isize,
        }
    }
    fn uint_ty(&mut self) -> UintTy {
        match self.rng.random_range(0..5) {
            0 => UintTy::U8,
            1 => UintTy::U16,
            2 => UintTy::U32,
            3 => UintTy::U64,
            _ => UintTy::Usize,
        }
    }
    fn float_ty(&mut self) -> FloatTy {
        if self.rng.random_bool(0.5) {
            FloatTy::F32
        } else {
            FloatTy::F64
        }
    }
    fn mutability(&mut self) -> Mutability {
        if self.rng.random_bool(0.5) {
            Mutability::Mut
        } else {
            Mutability::Not
        }
    }
}

/// sentinel_invariant.
pub fn sentinel_invariant(ctx: &TyCtx) {
    assert!(matches!(ctx.ty_kind(Ty::ERROR), TyKind::Error));
    assert!(matches!(ctx.ty_kind(Ty::NEVER), TyKind::Never));
    assert!(matches!(ctx.ty_kind(Ty::UNIT), TyKind::Unit));
    assert!(matches!(ctx.ty_kind(Ty::BOOL), TyKind::Bool));
}
