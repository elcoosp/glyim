//! BFS work scheduling for monomorphization.
//!
//! A `WorkItem` represents one unit of work: specialize a generic item
//! with concrete type arguments, or pass through a non-generic item.
//! The `WorkQueue` deduplicates identical work items so each specialization
//! is produced exactly once.

use crate::types::HirType;
use glyim_interner::Symbol;
use std::collections::{HashSet, VecDeque};

/// Whether the item needs specialization (generic) or just passthrough (non-generic).
#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq)]
pub enum ItemKind {
    /// Specialize a generic function with the given type args.
    FnSpecialize,

    /// Walk a non-generic function body to rewrite call targets
    /// and build concrete expr_types. No type substitution needed.
    FnPassthrough,

    /// Specialize a generic struct with the given type args.
    StructSpecialize,

    /// Emit a non-generic struct unchanged.
    StructPassthrough,

    /// Specialize a generic enum with the given type args.
    EnumSpecialize,

    /// Emit a non-generic enum unchanged.
    EnumPassthrough,
}

/// A unit of work for the monomorphizer BFS loop.
///
/// Each item uniquely identifies one specialization:
///   - `FnSpecialize("id", [Int])` → produce `id__i64`
///   - `FnSpecialize("id", [Bool])` → produce `id__bool`
///   - `FnPassthrough("main")` → emit `main` with rewritten calls
///
/// Two items are equal iff all three fields are equal, so
/// `FnSpecialize("id", [Int])` ≠ `FnPassthrough("id")`.
#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub struct WorkItem {
    /// The definition's name symbol (as stored in MonoIndex).
    /// For impl methods, this is the mangled name (e.g., `Vec_push`).
    pub def_id: Symbol,

    /// What kind of processing this item needs.
    pub kind: ItemKind,

    /// Concrete type arguments for specialization.
    /// Empty for passthrough items.
    /// Must be fully concrete (no unresolved type params) for specialize items.
    pub type_args: Vec<HirType>,
}

impl WorkItem {
    // ── Constructor helpers ──────────────────────────────────────────

    pub fn fn_specialize(id: Symbol, args: Vec<HirType>) -> Self {
        Self { def_id: id, kind: ItemKind::FnSpecialize, type_args: args }
    }

    pub fn fn_passthrough(id: Symbol) -> Self {
        Self { def_id: id, kind: ItemKind::FnPassthrough, type_args: vec![] }
    }

    pub fn struct_specialize(id: Symbol, args: Vec<HirType>) -> Self {
        Self { def_id: id, kind: ItemKind::StructSpecialize, type_args: args }
    }

    pub fn struct_passthrough(id: Symbol) -> Self {
        Self { def_id: id, kind: ItemKind::StructPassthrough, type_args: vec![] }
    }

    pub fn enum_specialize(id: Symbol, args: Vec<HirType>) -> Self {
        Self { def_id: id, kind: ItemKind::EnumSpecialize, type_args: args }
    }

    pub fn enum_passthrough(id: Symbol) -> Self {
        Self { def_id: id, kind: ItemKind::EnumPassthrough, type_args: vec![] }
    }
}

/// Deduplicating BFS work queue.
///
/// Items are popped in FIFO order. If an identical `WorkItem` was
/// previously pushed, subsequent pushes are silently dropped.
/// This prevents infinite loops and redundant specialization.
pub struct WorkQueue {
    queue: VecDeque<WorkItem>,
    seen: HashSet<WorkItem>,
}

impl WorkQueue {
    pub fn new() -> Self {
        Self {
            queue: VecDeque::new(),
            seen: HashSet::new(),
        }
    }

    /// Push an item. If an identical item was already pushed or processed,
    /// this is a no-op.
    pub fn push(&mut self, item: WorkItem) {
        if self.seen.insert(item.clone()) {
            self.queue.push_back(item);
        }
    }

    /// Push multiple items at once.
    pub fn extend(&mut self, items: impl IntoIterator<Item = WorkItem>) {
        for item in items {
            self.push(item);
        }
    }

    /// Pop the next item to process (FIFO order).
    pub fn pop(&mut self) -> Option<WorkItem> {
        self.queue.pop_front()
    }

    /// Returns true if no items remain.
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Number of items waiting to be processed.
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    /// Number of unique items ever pushed (including already-processed ones).
    #[allow(dead_code)]
    pub fn total_seen(&self) -> usize {
        self.seen.len()
    }
}

impl Default for WorkQueue {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glyim_interner::Interner;

    #[test]
    fn work_queue_fifo_order() {
        let mut queue = WorkQueue::new();
        let mut i = Interner::new();
        let a = i.intern("a");
        let b = i.intern("b");

        queue.push(WorkItem::fn_passthrough(a));
        queue.push(WorkItem::fn_passthrough(b));

        assert_eq!(queue.pop().unwrap().def_id, a);
        assert_eq!(queue.pop().unwrap().def_id, b);
        assert!(queue.is_empty());
    }

    #[test]
    fn work_queue_deduplicates_identical_items() {
        let mut queue = WorkQueue::new();
        let mut i = Interner::new();
        let foo = i.intern("foo");

        queue.push(WorkItem::fn_specialize(foo, vec![HirType::Int]));
        queue.push(WorkItem::fn_specialize(foo, vec![HirType::Int])); // duplicate
        queue.push(WorkItem::fn_specialize(foo, vec![HirType::Bool])); // different

        assert_eq!(queue.len(), 2);

        let first = queue.pop().unwrap();
        assert_eq!(first.type_args, vec![HirType::Int]);

        let second = queue.pop().unwrap();
        assert_eq!(second.type_args, vec![HirType::Bool]);

        assert!(queue.is_empty());
    }

    #[test]
    fn work_queue_different_kinds_not_deduplicated() {
        let mut queue = WorkQueue::new();
        let mut i = Interner::new();
        let foo = i.intern("foo");

        queue.push(WorkItem::fn_specialize(foo, vec![]));
        queue.push(WorkItem::fn_passthrough(foo));

        assert_eq!(queue.len(), 2, "different ItemKinds should not deduplicate");
    }

    #[test]
    fn work_queue_extend() {
        let mut queue = WorkQueue::new();
        let mut i = Interner::new();

        queue.extend(vec![
            WorkItem::fn_passthrough(i.intern("a")),
            WorkItem::fn_passthrough(i.intern("b")),
            WorkItem::fn_passthrough(i.intern("a")), // duplicate
        ]);

        assert_eq!(queue.len(), 2);
    }

    #[test]
    fn work_item_equality_by_all_fields() {
        let mut i = Interner::new();
        let id = i.intern("id");

        let spec = WorkItem::fn_specialize(id, vec![HirType::Int]);
        let pass = WorkItem::fn_passthrough(id);

        assert_ne!(spec, pass, "FnSpecialize and FnPassthrough are different items");
    }
}
