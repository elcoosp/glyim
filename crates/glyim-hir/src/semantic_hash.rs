use crate::normalize::{SemanticNormalizer, NormalizedHirFn, NormalizedExpr, NormalizedStmt, NormalizedPattern};
use crate::{HirItem, HirFn};
use crate::item::StructDef;
use glyim_interner::Interner;
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct SemanticHash([u8; 32]);

impl SemanticHash {
    pub const ZERO: Self = Self([0u8; 32]);
    pub fn of(data: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(data);
        let digest = hasher.finalize();
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&digest);
        Self(bytes)
    }
    pub fn as_bytes(&self) -> &[u8; 32] { &self.0 }
    pub fn to_hex(self) -> String { self.0.iter().map(|b| format!("{:02x}", b)).collect() }
    pub fn combine(a: Self, b: Self) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(b"combine:");
        hasher.update(a.0);
        hasher.update(b.0);
        let digest = hasher.finalize();
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&digest);
        Self(bytes)
    }
}

impl std::fmt::Display for SemanticHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

pub fn semantic_hash_fn(hir_fn: &HirFn, interner: &Interner) -> SemanticHash {
    let mut normalizer = SemanticNormalizer::new(interner);
    let normalized = normalizer.normalize_fn(hir_fn);
    hash_normalized_fn(&normalized)
}

pub fn semantic_hash_item(item: &HirItem, interner: &Interner) -> SemanticHash {
    match item {
        HirItem::Fn(hir_fn) => semantic_hash_fn(hir_fn, interner),
        HirItem::Struct(s) => semantic_hash_struct(s, interner),
        HirItem::Enum(e) => {
            let mut data = Vec::new();
            data.extend_from_slice(b"enum:");
            data.extend_from_slice(interner.resolve(e.name).as_bytes());
            data.push(0);
            for v in &e.variants {
                data.extend_from_slice(interner.resolve(v.name).as_bytes());
                data.push(b':');
                data.extend_from_slice(&v.tag.to_le_bytes());
                data.push(0);
            }
            SemanticHash::of(&data)
        }
        HirItem::Impl(imp) => {
            let mut h = SemanticHash::of(interner.resolve(imp.target_name).as_bytes());
            for m in &imp.methods {
                h = SemanticHash::combine(h, semantic_hash_fn(m, interner));
            }
            h
        }
        HirItem::Extern(ext) => {
            let mut data = Vec::new();
            data.extend_from_slice(b"extern:");
            for func in &ext.functions {
                data.extend_from_slice(interner.resolve(func.name).as_bytes());
                data.push(0);
            }
            SemanticHash::of(&data)
        }
    }
}

fn semantic_hash_struct(s: &StructDef, interner: &Interner) -> SemanticHash {
    let mut data = Vec::new();
    data.extend_from_slice(b"struct:");
    data.extend_from_slice(interner.resolve(s.name).as_bytes());
    data.push(0);
    for tp in &s.type_params {
        data.extend_from_slice(interner.resolve(*tp).as_bytes());
        data.push(b',');
    }
    data.push(0);
    for field in &s.fields {
        data.extend_from_slice(interner.resolve(field.name).as_bytes());
        data.push(b':');
        data.extend_from_slice(format!("{:?}", field.ty).as_bytes());
        data.push(0);
    }
    SemanticHash::of(&data)
}

fn hash_normalized_fn(norm: &NormalizedHirFn) -> SemanticHash {
    let mut data = Vec::new();
    data.extend_from_slice(b"fn:");
    data.extend_from_slice(norm.name.as_bytes()); data.push(0);
    for tp in &norm.type_params {
        data.extend_from_slice(tp.as_bytes()); data.push(b',');
    }
    data.push(0);
    for (name, ty) in &norm.params {
        data.extend_from_slice(name.as_bytes()); data.push(b':');
        data.extend_from_slice(format!("{:?}", ty).as_bytes()); data.push(b',');
    }
    data.push(0);
    if let Some(ret) = &norm.ret { data.extend_from_slice(format!("{:?}", ret).as_bytes()); }
    data.push(0);
    data.push(norm.is_pub as u8);
    data.push(norm.is_extern_backed as u8);
    let body_hash = hash_normalized_expr(&norm.body);
    data.extend_from_slice(body_hash.as_bytes());
    SemanticHash::of(&data)
}

fn hash_normalized_expr(expr: &NormalizedExpr) -> SemanticHash {
    let mut buf = Vec::new();
    write_normalized_expr(&mut buf, expr);
    SemanticHash::of(&buf)
}

fn write_normalized_expr(buf: &mut Vec<u8>, expr: &NormalizedExpr) {
    match expr {
        NormalizedExpr::IntLit(v) => { buf.push(0x01); buf.extend_from_slice(&v.to_le_bytes()); }
        NormalizedExpr::FloatLit(bits) => { buf.push(0x02); buf.extend_from_slice(&bits.to_le_bytes()); }
        NormalizedExpr::BoolLit(b) => { buf.push(0x03); buf.push(*b as u8); }
        NormalizedExpr::StrLit(s) => { buf.push(0x04); buf.extend_from_slice(&(s.len() as u64).to_le_bytes()); buf.extend_from_slice(s.as_bytes()); }
        NormalizedExpr::UnitLit => { buf.push(0x05); }
        NormalizedExpr::Local(idx) => { buf.push(0x06); buf.extend_from_slice(&idx.to_le_bytes()); }
        NormalizedExpr::Name(s) => { buf.push(0x07); buf.extend_from_slice(&(s.len() as u64).to_le_bytes()); buf.extend_from_slice(s.as_bytes()); }
        NormalizedExpr::Binary { op, lhs, rhs } => { buf.push(0x08); buf.extend_from_slice(format!("{:?}", op).as_bytes()); buf.push(0); write_normalized_expr(buf, lhs); write_normalized_expr(buf, rhs); }
        NormalizedExpr::Unary { op, operand } => { buf.push(0x09); buf.extend_from_slice(format!("{:?}", op).as_bytes()); buf.push(0); write_normalized_expr(buf, operand); }
        NormalizedExpr::Block { stmts } => { buf.push(0x0A); buf.extend_from_slice(&(stmts.len() as u64).to_le_bytes()); for s in stmts { write_normalized_stmt(buf, s); } }
        NormalizedExpr::If { condition, then_branch, else_branch } => { buf.push(0x0B); write_normalized_expr(buf, condition); write_normalized_expr(buf, then_branch); if let Some(e) = else_branch { buf.push(1); write_normalized_expr(buf, e); } else { buf.push(0); } }
        NormalizedExpr::Call { callee, args } => { buf.push(0x0C); buf.extend_from_slice(callee.as_bytes()); buf.push(0); buf.extend_from_slice(&(args.len() as u64).to_le_bytes()); for a in args { write_normalized_expr(buf, a); } }
        NormalizedExpr::MethodCall { receiver, method_name, resolved_callee, args } => { buf.push(0x0D); write_normalized_expr(buf, receiver); buf.extend_from_slice(method_name.as_bytes()); buf.push(0); if let Some(c) = resolved_callee { buf.push(1); buf.extend_from_slice(c.as_bytes()); buf.push(0); } else { buf.push(0); } buf.extend_from_slice(&(args.len() as u64).to_le_bytes()); for a in args { write_normalized_expr(buf, a); } }
        NormalizedExpr::Assert { condition, message } => { buf.push(0x0E); write_normalized_expr(buf, condition); if let Some(m) = message { buf.push(1); write_normalized_expr(buf, m); } else { buf.push(0); } }
        NormalizedExpr::Match { scrutinee, arms } => { buf.push(0x0F); write_normalized_expr(buf, scrutinee); buf.extend_from_slice(&(arms.len() as u64).to_le_bytes()); for arm in arms { write_normalized_pattern(buf, &arm.pattern); if let Some(g) = &arm.guard { buf.push(1); write_normalized_expr(buf, g); } else { buf.push(0); } write_normalized_expr(buf, &arm.body); } }
        NormalizedExpr::FieldAccess { object, field } => { buf.push(0x10); write_normalized_expr(buf, object); buf.extend_from_slice(field.as_bytes()); buf.push(0); }
        NormalizedExpr::StructLit { struct_name, fields } => { buf.push(0x11); buf.extend_from_slice(struct_name.as_bytes()); buf.push(0); buf.extend_from_slice(&(fields.len() as u64).to_le_bytes()); for (n, e) in fields { buf.extend_from_slice(n.as_bytes()); buf.push(0); write_normalized_expr(buf, e); } }
        NormalizedExpr::EnumVariant { enum_name, variant_name, args } => { buf.push(0x12); buf.extend_from_slice(enum_name.as_bytes()); buf.push(0); buf.extend_from_slice(variant_name.as_bytes()); buf.push(0); buf.extend_from_slice(&(args.len() as u64).to_le_bytes()); for a in args { write_normalized_expr(buf, a); } }
        NormalizedExpr::ForIn { pattern, iter, body } => { buf.push(0x13); write_normalized_pattern(buf, pattern); write_normalized_expr(buf, iter); write_normalized_expr(buf, body); }
        NormalizedExpr::While { condition, body } => { buf.push(0x14); write_normalized_expr(buf, condition); write_normalized_expr(buf, body); }
        NormalizedExpr::Return { value } => { buf.push(0x15); if let Some(v) = value { buf.push(1); write_normalized_expr(buf, v); } else { buf.push(0); } }
        NormalizedExpr::As { expr, target_type } => { buf.push(0x16); write_normalized_expr(buf, expr); buf.extend_from_slice(format!("{:?}", target_type).as_bytes()); buf.push(0); }
        NormalizedExpr::SizeOf { target_type } => { buf.push(0x17); buf.extend_from_slice(format!("{:?}", target_type).as_bytes()); buf.push(0); }
        NormalizedExpr::TupleLit { elements } => { buf.push(0x18); buf.extend_from_slice(&(elements.len() as u64).to_le_bytes()); for e in elements { write_normalized_expr(buf, e); } }
        NormalizedExpr::AddrOf { target } => { buf.push(0x19); buf.extend_from_slice(target.as_bytes()); buf.push(0); }
        NormalizedExpr::Deref { expr: e } => { buf.push(0x1A); write_normalized_expr(buf, e); }
        NormalizedExpr::Println { arg } => { buf.push(0x1B); write_normalized_expr(buf, arg); }
    }
}

fn write_normalized_stmt(buf: &mut Vec<u8>, stmt: &NormalizedStmt) {
    match stmt {
        NormalizedStmt::Let { local_id, mutable, value } => { buf.push(0x01); buf.extend_from_slice(&local_id.to_le_bytes()); buf.push(*mutable as u8); write_normalized_expr(buf, value); }
        NormalizedStmt::Assign { local_id, value } => { buf.push(0x02); buf.extend_from_slice(&local_id.to_le_bytes()); write_normalized_expr(buf, value); }
        NormalizedStmt::AssignField { object, field, value } => { buf.push(0x03); write_normalized_expr(buf, object); buf.extend_from_slice(field.as_bytes()); buf.push(0); write_normalized_expr(buf, value); }
        NormalizedStmt::AssignDeref { target, value } => { buf.push(0x04); write_normalized_expr(buf, target); write_normalized_expr(buf, value); }
        NormalizedStmt::Expr(expr) => { buf.push(0x05); write_normalized_expr(buf, expr); }
    }
}

fn write_normalized_pattern(buf: &mut Vec<u8>, pat: &NormalizedPattern) {
    match pat {
        NormalizedPattern::Wild => buf.push(0x01),
        NormalizedPattern::BoolLit(b) => { buf.push(0x02); buf.push(*b as u8); }
        NormalizedPattern::IntLit(n) => { buf.push(0x03); buf.extend_from_slice(&n.to_le_bytes()); }
        NormalizedPattern::FloatLit(bits) => { buf.push(0x04); buf.extend_from_slice(&bits.to_le_bytes()); }
        NormalizedPattern::StrLit(s) => { buf.push(0x05); buf.extend_from_slice(s.as_bytes()); buf.push(0); }
        NormalizedPattern::Unit => buf.push(0x06),
        NormalizedPattern::Local(idx) => { buf.push(0x07); buf.extend_from_slice(&idx.to_le_bytes()); }
        NormalizedPattern::Struct { name, bindings } => { buf.push(0x08); buf.extend_from_slice(name.as_bytes()); buf.push(0); buf.extend_from_slice(&(bindings.len() as u64).to_le_bytes()); for (f, p) in bindings { buf.extend_from_slice(f.as_bytes()); buf.push(0); write_normalized_pattern(buf, p); } }
        NormalizedPattern::EnumVariant { enum_name, variant_name, bindings } => { buf.push(0x09); buf.extend_from_slice(enum_name.as_bytes()); buf.push(0); buf.extend_from_slice(variant_name.as_bytes()); buf.push(0); buf.extend_from_slice(&(bindings.len() as u64).to_le_bytes()); for (n, p) in bindings { buf.extend_from_slice(n.as_bytes()); buf.push(0); write_normalized_pattern(buf, p); } }
        NormalizedPattern::Tuple { elements } => { buf.push(0x0A); buf.extend_from_slice(&(elements.len() as u64).to_le_bytes()); for e in elements { write_normalized_pattern(buf, e); } }
        NormalizedPattern::OptionSome(inner) => { buf.push(0x0B); write_normalized_pattern(buf, inner); }
        NormalizedPattern::OptionNone => buf.push(0x0C),
        NormalizedPattern::ResultOk(inner) => { buf.push(0x0D); write_normalized_pattern(buf, inner); }
        NormalizedPattern::ResultErr(inner) => { buf.push(0x0E); write_normalized_pattern(buf, inner); }
    }
}
