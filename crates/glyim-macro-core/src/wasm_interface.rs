use glyim_diag::Span;
use glyim_hir::{HirBinOp, HirExpr, HirStmt};
use glyim_interner::Symbol;

/// Serialize a HirExpr into bytes with a string table.
pub fn serialize_expr(expr: &HirExpr) -> Vec<u8> {
    let mut buf = Vec::new();
    let mut symbols: Vec<String> = Vec::new();
    collect_symbols_expr(expr, &mut symbols);
    // Write string table: u32 count, then for each: u32 len + bytes
    write_u32(&mut buf, symbols.len() as u32);
    for s in &symbols {
        write_str(&mut buf, s);
    }
    // Write the AST using symbol indices + spans
    write_expr(expr, &mut buf, &symbols);
    buf
}

/// Deserialize bytes back into a HirExpr and the recovered symbol names.
pub fn deserialize_expr(data: &[u8]) -> Option<(HirExpr, Vec<String>)> {
    let mut pos = 0;
    let count = read_u32(data, &mut pos)? as usize;
    let mut symbols = Vec::with_capacity(count);
    for _ in 0..count {
        let sym = read_str_to_owned(data, &mut pos)?;
        symbols.push(sym);
    }
    let expr = read_expr(data, &mut pos, &symbols)?;
    Some((expr, symbols))
}

// ── Constants ────────────────────────────────────────────────

const TAG_INT_LIT: u8 = 1;
const TAG_FLOAT_LIT: u8 = 2;
const TAG_BOOL_LIT: u8 = 3;
const TAG_STR_LIT: u8 = 4;
const TAG_IDENT: u8 = 5;
const TAG_BINARY: u8 = 6;
const TAG_UNARY: u8 = 7;
const TAG_BLOCK: u8 = 8;
const TAG_IF: u8 = 9;
const TAG_CALL: u8 = 11;
const TAG_STRUCT_LIT: u8 = 12;
const TAG_ENUM_VARIANT: u8 = 13;
const TAG_FIELD_ACCESS: u8 = 14;
const TAG_TUPLE_LIT: u8 = 15;
const TAG_RETURN: u8 = 16;
const TAG_WHILE: u8 = 17;
const TAG_FOR_IN: u8 = 18;
const TAG_UNIT: u8 = 19;
const TAG_DEREF: u8 = 20;
const TAG_SIZE_OF: u8 = 21;
const TAG_AS: u8 = 22;
const TAG_ADDR_OF: u8 = 23;
const TAG_PRINTLN: u8 = 24;
const TAG_ASSERT: u8 = 25;
const TAG_METHOD_CALL: u8 = 26;

// ── Symbol collection ───────────────────────────────────────

fn collect_symbols_expr(expr: &HirExpr, symbols: &mut Vec<String>) {
    match expr {
        HirExpr::StrLit { value, .. } => {
            let s = value.clone();
            if !symbols.contains(&s) {
                symbols.push(s);
            }
        }
        HirExpr::Ident { name, .. } => {
            let s = name.raw().to_string();
            if !symbols.contains(&s) {
                symbols.push(s);
            }
        }
        HirExpr::Binary { lhs, rhs, .. } => {
            collect_symbols_expr(lhs, symbols);
            collect_symbols_expr(rhs, symbols);
        }
        HirExpr::Unary { operand, .. } => collect_symbols_expr(operand, symbols),
        HirExpr::Block { stmts, .. } => {
            for stmt in stmts {
                collect_symbols_stmt(stmt, symbols);
            }
        }
        HirExpr::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            collect_symbols_expr(condition, symbols);
            collect_symbols_expr(then_branch, symbols);
            if let Some(e) = else_branch {
                collect_symbols_expr(e, symbols);
            }
        }
        HirExpr::Call { callee, args, .. } => {
            let s = callee.raw().to_string();
            if !symbols.contains(&s) {
                symbols.push(s);
            }
            for a in args {
                collect_symbols_expr(a, symbols);
            }
        }
        HirExpr::MethodCall {
            receiver,
            method_name,
            args,
            ..
        } => {
            collect_symbols_expr(receiver, symbols);
            let s = method_name.raw().to_string();
            if !symbols.contains(&s) {
                symbols.push(s);
            }
            for a in args {
                collect_symbols_expr(a, symbols);
            }
        }
        HirExpr::StructLit {
            struct_name,
            fields,
            ..
        } => {
            let s = struct_name.raw().to_string();
            if !symbols.contains(&s) {
                symbols.push(s);
            }
            for (f, v) in fields {
                let fs = f.raw().to_string();
                if !symbols.contains(&fs) {
                    symbols.push(fs);
                }
                collect_symbols_expr(v, symbols);
            }
        }
        HirExpr::EnumVariant {
            enum_name,
            variant_name,
            args,
            ..
        } => {
            let e = enum_name.raw().to_string();
            if !symbols.contains(&e) {
                symbols.push(e);
            }
            let v = variant_name.raw().to_string();
            if !symbols.contains(&v) {
                symbols.push(v);
            }
            for a in args {
                collect_symbols_expr(a, symbols);
            }
        }
        HirExpr::FieldAccess { object, field, .. } => {
            collect_symbols_expr(object, symbols);
            let s = field.raw().to_string();
            if !symbols.contains(&s) {
                symbols.push(s);
            }
        }
        HirExpr::TupleLit { elements, .. } => {
            for e in elements {
                collect_symbols_expr(e, symbols);
            }
        }
        HirExpr::Return { value: Some(v), .. } => collect_symbols_expr(v, symbols),
        HirExpr::Return { value: None, .. } => {}
        HirExpr::While {
            condition, body, ..
        } => {
            collect_symbols_expr(condition, symbols);
            collect_symbols_expr(body, symbols);
        }
        HirExpr::ForIn { iter, body, .. } => {
            collect_symbols_expr(iter, symbols);
            collect_symbols_expr(body, symbols);
        }
        HirExpr::Deref { expr, .. } => collect_symbols_expr(expr, symbols),
        HirExpr::Println { arg, .. } => collect_symbols_expr(arg, symbols),
        HirExpr::Assert {
            condition, message, ..
        } => {
            collect_symbols_expr(condition, symbols);
            if let Some(m) = message {
                collect_symbols_expr(m, symbols);
            }
        }
        HirExpr::Match {
            scrutinee, arms, ..
        } => {
            collect_symbols_expr(scrutinee, symbols);
            for arm in arms {
                if let Some(g) = &arm.guard {
                    collect_symbols_expr(g, symbols);
                }
                collect_symbols_expr(&arm.body, symbols);
            }
        }
        _ => {}
    }
}

fn collect_symbols_stmt(stmt: &HirStmt, symbols: &mut Vec<String>) {
    match stmt {
        HirStmt::Let { name, value, .. } => {
            let s = name.raw().to_string();
            if !symbols.contains(&s) {
                symbols.push(s);
            }
            collect_symbols_expr(value, symbols);
        }
        HirStmt::LetPat { value, .. } => collect_symbols_expr(value, symbols),
        HirStmt::Assign { target, value, .. } => {
            let s = target.raw().to_string();
            if !symbols.contains(&s) {
                symbols.push(s);
            }
            collect_symbols_expr(value, symbols);
        }
        HirStmt::AssignDeref { target, value, .. } => {
            collect_symbols_expr(target, symbols);
            collect_symbols_expr(value, symbols);
        }
        HirStmt::AssignField {
            object,
            field,
            value,
            ..
        } => {
            collect_symbols_expr(object, symbols);
            let s = field.raw().to_string();
            if !symbols.contains(&s) {
                symbols.push(s);
            }
            collect_symbols_expr(value, symbols);
        }
        HirStmt::Expr(e) => collect_symbols_expr(e, symbols),
    }
}

// ── Writing ──────────────────────────────────────────────────

fn write_u8(buf: &mut Vec<u8>, v: u8) {
    buf.push(v);
}
fn write_u32(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_le_bytes());
}
fn write_u64(buf: &mut Vec<u8>, v: u64) {
    buf.extend_from_slice(&v.to_le_bytes());
}
fn write_i64(buf: &mut Vec<u8>, v: i64) {
    buf.extend_from_slice(&v.to_le_bytes());
}
fn write_f64(buf: &mut Vec<u8>, v: f64) {
    buf.extend_from_slice(&v.to_le_bytes());
}
fn write_bool(buf: &mut Vec<u8>, v: bool) {
    buf.push(v as u8);
}
fn write_str(buf: &mut Vec<u8>, s: &str) {
    write_u32(buf, s.len() as u32);
    buf.extend_from_slice(s.as_bytes());
}
fn write_span(buf: &mut Vec<u8>, span: Span) {
    write_u64(buf, span.start as u64);
    write_u64(buf, span.end as u64);
}
fn write_sym_index(buf: &mut Vec<u8>, sym: Symbol, symbols: &[String]) {
    let raw = sym.raw().to_string();
    let idx = symbols.iter().position(|s| *s == raw).unwrap_or(0) as u32;
    write_u32(buf, idx);
}

fn write_expr(expr: &HirExpr, buf: &mut Vec<u8>, symbols: &[String]) {
    match expr {
        HirExpr::IntLit { value, span, .. } => {
            write_u8(buf, TAG_INT_LIT);
            write_i64(buf, *value);
            write_span(buf, *span);
        }
        HirExpr::FloatLit { value, span, .. } => {
            write_u8(buf, TAG_FLOAT_LIT);
            write_f64(buf, *value);
            write_span(buf, *span);
        }
        HirExpr::BoolLit { value, span, .. } => {
            write_u8(buf, TAG_BOOL_LIT);
            write_bool(buf, *value);
            write_span(buf, *span);
        }
        HirExpr::StrLit { value, span, .. } => {
            write_u8(buf, TAG_STR_LIT);
            write_str(buf, value);
            write_span(buf, *span);
        }
        HirExpr::Ident { name, span, .. } => {
            write_u8(buf, TAG_IDENT);
            write_sym_index(buf, *name, symbols);
            write_span(buf, *span);
        }
        HirExpr::UnitLit { span, .. } => {
            write_u8(buf, TAG_UNIT);
            write_span(buf, *span);
        }
        HirExpr::Binary {
            op, lhs, rhs, span, ..
        } => {
            write_u8(buf, TAG_BINARY);
            write_u8(buf, binop_to_u8(op));
            write_expr(lhs, buf, symbols);
            write_expr(rhs, buf, symbols);
            write_span(buf, *span);
        }
        HirExpr::Unary {
            op, operand, span, ..
        } => {
            write_u8(buf, TAG_UNARY);
            write_u8(buf, unop_to_u8(op));
            write_expr(operand, buf, symbols);
            write_span(buf, *span);
        }
        HirExpr::Block { stmts, span, .. } => {
            write_u8(buf, TAG_BLOCK);
            write_u32(buf, stmts.len() as u32);
            for s in stmts {
                write_stmt(s, buf, symbols);
            }
            write_span(buf, *span);
        }
        HirExpr::If {
            condition,
            then_branch,
            else_branch,
            span,
            ..
        } => {
            write_u8(buf, TAG_IF);
            write_expr(condition, buf, symbols);
            write_expr(then_branch, buf, symbols);
            match else_branch {
                Some(e) => {
                    write_u8(buf, 1);
                    write_expr(e, buf, symbols);
                }
                None => write_u8(buf, 0),
            }
            write_span(buf, *span);
        }
        HirExpr::Call {
            callee, args, span, ..
        } => {
            write_u8(buf, TAG_CALL);
            write_sym_index(buf, *callee, symbols);
            write_u32(buf, args.len() as u32);
            for a in args {
                write_expr(a, buf, symbols);
            }
            write_span(buf, *span);
        }
        HirExpr::MethodCall {
            receiver,
            method_name,
            args,
            span,
            ..
        } => {
            write_u8(buf, TAG_METHOD_CALL);
            write_expr(receiver, buf, symbols);
            write_sym_index(buf, *method_name, symbols);
            write_u32(buf, args.len() as u32);
            for a in args {
                write_expr(a, buf, symbols);
            }
            write_span(buf, *span);
        }
        HirExpr::StructLit {
            struct_name,
            fields,
            span,
            ..
        } => {
            write_u8(buf, TAG_STRUCT_LIT);
            write_sym_index(buf, *struct_name, symbols);
            write_u32(buf, fields.len() as u32);
            for (f, v) in fields {
                write_sym_index(buf, *f, symbols);
                write_expr(v, buf, symbols);
            }
            write_span(buf, *span);
        }
        HirExpr::EnumVariant {
            enum_name,
            variant_name,
            args,
            span,
            ..
        } => {
            write_u8(buf, TAG_ENUM_VARIANT);
            write_sym_index(buf, *enum_name, symbols);
            write_sym_index(buf, *variant_name, symbols);
            write_u32(buf, args.len() as u32);
            for a in args {
                write_expr(a, buf, symbols);
            }
            write_span(buf, *span);
        }
        HirExpr::FieldAccess {
            object,
            field,
            span,
            ..
        } => {
            write_u8(buf, TAG_FIELD_ACCESS);
            write_expr(object, buf, symbols);
            write_sym_index(buf, *field, symbols);
            write_span(buf, *span);
        }
        HirExpr::TupleLit { elements, span, .. } => {
            write_u8(buf, TAG_TUPLE_LIT);
            write_u32(buf, elements.len() as u32);
            for e in elements {
                write_expr(e, buf, symbols);
            }
            write_span(buf, *span);
        }
        HirExpr::Return { value, span, .. } => {
            write_u8(buf, TAG_RETURN);
            match value {
                Some(v) => {
                    write_u8(buf, 1);
                    write_expr(v, buf, symbols);
                }
                None => write_u8(buf, 0),
            }
            write_span(buf, *span);
        }
        HirExpr::While {
            condition,
            body,
            span,
            ..
        } => {
            write_u8(buf, TAG_WHILE);
            write_expr(condition, buf, symbols);
            write_expr(body, buf, symbols);
            write_span(buf, *span);
        }
        HirExpr::ForIn {
            iter, body, span, ..
        } => {
            write_u8(buf, TAG_FOR_IN);
            write_expr(iter, buf, symbols);
            write_expr(body, buf, symbols);
            write_span(buf, *span);
        }
        HirExpr::Deref { expr: e, span, .. } => {
            write_u8(buf, TAG_DEREF);
            write_expr(e, buf, symbols);
            write_span(buf, *span);
        }
        HirExpr::SizeOf {
            target_type, span, ..
        } => {
            write_u8(buf, TAG_SIZE_OF);
            write_str(buf, &format!("{:?}", target_type));
            write_span(buf, *span);
        }
        HirExpr::As {
            expr: e,
            target_type,
            span,
            ..
        } => {
            write_u8(buf, TAG_AS);
            write_expr(e, buf, symbols);
            write_str(buf, &format!("{:?}", target_type));
            write_span(buf, *span);
        }
        HirExpr::AddrOf { target, span, .. } => {
            write_u8(buf, TAG_ADDR_OF);
            write_sym_index(buf, *target, symbols);
            write_span(buf, *span);
        }
        HirExpr::Println { arg, span, .. } => {
            write_u8(buf, TAG_PRINTLN);
            write_expr(arg, buf, symbols);
            write_span(buf, *span);
        }
        HirExpr::Assert {
            condition,
            message,
            span,
            ..
        } => {
            write_u8(buf, TAG_ASSERT);
            write_expr(condition, buf, symbols);
            match message {
                Some(m) => {
                    write_u8(buf, 1);
                    write_expr(m, buf, symbols);
                }
                None => write_u8(buf, 0),
            }
            write_span(buf, *span);
        }
        _ => {
            write_u8(buf, TAG_UNIT);
            write_span(buf, Span::new(0, 0));
        }
    }
}

fn write_stmt(stmt: &HirStmt, buf: &mut Vec<u8>, symbols: &[String]) {
    match stmt {
        HirStmt::Let {
            name,
            mutable,
            value,
            span,
        } => {
            write_u8(buf, 1);
            write_sym_index(buf, *name, symbols);
            write_bool(buf, *mutable);
            write_expr(value, buf, symbols);
            write_span(buf, *span);
        }
        HirStmt::LetPat {
            mutable,
            value,
            span,
            ..
        } => {
            write_u8(buf, 2);
            write_bool(buf, *mutable);
            write_expr(value, buf, symbols);
            write_span(buf, *span);
        }
        HirStmt::Assign {
            target,
            value,
            span,
        } => {
            write_u8(buf, 3);
            write_sym_index(buf, *target, symbols);
            write_expr(value, buf, symbols);
            write_span(buf, *span);
        }
        HirStmt::AssignDeref {
            target,
            value,
            span,
        } => {
            write_u8(buf, 4);
            write_expr(target, buf, symbols);
            write_expr(value, buf, symbols);
            write_span(buf, *span);
        }
        HirStmt::AssignField {
            object,
            field,
            value,
            span,
        } => {
            write_u8(buf, 5);
            write_expr(object, buf, symbols);
            write_sym_index(buf, *field, symbols);
            write_expr(value, buf, symbols);
            write_span(buf, *span);
        }
        HirStmt::Expr(e) => {
            write_u8(buf, 6);
            write_expr(e, buf, symbols);
        }
    }
}

fn binop_to_u8(op: &HirBinOp) -> u8 {
    match op {
        HirBinOp::Add => 1,
        HirBinOp::Sub => 2,
        HirBinOp::Mul => 3,
        HirBinOp::Div => 4,
        HirBinOp::Mod => 5,
        HirBinOp::Eq => 6,
        HirBinOp::Neq => 7,
        HirBinOp::Lt => 8,
        HirBinOp::Gt => 9,
        HirBinOp::Lte => 10,
        HirBinOp::Gte => 11,
        HirBinOp::And => 12,
        HirBinOp::Or => 13,
    }
}

fn unop_to_u8(op: &glyim_hir::HirUnOp) -> u8 {
    match op {
        glyim_hir::HirUnOp::Neg => 0,
        glyim_hir::HirUnOp::Not => 1,
    }
}

// ── Reading ──────────────────────────────────────────────────

fn read_u8(data: &[u8], pos: &mut usize) -> Option<u8> {
    if *pos >= data.len() {
        None
    } else {
        let v = data[*pos];
        *pos += 1;
        Some(v)
    }
}
fn read_u32(data: &[u8], pos: &mut usize) -> Option<u32> {
    if *pos + 4 > data.len() {
        None
    } else {
        let v = u32::from_le_bytes([data[*pos], data[*pos + 1], data[*pos + 2], data[*pos + 3]]);
        *pos += 4;
        Some(v)
    }
}
fn read_u64(data: &[u8], pos: &mut usize) -> Option<u64> {
    if *pos + 8 > data.len() {
        None
    } else {
        let v = u64::from_le_bytes(data[*pos..*pos + 8].try_into().unwrap());
        *pos += 8;
        Some(v)
    }
}
fn read_i64(data: &[u8], pos: &mut usize) -> Option<i64> {
    read_u64(data, pos).map(|v| v as i64)
}
fn read_f64(data: &[u8], pos: &mut usize) -> Option<f64> {
    read_u64(data, pos).map(f64::from_bits)
}
fn read_bool(data: &[u8], pos: &mut usize) -> Option<bool> {
    read_u8(data, pos).map(|v| v != 0)
}
fn read_str_to_owned(data: &[u8], pos: &mut usize) -> Option<String> {
    let len = read_u32(data, pos)? as usize;
    if *pos + len > data.len() {
        None
    } else {
        let s = std::str::from_utf8(&data[*pos..*pos + len])
            .ok()?
            .to_string();
        *pos += len;
        Some(s)
    }
}
fn read_span(data: &[u8], pos: &mut usize) -> Option<Span> {
    let start = read_u64(data, pos)? as usize;
    let end = read_u64(data, pos)? as usize;
    Some(Span::new(start, end))
}
fn read_sym_index(data: &[u8], pos: &mut usize, symbols: &[String]) -> Option<Symbol> {
    let idx = read_u32(data, pos)? as usize;
    if idx < symbols.len() {
        Some(Symbol::from_raw(idx as u32))
    } else {
        None
    }
}

fn read_expr(data: &[u8], pos: &mut usize, symbols: &[String]) -> Option<HirExpr> {
    let tag = read_u8(data, pos)?;
    let make_id = || glyim_hir::types::ExprId::new(0);
    match tag {
        TAG_INT_LIT => {
            let v = read_i64(data, pos)?;
            let s = read_span(data, pos)?;
            Some(HirExpr::IntLit {
                id: make_id(),
                value: v,
                span: s,
            })
        }
        TAG_FLOAT_LIT => {
            let v = read_f64(data, pos)?;
            let s = read_span(data, pos)?;
            Some(HirExpr::FloatLit {
                id: make_id(),
                value: v,
                span: s,
            })
        }
        TAG_BOOL_LIT => {
            let v = read_bool(data, pos)?;
            let s = read_span(data, pos)?;
            Some(HirExpr::BoolLit {
                id: make_id(),
                value: v,
                span: s,
            })
        }
        TAG_STR_LIT => {
            let v = read_str_to_owned(data, pos)?;
            let s = read_span(data, pos)?;
            Some(HirExpr::StrLit {
                id: make_id(),
                value: v,
                span: s,
            })
        }
        TAG_IDENT => {
            let n = read_sym_index(data, pos, symbols)?;
            let s = read_span(data, pos)?;
            Some(HirExpr::Ident {
                id: make_id(),
                name: n,
                span: s,
            })
        }
        TAG_UNIT => {
            let s = read_span(data, pos)?;
            Some(HirExpr::UnitLit {
                id: make_id(),
                span: s,
            })
        }
        TAG_BINARY => {
            let op = u8_to_binop(read_u8(data, pos)?)?;
            let l = read_expr(data, pos, symbols)?;
            let r = read_expr(data, pos, symbols)?;
            let s = read_span(data, pos)?;
            Some(HirExpr::Binary {
                id: make_id(),
                op,
                lhs: Box::new(l),
                rhs: Box::new(r),
                span: s,
            })
        }
        TAG_UNARY => {
            let op = u8_to_unop(read_u8(data, pos)?)?;
            let o = read_expr(data, pos, symbols)?;
            let s = read_span(data, pos)?;
            Some(HirExpr::Unary {
                id: make_id(),
                op,
                operand: Box::new(o),
                span: s,
            })
        }
        TAG_BLOCK => {
            let c = read_u32(data, pos)? as usize;
            let mut stmts = Vec::with_capacity(c);
            for _ in 0..c {
                stmts.push(read_stmt(data, pos, symbols)?);
            }
            let s = read_span(data, pos)?;
            Some(HirExpr::Block {
                id: make_id(),
                stmts,
                span: s,
            })
        }
        TAG_IF => {
            let c = read_expr(data, pos, symbols)?;
            let t = read_expr(data, pos, symbols)?;
            let e = if read_u8(data, pos)? != 0 {
                Some(Box::new(read_expr(data, pos, symbols)?))
            } else {
                None
            };
            let s = read_span(data, pos)?;
            Some(HirExpr::If {
                id: make_id(),
                condition: Box::new(c),
                then_branch: Box::new(t),
                else_branch: e,
                span: s,
            })
        }
        TAG_CALL => {
            let callee = read_sym_index(data, pos, symbols)?;
            let c = read_u32(data, pos)? as usize;
            let mut args = Vec::with_capacity(c);
            for _ in 0..c {
                args.push(read_expr(data, pos, symbols)?);
            }
            let s = read_span(data, pos)?;
            Some(HirExpr::Call {
                id: make_id(),
                callee,
                args,
                span: s,
            })
        }
        TAG_STRUCT_LIT => {
            let n = read_sym_index(data, pos, symbols)?;
            let c = read_u32(data, pos)? as usize;
            let mut fields = Vec::with_capacity(c);
            for _ in 0..c {
                let f = read_sym_index(data, pos, symbols)?;
                let v = read_expr(data, pos, symbols)?;
                fields.push((f, v));
            }
            let s = read_span(data, pos)?;
            Some(HirExpr::StructLit {
                id: make_id(),
                struct_name: n,
                fields,
                span: s,
            })
        }
        TAG_ENUM_VARIANT => {
            let en = read_sym_index(data, pos, symbols)?;
            let vn = read_sym_index(data, pos, symbols)?;
            let c = read_u32(data, pos)? as usize;
            let mut args = Vec::with_capacity(c);
            for _ in 0..c {
                args.push(read_expr(data, pos, symbols)?);
            }
            let s = read_span(data, pos)?;
            Some(HirExpr::EnumVariant {
                id: make_id(),
                enum_name: en,
                variant_name: vn,
                args,
                span: s,
            })
        }
        TAG_FIELD_ACCESS => {
            let o = read_expr(data, pos, symbols)?;
            let f = read_sym_index(data, pos, symbols)?;
            let s = read_span(data, pos)?;
            Some(HirExpr::FieldAccess {
                id: make_id(),
                object: Box::new(o),
                field: f,
                span: s,
            })
        }
        TAG_TUPLE_LIT => {
            let c = read_u32(data, pos)? as usize;
            let mut elems = Vec::with_capacity(c);
            for _ in 0..c {
                elems.push(read_expr(data, pos, symbols)?);
            }
            let s = read_span(data, pos)?;
            Some(HirExpr::TupleLit {
                id: make_id(),
                elements: elems,
                span: s,
            })
        }
        TAG_RETURN => {
            let v = if read_u8(data, pos)? != 0 {
                Some(Box::new(read_expr(data, pos, symbols)?))
            } else {
                None
            };
            let s = read_span(data, pos)?;
            Some(HirExpr::Return {
                id: make_id(),
                value: v,
                span: s,
            })
        }
        TAG_WHILE => {
            let c = read_expr(data, pos, symbols)?;
            let b = read_expr(data, pos, symbols)?;
            let s = read_span(data, pos)?;
            Some(HirExpr::While {
                id: make_id(),
                condition: Box::new(c),
                body: Box::new(b),
                span: s,
            })
        }
        TAG_DEREF => {
            let e = read_expr(data, pos, symbols)?;
            let s = read_span(data, pos)?;
            Some(HirExpr::Deref {
                id: make_id(),
                expr: Box::new(e),
                span: s,
            })
        }
        TAG_PRINTLN => {
            let a = read_expr(data, pos, symbols)?;
            let s = read_span(data, pos)?;
            Some(HirExpr::Println {
                id: make_id(),
                arg: Box::new(a),
                span: s,
            })
        }
        TAG_ASSERT => {
            let c = read_expr(data, pos, symbols)?;
            let m = if read_u8(data, pos)? != 0 {
                Some(Box::new(read_expr(data, pos, symbols)?))
            } else {
                None
            };
            let s = read_span(data, pos)?;
            Some(HirExpr::Assert {
                id: make_id(),
                condition: Box::new(c),
                message: m,
                span: s,
            })
        }
        _ => None,
    }
}

fn read_stmt(data: &[u8], pos: &mut usize, symbols: &[String]) -> Option<HirStmt> {
    let tag = read_u8(data, pos)?;
    let _make_id = || glyim_hir::types::ExprId::new(0);
    match tag {
        1 => {
            let n = read_sym_index(data, pos, symbols)?;
            let m = read_bool(data, pos)?;
            let v = read_expr(data, pos, symbols)?;
            let s = read_span(data, pos)?;
            Some(HirStmt::Let {
                name: n,
                mutable: m,
                value: v,
                span: s,
            })
        }
        2 => {
            let m = read_bool(data, pos)?;
            let v = read_expr(data, pos, symbols)?;
            let s = read_span(data, pos)?;
            Some(HirStmt::LetPat {
                pattern: glyim_hir::HirPattern::Wild,
                mutable: m,
                value: v,
                ty: None,
                span: s,
            })
        }
        3 => {
            let t = read_sym_index(data, pos, symbols)?;
            let v = read_expr(data, pos, symbols)?;
            let s = read_span(data, pos)?;
            Some(HirStmt::Assign {
                target: t,
                value: v,
                span: s,
            })
        }
        4 => {
            let t = read_expr(data, pos, symbols)?;
            let v = read_expr(data, pos, symbols)?;
            let s = read_span(data, pos)?;
            Some(HirStmt::AssignDeref {
                target: Box::new(t),
                value: v,
                span: s,
            })
        }
        5 => {
            let o = read_expr(data, pos, symbols)?;
            let f = read_sym_index(data, pos, symbols)?;
            let v = read_expr(data, pos, symbols)?;
            let s = read_span(data, pos)?;
            Some(HirStmt::AssignField {
                object: Box::new(o),
                field: f,
                value: v,
                span: s,
            })
        }
        6 => {
            let e = read_expr(data, pos, symbols)?;
            Some(HirStmt::Expr(e))
        }
        _ => None,
    }
}

fn u8_to_binop(v: u8) -> Option<HirBinOp> {
    match v {
        1 => Some(HirBinOp::Add),
        2 => Some(HirBinOp::Sub),
        3 => Some(HirBinOp::Mul),
        4 => Some(HirBinOp::Div),
        5 => Some(HirBinOp::Mod),
        6 => Some(HirBinOp::Eq),
        7 => Some(HirBinOp::Neq),
        8 => Some(HirBinOp::Lt),
        9 => Some(HirBinOp::Gt),
        10 => Some(HirBinOp::Lte),
        11 => Some(HirBinOp::Gte),
        12 => Some(HirBinOp::And),
        13 => Some(HirBinOp::Or),
        _ => None,
    }
}

fn u8_to_unop(v: u8) -> Option<glyim_hir::HirUnOp> {
    match v {
        0 => Some(glyim_hir::HirUnOp::Neg),
        1 => Some(glyim_hir::HirUnOp::Not),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glyim_diag::Span;
    use glyim_hir::types::ExprId;

    #[test]
    fn roundtrip_int_lit_preserves_value_and_span() {
        let expr = HirExpr::IntLit {
            id: ExprId::new(0),
            value: 42,
            span: Span::new(10, 12),
        };
        let bytes = serialize_expr(&expr);
        let (back, _) = deserialize_expr(&bytes).expect("deserialize");
        match back {
            HirExpr::IntLit { value, span, .. } => {
                assert_eq!(value, 42);
                assert_eq!(span.start, 10);
                assert_eq!(span.end, 12);
            }
            _ => panic!("expected IntLit"),
        }
    }

    #[test]
    fn roundtrip_str_lit_preserves_content() {
        let expr = HirExpr::StrLit {
            id: ExprId::new(0),
            value: "hello world".into(),
            span: Span::new(0, 11),
        };
        let bytes = serialize_expr(&expr);
        let (back, _) = deserialize_expr(&bytes).expect("deserialize");
        match back {
            HirExpr::StrLit { value, .. } => assert_eq!(value, "hello world"),
            _ => panic!("expected StrLit"),
        }
    }

    #[test]
    fn roundtrip_binary_expr() {
        let lhs = HirExpr::IntLit {
            id: ExprId::new(0),
            value: 1,
            span: Span::new(0, 1),
        };
        let rhs = HirExpr::IntLit {
            id: ExprId::new(1),
            value: 2,
            span: Span::new(2, 3),
        };
        let expr = HirExpr::Binary {
            id: ExprId::new(2),
            op: HirBinOp::Add,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
            span: Span::new(0, 3),
        };
        let bytes = serialize_expr(&expr);
        let (back, _syms) = deserialize_expr(&bytes).expect("deserialize");
        // No symbols needed for int-only binary expression; just verify correctness
        match back {
            HirExpr::Binary { op, .. } => assert_eq!(op, HirBinOp::Add),
            _ => panic!("expected Binary"),
        }
    }

    #[test]
    fn roundtrip_nested_block() {
        let inner = HirExpr::IntLit {
            id: ExprId::new(0),
            value: 99,
            span: Span::new(1, 3),
        };
        let block = HirExpr::Block {
            id: ExprId::new(1),
            stmts: vec![HirStmt::Expr(inner.clone()), HirStmt::Expr(inner)],
            span: Span::new(0, 5),
        };
        let bytes = serialize_expr(&block);
        let (back, _) = deserialize_expr(&bytes).expect("deserialize");
        match back {
            HirExpr::Block { stmts, .. } => assert_eq!(stmts.len(), 2),
            _ => panic!("expected Block"),
        }
    }
}
