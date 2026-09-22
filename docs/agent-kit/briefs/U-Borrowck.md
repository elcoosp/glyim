# Stream U-Borrowck: Unstub Borrow Checker

## Mission
Remove all stubs in `glyim-borrowck` related to move analysis, two-phase borrow cross-block tracking, and projection conflicts. Ensure fully functional, production-ready implementations with zero stubs.

## What You Own Exclusively
- `crates/glyim-borrowck/src/move_analysis.rs`
- `crates/glyim-borrowck/src/twophase.rs`
- `crates/glyim-borrowck/src/visitor.rs`
- `crates/glyim-borrowck/src/tests/u_borrowck.rs` (NEW FILE)
- `crates/glyim-borrowck/src/tests/mod.rs` (MODIFY - safe append only)

## Exact Implementation Guide (NO STUBS ALLOWED)

### 1. Fix `count_fields` in `move_analysis.rs`
Replace the flawed `substs.len()` logic for ADTs. Query the `AdtDef` from the context to get the real field count.
Replace:
```rust
        glyim_type::TyKind::Adt(_, substs) => Some(u32::from(substs.len())),
```
With:
```rust
        glyim_type::TyKind::Adt(adt_id, _) => {
            ctx.adt_def(*adt_id).and_then(|adt| adt.variants.first().map(|v| v.fields.len() as u32))
        }
```

### 2. Fix `MovePathArena::find` for Index Projections in `move_analysis.rs`
Arrays and slices are moved wholesale. If a `ProjectionElem::Index` is encountered, return the root move path of the local immediately.
Inside the `for proj_elem in place.projection.iter()` loop, add this before the existing match:
```rust
            if let ProjectionElem::Index(_) = proj_elem {
                return Some(current_idx);
            }
```

### 3. Implement Cross-Block Reservation in `twophase.rs`
Replace the `if block != self.creation_block { return false; }` stub with a forward dataflow analysis in `ReservationAnalysis::compute`.
- Initialize a worklist starting with the successors of `loan_block`.
- Track visited blocks to prevent cycles.
- For each block, scan statements. If `dest_local` is read, STOP (reservation ends, activated).
- If the end of the block is reached without reading `dest_local`, add its successors to the worklist.
- Store all visited `(block, stmt_idx)` points in the `per_block` BitSet.

### 4. Fix Mixed Projections in `visitor.rs::places_conflict`
Replace `_ => return true` with disjointness logic. If one projection is `Field` and the other is `Index` on the same local, they refer to different physical regions. Return `false`.
Replace:
```rust
            // Mixed projection types at the same depth — conservatively conflict
            _ => return true,
```
With:
```rust
            (ProjectionElem::Field(_), ProjectionElem::Index(_)) |
            (ProjectionElem::Index(_), ProjectionElem::Field(_)) => return false,
            // Mixed projection types at the same depth — conservatively conflict
            _ => return true,
```

## Verification Commands
```bash
cargo test -p glyim-borrowck
cargo clippy -p glyim-borrowck -- -D warnings
cargo check --workspace
```
