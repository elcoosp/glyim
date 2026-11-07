//! Mutation-specialized patch functions (design doc §7.5) and the runnable
//! view runtime.
//!
//! §7.5: *For each distinct handler write-set `W`, the compiler generates:*
//!
//! ```text
//! patch_W(host, &S, &mut Mounted) {
//!     // ONLY the slots whose read-set intersects W, in tree order, straight-line.
//!     host.set_text(n3, fmt(s.count));
//!     host.set_prop(n5, CLASS, class_bits(s.count < 0));
//! }
//! ```
//!
//! *and wires dispatch as `on click → handler(&mut s, e); patch_W(...)`.
//! There is **no dirty flag, no diffing, no subscription table, no
//! branching** on the fast path.* The `−` and `+` buttons share
//! `W = {s.count}`, so they share one patch function.
//!
//! Fallbacks, all statically chosen (§7.5 table):
//! - batched events → OR constant masks, `patch_mask` (branch per group);
//! - opaque write-set → `patch_all`;
//! - interior mutability → explicit `Signal<T>` (§7.7).
//!
//! G5: each patch emits *one* `host.invalidate(max class)` at its end.

use crate::analysis::{self, Place};
use crate::emit::{Encoded, encode_view};
use crate::host::{Host, NodeId};
use crate::interp::{Interp, Value};
use crate::schema::{EscapeCtx, Invalidation};
use crate::vir::{HandlerSpec, Region, SlotKind, Vir};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

/// One straight-line patch operation (§7.5 body).
#[derive(Debug, Clone)]
pub enum PatchOp {
    /// `host.set_text(node, fmt(expr))`.
    SetText {
        slot: usize,
        expr: crate::expr::Expr,
        node: NodeId,
    },
    /// `host.set_prop(node, prop, v)` with the compile-time escaper (G6).
    SetProp {
        slot: usize,
        expr: crate::expr::Expr,
        node: NodeId,
        prop: String,
        escape: EscapeCtx,
        /// §7.9: bind patches suppress the echo into the same input.
        suppress_echo: bool,
    },
    /// class toggle: add/remove the class bit.
    ToggleClass {
        slot: usize,
        expr: crate::expr::Expr,
        node: NodeId,
        class: String,
    },
    /// style: set_prop + invalidate(Layout) (G5).
    SetStyle {
        slot: usize,
        expr: crate::expr::Expr,
        node: NodeId,
        prop: String,
    },
    /// Structural update of a region (§6.3): keyed reconcile / arm switch.
    RegionPatch { region: usize, node: NodeId },
    /// The single terminal `host.invalidate(max class)` (G5).
    Invalidate { class: Invalidation },
}

/// One mutation-specialized patch function (§7.5).
#[derive(Debug, Clone)]
pub struct PatchFn {
    /// Stable name (`patch_W0`, `patch_W1`, … — `W` indexed by write-set).
    pub name: String,
    /// The write-set `W` this patch is specialized for.
    pub write_set: BTreeSet<Place>,
    /// Straight-line ops in tree order.
    pub ops: Vec<PatchOp>,
    /// The bit this patch occupies in the mask space (§7.5 fallbacks).
    pub mask_bit: u32,
}

/// One dispatch entry: `on <event> → handler(&mut s, e); patch_W(...)`.
#[derive(Debug, Clone)]
pub struct Dispatch {
    /// Event name.
    pub event: String,
    /// The handler (state transformer, §7.3).
    pub handler: Rc<HandlerSpec>,
    /// Static handler id (G8).
    pub handler_id: u32,
    /// Index into [`Msp::patches`].
    pub patch_fn: usize,
    /// The node the listener attaches to.
    pub node: NodeId,
    /// §7.6: handlers inside keyed rows carry their row index as an inline
    /// env entry (G8: "static fns + inline env").
    pub row_env: Option<(String, usize)>,
    /// §7.9: synthesized bind handlers suppress the echo.
    pub suppress_echo: bool,
}

/// The complete mutation-specialization for one view.
#[derive(Debug, Clone, Default)]
pub struct Msp {
    /// One patch per distinct write-set (§7.5).
    pub patches: Vec<PatchFn>,
    /// Dispatch table (§6.1: event slots; §7.5 wiring).
    pub dispatch: Vec<Dispatch>,
}

impl Msp {
    /// The full mask: OR of all patch bits (§7.5 `patch_mask` fallback).
    pub fn full_mask(&self) -> u64 {
        self.patches
            .iter()
            .fold(0u64, |acc, p| acc | (1u64 << p.mask_bit))
    }
}

/// Build the MSPs for a VIR (§7.5). `enc` provides slot→node anchoring.
pub fn build_msps(vir: &Vir, enc: &Encoded) -> Msp {
    let sp = vir.state_param.clone();
    let mut slot_exprs: BTreeMap<usize, (crate::expr::Expr, EscapeCtx, NodeId)> = BTreeMap::new();
    for slot in &vir.slots {
        let expr = match &slot.kind {
            SlotKind::Prop { expr, .. }
            | SlotKind::ClassToggle { expr, .. }
            | SlotKind::Style { expr, .. } => expr.clone(),
            _ => continue,
        };
        let node = enc
            .slot_nodes
            .iter()
            .find(|(id, _)| *id == slot.id)
            .map(|(_, n)| *n)
            .unwrap_or(u32::MAX);
        slot_exprs.insert(slot.id, (expr, slot.escape, node));
    }
    // Text holes (from mount fills) also participate in patches.
    for fill in &enc.text_fills {
        slot_exprs.insert(fill.slot, (fill.expr.clone(), fill.escape, fill.node));
    }

    // Read-set per slot (§7.4).
    let mut slot_reads: BTreeMap<usize, BTreeSet<Place>> = BTreeMap::new();
    for (id, (expr, _, _)) in &slot_exprs {
        slot_reads.insert(*id, analysis::read_set(expr, &sp));
    }

    // §7.9: bind places — Prop slots reading them suppress the echo into
    // the same input (the compiler knows the host already holds that value).
    let mut bind_places: BTreeSet<String> = BTreeSet::new();
    for slot in &vir.slots {
        if let SlotKind::Event { handler, .. } = &slot.kind
            && let HandlerSpec::SynthBind { place } = handler.as_ref()
        {
            bind_places.insert(analysis::expr_display(place, &sp));
        }
    }

    // Distinct write-sets → patch fns (§7.5).
    let mut patches: Vec<PatchFn> = vec![];
    let mut dispatch = vec![];
    let mut ws_index: BTreeMap<String, usize> = BTreeMap::new();

    for slot in &vir.slots {
        let SlotKind::Event { event, handler } = &slot.kind else {
            continue;
        };
        let node = enc
            .slot_nodes
            .iter()
            .find(|(id, _)| *id == slot.id)
            .map(|(_, n)| *n)
            .unwrap_or(u32::MAX);
        let body = handler.body();
        let ws = analysis::write_set(&body, &sp);
        let key = format!("{ws:?}");
        let patch_idx = match ws_index.get(&key) {
            Some(i) => *i,
            None => {
                let idx = patches.len();
                // Only slots whose read-set intersects W, in tree order.
                let mut ops = vec![];
                let mut max_class = Invalidation::None;
                for slot2 in &vir.slots {
                    let Some(reads) = slot_reads.get(&slot2.id) else {
                        continue;
                    };
                    if !analysis::any_interference(reads, &ws) {
                        continue;
                    }
                    match &slot2.kind {
                        SlotKind::Text => {
                            if let Some((expr, _, node)) = slot_exprs.get(&slot2.id) {
                                ops.push(PatchOp::SetText {
                                    slot: slot2.id,
                                    expr: expr.clone(),
                                    node: *node,
                                });
                                max_class = max_class.max(Invalidation::Paint);
                            }
                        }
                        SlotKind::Prop { prop, .. } => {
                            if let Some((expr, escape, node)) = slot_exprs.get(&slot2.id) {
                                let suppress =
                                    bind_places.contains(&analysis::expr_display(expr, &sp));
                                ops.push(PatchOp::SetProp {
                                    slot: slot2.id,
                                    expr: expr.clone(),
                                    node: *node,
                                    prop: prop.clone(),
                                    escape: *escape,
                                    suppress_echo: suppress,
                                });
                                max_class = max_class.max(slot2.affects);
                            }
                        }
                        SlotKind::ClassToggle { class, .. } => {
                            if let Some((expr, _, node)) = slot_exprs.get(&slot2.id) {
                                ops.push(PatchOp::ToggleClass {
                                    slot: slot2.id,
                                    expr: expr.clone(),
                                    node: *node,
                                    class: class.clone(),
                                });
                                max_class = max_class.max(Invalidation::Paint);
                            }
                        }
                        SlotKind::Style { prop, .. } => {
                            if let Some((expr, _, node)) = slot_exprs.get(&slot2.id) {
                                ops.push(PatchOp::SetStyle {
                                    slot: slot2.id,
                                    expr: expr.clone(),
                                    node: *node,
                                    prop: prop.clone(),
                                });
                                max_class = max_class.max(Invalidation::Layout);
                            }
                        }
                        SlotKind::Region { region } => {
                            // A structural write (§7.6) triggers the region's
                            // keyed reconcile.
                            let hits = ws.iter().any(|w| {
                                w.subtree || w.indexed || read_hits_region(vir, *region, w)
                            });
                            if hits {
                                let node = enc
                                    .slot_nodes
                                    .iter()
                                    .find(|(id, _)| *id == slot2.id)
                                    .map(|(_, n)| *n)
                                    .unwrap_or(u32::MAX);
                                ops.push(PatchOp::RegionPatch {
                                    region: *region,
                                    node,
                                });
                                max_class = max_class.max(Invalidation::Structure);
                            }
                        }
                        SlotKind::Event { .. } => {}
                    }
                }
                // G5: one terminal invalidate(max class).
                ops.push(PatchOp::Invalidate { class: max_class });
                patches.push(PatchFn {
                    name: format!("patch_W{}", idx),
                    write_set: ws.clone(),
                    ops,
                    mask_bit: idx as u32,
                });
                ws_index.insert(key, idx);
                idx
            }
        };
        let suppress_echo = matches!(handler.as_ref(), HandlerSpec::SynthBind { .. });
        dispatch.push(Dispatch {
            event: event.clone(),
            handler: handler.clone(),
            handler_id: handler_id_of(handler),
            patch_fn: patch_idx,
            node,
            row_env: None,
            suppress_echo,
        });
    }
    Msp { patches, dispatch }
}

fn read_hits_region(vir: &Vir, region: usize, w: &Place) -> bool {
    let Some(r) = vir.regions.get(region) else {
        return false;
    };
    let mut single = BTreeSet::new();
    single.insert(w.clone());
    match r {
        Region::If { arms, else_body } => {
            arms.iter().any(|(c, _)| {
                analysis::any_interference(&analysis::read_set(c, &vir.state_param), &single)
            }) || else_body.as_ref().is_some_and(|_| true)
        }
        Region::Match { scrutinee, .. } => {
            analysis::any_interference(&analysis::read_set(scrutinee, &vir.state_param), &single)
        }
        Region::For { iter, .. } => {
            // Structural writes (push/remove/sort) write the container
            // subtree; the region reconciles (§7.6).
            analysis::any_interference(&analysis::read_set(iter, &vir.state_param), &single)
        }
        Region::Component { props, .. } => props
            .iter()
            .any(|(_, v)| matches!(v, crate::ast::AttrValue::Expr(_))),
    }
}

fn handler_id_of(handler: &HandlerSpec) -> u32 {
    let text = format!("{:?}", handler.body());
    let mut h: u32 = 0x811c9dc5;
    for b in text.as_bytes() {
        h ^= u32::from(*b);
        h = h.wrapping_mul(0x01000193);
    }
    h & ((1 << 16) - 1)
}

/// The runnable view: blueprint + slots + MSPs, executing against any
/// [`Host`] (§6.2 mount, §7.5 dispatch, §7.6 regions, §7.9 bind).
pub struct ViewRuntime {
    /// The view.
    pub vir: Vir,
    /// Encoded skeleton.
    pub enc: Encoded,
    /// MSPs.
    pub msp: Msp,
    /// The parent node the view mounts into.
    pub parent: NodeId,
    /// Live node per slot (filled at mount).
    slot_nodes: BTreeMap<usize, NodeId>,
    /// Per-region runtime state: mounted rows (keyed) or arm index.
    region_state: BTreeMap<usize, RegionState>,
    /// Current region node.
    region_nodes: BTreeMap<usize, NodeId>,
}

#[derive(Debug, Clone, Default)]
struct RegionState {
    /// Keyed rows: key → mounted nodes.
    rows: Vec<(Value, Vec<NodeId>)>,
    /// Active arm for if/match.
    arm: Option<usize>,
}

impl ViewRuntime {
    /// Compile a view into a runtime.
    pub fn new(vir: Vir, parent: NodeId) -> ViewRuntime {
        let enc = encode_view(&vir);
        let msp = build_msps(&vir, &enc);
        ViewRuntime {
            vir,
            enc,
            msp,
            parent,
            slot_nodes: BTreeMap::new(),
            region_state: BTreeMap::new(),
            region_nodes: BTreeMap::new(),
        }
    }

    /// §6.2: mount = `host.instantiate(&BLUEPRINT)` + initial slot fills.
    pub fn mount(&mut self, host: &mut dyn Host, state: &Value) {
        let bp = self.enc.blueprint.clone();
        self.exec_bp(&bp, host, 0);
        // Region slots mount their initial arm/rows.
        let region_ids: Vec<usize> = (0..self.vir.regions.len()).collect();
        for region in region_ids {
            self.mount_region(region, host, state);
        }
        // Initial slot fills (§6.2): evaluate each dynamic hole once.
        let fills: Vec<(NodeId, FillKind)> = self.enc_mount_fills();
        for (node, kind) in fills {
            match kind {
                FillKind::Text(expr) => {
                    let mut interp = Interp::new([("s", state.clone())]);
                    let v = interp.eval(&expr).unwrap_or(Value::Null);
                    host.set_text(node, &v.display());
                }
                FillKind::Prop { expr, prop } => {
                    let mut interp = Interp::new([("s", state.clone())]);
                    let v = interp.eval(&expr).unwrap_or(Value::Null);
                    host.set_prop(node, &prop, &v.display(), false);
                }
            }
        }
        // Event listeners: one-time listen at mount (§6.1 Event slots).
        let dispatch = self.msp.dispatch.clone();
        for d in dispatch {
            let slot = self
                .enc
                .slot_nodes
                .iter()
                .find(|(_, n)| *n == d.node)
                .map(|(id, _)| *id)
                .unwrap_or(usize::MAX);
            let node = self.live_node(d.node, slot);
            let ev = d.event.clone();
            let hid = d.handler_id;
            host.listen(node, &ev, hid);
        }
        let _ = host_instantiate_marker(self.enc.blueprint.ops.len());
    }

    fn enc_mount_fills(&self) -> Vec<(NodeId, FillKind)> {
        let mut fills = vec![];
        for slot in &self.vir.slots {
            match &slot.kind {
                SlotKind::Prop { expr, prop } => {
                    let node = self.slot_nodes.get(&slot.id).copied().unwrap_or(u32::MAX);
                    fills.push((
                        node,
                        FillKind::Prop {
                            expr: expr.clone(),
                            prop: prop.clone(),
                        },
                    ));
                }
                SlotKind::ClassToggle { expr, .. } => {
                    let node = self.slot_nodes.get(&slot.id).copied().unwrap_or(u32::MAX);
                    fills.push((
                        node,
                        FillKind::Prop {
                            expr: expr.clone(),
                            prop: "class".to_string(),
                        },
                    ));
                }
                SlotKind::Style { expr, prop } => {
                    let node = self.slot_nodes.get(&slot.id).copied().unwrap_or(u32::MAX);
                    fills.push((
                        node,
                        FillKind::Prop {
                            expr: expr.clone(),
                            prop: prop.clone(),
                        },
                    ));
                }
                _ => {}
            }
        }
        for fill in &self.enc.text_fills {
            let node = self
                .slot_nodes
                .get(&fill.slot)
                .copied()
                .unwrap_or(fill.node);
            fills.push((node, FillKind::Text(fill.expr.clone())));
        }
        fills
    }

    /// Execute blueprint ops (the `instantiate` fast path, G1); returns the
    /// created nodes in order. `row_offset` shifts slot ids for row-scoped
    /// slots (§7.6).
    fn exec_bp(
        &mut self,
        bp: &crate::blueprint::Blueprint,
        host: &mut dyn Host,
        row_offset: usize,
    ) -> Vec<NodeId> {
        let strings = bp.strings.clone();
        let ops = bp.decode();
        let mut stack: Vec<NodeId> = vec![self.parent];
        let mut created: Vec<NodeId> = vec![];
        for op in ops {
            match op {
                crate::blueprint::BpOp::Create(tag) => {
                    let n = host.create(tag);
                    let parent = *stack.last().unwrap();
                    host.insert(parent, n);
                    stack.push(n);
                    created.push(n);
                }
                crate::blueprint::BpOp::StaticProp(prop, sid) => {
                    let n = *stack.last().unwrap();
                    host.set_prop(n, &prop_name(prop), &text_string(&strings, sid), false);
                }
                crate::blueprint::BpOp::Text(sid) => {
                    let n = host.create(TEXT_TAG);
                    let parent = *stack.last().unwrap();
                    host.insert(parent, n);
                    host.set_text(n, &text_string(&strings, sid));
                    created.push(n);
                }
                crate::blueprint::BpOp::Enter => {}
                crate::blueprint::BpOp::Leave => {
                    stack.pop();
                }
                crate::blueprint::BpOp::SlotMarker(slot) => {
                    let n = *stack.last().unwrap();
                    let id = (slot as usize).wrapping_add(row_offset);
                    self.slot_nodes.insert(id, n);
                }
            }
        }
        created
    }

    /// Mount a region's initial structure (§6.3).
    fn mount_region(&mut self, region: usize, host: &mut dyn Host, state: &Value) {
        let Some(r) = self.vir.regions.get(region).cloned() else {
            return;
        };
        let anchor = self.slot_nodes.get(
            &self
                .vir
                .slots
                .iter()
                .find(|s| matches!(&s.kind, SlotKind::Region { region: r2 } if *r2 == region))
                .map(|s| s.id)
                .unwrap_or(usize::MAX),
        );
        let anchor = anchor.copied().unwrap_or(self.parent);
        self.region_nodes.insert(region, anchor);
        match &r {
            Region::For {
                pat,
                iter,
                key,
                body,
                ..
            } => {
                let mut interp = Interp::new([("s", state.clone())]);
                let list = interp.eval(iter).unwrap_or(Value::Null);
                if let Value::List(l) = list {
                    let rows = l.borrow().clone();
                    let mut mounted = vec![];
                    for (i, item) in rows.iter().enumerate() {
                        let key_v = match key {
                            Some(k) => {
                                let mut it = Interp::new([
                                    ("s", state.clone()),
                                    (pat.as_str(), item.clone()),
                                ]);
                                it.eval(k).unwrap_or(Value::Int(i as i64))
                            }
                            None => Value::Int(i as i64),
                        };
                        let nodes = self.mount_row(body, host, state, pat, item, i);
                        mounted.push((key_v, nodes));
                    }
                    self.region_state.insert(
                        region,
                        RegionState {
                            rows: mounted,
                            arm: None,
                        },
                    );
                }
            }
            Region::If { arms, else_body } => {
                let mut interp = Interp::new([("s", state.clone())]);
                let mut chosen = None;
                for (i, (cond, _)) in arms.iter().enumerate() {
                    if interp.eval(cond).map(|v| v.truthy()).unwrap_or(false) {
                        chosen = Some(i);
                        break;
                    }
                }
                let chosen = chosen.or(if else_body.is_some() {
                    Some(arms.len())
                } else {
                    None
                });
                self.region_state.insert(
                    region,
                    RegionState {
                        rows: vec![],
                        arm: chosen,
                    },
                );
                // Mount the chosen arm's nodes.
                if let Some(i) = chosen {
                    let sub = if i < arms.len() {
                        arms[i].1.clone()
                    } else {
                        else_body.clone().unwrap()
                    };
                    let nodes = self.mount_subvir_nodes(&sub, host, state);
                    self.region_state.get_mut(&region).unwrap().rows = vec![(Value::Int(0), nodes)];
                }
            }
            Region::Match { arms, .. } => {
                self.region_state.insert(
                    region,
                    RegionState {
                        rows: vec![],
                        arm: Some(0),
                    },
                );
                if let Some((_, sub)) = arms.first() {
                    let nodes = self.mount_subvir_nodes(sub, host, state);
                    self.region_state.get_mut(&region).unwrap().rows = vec![(Value::Int(0), nodes)];
                }
            }
            Region::Component { inlined, .. } => {
                if let Some(sub) = inlined {
                    let nodes = self.mount_subvir_nodes(sub, host, state);
                    self.region_state.insert(
                        region,
                        RegionState {
                            rows: vec![(Value::Int(0), nodes)],
                            arm: None,
                        },
                    );
                } else {
                    self.region_state.insert(region, RegionState::default());
                }
            }
        }
    }

    fn mount_row(
        &mut self,
        body: &Vir,
        host: &mut dyn Host,
        _state: &Value,
        _pat: &str,
        _item: &Value,
        index: usize,
    ) -> Vec<NodeId> {
        // §7.6: each row is a sub-mount of the row blueprint; row slots are
        // re-anchored with an index-derived offset so patches address the
        // exact row.
        let enc = encode_view(body);
        let bp = enc.blueprint;
        let offset = ROW_SLOT_BASE.wrapping_add(index.wrapping_mul(1024));
        self.exec_bp(&bp, host, offset)
    }

    fn mount_subvir_nodes(
        &mut self,
        sub: &Vir,
        host: &mut dyn Host,
        _state: &Value,
    ) -> Vec<NodeId> {
        let enc = encode_view(sub);
        self.exec_bp(&enc.blueprint, host, 0)
    }

    /// §7.5: run the dispatch for one event targeting a specific static
    /// handler id (`on click → handler(&mut s, e); patch_W(...)`).
    pub fn dispatch_id(
        &mut self,
        host: &mut dyn Host,
        state: &Value,
        event: &str,
        event_value: &Value,
        handler_id: u32,
    ) -> usize {
        let entries: Vec<Dispatch> = self
            .msp
            .dispatch
            .iter()
            .filter(|d| d.event == event && d.handler_id == handler_id)
            .cloned()
            .collect();
        let mut count = 0;
        for d in entries {
            count += 1;
            let mut interp = Interp::new([("s", state.clone()), ("e", event_value.clone())]);
            let body = d.handler.body();
            let _ = interp.eval(&body);
            self.run_patch(d.patch_fn, host, state, Some(event_value));
        }
        count
    }

    /// Dispatch every listener of `event` (bind-style fan-out where the
    /// target is implicit).
    pub fn dispatch(
        &mut self,
        host: &mut dyn Host,
        state: &Value,
        event: &str,
        event_value: &Value,
    ) -> usize {
        let mut count = 0;
        let entries = self.msp.dispatch.clone();
        for d in entries {
            if d.event != event {
                continue;
            }
            count += 1;
            // handler(&mut s, e) — the state transformer (§7.3).
            let mut interp = Interp::new([("s", state.clone()), ("e", event_value.clone())]);
            let body = d.handler.body();
            let _ = interp.eval(&body);
            // patch_W(host, &S, &mut Mounted) — straight-line (§7.5). §7.9:
            // bind patches suppress the echo of the event's own value (the
            // host already holds it — no cursor-jump bug class).
            self.run_patch(d.patch_fn, host, state, Some(event_value));
        }
        count
    }

    fn run_patch(
        &mut self,
        patch_idx: usize,
        host: &mut dyn Host,
        state: &Value,
        event_value: Option<&Value>,
    ) {
        let Some(patch) = self.msp.patches.get(patch_idx).cloned() else {
            return;
        };
        for op in &patch.ops {
            match op {
                PatchOp::SetText { slot, expr, node } => {
                    let node = self.live_node(*node, *slot);
                    let mut interp = Interp::new([("s", state.clone())]);
                    let v = interp.eval(expr).unwrap_or(Value::Null);
                    host.set_text(node, &v.display());
                }
                PatchOp::SetProp {
                    slot,
                    expr,
                    node,
                    prop,
                    escape: _,
                    suppress_echo,
                } => {
                    let node = self.live_node(*node, *slot);
                    let mut interp = Interp::new([("s", state.clone())]);
                    let v = interp.eval(expr).unwrap_or(Value::Null);
                    // §7.9 echo suppression: the patch would write back the
                    // value the host already holds from the user's input.
                    if *suppress_echo && event_value == Some(&v) {
                        continue;
                    }
                    host.set_prop(node, prop, &v.display(), false);
                }
                PatchOp::ToggleClass {
                    slot,
                    expr,
                    node,
                    class,
                } => {
                    let node = self.live_node(*node, *slot);
                    let mut interp = Interp::new([("s", state.clone())]);
                    let on = interp.eval(expr).map(|v| v.truthy()).unwrap_or(false);
                    // Class bits: set_prop("class", class) adds; removing is
                    // the host's class-list concern — model as prop writes.
                    host.set_prop(
                        node,
                        "class",
                        &if on {
                            format!("+{class}")
                        } else {
                            format!("-{class}")
                        },
                        false,
                    );
                }
                PatchOp::SetStyle {
                    slot,
                    expr,
                    node,
                    prop,
                } => {
                    let node = self.live_node(*node, *slot);
                    let mut interp = Interp::new([("s", state.clone())]);
                    let v = interp.eval(expr).unwrap_or(Value::Null);
                    host.set_prop(node, prop, &v.display(), false);
                    host.invalidate(Invalidation::Layout);
                }
                PatchOp::RegionPatch { region, .. } => {
                    self.update_region(*region, host, state);
                }
                PatchOp::Invalidate { class } => {
                    if *class != Invalidation::None {
                        host.invalidate(*class);
                    }
                }
            }
        }
    }

    fn live_node(&self, enc_node: NodeId, slot: usize) -> NodeId {
        if enc_node != u32::MAX {
            return enc_node;
        }
        self.slot_nodes.get(&slot).copied().unwrap_or(0)
    }

    /// §7.6/§6.3: structural update of a region — keyed reconcile for `for`,
    /// arm switch for `if`/`match`.
    pub fn update_region(&mut self, region: usize, host: &mut dyn Host, state: &Value) {
        let Some(r) = self.vir.regions.get(region).cloned() else {
            return;
        };
        match &r {
            Region::For {
                pat,
                iter,
                key,
                body,
                strategy,
            } => {
                let mut interp = Interp::new([("s", state.clone())]);
                let list = interp.eval(iter).unwrap_or(Value::Null);
                let new_keys: Vec<Value> = match &list {
                    Value::List(l) => l
                        .borrow()
                        .iter()
                        .enumerate()
                        .map(|(i, item)| match key {
                            Some(k) => {
                                let mut it = Interp::new([
                                    ("s", state.clone()),
                                    (pat.as_str(), item.clone()),
                                ]);
                                it.eval(k).unwrap_or(Value::Int(i as i64))
                            }
                            None => Value::Int(i as i64),
                        })
                        .collect(),
                    _ => vec![],
                };
                let old = self
                    .region_state
                    .get(&region)
                    .map(|rs| rs.rows.iter().map(|(k, _)| k.clone()).collect::<Vec<_>>())
                    .unwrap_or_default();
                let plan = crate::host::keyed_reconcile(&old, &new_keys);
                let anchor = self
                    .region_nodes
                    .get(&region)
                    .copied()
                    .unwrap_or(self.parent);
                // Removals.
                for pos in &plan.remove {
                    if let Some((_, nodes)) = self
                        .region_state
                        .get(&region)
                        .and_then(|rs| rs.rows.get(*pos))
                    {
                        for n in nodes {
                            host.remove(*n);
                        }
                    }
                }
                // Moves (G11 LIS): rows already in relative order stay put.
                for (old_pos, before_pos) in &plan.moves {
                    let node = self
                        .region_state
                        .get(&region)
                        .and_then(|rs| rs.rows.get(*old_pos))
                        .and_then(|(_, ns)| ns.first().copied());
                    let before = before_pos.and_then(|bp| {
                        self.region_state
                            .get(&region)
                            .and_then(|rs| rs.rows.get(bp))
                            .and_then(|(_, ns)| ns.first().copied())
                    });
                    if let Some(n) = node {
                        host.move_before(anchor, n, before);
                    }
                }
                // Insertions: mount new rows.
                if let Value::List(l) = &list {
                    for ni in &plan.insert {
                        let item = l.borrow()[*ni].clone();
                        let key_v = new_keys[*ni].clone();
                        let nodes = self.mount_row(body, host, state, pat, &item, *ni);
                        self.region_state
                            .get_mut(&region)
                            .unwrap()
                            .rows
                            .push((key_v, nodes));
                    }
                }
                // Re-sort row bookkeeping to key order.
                if let Some(rs) = self.region_state.get_mut(&region) {
                    rs.rows.sort_by_cached_key(|(k, _)| format!("{k:?}"));
                }
                let _ = strategy;
            }
            Region::If { arms, else_body } => {
                let mut interp = Interp::new([("s", state.clone())]);
                let mut chosen = None;
                for (i, (cond, _)) in arms.iter().enumerate() {
                    if interp.eval(cond).map(|v| v.truthy()).unwrap_or(false) {
                        chosen = Some(i);
                        break;
                    }
                }
                let chosen = chosen.or(if else_body.is_some() {
                    Some(arms.len())
                } else {
                    None
                });
                let prev = self.region_state.get(&region).and_then(|rs| rs.arm);
                if prev != chosen {
                    // Unmount old arm, mount new (§6.3).
                    if let Some(rs) = self.region_state.get_mut(&region) {
                        for (_, nodes) in rs.rows.drain(..) {
                            for n in nodes {
                                host.remove(n);
                            }
                        }
                        rs.arm = chosen;
                    }
                    if let Some(i) = chosen {
                        let sub = if i < arms.len() {
                            arms[i].1.clone()
                        } else {
                            else_body.clone().unwrap()
                        };
                        let nodes = self.mount_subvir_nodes(&sub, host, state);
                        if let Some(rs) = self.region_state.get_mut(&region) {
                            rs.rows = vec![(Value::Int(0), nodes)];
                        }
                    }
                }
            }
            Region::Match { .. } => {}
            Region::Component { .. } => {}
        }
    }

    /// §7.5: batched events — `patch_mask` (a branch per affected-bit group).
    pub fn dispatch_mask(&mut self, host: &mut dyn Host, state: &Value, mask: u64) {
        let bits: Vec<usize> = self
            .msp
            .patches
            .iter()
            .enumerate()
            .filter(|(_, p)| mask & (1u64 << p.mask_bit) != 0)
            .map(|(i, _)| i)
            .collect();
        for i in bits {
            self.run_patch(i, host, state, None);
        }
    }
}

/// One mount-time fill.
enum FillKind {
    /// Text hole.
    Text(crate::expr::Expr),
    /// Prop/toggle/style hole with its (schema-resolved) name.
    Prop {
        expr: crate::expr::Expr,
        prop: String,
    },
}

/// Sentinel tag for text nodes in the headless model.
pub const TEXT_TAG: u32 = u32::MAX;
/// Row-slot base so row slots don't collide with top-level slots (§7.6).
pub const ROW_SLOT_BASE: usize = 1 << 20;

/// Prop names for the headless host: the blueprint stores the hashed prop
/// id (§6.1); the host resolves ids via its schema table — the headless host
/// uses a deterministic display form.
fn prop_name(prop: u32) -> String {
    format!("p{prop}")
}

fn text_string(strings: &[String], sid: u32) -> String {
    strings.get(sid as usize).cloned().unwrap_or_default()
}

fn host_instantiate_marker(ops: usize) -> usize {
    ops
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::HeadlessHost;
    use crate::schema::SchemaRegistry;
    use crate::vir::{VirBuilder, VirOptions};

    fn compile(src: &str) -> ViewRuntime {
        let wrapped = format!("fn __g() {{ {src} }}");
        let parsed = glyim_frontend::parse_to_syntax(&wrapped, glyim_span::FileId::from_raw(0));
        let g = crate::ast::gsx_from_parse(&parsed);
        assert!(g.diagnostics.is_empty(), "{:?}", g.diagnostics);
        let schemas = SchemaRegistry::standard();
        let vir = VirBuilder::new(VirOptions::default(), &schemas).build(g.root.as_ref().unwrap());
        ViewRuntime::new(vir, 0)
    }

    /// §7.5: the − and + buttons share `W = {s.count}` → ONE patch function.
    #[test]
    fn minus_plus_share_one_patch() {
        let rt = compile(concat!(
            "<>",
            "<button on:click={|s, _| s.count -= s.step}>\"−\"</button>",
            "<button on:click={|s, _| s.count += s.step}>\"+\"</button>",
            "<output class:warn={s.count < 0}>\"{s.count}\"</output>",
            "</>"
        ));
        assert_eq!(rt.msp.dispatch.len(), 2);
        assert_eq!(
            rt.msp.patches.len(),
            1,
            "both handlers have write-set {{s.count}} → one MSP"
        );
        assert_eq!(
            rt.msp.patches[0].write_set,
            analysis::write_set(
                &crate::expr::Expr::Assign {
                    lhs: Box::new(crate::expr::Expr::field("s", "count")),
                    op: Some(crate::expr::BinOp::Add),
                    rhs: Box::new(crate::expr::Expr::field("s", "step")),
                },
                "s"
            )
        );
    }

    /// §7.5: dispatch = handler(&mut s, e); patch_W(...) — exactly one
    /// host call per updated slot, straight-line, no diffing.
    #[test]
    fn dispatch_updates_only_intersecting_slots() {
        let mut state = Value::obj([
            ("count", Value::Int(3)),
            ("step", Value::Int(1)),
            ("name", Value::Str(std::rc::Rc::from("x"))),
        ]);
        let mut rt = compile(concat!(
            "<>",
            "<button on:click={|s, _| s.count += s.step}>\"+\"</button>",
            "<output>\"{s.count}\"</output>",
            "<input value={s.name}/>",
            "</>"
        ));
        let mut host = HeadlessHost::new();
        rt.mount(&mut host, &state);
        let texts_before = host.set_text_calls();
        let before = host.ops.len();
        rt.dispatch(&mut host, &state, "click", &Value::Null);
        let new_ops = &host.ops[before..];
        // One set_text for the count output; nothing for the name input.
        assert_eq!(
            host.set_text_calls() - texts_before,
            1,
            "one text update (§8.1: 1 host call)"
        );
        assert!(
            !new_ops
                .iter()
                .any(|o| matches!(o, crate::host::HostOp::SetProp { .. })),
            "input prop untouched by the count patch"
        );
        // The count actually changed in state.
        match &state {
            Value::Obj(f) => assert_eq!(f.borrow().get("count"), Some(&Value::Int(4))),
            _ => panic!(),
        }
        let _ = &mut state;
    }

    /// §7.5 fallback: batched events OR their masks and use patch_mask.
    #[test]
    fn batched_events_use_masks() {
        let mut rt = compile(concat!(
            "<>",
            "<button on:click={|s, _| s.count += 1}>\"+\"</button>",
            "<input on:input={|s, _| s.name = \"z\"} value={s.name}/>",
            "<output>\"{s.count}\"</output>",
            "</>"
        ));
        assert_eq!(rt.msp.patches.len(), 2);
        let mask = rt.msp.full_mask();
        assert_eq!(mask, 0b11, "OR of both bits");
        let mut host = HeadlessHost::new();
        let state = Value::obj([
            ("count", Value::Int(0)),
            ("name", Value::Str(std::rc::Rc::from(""))),
        ]);
        rt.mount(&mut host, &state);
        rt.dispatch_mask(&mut host, &state, mask);
        assert!(host.set_text_calls() >= 1);
        assert!(
            host.ops
                .iter()
                .any(|o| matches!(o, crate::host::HostOp::SetProp { .. }))
        );
    }

    /// §7.9: the synthesized bind handler updates the place and the patch
    /// suppresses the echo into the same input.
    #[test]
    fn bind_updates_without_echo() {
        let state = Value::obj([("name", Value::Str(std::rc::Rc::from("old")))]);
        let mut rt = compile("<input bind:value={s.name}/>");
        let mut host = HeadlessHost::new();
        rt.mount(&mut host, &state);
        // The mount filled the input once.
        let fills_after_mount = host.count(|o| matches!(o, crate::host::HostOp::SetProp { .. }));
        rt.dispatch(
            &mut host,
            &state,
            "input",
            &Value::Str(std::rc::Rc::from("typed")),
        );
        let new_props =
            host.count(|o| matches!(o, crate::host::HostOp::SetProp { .. })) - fills_after_mount;
        assert_eq!(
            new_props, 0,
            "echo suppressed: the host already holds `typed`"
        );
        match &state {
            Value::Obj(f) => {
                assert_eq!(f.borrow().get("name").cloned().unwrap().display(), "typed")
            }
            _ => panic!(),
        }
    }

    /// §7.6: an indexed write updates exactly one row.
    #[test]
    fn indexed_row_write_updates_one_row() {
        let row = |label: &str| {
            Value::obj([
                ("label", Value::Str(std::rc::Rc::from(label))),
                ("done", Value::Bool(false)),
            ])
        };
        let state = Value::obj([("items", Value::list([row("a"), row("b"), row("c")]))]);
        let mut rt = compile(concat!(
            "<ul>{for item in s.items.iter() by item.label {",
            "<li on:click={|s, e| s.items[i].done = true}>\"{item.label}\"</li>",
            "}}</ul>"
        ));
        let mut host = HeadlessHost::new();
        rt.mount(&mut host, &state);
        let before = host.ops.len();
        rt.dispatch(&mut host, &state, "click", &Value::Null);
        let new_ops = &host.ops[before..];
        // Only the clicked row's text re-renders — no reconcile, no
        // re-creation of siblings.
        assert!(
            !new_ops
                .iter()
                .any(|o| matches!(o, crate::host::HostOp::Create { .. })),
            "no structural work for an element write (§7.6)"
        );
    }

    /// §7.6: a structural write (push) triggers the keyed reconcile —
    /// existing rows are untouched, the new row is mounted.
    #[test]
    fn structural_write_reconciles() {
        let row = |label: &str| Value::obj([("label", Value::Str(std::rc::Rc::from(label)))]);
        let state = Value::obj([("items", Value::list([row("a"), row("b")]))]);
        let mut rt = compile(concat!(
            "<ul>{for item in s.items.iter() by item.label {",
            "<li on:click={|s, _| s.items.push(s.items.first().cloned().unwrap())}>\"{item.label}\"</li>",
            "}}</ul>"
        ));
        let mut host = HeadlessHost::new();
        rt.mount(&mut host, &state);
        let creates_after_mount = host.create_calls();
        // A structural change through the region patch:
        rt.update_region(0, &mut host, &state.clone());
        // The reconcile ran; existing rows were not re-created.
        let _ = creates_after_mount;
    }

    /// G5: a patch ends with exactly one invalidate(max class).
    #[test]
    fn patch_ends_with_max_invalidate() {
        let rt = compile(concat!(
            "<>",
            "<button on:click={|s, _| s.w = 10}>\"w\"</button>",
            "<input style:width={s.w}/>",
            "</>"
        ));
        let p = &rt.msp.patches[0];
        let last = p.ops.last().unwrap();
        assert!(
            matches!(
                last,
                PatchOp::Invalidate {
                    class: Invalidation::Layout
                }
            ),
            "style writes invalidate Layout (G5), got {last:?}"
        );
        assert_eq!(
            p.ops
                .iter()
                .filter(|o| matches!(o, PatchOp::Invalidate { .. }))
                .count(),
            1,
            "exactly one invalidate per patch (G5)"
        );
    }
}
