//! CT-1 private-constructor ratchet — the workspace-level gate for plan M3.1.
//!
//! > plan `docs/roadmaps/glyim-master-plan.md` §6, milestone M3.1 DoD:
//! > "`from_raw` outside arena modules = 0 (ledger metric)" (CT-1 in §5.2).
//!
//! The metric (`RepoMetrics::from_raw_count`) counts exact `from_raw(`
//! call sites across every non-test `.rs` file, with test regions and
//! `tests/` files excluded exactly as the walker does. After the M3.1
//! codemod every ID family outside its arena module is constructed via the
//! sanctioned `from_usize` factory, the raw trait machinery is named
//! `IdxLike::from_u32`, and `IndexVec::from_raw` became `IndexVec::from_vec`
//! — so this count must be zero. If it is non-zero, a raw constructor
//! escape hatch has reappeared and this test fails loudly (the plan's C2
//! "every claim has a witness" applied to the ratchet itself).

use std::path::Path;

#[test]
fn ct1_workspace_from_raw_count_is_zero() {
    // tools/glyim-metrics -> workspace root
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let metrics = glyim_metrics::scan_tree(&repo_root);
    assert_eq!(
        metrics.from_raw_count, 0,
        "plan M3.1 (CT-1) DoD `from_raw outside arena modules = 0` violated: \
         {} raw-constructor site(s) remain — migrate them to the sanctioned \
         `from_usize` factories (scripts/codemods/) instead of re-widening \
         the escape hatch",
        metrics.from_raw_count
    );
}
