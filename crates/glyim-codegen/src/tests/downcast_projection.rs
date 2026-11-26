//! Tests for enum downcast projection in codegen.
//! Verifies that `ProjectionElem::Downcast` emits no instructions
//! and that subsequent field projections use the correct type view.

use glyim_core::IndexVec;
use glyim_core::primitives::*;
use glyim_mir::*;
use glyim_span::Span;
use glyim_type::*;

use crate::{BytecodeBackend, LayoutProvider};

/// A layout provider that returns fixed offsets for testing.
struct TestLayoutProvider {
    offsets: std::collections::HashMap<(Ty, FieldIdx), u64>,
    sizes: std::collections::HashMap<Ty, u64>,
    variant_types: std::collections::HashMap<(Ty, VariantIdx), Ty>,
    tag_offsets: std::collections::HashMap<Ty, u64>,
}

impl TestLayoutProvider {
    fn new() -> Self {
        Self {
            offsets: std::collections::HashMap::new(),
            sizes: std::collections::HashMap::new(),
            variant_types: std::collections::HashMap::new(),
            tag_offsets: std::collections::HashMap::new(),
        }
    }

    fn with_field_offset(mut self, ty: Ty, idx: FieldIdx, offset: u64) -> Self {
        self.offsets.insert((ty, idx), offset);
        self
    }

    fn with_size(mut self, ty: Ty, size: u64) -> Self {
        self.sizes.insert(ty, size);
        self
    }

    fn with_variant_type(mut self, enum_ty: Ty, variant_idx: VariantIdx, variant_ty: Ty) -> Self {
        self.variant_types
            .insert((enum_ty, variant_idx), variant_ty);
        self
    }

    /// Set the discriminant tag byte-offset for a (direct-tagged) enum type.
    fn with_tag_offset(mut self, ty: Ty, offset: u64) -> Self {
        self.tag_offsets.insert(ty, offset);
        self
    }
}

impl LayoutProvider for TestLayoutProvider {
    fn field_offset(&self, ty: Ty, field_idx: FieldIdx) -> u64 {
        *self.offsets.get(&(ty, field_idx)).unwrap_or(&0)
    }
    fn size_of(&self, ty: Ty) -> u64 {
        *self.sizes.get(&ty).unwrap_or(&8)
    }
    fn variant_type(&self, enum_ty: Ty, variant_idx: VariantIdx) -> Ty {
        *self
            .variant_types
            .get(&(enum_ty, variant_idx))
            .unwrap_or(&Ty::ERROR)
    }
    fn tag_offset(&self, ty: Ty) -> u64 {
        *self.tag_offsets.get(&ty).unwrap_or(&0)
    }
}

#[test]
fn downcast_on_direct_tagged_enum_emits_tag_offset() {
    // A direct-tagged enum stores the discriminant in the leading `tag_size`
    // bytes, so downcasting to a variant must advance the address by that
    // offset before the field projections run (plan §20.1).
    let enum_ty = Ty::ERROR;
    let variant_struct_ty = Ty::UNIT;
    let field_idx = FieldIdx::from_usize(0);
    let tag_offset = 4u64;
    let field_offset = 8u64;

    let layout_provider = TestLayoutProvider::new()
        .with_tag_offset(enum_ty, tag_offset)
        .with_field_offset(variant_struct_ty, field_idx, field_offset)
        .with_size(enum_ty, 32)
        .with_variant_type(enum_ty, VariantIdx::from_usize(0), variant_struct_ty);

    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    )
    .with_layout_provider(Box::new(layout_provider));
    let local = LocalIdx::from_usize(0);
    let place = Place {
        local,
        projection: Box::new([
            ProjectionElem::Downcast(VariantIdx::from_usize(0)),
            ProjectionElem::Field(field_idx),
        ]),
    };
    let local_tys = IndexVec::from_vec(vec![LocalDecl {
        ty: enum_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    }]);

    let mut bc = Vec::new();
    backend
        .emit_place_address(&mut bc, &place, &local_tys)
        .unwrap();

    // Expected: LOAD_LOCAL_ADDR, const(tag_offset), ADD, const(field_offset), ADD.
    let mut expected = vec![crate::OP_LOAD_LOCAL_ADDR, 0, 0, 0, 0, crate::OP_LOAD_CONST];
    expected.extend_from_slice(&(tag_offset as i64).to_le_bytes());
    expected.push(crate::OP_ADD);
    expected.push(crate::OP_LOAD_CONST);
    expected.extend_from_slice(&(field_offset as i64).to_le_bytes());
    expected.push(crate::OP_ADD);

    assert_eq!(bc, expected);
}

#[test]
fn downcast_projection_no_extra_instruction() {
    let local = LocalIdx::from_usize(0);
    let place_no_downcast = Place {
        local,
        projection: Box::new([]),
    };
    let place_with_downcast = Place {
        local,
        projection: Box::new([ProjectionElem::Downcast(VariantIdx::from_usize(0))]),
    };
    let local_tys = IndexVec::from_vec(vec![LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    }]);

    let backend = BytecodeBackend::with_ty_ctx(std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()), glyim_core::TargetInfo::default());
    let mut bc_no = Vec::new();
    let mut bc_with = Vec::new();

    backend
        .emit_place_address(&mut bc_no, &place_no_downcast, &local_tys)
        .unwrap();
    backend
        .emit_place_address(&mut bc_with, &place_with_downcast, &local_tys)
        .unwrap();

    assert_eq!(bc_no, bc_with);
}

#[test]
fn downcast_with_field_emits_correct_offset() {
    let enum_ty = Ty::ERROR;
    let variant_struct_ty = Ty::UNIT;
    let field_idx = FieldIdx::from_usize(0);
    let expected_offset = 16u64;

    let layout_provider = TestLayoutProvider::new()
        .with_field_offset(variant_struct_ty, field_idx, expected_offset)
        .with_size(enum_ty, 32)
        .with_variant_type(enum_ty, VariantIdx::from_usize(0), variant_struct_ty);

    let backend = BytecodeBackend::with_ty_ctx(std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()), glyim_core::TargetInfo::default()).with_layout_provider(Box::new(layout_provider));
    let local = LocalIdx::from_usize(0);
    let place = Place {
        local,
        projection: Box::new([
            ProjectionElem::Downcast(VariantIdx::from_usize(0)),
            ProjectionElem::Field(field_idx),
        ]),
    };
    let local_tys = IndexVec::from_vec(vec![LocalDecl {
        ty: enum_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    }]);

    let mut bc = Vec::new();
    backend
        .emit_place_address(&mut bc, &place, &local_tys)
        .unwrap();

    let expected = vec![crate::OP_LOAD_LOCAL_ADDR, 0, 0, 0, 0, crate::OP_LOAD_CONST];
    let offset_bytes = expected_offset as i64;
    let mut expected_with_const = expected;
    expected_with_const.extend_from_slice(&offset_bytes.to_le_bytes());
    expected_with_const.push(crate::OP_ADD);

    assert_eq!(bc, expected_with_const);
}

#[test]
fn downcast_does_not_affect_pointer_arithmetic() {
    let local = LocalIdx::from_usize(1);
    let place = Place {
        local,
        projection: Box::new([ProjectionElem::Downcast(VariantIdx::from_usize(0))]),
    };
    let local_tys = IndexVec::from_vec(vec![
        LocalDecl {
            ty: Ty::UNIT,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        LocalDecl {
            ty: Ty::UNIT,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        },
    ]);
    let backend = BytecodeBackend::with_ty_ctx(std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()), glyim_core::TargetInfo::default());
    let mut bc = Vec::new();
    backend
        .emit_place_address(&mut bc, &place, &local_tys)
        .unwrap();

    assert_eq!(bc[0], crate::OP_LOAD_LOCAL_ADDR);
    assert_eq!(&bc[1..5], &1u32.to_le_bytes());
    assert_eq!(bc.len(), 5);
}

#[test]
fn downcast_before_multiple_fields_no_extra_ops() {
    let field1 = FieldIdx::from_usize(0);
    let field2 = FieldIdx::from_usize(1);
    let variant_ty = Ty::ERROR;
    let layout_provider = TestLayoutProvider::new()
        .with_field_offset(variant_ty, field1, 8)
        .with_field_offset(variant_ty, field2, 12)
        .with_size(variant_ty, 24)
        .with_variant_type(Ty::ERROR, VariantIdx::from_usize(0), variant_ty);

    let backend = BytecodeBackend::with_ty_ctx(std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()), glyim_core::TargetInfo::default()).with_layout_provider(Box::new(layout_provider));
    let local = LocalIdx::from_usize(0);
    let place = Place {
        local,
        projection: Box::new([
            ProjectionElem::Downcast(VariantIdx::from_usize(0)),
            ProjectionElem::Field(field1),
            ProjectionElem::Field(field2),
        ]),
    };
    let local_tys = IndexVec::from_vec(vec![LocalDecl {
        ty: Ty::ERROR,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    }]);

    let mut bc = Vec::new();
    backend
        .emit_place_address(&mut bc, &place, &local_tys)
        .unwrap();

    let mut i = 0;
    assert_eq!(bc[i], crate::OP_LOAD_LOCAL_ADDR);
    i += 5;
    assert_eq!(bc[i], crate::OP_LOAD_CONST);
    i += 1;
    i += 8;
    assert_eq!(bc[i], crate::OP_ADD);
    i += 1;
    assert_eq!(bc[i], crate::OP_LOAD_CONST);
    i += 1;
    i += 8;
    assert_eq!(bc[i], crate::OP_ADD);
    i += 1;
    assert_eq!(i, bc.len());
}
