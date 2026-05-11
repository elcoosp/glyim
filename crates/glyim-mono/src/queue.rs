use glyim_hir::types::HirType;
use glyim_diag::Span;
use glyim_interner::Symbol;
use std::collections::VecDeque;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ItemKind {
    FnSpecialize,
    FnPassthrough,
    StructSpecialize,
    StructPassthrough,
    EnumSpecialize,
    EnumPassthrough,
}

#[derive(Clone, Debug)]
pub struct WorkItem {
    pub kind: ItemKind,
    pub def_id: Symbol,
    pub type_args: Vec<HirType>,
}

#[derive(Clone, Debug)]
pub struct WorkItemContext {
    pub discovered_from: Option<Symbol>,
    pub discovery_span: Span,
}

pub struct WorkQueue {
    queue: VecDeque<(WorkItem, WorkItemContext)>,
    seen: Vec<bool>,
}

impl Default for WorkQueue { fn default() -> Self { Self::new() } }

impl WorkQueue {
    pub fn new() -> Self { Self { queue: VecDeque::new(), seen: Vec::new() } }

    fn mark_seen(&mut self, sym: Symbol) -> bool {
        let idx = sym.raw() as usize;
        if idx >= self.seen.len() { self.seen.resize(idx + 64, false); }
        let was_seen = self.seen[idx];
        self.seen[idx] = true;
        was_seen
    }

    pub fn push(&mut self, item: WorkItem, context: WorkItemContext, dedup_sym: Symbol) {
        if !self.mark_seen(dedup_sym) { self.queue.push_back((item, context)); }
    }

    pub fn pop(&mut self) -> Option<(WorkItem, WorkItemContext)> { self.queue.pop_front() }
    pub fn is_empty(&self) -> bool { self.queue.is_empty() }
    pub fn extend(&mut self, items: Vec<(WorkItem, WorkItemContext)>) {
        for (item, ctx) in items {
            self.push(item.clone(), ctx, item.def_id);
        }
    }
}
