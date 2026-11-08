//! Tests for primitive types: `IntTy`, `UintTy`, `FloatTy`, Mutability, etc.

use glyim_core::primitives::*;

#[test]
fn target_info_default() {
    let target = TargetInfo::default();
    assert_eq!(target.pointer_width(), 64);
    assert_eq!(target.pointer_size(), 8);
}

#[test]
fn target_info_x86_64() {
    let target = TargetInfo::x86_64();
    assert_eq!(target.pointer_width(), 64);
    assert_eq!(target.pointer_size(), 8);
    assert_eq!(target.pointer_align(), 8);
}

#[test]
fn int_ty_bit_widths() {
    let target = TargetInfo::x86_64();
    assert_eq!(IntTy::I8.bit_width(&target), 8);
    assert_eq!(IntTy::I16.bit_width(&target), 16);
    assert_eq!(IntTy::I32.bit_width(&target), 32);
    assert_eq!(IntTy::I64.bit_width(&target), 64);
    assert_eq!(IntTy::Isize.bit_width(&target), 64);
}

#[test]
fn int_ty_names() {
    assert_eq!(IntTy::I8.name(), "i8");
    assert_eq!(IntTy::I16.name(), "i16");
    assert_eq!(IntTy::I32.name(), "i32");
    assert_eq!(IntTy::I64.name(), "i64");
    assert_eq!(IntTy::Isize.name(), "isize");
}

#[test]
fn uint_ty_bit_widths() {
    let target = TargetInfo::x86_64();
    assert_eq!(UintTy::U8.bit_width(&target), 8);
    assert_eq!(UintTy::U16.bit_width(&target), 16);
    assert_eq!(UintTy::U32.bit_width(&target), 32);
    assert_eq!(UintTy::U64.bit_width(&target), 64);
    assert_eq!(UintTy::Usize.bit_width(&target), 64);
}

#[test]
fn uint_ty_names() {
    assert_eq!(UintTy::U8.name(), "u8");
    assert_eq!(UintTy::U16.name(), "u16");
    assert_eq!(UintTy::U32.name(), "u32");
    assert_eq!(UintTy::U64.name(), "u64");
    assert_eq!(UintTy::Usize.name(), "usize");
}

#[test]
fn float_ty_bit_widths() {
    assert_eq!(FloatTy::F32.bit_width(), 32);
    assert_eq!(FloatTy::F64.bit_width(), 64);
}

#[test]
fn float_ty_names() {
    assert_eq!(FloatTy::F32.name(), "f32");
    assert_eq!(FloatTy::F64.name(), "f64");
}

#[test]
fn mutability_is_mut() {
    assert!(!Mutability::Not.is_mut());
    assert!(Mutability::Mut.is_mut());
}

#[test]
fn mutability_prefix_str() {
    assert_eq!(Mutability::Not.prefix_str(), "");
    assert_eq!(Mutability::Mut.prefix_str(), "mut ");
}

#[test]
fn safety_equality() {
    assert_eq!(Safety::Safe, Safety::Safe);
    assert_eq!(Safety::Unsafe, Safety::Unsafe);
    assert_ne!(Safety::Safe, Safety::Unsafe);
}

#[test]
fn abi_names() {
    assert_eq!(Abi::C.name(), "C");
    assert_eq!(Abi::Glyim.name(), "glyim");
    assert_eq!(Abi::System.name(), "system");
}

#[test]
fn bin_ops_exist() {
    let _ = BinOp::Add;
    let _ = BinOp::Sub;
    let _ = BinOp::Mul;
    let _ = BinOp::Div;
    let _ = BinOp::Rem;
    let _ = BinOp::Eq;
    let _ = BinOp::Ne;
    let _ = BinOp::Lt;
    let _ = BinOp::Gt;
    let _ = BinOp::LtEq;
    let _ = BinOp::GtEq;
    let _ = BinOp::And;
    let _ = BinOp::Or;
    let _ = BinOp::BitAnd;
    let _ = BinOp::BitOr;
    let _ = BinOp::BitXor;
    let _ = BinOp::Shl;
    let _ = BinOp::Shr;
}

#[test]
fn un_ops() {
    let _ = UnOp::Not;
    let _ = UnOp::Neg;
    let _ = UnOp::Deref;
}

#[test]
fn visibility_variants() {
    let _ = Visibility::Public;
    let _ = Visibility::PubCrate;
    let _ = Visibility::PubSuper;
    let _ = Visibility::PubIn(vec![]);
    let _ = Visibility::Inherited;
}

#[test]
fn struct_kind_variants() {
    let _ = StructKind::Unit;
    let _ = StructKind::Tuple;
    let _ = StructKind::Record;
}
