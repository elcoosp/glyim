use glyim_hir::{HirFn, HirType};
use glyim_interner::Symbol;
use std::collections::HashMap;

#[allow(dead_code)]
pub(crate) fn instantiate_fn(f: &HirFn, concrete: &[HirType]) -> HirFn {
    let mut sub = HashMap::new();
    for (i, tp) in f.type_params.iter().enumerate() {
        if let Some(ct) = concrete.get(i) {
            sub.insert(*tp, ct.clone());
        }
    }
    let mut mono = f.clone();
    mono.type_params.clear();
    for (_, pt) in &mut mono.params {
        *pt = apply(&sub, pt);
    }
    if let Some(rt) = &mut mono.ret {
        *rt = apply(&sub, rt);
    }
    mono
}

fn apply(sub: &HashMap<Symbol, HirType>, t: &HirType) -> HirType {
    match t {
        HirType::Generic(s, _) if sub.contains_key(s) => sub[s].clone(),
        _ => t.clone(),
    }
}
