use crate::error::AssertionFailure;
use glyim_core::primitives::*;
use glyim_type::*;

/// assert_ty.
pub fn assert_ty<'a, L: TypeLookup>(lookup: &'a L, ty: Ty) -> TyAssert<'a, L> {
    TyAssert {
        lookup,
        ty,
        kind: lookup.ty_kind(ty).clone(),
    }
}

/// TyAssert.
pub struct TyAssert<'a, L: TypeLookup> {
    lookup: &'a L,
    ty: Ty,
    kind: TyKind,
}

impl<'a, L: TypeLookup> TyAssert<'a, L> {
    fn fail(&self, expected: &str) -> ! {
        panic!(
            "type assertion failed:\n  expected: {}\n  actual:   {}\n  TyKind:   {:?}",
            expected,
            PrintTy::new(self.ty, self.lookup),
            self.kind
        );
    }
    /// is_error.
    pub fn is_error(self) -> Self {
        if !matches!(self.kind, TyKind::Error) {
            self.fail("error type");
        }
        self
    }
    /// is_not_error.
    pub fn is_not_error(self) -> Self {
        if matches!(self.kind, TyKind::Error) {
            panic!("expected non-error");
        }
        self
    }
    /// is_never.
    pub fn is_never(self) -> Self {
        if !matches!(self.kind, TyKind::Never) {
            self.fail("never type");
        }
        self
    }
    /// is_bool.
    pub fn is_bool(self) -> Self {
        if !matches!(self.kind, TyKind::Bool) {
            self.fail("bool type");
        }
        self
    }
    /// is_unit.
    pub fn is_unit(self) -> Self {
        if !matches!(self.kind, TyKind::Unit) {
            self.fail("unit type");
        }
        self
    }
    /// is_int.
    pub fn is_int(self, expected: IntTy) -> Self {
        match &self.kind {
            TyKind::Int(i) if *i == expected => self,
            _ => self.fail(&format!("Int({:?})", expected)),
        }
    }
    /// is_any_int.
    pub fn is_any_int(self) -> Self {
        if !matches!(self.kind, TyKind::Int(_)) {
            self.fail("any Int");
        }
        self
    }
    /// is_uint.
    pub fn is_uint(self, expected: UintTy) -> Self {
        match &self.kind {
            TyKind::Uint(u) if *u == expected => self,
            _ => self.fail(&format!("Uint({:?})", expected)),
        }
    }
    /// is_float.
    pub fn is_float(self, expected: FloatTy) -> Self {
        match &self.kind {
            TyKind::Float(f) if *f == expected => self,
            _ => self.fail(&format!("Float({:?})", expected)),
        }
    }
    /// is_ref.
    pub fn is_ref(self, mutability: Mutability) -> TyAssert<'a, L> {
        match &self.kind {
            TyKind::Ref(_, inner, m) if *m == mutability => TyAssert {
                lookup: self.lookup,
                ty: *inner,
                kind: self.lookup.ty_kind(*inner).clone(),
            },
            _ => self.fail(&format!("&{} type", mutability.prefix_str().trim())),
        }
    }
    /// is_slice.
    pub fn is_slice(self) -> TyAssert<'a, L> {
        match &self.kind {
            TyKind::Slice(inner) => TyAssert {
                lookup: self.lookup,
                ty: *inner,
                kind: self.lookup.ty_kind(*inner).clone(),
            },
            _ => self.fail("slice type"),
        }
    }
    /// has_infer.
    pub fn has_infer(self) -> Self {
        if !self
            .lookup
            .ty_flags(self.ty)
            .contains(TypeFlags::HAS_TY_INFER)
        {
            self.fail("type with inference vars");
        }
        self
    }
    /// has_no_infer.
    pub fn has_no_infer(self) -> Self {
        if self
            .lookup
            .ty_flags(self.ty)
            .contains(TypeFlags::HAS_TY_INFER)
        {
            self.fail("fully resolved type");
        }
        self
    }
}

/// assert_ty_eq.
pub fn assert_ty_eq<L: TypeLookup>(ctx: &L, a: Ty, b: Ty) {
    assert_eq!(
        a,
        b,
        "types not equal: {} vs {}",
        PrintTy::new(a, ctx),
        PrintTy::new(b, ctx)
    );
}

/// check_ty.
pub fn check_ty<'a, L: TypeLookup>(lookup: &'a L, ty: Ty) -> TyCheck<'a, L> {
    TyCheck {
        lookup,
        ty,
        kind: lookup.ty_kind(ty).clone(),
        failures: Vec::new(),
    }
}

/// TyCheck.
pub struct TyCheck<'a, L: TypeLookup> {
    lookup: &'a L,
    ty: Ty,
    kind: TyKind,
    failures: Vec<AssertionFailure>,
}

impl<'a, L: TypeLookup> TyCheck<'a, L> {
    fn push_failure(&mut self, expected: &str) {
        self.failures.push(AssertionFailure {
            expected: expected.to_string(),
            actual: format!("{:?}", self.kind),
            ty_description: PrintTy::new(self.ty, self.lookup).to_string(),
        });
    }
    /// is_error.
    pub fn is_error(mut self) -> Self {
        if !matches!(self.kind, TyKind::Error) {
            self.push_failure("error type");
        }
        self
    }
    /// is_not_error.
    pub fn is_not_error(mut self) -> Self {
        if matches!(self.kind, TyKind::Error) {
            self.push_failure("non-error");
        }
        self
    }
    /// is_bool.
    pub fn is_bool(mut self) -> Self {
        if !matches!(self.kind, TyKind::Bool) {
            self.push_failure("bool");
        }
        self
    }
    /// is_unit.
    pub fn is_unit(mut self) -> Self {
        if !matches!(self.kind, TyKind::Unit) {
            self.push_failure("unit");
        }
        self
    }
    /// is_int.
    pub fn is_int(mut self, expected: IntTy) -> Self {
        match &self.kind {
            TyKind::Int(i) if *i == expected => {}
            _ => self.push_failure(&format!("Int({:?})", expected)),
        }
        self
    }
    /// is_any_int.
    pub fn is_any_int(mut self) -> Self {
        if !matches!(self.kind, TyKind::Int(_)) {
            self.push_failure("any Int");
        }
        self
    }
    /// is_float.
    pub fn is_float(mut self, expected: FloatTy) -> Self {
        match &self.kind {
            TyKind::Float(f) if *f == expected => {}
            _ => self.push_failure(&format!("Float({:?})", expected)),
        }
        self
    }
    /// is_ref.
    pub fn is_ref(mut self, mutability: Mutability) -> TyCheck<'a, L> {
        match &self.kind {
            TyKind::Ref(_, inner, m) if *m == mutability => TyCheck {
                lookup: self.lookup,
                ty: *inner,
                kind: self.lookup.ty_kind(*inner).clone(),
                failures: self.failures,
            },
            _ => {
                self.push_failure(&format!("&{} type", mutability.prefix_str().trim()));
                self
            }
        }
    }
    /// has_infer.
    pub fn has_infer(mut self) -> Self {
        if !self
            .lookup
            .ty_flags(self.ty)
            .contains(TypeFlags::HAS_TY_INFER)
        {
            self.push_failure("type with inference vars");
        }
        self
    }
    /// has_no_infer.
    pub fn has_no_infer(mut self) -> Self {
        if self
            .lookup
            .ty_flags(self.ty)
            .contains(TypeFlags::HAS_TY_INFER)
        {
            self.push_failure("fully resolved type");
        }
        self
    }
    /// finish.
    pub fn finish(self) -> Result<(), Vec<AssertionFailure>> {
        if self.failures.is_empty() {
            Ok(())
        } else {
            Err(self.failures)
        }
    }
}
