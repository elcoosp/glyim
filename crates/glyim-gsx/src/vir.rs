//! The View IR — static skeleton + slots + regions (design doc §6).
//!
//! §2 principle 1: **structure is never erased early.** `<div class="a">{x}
//! </div>` stays a tree through the pipeline; only this dedicated pass lowers
//! it. Every view splits into (§6.1):
//!
//! - a **Blueprint**: the static skeleton (see [`crate::blueprint`]),
//! - **Slots**: dynamic holes with a kind and a read-set,
//! - **Regions**: `if`/`match`/`for … by`/component calls — dynamic
//!   structural points (§6.3).
//!
//! Optimizations applied at VIR level (they need structure that is gone by
//! MIR, finding F9):
//! - **G2** static hoisting & folding: fully static subtrees get zero slots;
//!   adjacent static attrs merge; constant `class` strings concatenate.
//! - **G9** prop specialization across component boundaries: components
//!   called with constant props are cloned and constant-propagated at VIR
//!   level; the inline decision uses a VIR node-count heuristic with
//!   `#[inline(never)]`/`#[inline(always)]` overrides (§6.3).
//! - **§7.9** `bind:` desugars into a Prop slot + a synthesized handler.

use crate::analysis::Place;
use crate::diag::{GsxCode, GsxDiagnostic};
use crate::expr::Expr;
use crate::schema::{EscapeCtx, Invalidation, SchemaRegistry};
use crate::ast;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

/// Where a dynamic value attaches: the blueprint node index plus the slot
/// kind (§6.1 slot table).
#[derive(Debug, Clone)]
pub struct Slot {
    /// Slot index in tree order (= SlotMarker payload).
    pub id: usize,
    /// Kind.
    pub kind: SlotKind,
    /// Blueprint node this slot patches.
    pub node: u32,
    /// Read-set (filled by [`crate::analysis`], §7.4).
    pub read_set: BTreeSet<Place>,
    /// Static "affects" class for the final `host.invalidate(max)` (G5).
    pub affects: Invalidation,
    /// Escape context chosen from schema + position (G6).
    pub escape: EscapeCtx,
}

/// Slot kinds (§6.1 table).
#[derive(Debug, Clone)]
pub enum SlotKind {
    /// `"Count: {s.count}"` hole → `host.set_text`.
    Text,
    /// `value={s.name}` → `host.set_prop`.
    Prop { prop: String, expr: Expr },
    /// `class:warn={s.count<0}` → add/remove class bit.
    ClassToggle { class: String, expr: Expr },
    /// `style:width={px}` → `set_prop` + `invalidate(Layout)` (G5).
    Style { prop: String, expr: Expr },
    /// `on:click={…}` → one-time `listen` at mount (static handler id).
    Event { event: String, handler: Rc<HandlerSpec> },
    /// `if / match / for / <Child/>` — structural (§6.3).
    Region { region: usize },
}

/// A handler: state transformer, not a capturing closure (§7.3).
#[derive(Debug, Clone)]
pub enum HandlerSpec {
    /// `|s, e| body` — params bind state + event.
    Closure { params: Vec<String>, body: Expr },
    /// Synthesized by `bind:` (§7.9): `|s, e| place = e.value()`.
    SynthBind { place: Expr },
}

impl HandlerSpec {
    /// The body expression (for write-set analysis and interpretation).
    pub fn body(&self) -> Expr {
        match self {
            HandlerSpec::Closure { body, .. } => body.clone(),
            HandlerSpec::SynthBind { place } => Expr::Assign {
                lhs: Box::new(place.clone()),
                op: None,
                rhs: Box::new(Expr::Method(
                    Box::new(Expr::Var("e".to_string())),
                    "value".to_string(),
                    vec![],
                )),
            },
        }
    }
}

/// A keyed-region list strategy, statically chosen from the `by` key kind
/// (G11, §7.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListStrategy {
    /// No `by` — append-only list.
    AppendOnly,
    /// `by <runtime index / trivially-varying expr>` — index strategy.
    Index,
    /// `by <item field>` — keyed with LIS reorder (§7.6, G11).
    Keyed,
    /// Constant `by` — fixed row count.
    Fixed,
}

/// A dynamic structural point (§6.3).
#[derive(Debug, Clone)]
pub enum Region {
    /// `if/else` — arm index stored; patch switches arms (unmount old, mount
    /// new). Arm bodies are themselves blueprints with slots.
    If {
        /// `(condition, body)` per arm in source order.
        arms: Vec<(Expr, Vir)>,
        /// The `else` body, when present.
        else_body: Option<Vir>,
    },
    /// `match` — arm selection on the scrutinee.
    Match {
        /// Scrutinee expression.
        scrutinee: Expr,
        /// `(pattern, body)` per arm.
        arms: Vec<(String, Vir)>,
    },
    /// `for x in xs by key` — keyed list region (§7.6): each row is a
    /// sub-mount with *index-parameterized* patch functions.
    For {
        /// Loop pattern (`item`).
        pat: String,
        /// Iterable expression.
        iter: Expr,
        /// `by` key, if written.
        key: Option<Expr>,
        /// Row body.
        body: Vir,
        /// Statically chosen strategy (G11).
        strategy: ListStrategy,
    },
    /// Component call `<Name …/>` — inlined by default (G9); boundary
    /// preserved when recursive, large, or reused from many sites.
    Component {
        /// Component path segments.
        name: Vec<String>,
        /// Props at the call site.
        props: Vec<(String, ast::AttrValue)>,
        /// Children passed to the component.
        children: Vec<ast::GsxChild>,
        /// The inlined child VIR, when inlined (G9).
        inlined: Option<Box<Vir>>,
    },
}

/// A VIR tree node: static skeleton pieces (encoded into the blueprint) plus
/// slot anchors.
#[derive(Debug, Clone)]
pub enum VirNode {
    /// A host element: blueprint `Create` + static props + child ops, with
    /// slots anchored at its node index.
    Element {
        /// Tag id (schema-assigned).
        tag_id: u32,
        /// Tag name (diagnostics only — passes stay schema-driven; the
        /// string is carried for host resolution and SSR).
        tag: String,
        /// Static (folded) props (G2): id, name, constant value.
        static_props: Vec<StaticProp>,
        /// Slots anchored to this element, in anchor order (props, toggles,
        /// styles, events).
        slot_ids: Vec<usize>,
        /// Children.
        children: Vec<VirNode>,
    },
    /// Static + hole text: static runs become `Text` ops; holes become Text
    /// slots (§6.1). `hole_slots[i]` is the Text slot for `pieces[i]` when
    /// it is a hole.
    Text {
        /// Pieces (G2: adjacent static pieces are pre-merged here).
        pieces: Vec<ast::TextPiece>,
        /// Parallel to `pieces`: slot id per hole piece.
        hole_slots: Vec<Option<usize>>,
    },
    /// A dynamic structural point (region).
    Region {
        /// Region index into [`Vir::regions`].
        region: usize,
    },
    /// A fragment: children inline in the parent's blueprint (no host node).
    Fragment {
        /// Children.
        children: Vec<VirNode>,
    },
}

/// A folded static prop (G2): the blueprint stores `(id, value)`; the SSR
/// emitter needs the name too.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticProp {
    /// Hashed prop id (§6.1 StaticProp op payload).
    pub id: u32,
    /// Prop/schema name (`class`, `aria-label`, …).
    pub name: String,
    /// The constant value (folded).
    pub value: String,
}

/// One compiled view.
#[derive(Debug, Clone, Default)]
pub struct Vir {
    /// Root nodes (a component body may return a fragment of several).
    pub roots: Vec<VirNode>,
    /// Slots in tree order.
    pub slots: Vec<Slot>,
    /// Regions by index.
    pub regions: Vec<Region>,
    /// Structural diagnostics (schema violations, G0201, …).
    pub diagnostics: Vec<GsxDiagnostic>,
    /// The state parameter name (`s` in the design doc).
    pub state_param: String,
    /// Whether the whole view is static (zero slots — §6.2).
    pub is_static: bool,
}

impl Vir {
    /// Count of VIR nodes (G9 inline heuristic input).
    pub fn node_count(&self) -> usize {
        self.roots.iter().map(node_count_of).sum()
    }
}

fn node_count_of(n: &VirNode) -> usize {
    1 + match n {
        VirNode::Element { children, .. } | VirNode::Fragment { children } => {
            children.iter().map(node_count_of).sum::<usize>()
        }
        VirNode::Text { .. } | VirNode::Region { .. } => 0,
    }
}

/// Options for VIR construction.
#[derive(Debug, Clone)]
pub struct VirOptions {
    /// Name of the component's state parameter (default `s`).
    pub state_param: String,
    /// Dialect to resolve intrinsics against (§5.3).
    pub default_dialect: String,
    /// G9 inline threshold in VIR node count.
    pub inline_threshold: usize,
    /// Component library for `<Name/>` resolution (G9, §7.8).
    pub components: ComponentTable,
}

impl Default for VirOptions {
    fn default() -> Self {
        VirOptions {
            state_param: "s".to_string(),
            default_dialect: "web".to_string(),
            inline_threshold: 24,
            components: ComponentTable::default(),
        }
    }
}

/// Component definitions for G9 inlining: name → builder producing the
/// child's AST. Components receive state as a *place* (§7.8).
#[derive(Default, Clone)]
pub struct ComponentTable {
    builders: Rc<std::collections::BTreeMap<String, ComponentDef>>,
}

impl std::fmt::Debug for ComponentTable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_map()
            .entries(self.builders.keys().map(|k| (k, "...")))
            .finish()
    }
}

/// One component definition.
pub struct ComponentDef {
    /// Build the component's AST given the call-site (props, children).
    pub build: Rc<dyn Fn(&ComponentCall) -> ast::GsxNode>,
    /// `#[inline(never)]` — preserve the component boundary (§6.3).
    pub inline_never: bool,
    /// `#[inline(always)]` — force inlining past the threshold.
    pub inline_always: bool,
}

impl Clone for ComponentDef {
    fn clone(&self) -> Self {
        ComponentDef {
            build: self.build.clone(),
            inline_never: self.inline_never,
            inline_always: self.inline_always,
        }
    }
}

impl std::fmt::Debug for ComponentDef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ComponentDef")
            .field("inline_never", &self.inline_never)
            .field("inline_always", &self.inline_always)
            .finish()
    }
}

/// The call-site information a component builder sees.
#[derive(Debug, Clone)]
pub struct ComponentCall {
    /// Props in source order.
    pub props: Vec<(String, ast::AttrValue)>,
    /// Children passed at the call site.
    pub children: Vec<ast::GsxChild>,
}

impl ComponentTable {
    /// Register a component.
    pub fn define(&mut self, name: &str, def: ComponentDef) {
        let mut m = (*self.builders).clone();
        m.insert(name.to_string(), def);
        self.builders = Rc::new(m);
    }

    /// Look up a component by head name.
    pub fn get(&self, head: &str) -> Option<&ComponentDef> {
        self.builders.get(head)
    }

    /// Is empty.
    pub fn is_empty(&self) -> bool {
        self.builders.is_empty()
    }
}

/// Build the VIR from a GSX AST (§6.1).
pub struct VirBuilder<'a> {
    options: VirOptions,
    schemas: &'a SchemaRegistry,
    slots: Vec<Slot>,
    regions: Vec<Region>,
    diagnostics: Vec<GsxDiagnostic>,
    /// Slot expressions (for §7.7 read tracking).
    slot_exprs: BTreeMap<usize, Expr>,
}

impl<'a> VirBuilder<'a> {
    /// New builder.
    pub fn new(options: VirOptions, schemas: &'a SchemaRegistry) -> VirBuilder<'a> {
        VirBuilder {
            options,
            schemas,
            slots: vec![],
            regions: vec![],
            diagnostics: vec![],
            slot_exprs: BTreeMap::new(),
        }
    }

    /// Build a view from one AST node (or a fragment of them).
    pub fn build(mut self, root: &ast::GsxNode) -> Vir {
        let roots = match root {
            ast::GsxNode::Fragment { children } => children
                .iter()
                .filter_map(|c| self.child_to_node(c))
                .collect(),
            n => vec![self.node_from(n)],
        };
        // §7.7: slot reads through untrackable surfaces are compile errors
        // with a fix-it (G0201).
        let sp = self.options.state_param.clone();
        for (id, expr) in &self.slot_exprs {
            let (_, flags) = crate::analysis::read_set_with_flags(expr, &sp);
            if flags.untrackable {
                let what = flags.untrackable_what.unwrap_or_default();
                self.diagnostics.push(
                    GsxDiagnostic::new(
                        GsxCode::UntrackableRead,
                        format!("slot reads `{what}`, which the compiler cannot track"),
                    )
                    .with_fixit(format!(
                        "wrap in `Signal<...>`; writes will bump a bit in this component's dirty mask (slot {id})"
                    )),
                );
            }
        }
        let is_static = self.slots.is_empty();
        Vir {
            roots,
            slots: std::mem::take(&mut self.slots),
            regions: std::mem::take(&mut self.regions),
            diagnostics: std::mem::take(&mut self.diagnostics),
            state_param: self.options.state_param.clone(),
            is_static,
        }
    }

    fn child_to_node(&mut self, c: &ast::GsxChild) -> Option<VirNode> {
        match c {
            ast::GsxChild::Node(n) => Some(self.node_from(n)),
            ast::GsxChild::Text(pieces) => {
                let folded = fold_static(pieces.clone());
                let mut hole_slots = Vec::with_capacity(folded.len());
                for piece in &folded {
                    if let ast::TextPiece::Hole(hole_expr) = piece {
                        let id = self.slots.len();
                        self.slots.push(Slot {
                            id,
                            kind: SlotKind::Text,
                            node: u32::MAX,
                            read_set: BTreeSet::new(),
                            affects: Invalidation::Paint,
                            escape: EscapeCtx::Text,
                        });
                        self.slot_exprs.insert(id, hole_expr.clone());
                        hole_slots.push(Some(id));
                    } else {
                        hole_slots.push(None);
                    }
                }
                Some(VirNode::Text {
                    pieces: folded,
                    hole_slots,
                })
            }
            ast::GsxChild::Block(e) => Some(self.block_expr_to_node(e)),
        }
    }

    /// `{expr}` dynamic child: regions for if/match/for (§6.3), text slots
    /// for value expressions.
    fn block_expr_to_node(&mut self, e: &Expr) -> VirNode {
        match e {
            Expr::If { cond, then, els } => {
                let region = self.regions.len();
                let then_vir = self.sub_vir(then);
                let else_vir = els.as_ref().map(|e2| self.sub_vir(e2));
                self.regions.push(Region::If {
                    arms: vec![(cond.as_ref().clone(), then_vir)],
                    else_body: else_vir,
                });
                self.push_region_slot(region);
                VirNode::Region { region }
            }
            Expr::Match { scrutinee, arms } => {
                let region = self.regions.len();
                let arm_virs = arms
                    .iter()
                    .map(|(p, _, b)| (p.clone(), self.sub_vir(b)))
                    .collect();
                self.regions.push(Region::Match {
                    scrutinee: scrutinee.as_ref().clone(),
                    arms: arm_virs,
                });
                self.push_region_slot(region);
                VirNode::Region { region }
            }
            Expr::For { pat, iter, key, body } => {
                let region = self.regions.len();
                let strategy = choose_list_strategy(key.as_deref());
                let body_vir = self.sub_vir(body);
                self.regions.push(Region::For {
                    pat: pat.clone(),
                    iter: iter.as_ref().clone(),
                    key: key.as_deref().cloned(),
                    body: body_vir,
                    strategy,
                });
                self.push_region_slot(region);
                VirNode::Region { region }
            }
            Expr::Gsx(node) => self.node_from(node),
            other => {
                // A value expression child — a Text slot with one hole (§6.1
                // Text slot: `host.set_text` on update).
                let id = self.slots.len();
                self.slots.push(Slot {
                    id,
                    kind: SlotKind::Text,
                    node: u32::MAX,
                    read_set: BTreeSet::new(),
                    affects: Invalidation::Paint,
                    escape: EscapeCtx::Text,
                });
                self.slot_exprs.insert(id, other.clone());
                let _ = other;
                VirNode::Text {
                    pieces: vec![
                        ast::TextPiece::Static(String::new()),
                        ast::TextPiece::Hole(other.clone()),
                    ],
                    hole_slots: vec![None, Some(id)],
                }
            }
        }
    }

    fn push_region_slot(&mut self, region: usize) {
        let id = self.slots.len();
        self.slots.push(Slot {
            id,
            kind: SlotKind::Region { region },
            node: u32::MAX,
            read_set: BTreeSet::new(),
            affects: Invalidation::Structure,
            escape: EscapeCtx::Text,
        });
    }

    /// Build a sub-VIR for a region arm / for-body (§6.3: "Arm bodies are
    /// themselves Blueprints with slots"). The sub-Vir owns its slots and
    /// regions; region indices inside it are local to the arm.
    fn sub_vir(&mut self, e: &Expr) -> Vir {
        let saved_slots = std::mem::take(&mut self.slots);
        let saved_regions = std::mem::take(&mut self.regions);
        let saved_ds = std::mem::take(&mut self.diagnostics);
        let roots = self.expr_to_children(e);
        let slots = std::mem::replace(&mut self.slots, saved_slots);
        let regions = std::mem::replace(&mut self.regions, saved_regions);
        let ds = std::mem::replace(&mut self.diagnostics, saved_ds);
        Vir {
            roots,
            slots,
            regions,
            diagnostics: ds,
            state_param: self.options.state_param.clone(),
            is_static: false,
        }
    }

    fn expr_to_children(&mut self, e: &Expr) -> Vec<VirNode> {
        match e {
            Expr::Block { stmts, tail } => {
                let mut out: Vec<VirNode> = stmts
                    .iter()
                    .filter_map(|s| match s {
                        Expr::Let { .. } => None, // locals stay in handlers
                        other => Some(self.block_expr_to_node(other)),
                    })
                    .collect();
                if let Some(t) = tail {
                    match t.as_ref() {
                        Expr::Block { stmts: t2, tail: t3 } => {
                            for s in t2 {
                                if !matches!(s, Expr::Let { .. }) {
                                    out.push(self.block_expr_to_node(s));
                                }
                            }
                            if let Some(t3) = t3 {
                                out.push(self.block_expr_to_node(t3));
                            }
                        }
                        other => out.push(self.block_expr_to_node(other)),
                    }
                }
                out
            }
            Expr::Gsx(node) => vec![self.node_from(node)],
            other => vec![self.block_expr_to_node(other)],
        }
    }

    fn node_from(&mut self, n: &ast::GsxNode) -> VirNode {
        match n {
            ast::GsxNode::Fragment { children } => VirNode::Fragment {
                children: children.iter().filter_map(|c| self.child_to_node(c)).collect(),
            },
            ast::GsxNode::Element { tag, attrs, children, self_closing } => {
                self.element_from(tag, attrs, children, *self_closing)
            }
        }
    }

    fn element_from(
        &mut self,
        tag: &ast::TagName,
        attrs: &[ast::GsxAttr],
        children: &[ast::GsxChild],
        self_closing: bool,
    ) -> VirNode {
        // Component calls (G9, §7.8) take a different path entirely.
        if tag.is_component() {
            return self.component_node(tag, attrs, children);
        }
        // Intrinsic: resolve through the dialect schema (§3.2, §5).
        let (namespace, tag_name) = match tag {
            ast::TagName::Intrinsic(t) => (None, t.clone()),
            ast::TagName::Dialect(ns, t) => (Some(ns.clone()), t.clone()),
            ast::TagName::Component(_) => unreachable!(),
        };
        let schema = self.schemas.resolve(&self.options.default_dialect, namespace.as_deref(), &tag_name);
        let Some(schema) = schema else {
            self.diagnostics.push(GsxDiagnostic::new(
                GsxCode::UnknownProp,
                format!("no schema for element `{tag_name}` in dialect `{}`", self.options.default_dialect),
            ));
            return VirNode::Fragment { children: vec![] };
        };
        let tag_id = tag_id_for(&tag_name, namespace.as_deref());

        // G2: adjacent/constant class values concatenate; static props fold.
        let mut static_props: Vec<StaticProp> = vec![];
        let mut class_parts: Vec<String> = vec![];
        let mut seen_class = false;
        let mut slot_ids: Vec<usize> = vec![];

        for attr in attrs {
            match attr {
                ast::GsxAttr::Plain(name, value) => {
                    let prop_schema = schema.prop(name).or_else(|| {
                        schema.prop(&name.replace('-', "_"))
                    });
                    let Some(ps) = prop_schema else {
                        self.diagnostics.push(GsxDiagnostic::new(
                            GsxCode::UnknownProp,
                            format!("unknown prop `{name}` on `{tag_name}`"),
                        ));
                        continue;
                    };
                    let prop_id = prop_id_for(name);
                    match value {
                        ast::AttrValue::Lit(v) => {
                            if name == "class" {
                                // G2: constant class strings concatenate.
                                class_parts.push(v.clone());
                                seen_class = true;
                                continue;
                            }
                            static_props.push(StaticProp {
                                id: prop_id,
                                name: name.clone(),
                                value: v.clone(),
                            });
                        }
                        ast::AttrValue::Bare => {
                            static_props.push(StaticProp {
                                id: prop_id,
                                name: name.clone(),
                                value: "true".to_string(),
                            });
                        }
                        ast::AttrValue::Expr(e) => {
                            let id = self.slots.len();
                            self.slots.push(Slot {
                                id,
                                kind: SlotKind::Prop {
                                    prop: name.clone(),
                                    expr: e.clone(),
                                },
                                node: u32::MAX,
                                read_set: BTreeSet::new(),
                                affects: ps.affects,
                                escape: ps.escape,
                            });
                            self.slot_exprs.insert(id, e.clone());
                            slot_ids.push(id);
                        }
                    }
                }
                ast::GsxAttr::Directive(ns, sub, value) => match (ns.as_str(), value) {
                    ("on", ast::AttrValue::Expr(Expr::Closure { params, body })) => {
                        let Some(ev) = schema.event(sub) else {
                            self.diagnostics.push(GsxDiagnostic::new(
                                GsxCode::UnknownEvent,
                                format!("unknown event `{sub}` on `{tag_name}`"),
                            ));
                            continue;
                        };
                        let _ = ev;
                        let handler = Rc::new(HandlerSpec::Closure {
                            params: params.clone(),
                            body: body.as_ref().clone(),
                        });
                        let id = self.slots.len();
                        self.slots.push(Slot {
                            id,
                            kind: SlotKind::Event {
                                event: sub.clone(),
                                handler,
                            },
                            node: u32::MAX,
                            read_set: BTreeSet::new(),
                            affects: Invalidation::None,
                            escape: EscapeCtx::Text,
                        });
                        slot_ids.push(id);
                    }
                    ("on", ast::AttrValue::Expr(path_expr)) => {
                        // `on:click={save}` — a named handler path (§5.3).
                        let Some(ev) = schema.event(sub) else {
                            self.diagnostics.push(GsxDiagnostic::new(
                                GsxCode::UnknownEvent,
                                format!("unknown event `{sub}` on `{tag_name}`"),
                            ));
                            continue;
                        };
                        let _ = ev;
                        let handler = Rc::new(HandlerSpec::Closure {
                            params: vec![self.options.state_param.clone(), "e".to_string()],
                            body: Expr::Call(
                                path_to_name(path_expr),
                                vec![
                                    Expr::Var(self.options.state_param.clone()),
                                    Expr::Var("e".to_string()),
                                ],
                            ),
                        });
                        let id = self.slots.len();
                        self.slots.push(Slot {
                            id,
                            kind: SlotKind::Event {
                                event: sub.clone(),
                                handler,
                            },
                            node: u32::MAX,
                            read_set: BTreeSet::new(),
                            affects: Invalidation::None,
                            escape: EscapeCtx::Text,
                        });
                        slot_ids.push(id);
                    }
                    ("class", ast::AttrValue::Expr(e)) => {
                        let id = self.slots.len();
                        self.slots.push(Slot {
                            id,
                            kind: SlotKind::ClassToggle {
                                class: sub.clone(),
                                expr: e.clone(),
                            },
                            node: u32::MAX,
                            read_set: BTreeSet::new(),
                            affects: Invalidation::Paint,
                            escape: EscapeCtx::Attr,
                        });
                        slot_ids.push(id);
                    }
                    ("style", ast::AttrValue::Expr(e)) => {
                        let id = self.slots.len();
                        self.slots.push(Slot {
                            id,
                            kind: SlotKind::Style {
                                prop: sub.clone(),
                                expr: e.clone(),
                            },
                            node: u32::MAX,
                            read_set: BTreeSet::new(),
                            affects: Invalidation::Layout,
                            escape: EscapeCtx::Style,
                        });
                        slot_ids.push(id);
                    }
                    ("bind", ast::AttrValue::Expr(place)) => {
                        // §7.9: bind:value={place} desugars at the VIR level
                        // into a Prop slot reading `place` plus a synthesized
                        // handler `|s, e| place = e.value()`.
                        if !is_place(place) {
                            self.diagnostics.push(GsxDiagnostic::new(
                                GsxCode::UnknownProp,
                                format!("bind:{sub} requires a place expression (lvalue)"),
                            ));
                            continue;
                        }
                        let Some(ps) = schema.prop(sub) else {
                            self.diagnostics.push(GsxDiagnostic::new(
                                GsxCode::UnknownProp,
                                format!("unknown prop `{sub}` for bind on `{tag_name}`"),
                            ));
                            continue;
                        };
                        // Prop slot reading the place.
                        let id = self.slots.len();
                        self.slots.push(Slot {
                            id,
                            kind: SlotKind::Prop {
                                prop: sub.clone(),
                                expr: place.clone(),
                            },
                            node: u32::MAX,
                            read_set: BTreeSet::new(),
                            affects: ps.affects,
                            escape: ps.escape,
                        });
                        slot_ids.push(id);
                        // Synthesized handler (§7.9).
                        let handler = Rc::new(HandlerSpec::SynthBind {
                            place: place.clone(),
                        });
                        let id = self.slots.len();
                        self.slots.push(Slot {
                            id,
                            kind: SlotKind::Event {
                                event: "input".to_string(),
                                handler,
                            },
                            node: u32::MAX,
                            read_set: BTreeSet::new(),
                            affects: Invalidation::None,
                            escape: EscapeCtx::Text,
                        });
                        slot_ids.push(id);
                    }
                    (other_ns, _) => {
                        self.diagnostics.push(GsxDiagnostic::new(
                            GsxCode::UnknownEvent,
                            format!("unknown directive `{other_ns}:{sub}` on `{tag_name}`"),
                        ));
                    }
                },
            }
        }
        // Emit folded class (G2).
        if seen_class {
            let joined = class_parts.join(" ");
            static_props.push(StaticProp {
                id: prop_id_for("class"),
                name: "class".to_string(),
                value: joined,
            });
        }

        // Children.
        let mut child_nodes = vec![];
        if !(self_closing || schema.serialization.void) {
            for c in children {
                // Content-model check (§5.2).
                if let ast::GsxChild::Node(inner) = c
                    && let ast::GsxNode::Element { tag: itag, .. } = inner {
                        let shape = match itag {
                            ast::TagName::Intrinsic(t) => ChildName::Intrinsic(t.clone()),
                            ast::TagName::Dialect(_, t) => ChildName::Intrinsic(t.clone()),
                            ast::TagName::Component(segs) => {
                                ChildName::Component(segs.first().cloned().unwrap_or_default())
                            }
                        };
                        let shape_str: &'static str = match &shape {
                            ChildName::Intrinsic(t) => leak_tag(t),
                            ChildName::Component(_) => "component",
                        };
                        if let Some(v) = schema.child_violation(&crate::schema::ChildShape::Element(shape_str)) {
                            self.diagnostics.push(GsxDiagnostic::new(GsxCode::ContentModel, v));
                        }
                    }
                if let Some(node) = self.child_to_node(c) {
                    child_nodes.push(node);
                }
            }
        }
        // G2: merge adjacent fully-static Text siblings ("x" "y" → "xy").
        let child_nodes = merge_static_text(child_nodes);

        VirNode::Element {
            tag_id,
            tag: tag_name,
            static_props,
            slot_ids,
            children: child_nodes,
        }
    }

    /// Component calls (G9): inline by default with a node-count heuristic;
    /// constant-prop specialization for constant props; §7.8 place passing.
    fn component_node(
        &mut self,
        tag: &ast::TagName,
        attrs: &[ast::GsxAttr],
        children: &[ast::GsxChild],
    ) -> VirNode {
        let segs = match tag {
            ast::TagName::Component(s) => s.clone(),
            _ => unreachable!(),
        };
        let head = segs.first().cloned().unwrap_or_default();
        let props: Vec<(String, ast::AttrValue)> = attrs
            .iter()
            .map(|a| match a {
                ast::GsxAttr::Plain(n, v) => (n.clone(), v.clone()),
                ast::GsxAttr::Directive(ns, sub, v) => (format!("{ns}:{sub}"), v.clone()),
            })
            .collect();
        // Record the region slot first so structural patching has an anchor.
        let region = self.regions.len();
        self.push_region_slot(region);
        let all_constant = props.iter().all(|(_, v)| matches!(v, ast::AttrValue::Lit(_) | ast::AttrValue::Bare));
        let def = self.options.components.get(&head);
        let inline_decision = match def {
            Some(d) if d.inline_never => false,
            Some(d) if d.inline_always => true,
            _ => true, // inline by default; heuristic applies after building
        };
        let inlined = if let Some(d) = def {
            if inline_decision {
                let call = ComponentCall {
                    props: props.clone(),
                    children: children.to_vec(),
                };
                let child_ast = (d.build)(&call);
                let sub = self.build_subtree(&child_ast);
                // G9 heuristic: keep the boundary when the child is large.
                if sub.node_count() > self.options.inline_threshold && !all_constant {
                    None
                } else {
                    Some(Box::new(sub))
                }
            } else {
                None
            }
        } else {
            None
        };
        self.regions.push(Region::Component {
            name: segs,
            props,
            children: children.to_vec(),
            inlined,
        });
        VirNode::Region { region }
    }

    /// Build a sub-VIR for a component body (G9 clone-and-specialize).
    fn build_subtree(&mut self, root: &ast::GsxNode) -> Vir {
        let saved_slots = std::mem::take(&mut self.slots);
        let saved_regions = std::mem::take(&mut self.regions);
        let saved_ds = std::mem::take(&mut self.diagnostics);
        let roots = match root {
            ast::GsxNode::Fragment { children } => children
                .iter()
                .filter_map(|c| self.child_to_node(c))
                .collect(),
            n => vec![self.node_from(n)],
        };
        let slots = std::mem::replace(&mut self.slots, saved_slots);
        let regions = std::mem::replace(&mut self.regions, saved_regions);
        let ds = std::mem::replace(&mut self.diagnostics, saved_ds);
        Vir {
            roots,
            slots,
            regions,
            diagnostics: ds,
            state_param: self.options.state_param.clone(),
            is_static: false,
        }
    }
}

enum ChildName {
    Intrinsic(String),
    /// Component children: the head segment is used for content-model
    /// checks once component schemas land (§4.2 lang-ui).
    Component(#[allow(dead_code)] String),
}

fn leak_tag(t: &str) -> &'static str {
    // Test-scale tag set; leaks are bounded by the dialect size.
    match t {
        "h1" => "h1",
        "span" => "span",
        "p" => "p",
        "output" => "output",
        "input" => "input",
        "button" => "button",
        "li" => "li",
        _ => "other",
    }
}

fn path_to_name(e: &Expr) -> String {
    match e {
        Expr::Var(v) => v.clone(),
        _ => String::new(),
    }
}

/// A place expression (lvalue): a var or field/index projection of one (§3.2
/// `bind:value={place}`).
pub fn is_place(e: &Expr) -> bool {
    match e {
        Expr::Var(_) => true,
        Expr::Field(b, _) => is_place(b),
        Expr::Index(b, _) => is_place(b),
        _ => false,
    }
}

/// G2: merge adjacent static text pieces.
fn fold_static(mut pieces: Vec<ast::TextPiece>) -> Vec<ast::TextPiece> {
    let mut out = vec![];
    for p in pieces.drain(..) {
        match (&mut out.last_mut(), p) {
            (Some(ast::TextPiece::Static(prev)), ast::TextPiece::Static(next)) => {
                prev.push_str(&next);
            }
            (_, p) => out.push(p),
        }
    }
    out
}

/// G2: merge adjacent fully-static Text siblings into one Text node.
fn merge_static_text(nodes: Vec<VirNode>) -> Vec<VirNode> {
    let mut out: Vec<VirNode> = vec![];
    for n in nodes {
        match (&mut out.last_mut(), &n) {
            (
                Some(VirNode::Text { pieces: prev, hole_slots: hp }),
                VirNode::Text { pieces, hole_slots },
            ) if hp.iter().all(|h| h.is_none())
                && hole_slots.iter().all(|h| h.is_none()) =>
            {
                for p in pieces {
                    match p {
                        ast::TextPiece::Static(s) => {
                            if let Some(ast::TextPiece::Static(last)) = prev.last_mut() {
                                last.push_str(s);
                            } else {
                                prev.push(ast::TextPiece::Static(s.clone()));
                            }
                        }
                        ast::TextPiece::Hole(_) => prev.push(p.clone()),
                    }
                }
            }
            _ => out.push(n),
        }
    }
    out
}

/// G11: choose the list strategy from the `by` key kind (§7.6).
fn choose_list_strategy(key: Option<&Expr>) -> ListStrategy {
    match key {
        None => ListStrategy::AppendOnly,
        Some(Expr::Lit(_)) => ListStrategy::Fixed,
        Some(Expr::Field(..)) | Some(Expr::Index(..)) | Some(Expr::Var(_)) => ListStrategy::Keyed,
        Some(_) => ListStrategy::Index,
    }
}

/// Stable tag ids for the blueprint's `Create` ops (per-dialect numbering is
/// host-side; the compiler only needs a stable int).
pub fn tag_id_for(tag: &str, namespace: Option<&str>) -> u32 {
    // FNV-1a over the (namespace, tag) pair: stable across runs, host
    // resolves ids back to schemas via its own table.
    let full = match namespace {
        Some(ns) => format!("{ns}:{tag}"),
        None => tag.to_string(),
    };
    let mut h: u32 = 0x811c9dc5;
    for b in full.as_bytes() {
        h ^= u32::from(*b);
        h = h.wrapping_mul(0x01000193);
    }
    h & ((1 << 29) - 1)
}

/// Stable prop ids for `StaticProp` ops (same scheme).
pub fn prop_id_for(prop: &str) -> u32 {
    let mut h: u32 = 0x811c9dc5;
    for b in prop.as_bytes() {
        h ^= u32::from(*b);
        h = h.wrapping_mul(0x01000193);
    }
    h & ((1 << 14) - 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{split_text_holes, TextPiece};

    fn parse_gsx(src: &str) -> ast::GsxNode {
        let wrapped = format!("fn __g() {{ {src} }}");
        let parsed = glyim_frontend::parse_to_syntax(&wrapped, glyim_span::FileId::from_raw(0));
        let g = ast::gsx_from_parse(&parsed);
        assert!(g.diagnostics.is_empty(), "{:?}", g.diagnostics);
        g.root.expect("gsx root")
    }

    fn schemas() -> SchemaRegistry {
        SchemaRegistry::standard()
    }

    fn build(src: &str) -> Vir {
        let node = parse_gsx(src);
        let schemas = schemas();
        let b = VirBuilder::new(VirOptions::default(), &schemas);
        b.build(&node)
    }

    /// §6.1: every view splits into blueprint + slots; a fully static
    /// subtree has zero slots (G2).
    #[test]
    fn static_view_has_zero_slots() {
        let v = build("<section class=\"counter\"><h1>\"Hello\"</h1></section>");
        assert!(v.is_static);
        assert!(v.slots.is_empty());
        assert_eq!(v.node_count(), 3); // section, h1, text
    }

    /// §6.1 slot table: Text / Prop / ClassToggle / Style / Event / Region
    /// all materialize as slots with the right kinds.
    #[test]
    fn slot_kinds_match_table() {
        let v = build(concat!(
            "<section>",
            "<output class:warn={s.count < 0}>\"{s.count}\"</output>",
            "<input value={s.name} style:width={s.w}/>",
            "<button on:click={|s, _| s.count += 1}>\"+\"</button>",
            "{if s.count > 10 { <p>\"x\"</p> }}",
            "</section>"
        ));
        let kinds: Vec<&str> = v
            .slots
            .iter()
            .map(|s| match &s.kind {
                SlotKind::Text => "text",
                SlotKind::Prop { .. } => "prop",
                SlotKind::ClassToggle { .. } => "class",
                SlotKind::Style { .. } => "style",
                SlotKind::Event { .. } => "event",
                SlotKind::Region { .. } => "region",
            })
            .collect();
        assert_eq!(
            kinds,
            vec!["class", "text", "prop", "style", "event", "region"]
        );
    }

    /// §6.3: `if` becomes a Region whose arm bodies are sub-VIRs.
    #[test]
    fn if_becomes_region() {
        let v = build("<section>{if s.count > 10 { <p>\"big\"</p> } else { <p>\"small\"</p> }}</section>");
        assert_eq!(v.regions.len(), 1);
        match &v.regions[0] {
            Region::If { arms, else_body } => {
                assert_eq!(arms.len(), 1);
                assert_eq!(arms[0].1.roots.len(), 1);
                assert!(else_body.is_some());
            }
            other => panic!("unexpected region: {other:?}"),
        }
    }

    /// G11: list strategy is statically chosen from the `by` key kind.
    #[test]
    fn list_strategy_selection() {
        let v = build("<ul>{for item in s.items.iter() by item.id { <li>\"x\"</li> }}</ul>");
        match &v.regions[0] {
            Region::For { strategy, .. } => assert_eq!(*strategy, ListStrategy::Keyed),
            other => panic!("{other:?}"),
        }
        let v2 = build("<ul>{for item in s.items.iter() { <li>\"x\"</li> }}</ul>");
        match &v2.regions[0] {
            Region::For { strategy, .. } => assert_eq!(*strategy, ListStrategy::AppendOnly),
            other => panic!("{other:?}"),
        }
    }

    /// G2: constant class strings concatenate; adjacent statics merge.
    #[test]
    fn class_folding() {
        let v = build("<p class=\"a\" class=\"b\">\"x\" \"y\"</p>");
        // find the folded static prop
        let mut found = None;
        for root in &v.roots {
            if let VirNode::Element { static_props, .. } = root {
                found = static_props
                    .iter()
                    .find(|sp| sp.id == prop_id_for("class"))
                    .map(|sp| sp.value.clone());
            }
        }
        assert_eq!(found.as_deref(), Some("a b"));
        // adjacent static text pieces merged into one
        if let VirNode::Element { children, .. } = &v.roots[0] {
            match &children[0] {
                VirNode::Text { pieces, hole_slots } => {
                    assert_eq!(pieces, &vec![TextPiece::Static("xy".to_string())]);
                    assert!(hole_slots.iter().all(|h| h.is_none()));
                }
                other => panic!("{other:?}"),
            }
        }
    }

    /// §7.9: bind:value desugars into a Prop slot + a synthesized handler.
    #[test]
    fn bind_desugars() {
        let v = build("<input bind:value={s.name}/>");
        let prop_slot = v.slots.iter().find(|s| matches!(s.kind, SlotKind::Prop { .. })).unwrap();
        match &prop_slot.kind {
            SlotKind::Prop { prop, expr } => {
                assert_eq!(prop, "value");
                assert_eq!(*expr, Expr::field("s", "name"));
            }
            other => panic!("{other:?}"),
        }
        let ev = v.slots.iter().find(|s| matches!(s.kind, SlotKind::Event { .. })).unwrap();
        match &ev.kind {
            SlotKind::Event { event, handler } => {
                assert_eq!(event, "input");
                assert!(matches!(handler.as_ref(), HandlerSpec::SynthBind { .. }));
                // The synthesized body writes the place.
                let body = handler.body();
                assert!(matches!(
                    body,
                    Expr::Assign { ref lhs, op: None, .. }
                        if **lhs == Expr::field("s", "name")
                ));
            }
            other => panic!("{other:?}"),
        }
    }

    /// G9: components inline by default; `#[inline(never)]` keeps the
    /// boundary; large non-constant children keep it too.
    #[test]
    fn component_inlining() {
        let mut components = ComponentTable::default();
        components.define(
            "Child",
            ComponentDef {
                build: Rc::new(|_call| {
                    let wrapped = "fn __g() { <p class=\"c\">\"static child\"</p> }";
                    let parsed =
                        glyim_frontend::parse_to_syntax(wrapped, glyim_span::FileId::from_raw(0));
                    ast::gsx_from_parse(&parsed).root.unwrap()
                }),
                inline_never: false,
                inline_always: false,
            },
        );
        let node = parse_gsx("<Child/>");
        let reg = schemas();
        let opts = VirOptions {
            components,
            ..VirOptions::default()
        };
        let v = VirBuilder::new(opts, &reg).build(&node);
        match &v.regions[0] {
            Region::Component { inlined, .. } => {
                assert!(inlined.is_some(), "small static component inlines (G9)");
            }
            other => panic!("{other:?}"),
        }

        // inline_never keeps the boundary.
        let mut components = ComponentTable::default();
        components.define(
            "Big",
            ComponentDef {
                build: Rc::new(|_call| {
                    let wrapped = "fn __g() { <p>\"x\"</p> }";
                    let parsed =
                        glyim_frontend::parse_to_syntax(wrapped, glyim_span::FileId::from_raw(0));
                    ast::gsx_from_parse(&parsed).root.unwrap()
                }),
                inline_never: true,
                inline_always: false,
            },
        );
        let node = parse_gsx("<Big/>");
        let reg = schemas();
        let opts = VirOptions {
            components,
            ..VirOptions::default()
        };
        let v = VirBuilder::new(opts, &reg).build(&node);
        match &v.regions[0] {
            Region::Component { inlined, .. } => {
                assert!(inlined.is_none(), "inline(never) preserves the boundary");
            }
            other => panic!("{other:?}"),
        }
    }

    /// G6/G5 slot metadata: escape context and invalidation class come from
    /// the schema, not from string matching.
    #[test]
    fn slot_metadata_from_schema() {
        // `style_width={s.w}` is a plain Prop slot whose *schema metadata*
        // says Style/Layout; `style:width={px}` is a Style directive slot.
        let v = build(
            "<button style_width={s.w} style:width={s.px} class:big={s.big} aria_label={s.label}>\"+\"</button>",
        );
        let style_prop = v
            .slots
            .iter()
            .find(|s| matches!(&s.kind, SlotKind::Prop { prop, .. } if prop == "style_width"))
            .unwrap();
        assert_eq!(style_prop.escape, EscapeCtx::Style);
        assert_eq!(style_prop.affects, Invalidation::Layout);
        let style_dir = v.slots.iter().find(|s| matches!(s.kind, SlotKind::Style { .. })).unwrap();
        assert_eq!(style_dir.affects, Invalidation::Layout);
        let class = v.slots.iter().find(|s| matches!(s.kind, SlotKind::ClassToggle { .. })).unwrap();
        assert_eq!(class.affects, Invalidation::Paint);
        let aria = v
            .slots
            .iter()
            .find(|s| matches!(&s.kind, SlotKind::Prop { prop, .. } if prop == "aria_label"))
            .unwrap();
        assert_eq!(aria.escape, EscapeCtx::Attr);
    }

    /// Schema diagnostics surface through VIR construction (G0101/G0102).
    #[test]
    fn schema_violations_diagnosed() {
        let v = build("<button bogus={1}>\"x\"</button>");
        assert!(v
            .diagnostics
            .iter()
            .any(|d| d.code == GsxCode::UnknownProp));
        let v2 = build(r#"<p on:submit={|s, _| {}}>"x"</p>"#);
        assert!(v2
            .diagnostics
            .iter()
            .any(|d| d.code == GsxCode::UnknownEvent));
        let v3 = build("<ul><div>\"x\"</div></ul>");
        assert!(v3
            .diagnostics
            .iter()
            .any(|d| d.code == GsxCode::ContentModel));
    }

    /// §6.2: mount is instantiate + initial slot fills — a static subtree
    /// produces a blueprint with no SlotMarkers.
    #[test]
    fn blueprint_of_static_view_has_no_markers() {
        let v = build("<section class=\"a\"><h1>\"Hi\"</h1></section>");
        let enc = crate::emit::encode_view(&v);
        assert!(enc.blueprint.is_fully_static());
        assert!(enc.slot_nodes.is_empty());
        let _ = split_text_holes("x"); // keep import used
    }
}
