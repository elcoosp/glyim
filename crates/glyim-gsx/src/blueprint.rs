//! Blueprint: the static skeleton encoded as a flat `u32` op array + string
//! table (design doc §6.1, finding F10).
//!
//! F10: `MirConst` has no aggregate constants (`Int, Uint, FloatBits, Bool,
//! Char, String, Unit, Fn, ConstRef, StaticRef`), so the Blueprint must be
//! storable in a `static` using **only integer arrays** — hence a flat `u32`
//! op array plus a string table, not a tree of rich constants.
//!
//! Ops (§6.1): `Create(tag) / StaticProp(id, strid) / Text(strid) / Enter /
//! Leave / SlotMarker(i)`.
//!
//! Word layout: op tag in the top 3 bits, payload in the low 29 bits.
//! `StaticProp` packs `prop_id:14 | string_id:15` into the payload.
//!
//! `mount` = `host.instantiate(&BLUEPRINT)` — one bulk call (G1): DOM maps to
//! template clone, native to a prebuilt retained subtree. A fully static
//! subtree has no slots, no patch code, and nothing is ever visited again
//! (§6.2, G2).

use std::fmt::Write as _;

/// One blueprint op, decoded form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BpOp {
    /// `Create(tag_id)` — create the host node for a dialect element.
    Create(u32),
    /// `StaticProp(prop_id, string_id)` — a constant attribute.
    StaticProp(u32, u32),
    /// `Text(string_id)` — a static text child.
    Text(u32),
    /// `Enter` — descend into the element's children.
    Enter,
    /// `Leave` — ascend.
    Leave,
    /// `SlotMarker(slot_index)` — the mount point of dynamic slot `i`
    /// (filled once at mount; patched later by MSPs).
    SlotMarker(u32),
}

const TAG_OP_CREATE: u32 = 0;
const TAG_OP_STATIC_PROP: u32 = 1;
const TAG_OP_TEXT: u32 = 2;
const TAG_OP_ENTER: u32 = 3;
const TAG_OP_LEAVE: u32 = 4;
const TAG_OP_SLOT_MARKER: u32 = 5;
const OP_BITS: u32 = 3;
const PAYLOAD_MASK: u32 = (1 << (32 - OP_BITS)) - 1;

/// The static skeleton of one view: ops + string table (§6.1).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Blueprint {
    /// Flat op array (rodata-shaped; storable as `static OPS: [u32; N]`).
    pub ops: Vec<u32>,
    /// String table (`static STRS: [&str; N]`).
    pub strings: Vec<String>,
}

impl Blueprint {
    /// Encode one op into the array.
    pub fn push_op(&mut self, op: BpOp) {
        let word = match op {
            BpOp::Create(tag) => encode(TAG_OP_CREATE, tag),
            BpOp::StaticProp(prop, s) => {
                debug_assert!(
                    prop < (1 << 14) && s < (1 << 15),
                    "StaticProp field overflow"
                );
                encode(TAG_OP_STATIC_PROP, (prop << 15) | s)
            }
            BpOp::Text(s) => encode(TAG_OP_TEXT, s),
            BpOp::Enter => encode(TAG_OP_ENTER, 0),
            BpOp::Leave => encode(TAG_OP_LEAVE, 0),
            BpOp::SlotMarker(i) => encode(TAG_OP_SLOT_MARKER, i),
        };
        self.ops.push(word);
    }

    /// Intern a string; returns its id (deduplicated).
    pub fn intern(&mut self, s: &str) -> u32 {
        if let Some(i) = self.strings.iter().position(|x| x == s) {
            return i as u32;
        }
        self.strings.push(s.to_string());
        (self.strings.len() - 1) as u32
    }

    /// Decode all ops.
    pub fn decode(&self) -> Vec<BpOp> {
        self.ops.iter().map(|&w| decode_word(w)).collect()
    }

    /// True when the skeleton has no dynamic holes at all (§6.2: fully
    /// static subtree → nothing is ever visited again).
    pub fn is_fully_static(&self) -> bool {
        !self
            .ops
            .iter()
            .any(|&w| matches!(decode_word(w), BpOp::SlotMarker(_)))
    }

    /// Count of slot markers.
    pub fn slot_count(&self) -> usize {
        self.ops
            .iter()
            .filter(|&&w| matches!(decode_word(w), BpOp::SlotMarker(_)))
            .count()
    }

    /// Debug print: `Create(0) Text("…") Enter …` with strings inlined.
    pub fn print(&self) -> String {
        let mut out = String::new();
        for op in self.decode() {
            match op {
                BpOp::Create(t) => {
                    let _ = write!(out, "Create({t}) ");
                }
                BpOp::StaticProp(p, s) => {
                    let tag = self
                        .strings
                        .get(s as usize)
                        .map(|x| x.as_str())
                        .unwrap_or("?");
                    let _ = write!(out, "StaticProp({p}, {tag:?}) ");
                }
                BpOp::Text(s) => {
                    let txt = self
                        .strings
                        .get(s as usize)
                        .map(|x| x.as_str())
                        .unwrap_or("?");
                    let _ = write!(out, "Text({txt:?}) ");
                }
                BpOp::Enter => out.push_str("Enter "),
                BpOp::Leave => out.push_str("Leave "),
                BpOp::SlotMarker(i) => {
                    let _ = write!(out, "SlotMarker({i}) ");
                }
            }
        }
        out.trim_end().to_string()
    }
}

fn encode(tag: u32, payload: u32) -> u32 {
    debug_assert!(payload <= PAYLOAD_MASK, "blueprint payload overflow");
    (tag << (32 - OP_BITS)) | (payload & PAYLOAD_MASK)
}

fn decode_word(w: u32) -> BpOp {
    let tag = w >> (32 - OP_BITS);
    let rest = w & PAYLOAD_MASK;
    match tag {
        TAG_OP_CREATE => BpOp::Create(rest),
        TAG_OP_STATIC_PROP => {
            let prop = rest >> 15;
            let s = rest & ((1 << 15) - 1);
            BpOp::StaticProp(prop, s)
        }
        TAG_OP_TEXT => BpOp::Text(rest),
        TAG_OP_ENTER => BpOp::Enter,
        TAG_OP_LEAVE => BpOp::Leave,
        TAG_OP_SLOT_MARKER => BpOp::SlotMarker(rest),
        _ => BpOp::Leave, // unreachable for well-formed arrays
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §6.1: ops are exactly `Create / StaticProp / Text / Enter / Leave /
    /// SlotMarker`, encoded as a flat u32 array.
    #[test]
    fn op_roundtrip() {
        let mut bp = Blueprint::default();
        let s = bp.intern("hello");
        let s2 = bp.intern("hello"); // deduplicated
        assert_eq!(s, s2);
        bp.push_op(BpOp::Create(7));
        bp.push_op(BpOp::StaticProp(3, s));
        bp.push_op(BpOp::Text(s));
        bp.push_op(BpOp::Enter);
        bp.push_op(BpOp::SlotMarker(4));
        bp.push_op(BpOp::Leave);
        assert_eq!(
            bp.decode(),
            vec![
                BpOp::Create(7),
                BpOp::StaticProp(3, s),
                BpOp::Text(s),
                BpOp::Enter,
                BpOp::SlotMarker(4),
                BpOp::Leave,
            ]
        );
    }

    /// F10: the array is plain `u32`s — integer-array static, no rich
    /// aggregates.
    #[test]
    fn encoding_is_u32_only() {
        let mut bp = Blueprint::default();
        let s = bp.intern("Count");
        bp.push_op(BpOp::Create(1));
        bp.push_op(BpOp::Text(s));
        assert_eq!(bp.ops.len(), 2);
        assert_eq!(std::mem::size_of_val(bp.ops.as_slice()), 8);
    }

    /// §6.2: a fully static subtree has no `SlotMarker` — nothing is ever
    /// visited again.
    #[test]
    fn fully_static_detection() {
        let mut static_bp = Blueprint::default();
        let s = static_bp.intern("ok");
        static_bp.push_op(BpOp::Create(0));
        static_bp.push_op(BpOp::Text(s));
        assert!(static_bp.is_fully_static());
        assert_eq!(static_bp.slot_count(), 0);

        let mut dynamic_bp = Blueprint::default();
        dynamic_bp.push_op(BpOp::SlotMarker(0));
        assert!(!dynamic_bp.is_fully_static());
        assert_eq!(dynamic_bp.slot_count(), 1);
    }

    /// §8.1 cost model: "memory per static node: 0 bytes beyond the host node
    /// (skeleton lives in rodata)" — the skeleton is the op array + strings,
    /// nothing else.
    #[test]
    fn skeleton_size_is_ops_plus_strings() {
        let mut bp = Blueprint::default();
        let s = bp.intern("btn");
        bp.push_op(BpOp::Create(5));
        bp.push_op(BpOp::StaticProp(2, s));
        // The op array is the rodata: exactly 4 bytes per op, nothing else.
        assert_eq!(std::mem::size_of_val(bp.ops.as_slice()), bp.ops.len() * 4);
        // The string table is the only other allocation; no per-node objects.
        assert_eq!(bp.strings.len(), 1);
    }

    /// Readable disassembly for snapshot-style tests.
    #[test]
    fn print_is_readable() {
        let mut bp = Blueprint::default();
        let s = bp.intern("Hello");
        bp.push_op(BpOp::Create(9));
        bp.push_op(BpOp::Text(s));
        assert_eq!(bp.print(), "Create(9) Text(\"Hello\")");
    }

    /// Slot indices survive the u32 packing (29-bit payload).
    #[test]
    fn large_slot_indices_survive() {
        let mut bp = Blueprint::default();
        let idx: u32 = 1 << 24; // 16M slots — well within 29 bits
        bp.push_op(BpOp::SlotMarker(idx));
        assert_eq!(bp.decode(), vec![BpOp::SlotMarker(idx)]);
    }

    /// `StaticProp` field packing roundtrips both ids.
    #[test]
    fn static_prop_packing() {
        let mut bp = Blueprint::default();
        let s = bp.intern("x");
        bp.push_op(BpOp::StaticProp(0x3ff, s));
        assert_eq!(bp.decode(), vec![BpOp::StaticProp(0x3ff, s)]);
    }
}
