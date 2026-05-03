use glyim_hir::{HirExpr, HirStmt, HirBinOp};
use glyim_diag::Span;
use glyim_interner::Symbol;

/// Serialize a HirExpr into bytes.
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
fn write_str(buf: &mut Vec<u8>, s: &str) {
    write_u32(buf, s.len() as u32);
    buf.extend_from_slice(s.as_bytes());
}

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

fn make_span() -> Span { Span::new(0, 0) }
fn make_id() -> glyim_hir::types::ExprId { glyim_hir::types::ExprId::new(0) }

fn write_expr(expr: &HirExpr, buf: &mut Vec<u8>) {
    match expr {
        HirExpr::IntLit { value, .. } => {
            write_u8(buf, TAG_INT_LIT);
            write_i64(buf, *value);
        }
        HirExpr::Ident { .. } => {
            write_u8(buf, TAG_IDENT);
            write_str(buf, "<ident>");
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
            write_u8(buf, TAG_UNIT);
        }
    }
}

fn read_expr(data: &[u8], pos: &mut usize) -> Option<HirExpr> {
    let tag = read_u8(data, pos)?;
    match tag {
        TAG_INT_LIT => {
            let value = read_i64(data, pos)?;
            Some(HirExpr::IntLit { id: make_id(), value, span: make_span() })
        }
        TAG_IDENT => {
            let _sym_str = read_str(data, pos)?;
            Some(HirExpr::Ident { id: make_id(), name: Symbol::from_raw(0), span: make_span() })
        }
        TAG_BINARY => {
            let op = u8_to_binop(read_u8(data, pos)?)?;
            let lhs = read_expr(data, pos)?;
            let rhs = read_expr(data, pos)?;
            Some(HirExpr::Binary { id: make_id(), op, lhs: Box::new(lhs), rhs: Box::new(rhs), span: make_span() })
        }
        TAG_BLOCK => {
            let count = read_u32(data, pos)? as usize;
            let mut stmts = Vec::with_capacity(count);
            for _ in 0..count {
                stmts.push(read_stmt(data, pos)?);
            }
            Some(HirExpr::Block { id: make_id(), stmts, span: make_span() })
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
            Some(HirExpr::If { id: make_id(), condition: Box::new(condition), then_branch: Box::new(then_branch), else_branch, span: make_span() })
        }
        TAG_UNIT => {
            Some(HirExpr::UnitLit { id: make_id(), span: make_span() })
        }
        TAG_BOOL_LIT => {
            let value = read_u8(data, pos)? != 0;
            Some(HirExpr::BoolLit { id: make_id(), value, span: make_span() })
        }
        _ => None,
    }
}

fn write_stmt(stmt: &HirStmt, buf: &mut Vec<u8>) {
    match stmt {
        HirStmt::Let { value, .. } => {
            write_u8(buf, 1);
            write_str(buf, "<let-var>");
            write_expr(value, buf);
        }
        HirStmt::Assign { value, .. } => {
            write_u8(buf, 2);
            write_str(buf, "<assign-target>");
            write_expr(value, buf);
        }
        HirStmt::Expr(e) => {
            write_u8(buf, 3);
            write_expr(e, buf);
        }
        _ => write_u8(buf, 0),
    }
}

fn read_stmt(data: &[u8], pos: &mut usize) -> Option<HirStmt> {
    let tag = read_u8(data, pos)?;
    match tag {
        1 => {
            let _name_str = read_str(data, pos)?;
            let value = read_expr(data, pos)?;
            Some(HirStmt::Let { name: Symbol::from_raw(0), mutable: false, value, span: make_span() })
        }
        2 => {
            let _target_str = read_str(data, pos)?;
            let value = read_expr(data, pos)?;
            Some(HirStmt::Assign { target: Symbol::from_raw(0), value, span: make_span() })
        }
        3 => {
            let e = read_expr(data, pos)?;
            Some(HirStmt::Expr(e))
        }
        _ => None,
    }
}

fn binop_to_u8(op: &HirBinOp) -> u8 {
    match op {
        HirBinOp::Add => 1, HirBinOp::Sub => 2, HirBinOp::Mul => 3,
        HirBinOp::Div => 4, HirBinOp::Eq => 5, _ => 0,
    }
}

fn u8_to_binop(v: u8) -> Option<HirBinOp> {
    match v {
        1 => Some(HirBinOp::Add), 2 => Some(HirBinOp::Sub), 3 => Some(HirBinOp::Mul),
        4 => Some(HirBinOp::Div), 5 => Some(HirBinOp::Eq), _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glyim_diag::Span;
    use glyim_hir::types::ExprId;

    #[test]
    fn roundtrip_int_lit() {
        let expr = HirExpr::IntLit { id: ExprId::new(0), value: 42, span: Span::new(0, 2) };
        let bytes = serialize_expr(&expr);
        let back = deserialize_expr(&bytes).expect("deserialize");
        match back {
            HirExpr::IntLit { value, .. } => assert_eq!(value, 42),
            _ => panic!("expected IntLit"),
        }
    }

    #[test]
    fn roundtrip_ident_placeholder() {
        let expr = HirExpr::Ident { id: ExprId::new(0), name: Symbol::from_raw(0), span: Span::new(0, 1) };
        let bytes = serialize_expr(&expr);
        let back = deserialize_expr(&bytes).expect("deserialize");
        match back {
            HirExpr::Ident { .. } => {}
            _ => panic!("expected Ident"),
        }
    }

    #[test]
    fn roundtrip_binary() {
        let lhs = HirExpr::IntLit { id: ExprId::new(0), value: 1, span: Span::new(0, 1) };
        let rhs = HirExpr::IntLit { id: ExprId::new(1), value: 2, span: Span::new(2, 3) };
        let expr = HirExpr::Binary { id: ExprId::new(2), op: HirBinOp::Add, lhs: Box::new(lhs), rhs: Box::new(rhs), span: Span::new(0, 3) };
        let bytes = serialize_expr(&expr);
        let back = deserialize_expr(&bytes).expect("deserialize");
        match back {
            HirExpr::Binary { op, .. } => assert_eq!(op, HirBinOp::Add),
            _ => panic!("expected Binary"),
        }
    }
}
