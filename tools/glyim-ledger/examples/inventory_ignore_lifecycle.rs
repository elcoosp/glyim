//! M2 consumer walkthrough: file one §3.5 inventory exemption — the exact
//! validation and write path `glyim-ledger inventory ignore [ROOT]` runs —
//! then retire it through `inventory unignore`, re-deriving the committed
//! document through the read face at every step.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-ledger --example inventory_ignore_lifecycle
//! ```
//!
//! A real filing mutates the repository's `ledger/inventory-ignore.toml`,
//! so the walkthrough writes into a throwaway registry instead of the
//! live one. It first rejects two invalid requests (a key no extractor
//! produces — exact keys, never globs — and a key a feature claims;
//! nothing is written), then files a valid exemption, re-runs the §3.5
//! gate over the committed registry, and finally retires the exemption
//! through the canonical rewrite. Try the CLI on your own tree
//! deliberately:
//!
//! ```text
//! cargo run -p glyim-ledger -- inventory ignore --item mir.StatementKind.Nop \
//!   --reason "no per-variant feature exists; parity rows cover it"
//! ```

use std::path::PathBuf;

use glyim_ledger::{
    FileIgnoreRewriter, FileIgnoreSink, InventoryIgnoreRequest, InventoryUnignoreRequest,
    format_inventory_ignore_text, run_inventory, run_inventory_ignore, run_inventory_unignore,
};

fn main() {
    // The write faces always write, so the walkthrough uses a throwaway
    // registry instead of the repository's live `ledger/` tree.
    let dir = tempfile::tempdir().expect("walkthrough tmp");
    let root = PathBuf::from(dir.path());
    std::fs::create_dir_all(root.join("ledger/features")).expect("mkdir features");
    std::fs::create_dir_all(root.join("ledger/gaps")).expect("mkdir gaps");
    std::fs::create_dir_all(root.join("crates/glyim-runtime/src")).expect("mkdir src");
    std::fs::write(
        root.join("crates/glyim-runtime/src/lib.rs"),
        "#[unsafe(no_mangle)]\npub extern \"C\" fn glyim_demo(size: usize) -> usize { size }\n",
    )
    .expect("write demo symbol");

    // Face 1: exact keys, never globs — a key no extractor produces right
    // now is a rejection (a phantom exemption would silently swallow
    // future drift), and nothing is written.
    let phantom = InventoryIgnoreRequest {
        item: "mir.StatementKind.Nope".to_string(),
        reason: "a phantom exemption must not land".to_string(),
    };
    let rejected = run_inventory_ignore(&root, &phantom, &FileIgnoreSink);
    assert!(rejected.is_err(), "the phantom key must be rejected");
    eprintln!(
        "rejected (unknown item): {}\n",
        rejected.expect_err("checked above")
    );
    assert!(
        !root.join("ledger/inventory-ignore.toml").exists(),
        "a rejected request must not create the registry"
    );

    // Face 2: claimed-and-ignored cannot both hold — the §3.5 gate
    // reports that combination as LEDGER-INVENTORY-CONFLICT, so the face
    // refuses up front.
    std::fs::create_dir_all(root.join("ledger/features/runtime")).expect("mkdir feature dir");
    std::fs::write(
        root.join("ledger/features/runtime/ffi.toml"),
        "[[feature]]\n\
         id = \"RUNTIME-FFI-DEMO\"\n\
         title = \"demo feature\"\n\
         area = \"runtime.ffi\"\n\
         tier = \"std\"\n\
         spec = \"docs/specs/v0.1.0.md\"\n\
         target_level = \"L4\"\n\
         backends = [\"gam\"]\n\
         inventory = [\"runtime.ffi.glyim_demo\"]\n\
         owner_lane = \"B\"\n",
    )
    .expect("write claiming feature");
    let claimed = InventoryIgnoreRequest {
        item: "runtime.ffi.glyim_demo".to_string(),
        reason: "claimed and ignored cannot both hold".to_string(),
    };
    let rejected = run_inventory_ignore(&root, &claimed, &FileIgnoreSink);
    assert!(rejected.is_err(), "the claimed key must be rejected");
    eprintln!(
        "rejected (claimed item): {}\n",
        rejected.expect_err("checked above")
    );

    // Retract the claim: with no feature claiming the symbol, the same
    // request files the canonical `[[ignore]]` block and the §3.5 gate
    // goes green over the committed registry.
    std::fs::remove_file(root.join("ledger/features/runtime/ffi.toml"))
        .expect("retract the demo claim");
    let valid = InventoryIgnoreRequest {
        item: "runtime.ffi.glyim_demo".to_string(),
        reason:
            "per-variant coverage rows are the backend-parity matrix; no per-variant feature exists"
                .to_string(),
    };
    let outcome =
        run_inventory_ignore(&root, &valid, &FileIgnoreSink).expect("the valid exemption lands");
    print!("{}", format_inventory_ignore_text(&outcome));
    let gate = run_inventory(&root);
    assert!(
        gate.is_ok(),
        "the filed exemption must satisfy the §3.5 gate: {:?}",
        gate.diagnostics
    );
    eprintln!("gate: green over the committed registry\n");

    // Face 3: the retirement half — one exact key, canonical rewrite of
    // the remaining entries; the item is unmapped again the moment the
    // rewrite lands.
    let retire = InventoryUnignoreRequest {
        item: "runtime.ffi.glyim_demo".to_string(),
    };
    let outcome = run_inventory_unignore(&root, &retire, &FileIgnoreRewriter)
        .expect("a registered exemption retires");
    assert_eq!(outcome.reason, valid.reason);
    assert_eq!(outcome.remaining, 0);
    let gate = run_inventory(&root);
    assert!(
        !gate.is_ok(),
        "the retired exemption unmapped the item: the §3.5 gate must fail loudly again"
    );
    eprintln!(
        "unignored {} — the gate fails loudly again ({} diagnostic(s)), forcing a feature claim",
        outcome.item,
        gate.errors()
    );
}
