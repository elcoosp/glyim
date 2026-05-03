//! Binary serialization of typed AST nodes for the Wasm macro interface.
//!
//! The protocol is a simple tagged binary format:
//!   - kind: u8
//!   - payload length: u32 (little-endian)
//!   - payload: variable
//!
//! This allows macros to receive and return typed AST subtrees.

use glyim_hir::{HirExpr, HirType, HirPattern, HirStmt, HirBinOp, HirUnOp};
use glyim_interner::Symbol;

/// Serialize a HirExpr into bytes for passing to a Wasm macro.
pub fn serialize_expr(expr: &HirExpr) -> Vec<u8> {
    let mut buf = Vec::new();
    write_expr(expr, &mut buf);
    buf
}

/// Deserialize bytes back into a HirExpr.
pub fn deserialize_expr(data: &[u8]) -> Option<HirExpr> {
    let mut pos = 0;
    read_expr(data, &mut pos)
}

// ── Internal helpers ────────────────────────────────────────

fn write_u8(buf: &mut Vec<u8>, v: u8) { buf.push(v); }
fn write_u32(buf: &mut Vec<u8>, v: u32) { buf.extend_from_slice(&v.to_le_bytes()); }
fn write_i64(buf: &mut Vec<u8>, v: i64) { buf.extend_from_slice(&v.to_le_bytes()); }
fn write_bool(buf: &mut Vec<u8>, v: bool) { buf.push(v as u8); }

fn read_u8(data: &[u8], pos: &mut usize) -> Option<u8> {
    if *pos >= data.len() { return None; }
    let v = data[*pos];
    *pos += 1;
    Some(v)
}
fn read_u32(data: &[u8], pos: &mut usize) -> Option<u32> {
    if *pos + 4 > data.len() { return None; }
    let v = u32::from_le_bytes([data[*pos], data[*pos+1], data[*pos+2], data[*pos+3]]);
    *pos += 4;
    Some(v)
}
fn read_i64(data: &[u8], pos: &mut usize) -> Option<i64> {
    if *pos + 8 > data.len() { return None; }
    let v = i64::from_le_bytes([
        data[*pos], data[*pos+1], data[*pos+2], data[*pos+3],
        data[*pos+4], data[*pos+5], data[*pos+6], data[*pos+7]
    ]);
    *pos += 8;
    Some(v)
}

fn write_str(buf: &mut Vec<u8>, s: &str) {
    write_u32(buf, s.len() as u32);
    buf.extend_from_slice(s.as_bytes());
}
fn read_str<'a>(data: &'a [u8], pos: &mut usize) -> Option<&'a str> {
    let len = read_u32(data, pos)? as usize;
    if *pos + len > data.len() { return None; }
    let s = std::str::from_utf8(&data[*pos..*pos+len]).ok()?;
    *pos += len;
    Some(s)
}

const TAG_INT_LIT: u8 = 1;
const TAG_IDENT: u8 = 2;
const TAG_BINARY: u8 = 3;
const TAG_BLOCK: u8 = 4;
const TAG_IF: u8 = 5;
const TAG_UNIT: u8 = 6;
const TAG_BOOL_LIT: u8 = 7;

fn write_expr(expr: &HirExpr, buf: &mut Vec<u8>) {
    match expr {
        HirExpr::IntLit { value, .. } => {
            write_u8(buf, TAG_INT_LIT);
            write_i64(buf, *value);
        }
        HirExpr::Ident { name, .. } => {
            write_u8(buf, TAG_IDENT);
            write_u32(buf, name.0);
        }
        HirExpr::Binary { op, lhs, rhs, .. } => {
            write_u8(buf, TAG_BINARY);
            write_u8(buf, binop_to_u8(op));
            write_expr(lhs, buf);
            write_expr(rhs, buf);
        }
        HirExpr::Block { stmts, .. } => {
            write_u8(buf, TAG_BLOCK);
            write_u32(buf, stmts.len() as u32);
            for stmt in stmts {
                write_stmt(stmt, buf);
            }
        }
        HirExpr::If { condition, then_branch, else_branch, .. } => {
            write_u8(buf, TAG_IF);
            write_expr(condition, buf);
            write_expr(then_branch, buf);
            match else_branch {
                Some(e) => { write_u8(buf, 1); write_expr(e, buf); }
                None => write_u8(buf, 0),
            }
        }
        HirExpr::UnitLit { .. } => {
            write_u8(buf, TAG_UNIT);
        }
        HirExpr::BoolLit { value, .. } => {
            write_u8(buf, TAG_BOOL_LIT);
            write_bool(buf, *value);
        }
        _ => {
            // fallback: serialize as unit
            write_u8(buf, TAG_UNIT);
        }
    }
}

fn read_expr(data: &[u8], pos: &mut usize) -> Option<HirExpr> {
    let tag = read_u8(data, pos)?;
    match tag {
        TAG_INT_LIT => {
            let value = read_i64(data, pos)?;
            Some(HirExpr::IntLit { id: Default::default(), value, span: Default::default() })
        }
        TAG_IDENT => {
            let name = Symbol(read_u32(data, pos)?);
            Some(HirExpr::Ident { id: Default::default(), name, span: Default::default() })
        }
        TAG_BINARY => {
            let op = u8_to_binop(read_u8(data, pos)?)?;
            let lhs = read_expr(data, pos)?;
            let rhs = read_expr(data, pos)?;
            Some(HirExpr::Binary { id: Default::default(), op, lhs: Box::new(lhs), rhs: Box::new(rhs), span: Default::default() })
        }
        TAG_BLOCK => {
            let count = read_u32(data, pos)? as usize;
            let mut stmts = Vec::with_capacity(count);
            for _ in 0..count {
                stmts.push(read_stmt(data, pos)?);
            }
            Some(HirExpr::Block { id: Default::default(), stmts, span: Default::default() })
        }
        TAG_IF => {
            let condition = read_expr(data, pos)?;
            let then_branch = read_expr(data, pos)?;
            let has_else = read_u8(data, pos)?;
            let else_branch = if has_else != 0 {
                Some(Box::new(read_expr(data, pos)?))
            } else {
                None
            };
            Some(HirExpr::If { id: Default::default(), condition: Box::new(condition), then_branch: Box::new(then_branch), else_branch, span: Default::default() })
        }
        TAG_UNIT => {
            Some(HirExpr::UnitLit { id: Default::default(), span: Default::default() })
        }
        TAG_BOOL_LIT => {
            let value = read_u8(data, pos)? != 0;
            Some(HirExpr::BoolLit { id: Default::default(), value, span: Default::default() })
        }
        _ => None,
    }
}

fn write_stmt(stmt: &HirStmt, buf: &mut Vec<u8>) {
    match stmt {
        HirStmt::Let { name, mutable: _, value, span: _ } => {
            write_u8(buf, 1); // Let tag
            write_u32(buf, name.0);
            write_expr(value, buf);
        }
        HirStmt::Assign { target, value, span: _ } => {
            write_u8(buf, 2); // Assign tag
            write_u32(buf, target.0);
            write_expr(value, buf);
        }
        HirStmt::Expr(e) => {
            write_u8(buf, 3); // Expr tag
            write_expr(e, buf);
        }
        _ => write_u8(buf, 0), // unknown
    }
}

fn read_stmt(data: &[u8], pos: &mut usize) -> Option<HirStmt> {
    let tag = read_u8(data, pos)?;
    match tag {
        1 => { // Let
            let name = Symbol(read_u32(data, pos)?);
            let value = read_expr(data, pos)?;
            Some(HirStmt::Let { name, mutable: false, value, span: Default::default() })
        }
        2 => { // Assign
            let target = Symbol(read_u32(data, pos)?);
            let value = read_expr(data, pos)?;
            Some(HirStmt::Assign { target, value, span: Default::default() })
        }
        3 => { // Expr
            let e = read_expr(data, pos)?;
            Some(HirStmt::Expr(e))
        }
        _ => None,
    }
}

fn binop_to_u8(op: &HirBinOp) -> u8 {
    match op {
        HirBinOp::Add => 1,
        HirBinOp::Sub => 2,
        HirBinOp::Mul => 3,
        HirBinOp::Div => 4,
        HirBinOp::Eq => 5,
        _ => 0,
    }
}

fn u8_to_binop(v: u8) -> Option<HirBinOp> {
    match v {
        1 => Some(HirBinOp::Add),
        2 => Some(HirBinOp::Sub),
        3 => Some(HirBinOp::Mul),
        4 => Some(HirBinOp::Div),
        5 => Some(HirBinOp::Eq),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_int_lit() {
        let expr = HirExpr::IntLit { id: Default::default(), value: 42, span: Default::default() };
        let bytes = serialize_expr(&expr);
        let back = deserialize_expr(&bytes).expect("deserialize");
        match back {
            HirExpr::IntLit { value, .. } => assert_eq!(value, 42),
            _ => panic!("expected IntLit"),
        }
    }

    #[test]
    fn roundtrip_ident() {
        let expr = HirExpr::Ident { id: Default::default(), name: Symbol(7), span: Default::default() };
        let bytes = serialize_expr(&expr);
        let back = deserialize_expr(&bytes).expect("deserialize");
        match back {
            HirExpr::Ident { name, .. } => assert_eq!(name, Symbol(7)),
            _ => panic!("expected Ident"),
        }
    }

    #[test]
    fn roundtrip_binary() {
        let lhs = HirExpr::IntLit { id: Default::default(), value: 1, span: Default::default() };
        let rhs = HirExpr::IntLit { id: Default::default(), value: 2, span: Default::default() };
        let expr = HirExpr::Binary { id: Default::default(), op: HirBinOp::Add, lhs: Box::new(lhs), rhs: Box::new(rhs), span: Default::default() };
        let bytes = serialize_expr(&expr);
        let back = deserialize_expr(&bytes).expect("deserialize");
        match back {
            HirExpr::Binary { op, .. } => assert_eq!(op, HirBinOp::Add),
            _ => panic!("expected Binary"),
        }
    }
}
