use glyim_diag::Span;
use glyim_interner::Symbol;

/// A reference to a type in the arena. O(1) Clone, Copy, Hash, Eq.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Ty(pub usize);

/// Reference to a comptime value in the VM heap.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ValueId(pub usize);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TyKind {
    // Primitives
    Int,
    Float,
    Bool,
    Str,
    Unit,
    Never,
    Error,
    Infer,

    // Nominal types
    Named(Symbol),
    App(Symbol, Vec<Ty>),
    Fn(Vec<Ty>, Ty),
    RawPtr(Ty),

    // V3: Staging
    Code(Ty),

    // V3: Const Generics
    Const(Ty, ValueId),

    // V3: Effects
    EffectFn(Vec<Ty>, Ty, EffectRow),

    // V3: Reflection
    Any,
    TypeInfo(Ty),
}

/// Tracks which algebraic effects a function may perform.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EffectRow {
    Empty,
    Extend(Symbol, Box<EffectRow>),
    /// Unification variable for effects (like Ty::Infer, but for rows).
    Var(u32),
}

pub struct TyArena {
    kinds: Vec<TyKind>,
    /// Maps Ty::Infer to the exact Span that created it.
    infer_spans: Vec<Span>,
}

impl TyArena {
    pub fn new() -> Self {
        Self {
            kinds: Vec::new(),
            infer_spans: Vec::new(),
        }
    }

    pub fn alloc(&mut self, kind: TyKind) -> Ty {
        let id = self.kinds.len();
        self.kinds.push(kind);
        Ty(id)
    }

    pub fn fresh_infer(&mut self, span: Span) -> Ty {
        let id = self.kinds.len();
        self.infer_spans.push(span);
        self.kinds.push(TyKind::Infer);
        Ty(id)
    }
    /// Set a type to Error (poison it after an occurs-check failure).
    pub fn poison(&mut self, ty: Ty) {
        self.kinds[ty.0] = TyKind::Error;
    }


    pub fn get(&self, ty: Ty) -> &TyKind {
        &self.kinds[ty.0]
    }

    pub fn get_infer_span(&self, ty: Ty) -> Option<Span> {
        if matches!(self.get(ty), TyKind::Infer) {
            self.infer_spans.get(ty.0).copied()
        } else {
            None
        }
    }

    pub fn len(&self) -> usize {
        self.kinds.len()
    }

    pub fn is_empty(&self) -> bool {
        self.kinds.is_empty()
    }
}

impl Default for TyArena {
    fn default() -> Self {
        Self::new()
    }
}
