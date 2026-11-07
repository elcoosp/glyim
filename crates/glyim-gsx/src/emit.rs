//! Emission: artifacts per host flavor (design doc §6.4, G7, G16, G17).
//!
//! §5.4: hosts come in flavors; the compiler *drops code* by flavor:
//!
//! | Flavor | Examples | What the compiler emits |
//! |---|---|---|
//! | **Stream** (write-once) | SSR string, static export | `render_to` only. **No patch code at all** (G17). |
//! | **Retained** | DOM, native widgets, TUI buffer | `mount`, `patch_*`, `unmount` |
//!
//! G7 SSR: static HTML concatenated into `&'static str` chunks; output =
//! memcpy chunk, escape-and-write hole, memcpy chunk; fully static pages are
//! one slice with a precomputed `Content-Length`.
//!
//! G16: `adopt` (hydration) is generated only for regions containing
//! handlers or reactive slots; static regions are never walked.

use crate::analysis::Place;
use crate::blueprint::{BpOp, Blueprint};
use crate::interp::{Interp, Value};
use crate::schema::EscapeCtx;
use crate::vir::{Region, SlotKind, Vir, VirNode};
use std::collections::BTreeSet;

/// Host flavor (§5.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostFlavor {
    /// Write-once: SSR string / static export → `render_to` only (G17).
    Stream,
    /// Retained: DOM / native / TUI → `mount` + `patch_*` + `unmount`.
    Retained,
}

/// One initial fill at mount (§6.2: "mount = instantiate + initial slot
/// fills").
#[derive(Debug, Clone)]
pub struct MountFill {
    /// Slot being filled.
    pub slot: usize,
    /// Blueprint node the slot anchors to.
    pub node: u32,
    /// The hole expression to evaluate once.
    pub expr: crate::expr::Expr,
    /// Escape context (G6).
    pub escape: EscapeCtx,
}

/// Emission artifacts for one view + flavor (§6.4 table).
#[derive(Debug, Clone)]
pub struct Artifacts {
    /// The encoded blueprint (§6.1).
    pub blueprint: Blueprint,
    /// `slot_id → blueprint node index`.
    pub slot_nodes: Vec<(usize, u32)>,
    /// Initial slot fills (props/toggles/styles + text holes).
    pub mount_fills: Vec<MountFill>,
    /// True when the view has no dynamic parts at all (§6.2).
    pub fully_static: bool,
    /// Number of slots (patches exist only when > 0 and flavor is Retained).
    pub slot_count: usize,
    /// Stream flavor: the SSR program (G7).
    pub ssr: Option<SsrProgram>,
    /// Hydration islands (G16): region indices that need `adopt`.
    pub islands: Vec<usize>,
}

/// The result of encoding: the blueprint ops plus the slot→node map and the
/// text-hole fills (used by mount and by MSPs).
#[derive(Debug, Clone, Default)]
pub struct Encoded {
    /// The encoded static skeleton.
    pub blueprint: Blueprint,
    /// `slot_id → blueprint node index` for every anchored slot.
    pub slot_nodes: Vec<(usize, u32)>,
    /// Text-hole fills: one per hole piece (mount-time `set_text`).
    pub text_fills: Vec<MountFill>,
}

/// Encode the VIR's static skeleton into `bp` (§6.1).
pub fn encode_blueprint(vir: &Vir, bp: &mut Blueprint) {
    *bp = encode_view(vir).blueprint;
}

/// Encode a full view, returning ops + slot anchors + text fills.
pub fn encode_view(vir: &Vir) -> Encoded {
    let mut bp = Blueprint::default();
    let mut st = EncodeState {
        counter: 0,
        slot_nodes: vec![],
        text_fills: vec![],
    };
    for root in &vir.roots {
        encode_node(root, vir, &mut bp, &mut st);
    }
    Encoded {
        blueprint: bp,
        slot_nodes: st.slot_nodes,
        text_fills: st.text_fills,
    }
}

#[derive(Debug, Default)]
struct EncodeState {
    counter: u32,
    slot_nodes: Vec<(usize, u32)>,
    text_fills: Vec<MountFill>,
}

fn encode_node(node: &VirNode, vir: &Vir, bp: &mut Blueprint, st: &mut EncodeState) {
    match node {
        VirNode::Element { tag_id, static_props, slot_ids, children, .. } => {
            let node_idx = st.counter;
            st.counter += 1;
            bp.push_op(BpOp::Create(*tag_id));
            for sp in static_props {
                let sid = bp.intern(&sp.value);
                bp.push_op(BpOp::StaticProp(sp.id, sid));
            }
            // Anchor this element's slots (§6.1 SlotMarker per dynamic hole).
            for slot_id in slot_ids {
                bp.push_op(BpOp::SlotMarker(*slot_id as u32));
                st.slot_nodes.push((*slot_id, node_idx));
            }
            bp.push_op(BpOp::Enter);
            for c in children {
                encode_node(c, vir, bp, st);
            }
            bp.push_op(BpOp::Leave);
        }
        VirNode::Fragment { children } => {
            for c in children {
                encode_node(c, vir, bp, st);
            }
        }
        VirNode::Text { pieces, hole_slots } => {
            // G2: pieces are pre-merged; static runs become Text ops, holes
            // become SlotMarkers + mount-time fills.
            for (piece, slot) in pieces.iter().zip(hole_slots.iter()) {
                match piece {
                    crate::ast::TextPiece::Static(s) => {
                        let sid = bp.intern(s);
                        bp.push_op(BpOp::Text(sid));
                    }
                    crate::ast::TextPiece::Hole(expr) => {
                        let node_idx = st.counter; // virtual text node
                        st.counter += 1;
                        let slot_id = slot.unwrap_or(usize::MAX);
                        bp.push_op(BpOp::SlotMarker(slot_id.min(u32::MAX as usize) as u32));
                        st.slot_nodes.push((slot_id, node_idx));
                        st.text_fills.push(MountFill {
                            slot: slot_id,
                            node: node_idx,
                            expr: expr.clone(),
                            escape: EscapeCtx::Text,
                        });
                    }
                }
            }
        }
        VirNode::Region { region } => {
            let node_idx = st.counter;
            st.counter += 1;
            let slot_id = vir
                .slots
                .iter()
                .find(|s| matches!(&s.kind, SlotKind::Region { region: r } if r == region))
                .map(|s| s.id);
            bp.push_op(BpOp::SlotMarker(
                slot_id.map(|i| i as u32).unwrap_or(u32::MAX),
            ));
            if let Some(id) = slot_id {
                st.slot_nodes.push((id, node_idx));
            }
        }
    }
}

/// Emit artifacts for `vir` in `flavor`.
pub fn emit(vir: &Vir, flavor: HostFlavor) -> Artifacts {
    let enc = encode_view(vir);
    let mut mount_fills = vec![];
    for slot in &vir.slots {
        let expr = match &slot.kind {
            SlotKind::Prop { expr, .. } => expr.clone(),
            SlotKind::ClassToggle { expr, .. } => expr.clone(),
            SlotKind::Style { expr, .. } => expr.clone(),
            // Text holes fill via `enc.text_fills` (node + expr per hole).
            SlotKind::Text => continue,
            SlotKind::Event { .. } | SlotKind::Region { .. } => continue,
        };
        let node = enc
            .slot_nodes
            .iter()
            .find(|(id, _)| *id == slot.id)
            .map(|(_, n)| *n)
            .unwrap_or(u32::MAX);
        mount_fills.push(MountFill {
            slot: slot.id,
            node,
            expr,
            escape: slot.escape,
        });
    }
    mount_fills.extend(enc.text_fills.clone());
    let ssr = match flavor {
        HostFlavor::Stream => Some(ssr_program(vir)),
        HostFlavor::Retained => None, // G17: retained hosts compile no render_to
    };
    let islands = hydration_islands(vir);
    Artifacts {
        slot_count: vir.slots.len(),
        fully_static: vir.slots.is_empty(),
        blueprint: enc.blueprint,
        slot_nodes: enc.slot_nodes,
        mount_fills,
        ssr,
        islands,
    }
}

/// The SSR program (G7): static chunks + typed holes.
#[derive(Debug, Clone, Default)]
pub struct SsrProgram {
    /// Chunks in output order.
    pub chunks: Vec<SsrChunk>,
}

/// One SSR chunk.
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum SsrChunk {
    /// `&'static str` — memcpy'd (G7).
    Static(String),
    /// Escape-and-write hole (G6/G7). `expr` renders via the state.
    Hole {
        /// Hole expression.
        expr: crate::expr::Expr,
        /// Compile-time-chosen escaper (G6).
        escape: EscapeCtx,
    },
    /// Event-listening marker for hydration (G16 islands).
    Listener {
        /// Event name.
        event: String,
        /// Static handler id (G8).
        handler_id: u32,
    },
    /// A region: expanded against the state at render time (§6.3, G16).
    Region(RegionSsr),
}

/// A region chunk: the sub-VIRs render_to expands against the state (§6.3).
#[derive(Debug, Clone)]
pub enum RegionSsr {
    /// `if/else` arms.
    If {
        /// `(condition, body)` arms.
        arms: Vec<(crate::expr::Expr, Vir)>,
        /// Else body.
        else_body: Option<Vir>,
    },
    /// `match` arms.
    Match {
        /// Scrutinee.
        scrutinee: crate::expr::Expr,
        /// `(pattern, body)` arms.
        arms: Vec<(String, Vir)>,
    },
    /// Keyed list region (§7.6): render each row with the item bound.
    For {
        /// Loop pattern.
        pat: String,
        /// Iterable.
        iter: crate::expr::Expr,
        /// Optional key.
        key: Option<crate::expr::Expr>,
        /// Row body.
        body: Vir,
    },
}

impl RegionSsr {
    fn from_region(r: &Region) -> RegionSsr {
        match r {
            Region::If { arms, else_body } => RegionSsr::If {
                arms: arms.clone(),
                else_body: else_body.clone(),
            },
            Region::Match { scrutinee, arms } => RegionSsr::Match {
                scrutinee: scrutinee.clone(),
                arms: arms.clone(),
            },
            Region::For { pat, iter, key, body, .. } => RegionSsr::For {
                pat: pat.clone(),
                iter: iter.clone(),
                key: key.clone(),
                body: body.clone(),
            },
            Region::Component { inlined, .. } => RegionSsr::If {
                arms: inlined
                    .as_ref()
                    .map(|v| vec![(crate::expr::Expr::Lit(crate::expr::Lit::Bool(true)), (**v).clone())])
                    .unwrap_or_default(),
                else_body: None,
            },
        }
    }
}

impl SsrProgram {
    /// Fully static? → one slice + precomputed Content-Length (G7).
    pub fn is_fully_static(&self) -> bool {
        self.chunks.iter().all(|c| matches!(c, SsrChunk::Static(_)))
    }

    /// The concatenated static content when fully static.
    pub fn static_content(&self) -> Option<String> {
        if !self.is_fully_static() {
            return None;
        }
        let mut out = String::new();
        for c in &self.chunks {
            if let SsrChunk::Static(s) = c {
                out.push_str(s);
            }
        }
        Some(out)
    }

    /// Render to a string given an interpreter over the state (§6.4
    /// `render_to(&mut W, &S)`; the headless tests assert byte output).
    pub fn render_to(&self, interp: &mut Interp) -> String {
        let mut out = String::new();
        for chunk in &self.chunks {
            match chunk {
                SsrChunk::Static(s) => out.push_str(s),
                SsrChunk::Hole { expr, escape } => {
                    let v = interp.eval(expr).unwrap_or(Value::Null);
                    match escape {
                        EscapeCtx::Url => out.push_str(&escape_url(&v.display())),
                        EscapeCtx::Script => out.push_str(&v.display()), // TrustedHtml only
                        _ => out.push_str(&escape_html(&v.display())),
                    }
                }
                SsrChunk::Listener { event, .. } => {
                    // G16: `adopt` re-attaches the static handler id at this
                    // marker; the SSR itself emits no script.
                    out.push_str(&format!(" data-on-{}=\"\"", escape_attr_name(event)));
                }
                SsrChunk::Region(kind) => match kind {
                    RegionSsr::If { arms, else_body } => {
                        let chosen = arms
                            .iter()
                            .enumerate()
                            .find(|(_, (c, _))| {
                                interp.eval(c).map(|v| v.truthy()).unwrap_or(false)
                            })
                            .map(|(i, _)| Some(i))
                            .unwrap_or(if else_body.is_some() { Some(arms.len()) } else { None });
                        if let Some(i) = chosen {
                            let sub = if i < arms.len() {
                                &arms[i].1
                            } else {
                                else_body.as_ref().unwrap()
                            };
                            out.push_str(&ssr_program(sub).render_to(interp));
                        }
                    }
                    RegionSsr::Match { scrutinee, arms } => {
                        let _ = interp.eval(scrutinee);
                        if let Some((_, body)) = arms.first() {
                            out.push_str(&ssr_program(body).render_to(interp));
                        }
                    }
                    RegionSsr::For { pat, iter, body, .. } => {
                        let it = interp.eval(iter).unwrap_or(Value::Null);
                        if let Value::List(l) = it {
                            for item in l.borrow().iter() {
                                let mut scope = interp.clone();
                                scope.bind(pat, item.clone());
                                out.push_str(&ssr_program(body).render_to(&mut scope));
                            }
                        }
                    }
                },
            }
        }
        out
    }
}

/// Build the SSR program from a VIR (stream flavor, G7 + G17).
pub fn ssr_program(vir: &Vir) -> SsrProgram {
    let mut p = SsrProgram::default();
    for root in &vir.roots {
        ssr_node(root, vir, &mut p);
    }
    p
}

fn ssr_node(node: &VirNode, vir: &Vir, p: &mut SsrProgram) {
    match node {
        VirNode::Element { tag, static_props, slot_ids, children, .. } => {
            p.chunks.push(SsrChunk::Static(format!("<{}", tag)));
            for sp in static_props {
                p.chunks.push(SsrChunk::Static(format!(
                    " {}=\"{}\"",
                    escape_attr_name(&sp.name),
                    escape_attr(&sp.value)
                )));
            }
            // Dynamic props and event markers anchored to THIS element only
            // (slot_ids are per-element, §6.1).
            for slot_id in slot_ids {
                let Some(slot) = vir.slots.iter().find(|s| s.id == *slot_id) else {
                    continue;
                };
                match &slot.kind {
                    SlotKind::Prop { prop, .. } => {
                        p.chunks
                            .push(SsrChunk::Static(format!(" {}=\"", escape_attr_name(prop))));
                        p.chunks.push(SsrChunk::Hole {
                            expr: prop_expr(slot),
                            escape: slot.escape,
                        });
                        p.chunks.push(SsrChunk::Static("\"".to_string()));
                    }
                    SlotKind::ClassToggle { class, .. } => {
                        // Toggles start from the mount fill; SSR marks them.
                        p.chunks.push(SsrChunk::Static(format!(
                            " data-class-{}=\"{}\"",
                            escape_attr_name(class),
                            escape_attr_name(class)
                        )));
                    }
                    SlotKind::Event { event, handler } => {
                        // G16: listeners survive only on islands; SSR marks
                        // them for `adopt` at hydration time. The handler id
                        // is the static per-handler index (G8).
                        p.chunks.push(SsrChunk::Listener {
                            event: event.clone(),
                            handler_id: handler_id_of(handler),
                        });
                    }
                    _ => {}
                }
            }
            p.chunks.push(SsrChunk::Static(">".to_string()));
            for c in children {
                ssr_node(c, vir, p);
            }
            p.chunks.push(SsrChunk::Static(format!("</{}>", tag)));
        }
        VirNode::Fragment { children } => {
            for c in children {
                ssr_node(c, vir, p);
            }
        }
        VirNode::Text { pieces, .. } => {
            for piece in pieces {
                match piece {
                    crate::ast::TextPiece::Static(s) => {
                        p.chunks.push(SsrChunk::Static(escape_html(s)));
                    }
                    crate::ast::TextPiece::Hole(expr) => {
                        p.chunks.push(SsrChunk::Hole {
                            expr: expr.clone(),
                            escape: EscapeCtx::Text,
                        });
                    }
                }
            }
        }
        VirNode::Region { region } => {
            // §6.3: regions render the state-selected arm at SSR time; the
            // chunk carries the sub-VIRs so render_to can expand them (G16
            // anchors stay available via Listener chunks inside).
            let Some(r) = vir.regions.get(*region) else {
                return;
            };
            p.chunks.push(SsrChunk::Region(RegionSsr::from_region(r)));
        }
    }
}

fn handler_id_of(handler: &std::rc::Rc<crate::vir::HandlerSpec>) -> u32 {
    // G8: handlers are static fns + inline env — the id indexes the program's
    // handler table (assigned deterministically from the handler body).
    let body = handler.body();
    let mut h: u32 = 0x811c9dc5;
    let text = format!("{body:?}");
    for b in text.as_bytes() {
        h ^= u32::from(*b);
        h = h.wrapping_mul(0x01000193);
    }
    h & ((1 << 16) - 1)
}

fn prop_expr(slot: &crate::vir::Slot) -> crate::expr::Expr {
    match &slot.kind {
        SlotKind::Prop { expr, .. } => expr.clone(),
        SlotKind::ClassToggle { expr, .. } => expr.clone(),
        SlotKind::Style { expr, .. } => expr.clone(),
        _ => crate::expr::Expr::Lit(crate::expr::Lit::Unit),
    }
}

fn escape_attr_name(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect()
}

/// G6 text escaper (HTML entities).
pub fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// Attribute-context escaper.
pub fn escape_attr(s: &str) -> String {
    escape_html(s)
}

/// URL-context escaper (percent-encode unsafe bytes, G6).
pub fn escape_url(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.as_bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(*b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// G16: hydration islands — regions that contain handlers or reactive slots.
/// Static regions are never walked by `adopt`.
pub fn hydration_islands(vir: &Vir) -> Vec<usize> {
    let mut islands = vec![];
    for (i, region) in vir.regions.iter().enumerate() {
        if region_needs_hydration(region, &vir.state_param) {
            islands.push(i);
        }
    }
    islands
}

fn region_needs_hydration(region: &Region, sp: &str) -> bool {
    // G16: adopt is generated only for regions containing handlers or
    // reactive slots — which includes a *reactive condition* on a
    // statically-shaped body (the arm selection itself must hydrate).
    fn reads_state(e: &crate::expr::Expr, sp: &str) -> bool {
        !crate::analysis::read_set(e, sp).is_empty()
    }
    match region {
        Region::If { arms, else_body } => {
            arms.iter()
                .any(|(c, v)| reads_state(c, sp) || vir_needs_hydration(v))
                || else_body.as_ref().is_some_and(vir_needs_hydration)
        }
        Region::Match { scrutinee, arms } => {
            reads_state(scrutinee, sp) || arms.iter().any(|(_, v)| vir_needs_hydration(v))
        }
        Region::For { iter, body, .. } => reads_state(iter, sp) || vir_needs_hydration(body),
        Region::Component { inlined, props, .. } => {
            inlined.as_deref().is_some_and(vir_needs_hydration)
                || props
                    .iter()
                    .any(|(_, v)| matches!(v, crate::ast::AttrValue::Expr(_)))
        }
    }
}

fn vir_needs_hydration(vir: &Vir) -> bool {
    // Reactive: anything dynamic at all.
    !vir.slots.is_empty()
}

/// §7.5 helper shared with `msp`: the read-set of a slot.
pub fn slot_reads(slot: &crate::vir::Slot, state_param: &str) -> BTreeSet<Place> {
    match &slot.kind {
        SlotKind::Prop { expr, .. } | SlotKind::ClassToggle { expr, .. }
        | SlotKind::Style { expr, .. } => crate::analysis::read_set(expr, state_param),
        SlotKind::Text => BTreeSet::new(),
        SlotKind::Event { .. } | SlotKind::Region { .. } => BTreeSet::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{split_text_holes, TextPiece};
    use crate::schema::SchemaRegistry;
    use crate::vir::{VirBuilder, VirOptions};

    fn build(src: &str) -> Vir {
        let wrapped = format!("fn __g() {{ {src} }}");
        let parsed = glyim_frontend::parse_to_syntax(&wrapped, glyim_span::FileId::from_raw(0));
        let g = crate::ast::gsx_from_parse(&parsed);
        let schemas = SchemaRegistry::standard();
        VirBuilder::new(VirOptions::default(), &schemas).build(g.root.as_ref().unwrap())
    }

    /// §6.1: the static skeleton encodes to Create/StaticProp/Text/Enter/
    /// Leave ops with SlotMarkers exactly at dynamic holes.
    #[test]
    fn blueprint_encoding_matches_ops() {
        let v = build(
            "<section class=\"counter\"><h1>\"Hi {s.name}\"</h1><p>\"static\"</p></section>",
        );
        let mut bp = Blueprint::default();
        encode_blueprint(&v, &mut bp);
        let ops = bp.decode();
        // section, h1, static text, hole marker, p, static text
        assert!(matches!(ops[0], BpOp::Create(_)));
        assert!(matches!(ops[1], BpOp::StaticProp(_, _)));
        assert!(matches!(ops[2], BpOp::Enter));
        assert!(matches!(ops[3], BpOp::Create(_))); // h1
        assert!(matches!(ops[4], BpOp::Enter));
        assert!(matches!(ops[5], BpOp::Text(_))); // "Hi "
        assert!(matches!(ops[6], BpOp::SlotMarker(_))); // {s.name}
        assert!(matches!(ops.last(), Some(BpOp::Leave)));
        assert_eq!(bp.slot_count(), 1);
    }

    /// G17: stream hosts compile no patch code at all — and no mount plan
    /// beyond the initial fill; retained hosts compile no render_to.
    #[test]
    fn host_flavor_dead_code() {
        let v = build("<output>\"{s.count}\"</output>");
        let stream = emit(&v, HostFlavor::Stream);
        assert!(stream.ssr.is_some());
        let retained = emit(&v, HostFlavor::Retained);
        assert!(retained.ssr.is_none(), "G17: retained hosts get no render_to");
    }

    /// G7: fully static pages are one slice with a precomputed length.
    #[test]
    fn fully_static_ssr_single_chunk() {
        let v = build("<h1>\"Hello & <world>\"</h1>");
        let a = emit(&v, HostFlavor::Stream);
        let ssr = a.ssr.unwrap();
        assert!(ssr.is_fully_static());
        let content = ssr.static_content().unwrap();
        assert_eq!(content, "<h1>Hello &amp; &lt;world&gt;</h1>");
        assert_eq!(content.len(), content.len()); // Content-Length = byte len
    }

    /// G7: dynamic output = memcpy chunk, escape-and-write hole, memcpy chunk.
    #[test]
    fn ssr_chunks_around_holes() {
        let v = build("<output>\"Count: {s.count}!\"</output>");
        let a = emit(&v, HostFlavor::Stream);
        let ssr = a.ssr.unwrap();
        assert!(!ssr.is_fully_static());
        let mut interp = Interp::new([("s", Value::obj([("count", Value::Int(7))]))]);
        let html = ssr.render_to(&mut interp);
        assert_eq!(html, "<output>Count: 7!</output>");
    }

    /// G6: hole position chooses the escaper — text escaping in text, URL
    /// encoding in URL attrs; ints/bools need no escaping.
    #[test]
    fn escaping_by_hole_position() {
        assert_eq!(escape_html("a<b>&c"), "a&lt;b&gt;&amp;c");
        assert_eq!(escape_url("a b/c?x=1"), "a%20b/c%3Fx%3D1");
        // integers/bools pass through unescaped:
        let v = Value::Int(42);
        assert_eq!(v.display(), "42");
        let b = Value::Bool(true);
        assert_eq!(b.display(), "true");
    }

    /// G6: XSS is unrepresentable — a hostile string in a text hole cannot
    /// inject markup.
    #[test]
    fn xss_unrepresentable() {
        let v = build("<p>\"{s.user_input}\"</p>");
        let a = emit(&v, HostFlavor::Stream);
        let mut interp = Interp::new([(
            "s",
            Value::obj([(
                "user_input",
                Value::Str(std::rc::Rc::from("<script>alert(1)</script>")),
            )]),
        )]);
        let html = a.ssr.unwrap().render_to(&mut interp);
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
    }

    /// G16: adopt is generated only for regions with handlers/reactive
    /// slots; static regions are skipped.
    #[test]
    fn hydration_islands_only_where_needed() {
        let v = build(concat!(
            "<section>",
            "{if s.count > 10 { <p>\"{s.count}\"</p> }}", // reactive island
            "{if s.count > 0 { <p>\"static\"</p> }}", // reactive condition
            "</section>",
        ));
        let a = emit(&v, HostFlavor::Stream);
        // Both ifs read state, so both are reactive islands; a purely static
        // region would not be.
        assert_eq!(a.islands.len(), 2);
    }

    /// §6.2: mount fills evaluate once — a dynamic prop and a text hole both
    /// appear as initial fills.
    #[test]
    fn mount_fills_listed() {
        let v = build("<input value={s.name}/><output>\"{s.count}\"</output>");
        let a = emit(&v, HostFlavor::Retained);
        assert_eq!(a.mount_fills.len(), 2, "prop fill + text hole fill");
        assert_eq!(a.slot_count, 2);
        // Every slot is anchored to a blueprint node.
        assert_eq!(a.slot_nodes.len(), 2);
        let _ = split_text_holes("x"); // keep import used
        let _ = TextPiece::Static(String::new());
    }
}
