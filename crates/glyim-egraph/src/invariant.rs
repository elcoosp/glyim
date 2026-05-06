use glyim_hir::node::HirFn;
use glyim_hir::types::HirType;
use glyim_interner::Interner;
use sha2::{Digest, Sha256};

/// Summarizes optimization-relevant properties of a function.
/// When two builds produce the same certificate, the optimization
/// result is guaranteed identical, so the e-graph pass can be skipped.
#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct InvariantCertificate {
    pub signature_hash: [u8; 32],
    pub is_pure: bool,
    pub may_panic: bool,
    pub may_allocate: bool,
    pub complexity: u32,
    pub ir_size: u32,
    pub callees: Vec<String>,
    pub canonical_form_hash: [u8; 32],
    pub rule_set_version: u32,
}

impl InvariantCertificate {
    pub const RULE_SET_VERSION: u32 = 1;

    pub fn compute(
        hir_fn: &HirFn,
        interner: &Interner,
        _types: &[HirType],
    ) -> Self {
        let name = interner.resolve(hir_fn.name).to_string();
        let mut sig_hasher = Sha256::new();
        sig_hasher.update(name.as_bytes());
        for (_, ty) in &hir_fn.params {
            sig_hasher.update(format!("{:?}", ty).as_bytes());
        }
        if let Some(ret) = &hir_fn.ret {
            sig_hasher.update(format!("{:?}", ret).as_bytes());
        }
        let signature_hash = sig_hasher.finalize().into();

        let mut callees = Vec::new();
        collect_callees(&hir_fn.body, interner, &mut callees);

        let complexity = count_nodes(&hir_fn.body);
        let ir_size = complexity;

        let mut cf_hasher = Sha256::new();
        hash_expr(&hir_fn.body, interner, &mut cf_hasher);
        let canonical_form_hash = cf_hasher.finalize().into();

        InvariantCertificate {
            signature_hash,
            is_pure: false,
            may_panic: false,
            may_allocate: false,
            complexity,
            ir_size,
            callees,
            canonical_form_hash,
            rule_set_version: Self::RULE_SET_VERSION,
        }
    }

    /// Serialize this certificate to bytes (postcard).
    pub fn to_bytes(&self) -> Vec<u8> {
        postcard::to_allocvec(self).unwrap_or_default()
    }

    /// Deserialize a certificate from bytes (postcard).
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        postcard::from_bytes(bytes).ok()
    }

    /// Compute the content hash of this certificate (used as Merkle node hash).
    pub fn content_hash(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(self.signature_hash);
        hasher.update([self.is_pure as u8, self.may_panic as u8, self.may_allocate as u8]);
        hasher.update(self.complexity.to_le_bytes());
        hasher.update(self.ir_size.to_le_bytes());
        hasher.update(self.canonical_form_hash);
        hasher.update(self.rule_set_version.to_le_bytes());
        for callee in &self.callees {
            hasher.update(callee.as_bytes());
        }
        hasher.finalize().into()
    }
}

#[allow(unused_variables)]
fn collect_callees(expr: &glyim_hir::node::HirExpr, interner: &Interner, callees: &mut Vec<String>) {
    match expr {
        glyim_hir::node::HirExpr::Call { callee, .. } => {
            let name = interner.resolve(*callee).to_string();
            if !callees.contains(&name) { callees.push(name); }
        }
        _ => {}
    }
}

#[allow(unused_variables)]
fn count_nodes(expr: &glyim_hir::node::HirExpr) -> u32 {
    1
}

#[allow(unused_variables)]
fn hash_expr(expr: &glyim_hir::node::HirExpr, interner: &Interner, hasher: &mut Sha256) {
    hasher.update(format!("{:?}", expr).as_bytes());
}
