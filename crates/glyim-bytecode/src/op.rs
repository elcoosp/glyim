use serde::{Deserialize, Serialize};

/// A tag that identifies a binary operation.
pub type BinOpTag = u8;
/// A tag that identifies a unary operation.
pub type UnOpTag = u8;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum BytecodeOp {
    PushI64(i64),
    PushF64(f64),
    PushBool(bool),
    PushStr(String),
    PushUnit,
    LoadLocal(u32),
    StoreLocal(u32),
    BinOp(BinOpTag),
    UnOp(UnOpTag),
    Jump(u32),
    JumpIfFalse(u32),
    Return,
    Call { name: String, arg_count: u32 },
    AllocStruct { field_count: u32 },
    FieldAccess { index: u32 },
    FieldSet { index: u32 },
    EnumVariant { tag: u32 },
    Println,
    Assert { message: Option<String> },
    Nop,
}

// Conversion helpers between glyim_hir types and tags
pub fn binop_to_tag(op: glyim_hir::node::HirBinOp) -> BinOpTag {
    match op {
        glyim_hir::node::HirBinOp::Add => 0,
        glyim_hir::node::HirBinOp::Sub => 1,
        glyim_hir::node::HirBinOp::Mul => 2,
        glyim_hir::node::HirBinOp::Div => 3,
        glyim_hir::node::HirBinOp::Mod => 4,
        glyim_hir::node::HirBinOp::Eq => 5,
        glyim_hir::node::HirBinOp::Neq => 6,
        glyim_hir::node::HirBinOp::Lt => 7,
        glyim_hir::node::HirBinOp::Gt => 8,
        glyim_hir::node::HirBinOp::Lte => 9,
        glyim_hir::node::HirBinOp::Gte => 10,
        glyim_hir::node::HirBinOp::And => 11,
        glyim_hir::node::HirBinOp::Or => 12,
    }
}

pub fn tag_to_binop(tag: BinOpTag) -> Option<glyim_hir::node::HirBinOp> {
    match tag {
        0 => Some(glyim_hir::node::HirBinOp::Add),
        1 => Some(glyim_hir::node::HirBinOp::Sub),
        2 => Some(glyim_hir::node::HirBinOp::Mul),
        3 => Some(glyim_hir::node::HirBinOp::Div),
        4 => Some(glyim_hir::node::HirBinOp::Mod),
        5 => Some(glyim_hir::node::HirBinOp::Eq),
        6 => Some(glyim_hir::node::HirBinOp::Neq),
        7 => Some(glyim_hir::node::HirBinOp::Lt),
        8 => Some(glyim_hir::node::HirBinOp::Gt),
        9 => Some(glyim_hir::node::HirBinOp::Lte),
        10 => Some(glyim_hir::node::HirBinOp::Gte),
        11 => Some(glyim_hir::node::HirBinOp::And),
        12 => Some(glyim_hir::node::HirBinOp::Or),
        _ => None,
    }
}

pub fn unop_to_tag(op: glyim_hir::node::HirUnOp) -> UnOpTag {
    match op {
        glyim_hir::node::HirUnOp::Neg => 0,
        glyim_hir::node::HirUnOp::Not => 1,
    }
}

pub fn tag_to_unop(tag: UnOpTag) -> Option<glyim_hir::node::HirUnOp> {
    match tag {
        0 => Some(glyim_hir::node::HirUnOp::Neg),
        1 => Some(glyim_hir::node::HirUnOp::Not),
        _ => None,
    }
}
