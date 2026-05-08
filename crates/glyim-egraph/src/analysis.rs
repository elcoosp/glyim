use egg::{Analysis, DidMerge, EGraph, Id};
use glyim_hir::types::HirType;

#[derive(Clone, Debug, Default)]
pub struct GlyimAnalysis {
    pub constant: Option<ConstValue>,
    pub is_pure: bool,
    pub cost: f64,
    pub ty: Option<HirType>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ConstValue {
    Int(i64),
    Float(u64),
    Bool(bool),
}

impl Analysis<crate::lang::GlyimLang> for GlyimAnalysis {
    type Data = GlyimAnalysis;
    fn make(_egraph: &mut EGraph<crate::lang::GlyimLang, Self>, enode: &crate::lang::GlyimLang, _id: Id) -> Self::Data {
        let mut data = GlyimAnalysis::default();
        match enode {
            crate::lang::GlyimLang::Num(n) => {
                data.constant = Some(ConstValue::Int(*n));
                data.is_pure = true; data.cost = 1.0; data.ty = Some(HirType::Int);
            }
            crate::lang::GlyimLang::BoolLit(b) => {
                data.constant = Some(ConstValue::Bool(*b));
                data.is_pure = true; data.cost = 1.0; data.ty = Some(HirType::Bool);
            }
            crate::lang::GlyimLang::FNum(bits) => {
                data.constant = Some(ConstValue::Float(*bits));
                data.is_pure = true; data.cost = 1.0; data.ty = Some(HirType::Float);
            }
            _ => { data.is_pure = false; data.cost = 1.0; }
        }
        data
    }
    fn merge(&mut self, a: &mut Self::Data, b: Self::Data) -> DidMerge {
        let mut changed = false;
        if a.constant.is_none() && b.constant.is_some() { a.constant = b.constant; changed = true; }
        if b.is_pure && !a.is_pure { a.is_pure = true; changed = true; }
        if b.ty.is_some() && a.ty.is_none() { a.ty = b.ty.clone(); changed = true; }
        DidMerge(changed, false)
    }
}
