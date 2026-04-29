use crate::{HirBinOp, HirUnOp};
use glyim_parse::{BinOp, UnOp};

pub fn lower_binop(op: BinOp) -> HirBinOp {
    match op {
        BinOp::Add => HirBinOp::Add,
        BinOp::Sub => HirBinOp::Sub,
        BinOp::Mul => HirBinOp::Mul,
        BinOp::Div => HirBinOp::Div,
        BinOp::Mod => HirBinOp::Mod,
        BinOp::Eq => HirBinOp::Eq,
        BinOp::Neq => HirBinOp::Neq,
        BinOp::Lt => HirBinOp::Lt,
        BinOp::Gt => HirBinOp::Gt,
        BinOp::Lte => HirBinOp::Lte,
        BinOp::Gte => HirBinOp::Gte,
        BinOp::And => HirBinOp::And,
        BinOp::Or => HirBinOp::Or,
    }
}

pub fn lower_unop(op: UnOp) -> HirUnOp {
    match op {
        UnOp::Neg => HirUnOp::Neg,
        UnOp::Not => HirUnOp::Not,
    }
}
