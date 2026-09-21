//! Tests for slice/array projection code generation in the bytecode backend.
//!
//! Tier 5.4: `from_end` ConstantIndex on an *array* resolves to a
//! compile-time constant offset; on a *slice* the length is only known at
//! runtime, so the backend emits `OP_LEN; OP_SUB; OP_MUL; OP_ADD` to compute
//! `base + (runtime_len - offset) * elem_size`. This backend has no VM, so
//! golden-pattern (opcode-sequence) assertions are its verification
//! convention — see `discriminant_len.rs`.

use glyim_core::IndexVec;
use glyim_core::primitives::*;
use glyim_mir::*;
use glyim_span::Span;
use glyim_type::*;

use crate::{
    BytecodeBackend, LayoutProvider, OP_ADD, OP_LEN, OP_LOAD_CONST, OP_LOAD_LOCAL,
    OP_LOAD_LOCAL_ADDR, OP_MUL, OP_SUB,
};
use std::sync::Arc;

/// Fixed-size layout provider so emitted offsets are deterministic regardless
/// of the real layout computer.
struct TestLayoutProvider {
    sizes: std::collections::HashMap<Ty, u64>,
}

impl TestLayoutProvider {
    fn new() -> Self {
        Self {
            sizes: std::collections::HashMap::new(),
        }
    }
    fn with_size(mut self, ty: Ty, size: u64) -> Self {
        self.sizes.insert(ty, size);
        self
    }
}

impl LayoutProvider for TestLayoutProvider {
    fn field_offset(&self, _ty: Ty, _field_idx: FieldIdx) -> u64 {
        0
    }
    fn size_of(&self, ty: Ty) -> u64 {
        *self.sizes.get(&ty).unwrap_or(&8)
    }
    fn variant_type(&self, _enum_ty: Ty, _variant_idx: VariantIdx) -> Ty {
        Ty::ERROR
    }
    fn tag_offset(&self, _ty: Ty) -> u64 {
        0
    }
}

/// Build a frozen `TyCtx` holding `[i32; 4]` and `[i32]`, returning both types.
fn ctx_with_array_and_slice() -> (Arc<glyim_type::TyCtx>, Ty, Ty) {
    let mut ctx_mut = TyCtxMut::new(glyim_core::Interner::default());
    let elem = Ty::I32;
    let arr = ctx_mut.mk_ty(TyKind::Array(
        elem,
        Const {
            kind: ConstKind::Uint(4),
            ty: Ty::USIZE,
        },
    ));
    let slice = ctx_mut.mk_ty(TyKind::Slice(elem));
    (Arc::new(ctx_mut.freeze()), arr, slice)
}

#[test]
fn constant_index_array_from_end_is_constant_offset() {
    let (ctx, arr_ty, _slice) = ctx_with_array_and_slice();
    // elem_size is forced to 4 by the layout provider.
    let backend = BytecodeBackend::with_ty_ctx(ctx, glyim_core::TargetInfo::default())
        .with_layout_provider(Box::new(TestLayoutProvider::new().with_size(arr_ty, 4)));
    let local_tys = IndexVec::from_raw(vec![LocalDecl {
        ty: arr_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    }]);
    let place = Place {
        local: LocalIdx::from_raw(0),
        projection: Box::new([ProjectionElem::ConstantIndex {
            offset: 1,
            min_length: 0,
            from_end: true,
        }]),
    };

    let mut bc = Vec::new();
    backend
        .emit_place_address(&mut bc, &place, &local_tys)
        .expect("array from_end must resolve to a constant offset");

    // index_val = 4 - 1 = 3 ; byte_offset = 3 * 4 = 12.
    let mut expected = vec![OP_LOAD_LOCAL_ADDR, 0, 0, 0, 0, OP_LOAD_CONST];
    expected.extend_from_slice(&(12i64).to_le_bytes());
    expected.push(OP_ADD);
    assert_eq!(bc, expected);
}

#[test]
fn constant_index_slice_from_end_emits_runtime_len_sub() {
    let (ctx, _arr, slice_ty) = ctx_with_array_and_slice();
    // elem_size is forced to 4 by the layout provider for the slice type.
    let backend = BytecodeBackend::with_ty_ctx(ctx, glyim_core::TargetInfo::default())
        .with_layout_provider(Box::new(TestLayoutProvider::new().with_size(slice_ty, 4)));
    let local_tys = IndexVec::from_raw(vec![LocalDecl {
        ty: slice_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    }]);
    let place = Place {
        local: LocalIdx::from_raw(0),
        projection: Box::new([ProjectionElem::ConstantIndex {
            offset: 1,
            min_length: 0,
            from_end: true,
        }]),
    };

    let mut bc = Vec::new();
    backend
        .emit_place_address(&mut bc, &place, &local_tys)
        .expect("slice from_end emits runtime len subtraction, not an error");

    // The backend must read the slice's runtime length (OP_LEN) and subtract
    // the offset, scale by elem_size, then add to the base address.
    assert!(
        bc.contains(&OP_LEN),
        "missing OP_LEN (slice runtime length)"
    );
    assert!(
        bc.contains(&OP_SUB),
        "missing OP_SUB (runtime_len - offset)"
    );
    assert!(bc.contains(&OP_MUL), "missing OP_MUL (scaled by elem_size)");
    assert!(bc.contains(&OP_ADD), "missing final OP_ADD to base address");

    // The emitted sequence must start with the base address load and contain
    // the per-element arithmetic: push slice value, OP_LEN, push offset,
    // OP_SUB, push elem_size, OP_MUL, OP_ADD.
    let mut expected = vec![OP_LOAD_LOCAL_ADDR, 0, 0, 0, 0];
    expected.push(OP_LOAD_LOCAL);
    expected.extend_from_slice(&0u32.to_le_bytes());
    expected.push(OP_LEN);
    expected.push(OP_LOAD_CONST);
    expected.extend_from_slice(&1i64.to_le_bytes());
    expected.push(OP_SUB);
    expected.push(OP_LOAD_CONST);
    expected.extend_from_slice(&4i64.to_le_bytes());
    expected.push(OP_MUL);
    expected.push(OP_ADD);
    assert_eq!(bc, expected);
}
