//! Codegen unit partitioning.
//!
//! Groups monomorphized items into codegen units (CGUs) for parallel code generation.
//! Strategy: group by source module (deterministically, by module id), then if the
//! number of module groups exceeds `max_cgus`, merge the smallest groups into the
//! largest ones. The result is reproducible: identical input always yields identical
//! CGU assignments (content-addressed), which is required for stable incremental caches.

use crate::mono::MonoItemData;
use std::collections::BTreeMap;

/// Partition mono items into at most `max_cgus` codegen units.
///
/// Returns a vector of CGUs, each containing the indices of the items in that CGU.
/// Output is deterministic for identical input (groups are keyed by module id and
/// each group's indices are kept sorted), so the partition can be relied on by
/// content-addressed caches.
pub fn partition(items: &[MonoItemData], max_cgus: usize) -> Vec<Vec<usize>> {
    if items.is_empty() {
        return vec![];
    }

    // Group items by source module. `BTreeMap` keeps groups ordered by module id
    // so the partition is reproducible across calls (a plain `HashMap` would yield
    // a non-deterministic outer ordering).
    let mut module_groups: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    for (idx, item) in items.iter().enumerate() {
        module_groups
            .entry(item.source_module)
            .or_default()
            .push(idx);
    }

    // Keep each group's indices sorted so the partition is byte-for-byte stable.
    let mut groups: Vec<Vec<usize>> = module_groups
        .into_values()
        .map(|mut g| {
            g.sort_unstable();
            g
        })
        .collect();

    // If we have more groups than allowed, merge the smallest groups into the largest.
    while groups.len() > max_cgus {
        // Find index of smallest group and largest group by length. Ties are broken
        // deterministically: `min_by_key`/`max_by_key` keep the first element on
        // equal keys, and `groups` is already in a stable module-id order.
        let (smallest_idx, _) = groups
            .iter()
            .enumerate()
            .min_by_key(|(_, g)| g.len())
            .expect("groups is non-empty");

        let (largest_idx, _) = groups
            .iter()
            .enumerate()
            .max_by_key(|(_, g)| g.len())
            .expect("groups is non-empty");

        if smallest_idx == largest_idx {
            // All groups equal size; just merge any two.
            // Take first and second, merge second into first.
            if groups.len() < 2 {
                break;
            }
            let second = groups.remove(1);
            groups[0].extend(second);
            groups[0].sort_unstable();
        } else {
            // Move elements from smallest group into largest group.
            let mut smallest = groups.remove(smallest_idx);
            // After removal, largest_idx may have shifted if it was after smallest.
            let target = if largest_idx > smallest_idx {
                largest_idx - 1
            } else {
                largest_idx
            };
            groups[target].append(&mut smallest);
            groups[target].sort_unstable();
        }
    }

    groups
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mono::MonoItem;
    use glyim_core::def_id::{CrateId, DefId, LocalDefId, StaticDefId};
    use glyim_mir::Body;
    use std::sync::Arc;

    fn item(source_module: u32, idx: u32) -> MonoItemData {
        let def_id = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(idx));
        let static_id = StaticDefId::from_raw(idx);
        MonoItemData {
            item: MonoItem::Static { def_id: static_id },
            body: Arc::new(Body::dummy(def_id)),
            symbol: format!("sym_{idx}"),
            source_module,
        }
    }

    #[test]
    fn partition_into_cgus_is_a_true_partition() {
        // Every item appears in exactly one bucket; union of buckets equals the
        // full item set; nothing duplicated or dropped.
        let items: Vec<MonoItemData> = (0..12).map(|i| item(i % 4, i)).collect();
        let cgus = partition(&items, 3);

        let total: usize = cgus.iter().map(|g| g.len()).sum();
        assert_eq!(total, items.len(), "every item must be placed exactly once");

        let mut seen = std::collections::HashSet::new();
        for g in &cgus {
            for &idx in g {
                assert!(seen.insert(idx), "item {} duplicated across CGUs", idx);
            }
        }
        assert_eq!(seen.len(), items.len());
        // At most `max_cgus` groups (some may be empty only when items are empty).
        assert!(cgus.len() <= 3);
    }

    #[test]
    fn partition_is_stable_across_calls_with_same_input() {
        // Two calls with the same items + max_cgus produce identical bucket
        // assignments (content-addressed by source_module, not insertion order).
        let items: Vec<MonoItemData> = (0..10).map(|i| item(i % 3, i)).collect();
        let a = partition(&items, 4);
        let b = partition(&items, 4);
        assert_eq!(a, b, "partition must be deterministic for identical input");
    }
}
