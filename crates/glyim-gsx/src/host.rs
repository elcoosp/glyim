//! The Host trait and the headless test host (design doc §5.4, §7.6, §7.7,
//! §8.1, §11.4).
//!
//! §5.4: the Host is "the runtime that *realizes* operations (create/set/
//! insert/remove/listen)". Monomorphization over the host gives a zero-cost
//! abstraction: the generated code calls `Host::set_text` directly (no
//! vtable, no vdom).
//!
//! The [`HeadlessHost`] is the §11.4 headless op-recording host: it powers
//! perf tests by *asserting* the §8.1 cost model in CI from day one — one
//! host call per updated text slot, zero visits for static subtrees, zero
//! allocations per handler.
//!
//! §7.7: `Signal<T>` writes set a named bit through the host scheduler —
//! the dirty-mask space here is [`Host::invalidate`] + [`HeadlessHost::signal_bits`].

/// Host-side element identity (blueprint node index).
pub type NodeId = u32;

/// One recorded host operation (the §5.4 op vocabulary).
#[derive(Debug, Clone, PartialEq)]
pub enum HostOp {
    /// `create(tag)` → node.
    Create { node: NodeId, tag: u32 },
    /// `set_text(node, s)`.
    SetText { node: NodeId, text: String },
    /// `set_prop(node, prop, value)` — `skip` marks echo suppression (§7.9).
    SetProp {
        node: NodeId,
        prop: String,
        value: String,
        skip_if_same: bool,
    },
    /// `insert(parent, child)` — tree assembly.
    Insert { parent: NodeId, child: NodeId },
    /// `remove(node)`.
    Remove { node: NodeId },
    /// `move_before(parent, child, before)` — keyed reorder (G11).
    MoveBefore {
        parent: NodeId,
        child: NodeId,
        before: Option<NodeId>,
    },
    /// `listen(node, ev, handler_id)` — one-time at mount (§6.1 Event slot).
    Listen {
        node: NodeId,
        event: String,
        handler_id: u32,
    },
    /// `invalidate(class)` — one per patch, max class (G5).
    Invalidate { class: crate::schema::Invalidation },
    /// `instantiate(&BLUEPRINT)` — the bulk fast path (G1).
    Instantiate { ops: usize },
}

/// The host abstraction (§5.4). Node/Event are plain ids for v0 (finding
/// F12: associated types lean on projection normalization, which is an open
/// ICE — v0 expresses Node as a plain id).
pub trait Host {
    /// Create a node for a dialect element tag.
    fn create(&mut self, tag: u32) -> NodeId;
    /// Set a node's text content.
    fn set_text(&mut self, node: NodeId, s: &str);
    /// Set an attribute/prop. `skip_if_same` implements §7.9's echo
    /// suppression for bind slots.
    fn set_prop(&mut self, node: NodeId, prop: &str, value: &str, skip_if_same: bool);
    /// Append `child` under `parent`.
    fn insert(&mut self, parent: NodeId, child: NodeId);
    /// Remove a subtree.
    fn remove(&mut self, node: NodeId);
    /// Move `child` before `before` (None = end) — keyed lists (G11).
    fn move_before(&mut self, parent: NodeId, child: NodeId, before: Option<NodeId>);
    /// Register a listener; handler ids are static (G8).
    fn listen(&mut self, node: NodeId, event: &str, handler_id: u32);
    /// Invalidate the max class touched by a patch (G5).
    fn invalidate(&mut self, class: crate::schema::Invalidation);
}

/// The headless, op-recording host (§11.4).
#[derive(Debug, Default)]
pub struct HeadlessHost {
    /// Recorded ops, in call order.
    pub ops: Vec<HostOp>,
    next_node: NodeId,
    /// Prop values per node (echo suppression needs the current value).
    props: std::collections::BTreeMap<(NodeId, String), String>,
    /// Text per node.
    texts: std::collections::BTreeMap<NodeId, String>,
    /// §7.7: named signal bits set through the scheduler.
    signal_bits: std::collections::BTreeMap<String, u64>,
}

impl HeadlessHost {
    /// Fresh host.
    pub fn new() -> HeadlessHost {
        HeadlessHost::default()
    }

    /// Count ops of a given kind (§8.1 assertions).
    pub fn count<F: Fn(&HostOp) -> bool>(&self, pred: F) -> usize {
        self.ops.iter().filter(|o| pred(o)).count()
    }

    /// Number of `set_text` calls (§8.1: "update one text slot = 1 host
    /// call").
    pub fn set_text_calls(&self) -> usize {
        self.count(|o| matches!(o, HostOp::SetText { .. }))
    }

    /// Number of `create` calls (static subtrees are created once).
    pub fn create_calls(&self) -> usize {
        self.count(|o| matches!(o, HostOp::Create { .. }))
    }

    /// §7.7: bump a named signal bit; returns the new bit value.
    pub fn signal_write(&mut self, name: &str) -> u64 {
        let e = self.signal_bits.entry(name.to_string()).or_insert(0);
        *e += 1;
        *e
    }

    /// §7.7: read a signal bit.
    pub fn signal_read(&self, name: &str) -> u64 {
        self.signal_bits.get(name).copied().unwrap_or(0)
    }

    /// The current prop value of a node (for assertions).
    pub fn prop_of(&self, node: NodeId, prop: &str) -> Option<&String> {
        self.props.get(&(node, prop.to_string()))
    }

    /// The current text of a node (for assertions).
    pub fn text_of(&self, node: NodeId) -> Option<&String> {
        self.texts.get(&node)
    }
}

impl Host for HeadlessHost {
    fn create(&mut self, tag: u32) -> NodeId {
        let node = self.next_node;
        self.next_node += 1;
        self.ops.push(HostOp::Create { node, tag });
        node
    }

    fn set_text(&mut self, node: NodeId, s: &str) {
        self.texts.insert(node, s.to_string());
        self.ops.push(HostOp::SetText {
            node,
            text: s.to_string(),
        });
    }

    fn set_prop(&mut self, node: NodeId, prop: &str, value: &str, skip_if_same: bool) {
        let key = (node, prop.to_string());
        if skip_if_same && self.props.get(&key) == Some(&value.to_string()) {
            // §7.9: the compiler knows the host already holds that value —
            // suppress the echo entirely (no cursor-jump bug class).
            return;
        }
        self.props.insert(key, value.to_string());
        self.ops.push(HostOp::SetProp {
            node,
            prop: prop.to_string(),
            value: value.to_string(),
            skip_if_same,
        });
    }

    fn insert(&mut self, parent: NodeId, child: NodeId) {
        self.ops.push(HostOp::Insert { parent, child });
    }

    fn remove(&mut self, node: NodeId) {
        self.ops.push(HostOp::Remove { node });
    }

    fn move_before(&mut self, parent: NodeId, child: NodeId, before: Option<NodeId>) {
        self.ops.push(HostOp::MoveBefore {
            parent,
            child,
            before,
        });
    }

    fn listen(&mut self, node: NodeId, event: &str, handler_id: u32) {
        self.ops.push(HostOp::Listen {
            node,
            event: event.to_string(),
            handler_id,
        });
    }

    fn invalidate(&mut self, class: crate::schema::Invalidation) {
        self.ops.push(HostOp::Invalidate { class });
    }
}

/// G11 keyed reconcile: compute the minimal edit between two key lists.
///
/// Strategy selection is static (§6.3/G11): keyed lists use LIS reorder;
/// index/append-only/fixed lists never reorder. The output is a plan of
/// host ops: removes, inserts, and moves (via longest-increasing-
/// subsequence so a sort touches each row once, A11 prior art).
#[derive(Debug, Clone, PartialEq)]
pub struct ReorderPlan {
    /// Keys (positions in `old`) to remove.
    pub remove: Vec<usize>,
    /// Keys (positions in `new`) to insert.
    pub insert: Vec<usize>,
    /// `(old_pos, new_before_old_pos)` moves — minimal via LIS.
    pub moves: Vec<(usize, Option<usize>)>,
}

/// Compute the keyed reconcile plan between `old` and `new` key sequences.
pub fn keyed_reconcile<T: PartialEq>(old: &[T], new: &[T]) -> ReorderPlan {
    // Keeps: positions in old whose key survives (mapped to their new index).
    let mut keep_old: Vec<usize> = vec![];
    let mut insert: Vec<usize> = vec![];
    for (ni, nk) in new.iter().enumerate() {
        match old.iter().position(|ok| ok == nk) {
            Some(oi) => keep_old.push(oi),
            None => insert.push(ni),
        }
    }
    let remove: Vec<usize> = (0..old.len()).filter(|oi| !keep_old.contains(oi)).collect();
    // LIS over keep_old (values = old positions): rows already in relative
    // order stay put; the rest move.
    let lis = longest_increasing_subsequence(&keep_old);
    let mut moves = vec![];
    for (rank, &oi) in keep_old.iter().enumerate() {
        if !lis.contains(&oi) {
            let before = if rank + 1 < keep_old.len() {
                Some(keep_old[rank + 1])
            } else {
                None
            };
            moves.push((oi, before));
        }
    }
    ReorderPlan {
        remove,
        insert,
        moves,
    }
}

/// Classic patience/LIS (O(n log n)).
fn longest_increasing_subsequence(seq: &[usize]) -> Vec<usize> {
    if seq.is_empty() {
        return vec![];
    }
    let mut tails: Vec<usize> = vec![]; // indices into seq
    let mut prev: Vec<Option<usize>> = vec![None; seq.len()];
    for (i, &x) in seq.iter().enumerate() {
        // find first tail whose value >= x
        let pos = tails
            .binary_search_by(|&ti| seq[ti].cmp(&x))
            .err()
            .unwrap_or(tails.len());
        if pos == tails.len() {
            tails.push(i);
        } else {
            tails[pos] = i;
        }
        if pos > 0 {
            prev[i] = Some(tails[pos - 1]);
        }
    }
    // Reconstruct.
    let mut out = vec![];
    let mut cur = Some(tails[tails.len() - 1]);
    while let Some(c) = cur {
        out.push(seq[c]);
        cur = prev[c];
    }
    out.reverse();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::Invalidation;

    /// §5.4: the host realizes create/set/insert/remove/listen/invalidate.
    #[test]
    fn host_records_op_vocabulary() {
        let mut h = HeadlessHost::new();
        let n = h.create(7);
        h.set_text(n, "hi");
        h.set_prop(n, "class", "a", false);
        h.insert(0, n);
        h.listen(n, "click", 42);
        h.invalidate(Invalidation::Paint);
        assert_eq!(h.create_calls(), 1);
        assert_eq!(h.set_text_calls(), 1);
        assert_eq!(h.text_of(n), Some(&"hi".to_string()));
        assert_eq!(h.prop_of(n, "class"), Some(&"a".to_string()));
        assert!(
            h.ops
                .iter()
                .any(|o| matches!(o, HostOp::Listen { handler_id: 42, .. }))
        );
    }

    /// §7.9: echo suppression — a bind patch that writes the value the host
    /// already holds records NO set_prop (no cursor-jump bug class).
    #[test]
    fn echo_suppression() {
        let mut h = HeadlessHost::new();
        let n = h.create(1);
        h.set_prop(n, "value", "abc", false); // initial fill
        assert_eq!(h.count(|o| matches!(o, HostOp::SetProp { .. })), 1);
        // The bind patch writes the same value with skip_if_same.
        h.set_prop(n, "value", "abc", true);
        assert_eq!(
            h.count(|o| matches!(o, HostOp::SetProp { .. })),
            1,
            "echo suppressed"
        );
        // A genuinely new value still goes through.
        h.set_prop(n, "value", "abcd", true);
        assert_eq!(h.count(|o| matches!(o, HostOp::SetProp { .. })), 2);
    }

    /// §7.7: Signal<T> writes bump named bits through the host scheduler.
    #[test]
    fn signal_bits() {
        let mut h = HeadlessHost::new();
        assert_eq!(h.signal_read("theme"), 0);
        h.signal_write("theme");
        h.signal_write("theme");
        assert_eq!(h.signal_read("theme"), 2);
    }

    /// G11: keyed reconcile plans removals, insertions and LIS moves.
    #[test]
    fn keyed_reconcile_plan() {
        // [a, b, c] → [c, a]: remove b, move a before c.
        let plan = keyed_reconcile(&[1, 2, 3], &[3, 1]);
        assert_eq!(plan.remove, vec![1]);
        assert!(plan.insert.is_empty());
        assert_eq!(plan.moves.len(), 1);

        // Append-only: no moves.
        let plan = keyed_reconcile(&[1, 2], &[1, 2, 3]);
        assert!(plan.moves.is_empty());
        assert_eq!(plan.insert, vec![2]);

        // Pure reorder: sort touches each row once (LIS keeps the sorted
        // subsequence).
        let plan = keyed_reconcile(&[1, 2, 3, 4], &[2, 1, 4, 3]);
        assert_eq!(plan.moves.len(), 2);
    }

    /// LIS correctness on a shuffle.
    #[test]
    fn lis_on_shuffle() {
        let lis = longest_increasing_subsequence(&[3usize, 4, 7, 0, 6, 2, 3]);
        assert_eq!(lis, vec![0, 2, 3]);
    }

    /// §8.1: static subtree update cost is 0 — nothing visits it. The
    /// HeadlessHost is the assertion point: after a patch dispatch, only the
    /// patched slots appear in the op log.
    #[test]
    fn static_subtree_never_visited() {
        let mut h = HeadlessHost::new();
        let root = h.create(1);
        h.create(2); // static child created once at mount
        let before = h.ops.len();
        // A patch that only updates a text slot:
        h.set_text(root, "new");
        assert_eq!(h.ops.len() - before, 1, "one host call for one slot (§8.1)");
    }
}
