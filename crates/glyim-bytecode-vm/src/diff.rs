//! Module-image diff seam: what changed between two decoded artifacts?
//!
//! The image seam ([`crate::image`]) ships a module across a process
//! boundary and the disassembler ([`crate::disasm`]) lists what one
//! artifact *carries* — but nothing answered the *change* question. When a
//! rebuild after an emitter tweak produces bytes that behave differently,
//! the two images sit side by side and the only way to see why is a manual
//! byte-by-byte eyeball (or `cmp`, which reports the first offset and
//! stops). This module is the deterministic, structural counterpart: it
//! walks two [`Module`]s index by index — functions (code, local counts,
//! block tables), the string pool, the entry point — and renders the
//! differences in the disassembler's diagnostic-text style.
//!
//! # Contract
//!
//! The diff is **deterministic** (the same pair of modules renders
//! byte-identical text — the test suite pins a golden), **total** (any two
//! modules diff, including two functions where one code buffer is a
//! prefix of the other — the `first-diff` pc is the shorter buffer's end),
//! and **honest about equality** (identical modules render the explicit
//! `no differences` line; unchanged functions, unchanged pool entries,
//! and an unchanged entry are simply not listed — the header's counts say
//! what was compared, never silently omitted differences).
//!
//! The seam is a pure function over two decoded modules: it composes with
//! the image round trip (encode → decode → diff the loaded pair), and a
//! no-op rebuild diffs to `no differences` — the regression gate for any
//! emitter change that should not alter shipped bytes.

use std::fmt::Write as _;

use crate::{Function, Module};
use tracing::debug;

/// Render the structural differences between two modules into a
/// deterministic listing: a header line (image format version, the
/// function/string counts on each side, the entry index on each side),
/// then — when anything differs — the `functions:` section (one `-`/`+`/`~`
/// line per removed/added/changed function, in index order), the
/// `strings:` section (one `-`/`+`/`~` line per removed/added/changed pool
/// entry, in index order), and an explicit `entry:` line when the entry
/// index moved. Two identical modules render header + `no differences`.
///
/// Deterministic (the same pair renders byte-identical text) and total
/// (any two modules diff — the union of the two function indexes and the
/// two pool lengths is walked, so one-sided additions and removals are
/// displayed, never dropped).
#[must_use]
pub fn render_module_diff(old: &Module, new: &Module) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "module-diff glyim.module-image/{} functions {}->{} strings {}->{} entry {}->{}",
        crate::IMAGE_VERSION,
        old.functions.len(),
        new.functions.len(),
        old.strings.len(),
        new.strings.len(),
        old.entry,
        new.entry,
    );
    // Functions: the union of the two index spaces, ascending — removals,
    // additions, and in-place changes (code first, then metadata).
    let fn_indices = old.functions.len().max(new.functions.len());
    let mut fn_lines: Vec<String> = Vec::new();
    for i in 0..fn_indices {
        match (old.functions.get(i), new.functions.get(i)) {
            (Some(old_f), None) => {
                let _ = writeln!(
                    fn_lines_marker(&mut fn_lines),
                    "- fn {i} (removed, locals {}, args {}, bytes {})",
                    old_f.n_locals,
                    old_f.arg_count,
                    old_f.code.len()
                );
            }
            (None, Some(new_f)) => {
                let _ = writeln!(
                    fn_lines_marker(&mut fn_lines),
                    "+ fn {i} (added, locals {}, args {}, bytes {})",
                    new_f.n_locals,
                    new_f.arg_count,
                    new_f.code.len()
                );
            }
            (Some(old_f), Some(new_f)) => {
                if old_f == new_f {
                    continue;
                }
                let (locals, args, blocks) = metadata_pair(old_f, new_f);
                if old_f.code == new_f.code {
                    let _ = writeln!(
                        fn_lines_marker(&mut fn_lines),
                        "~ fn {i} metadata (locals {locals}, args {args}, blocks {blocks})"
                    );
                } else {
                    let _ = writeln!(
                        fn_lines_marker(&mut fn_lines),
                        "~ fn {i} first-diff pc {} bytes {}->{} (locals {locals}, args {args}, blocks {blocks})",
                        first_diff_pc(&old_f.code, &new_f.code),
                        old_f.code.len(),
                        new_f.code.len(),
                    );
                }
            }
            (None, None) => unreachable!("the union walk never visits an empty pair"),
        }
    }
    if !fn_lines.is_empty() {
        let _ = writeln!(out, "functions:");
        for line in &fn_lines {
            out.push_str(line);
        }
    }
    // Strings: the union of the two pool lengths, ascending.
    let str_indices = old.strings.len().max(new.strings.len());
    let mut str_lines: Vec<String> = Vec::new();
    for i in 0..str_indices {
        match (old.strings.get(i), new.strings.get(i)) {
            (Some(old_s), None) => {
                let _ = writeln!(str_lines_marker(&mut str_lines), "- pool[{i}] {old_s:?}");
            }
            (None, Some(new_s)) => {
                let _ = writeln!(str_lines_marker(&mut str_lines), "+ pool[{i}] {new_s:?}");
            }
            (Some(old_s), Some(new_s)) if old_s != new_s => {
                let _ = writeln!(
                    str_lines_marker(&mut str_lines),
                    "~ pool[{i}] {old_s:?} -> {new_s:?}"
                );
            }
            _ => {}
        }
    }
    if !str_lines.is_empty() {
        let _ = writeln!(out, "strings:");
        for line in &str_lines {
            out.push_str(line);
        }
    }
    if old.entry != new.entry {
        let _ = writeln!(out, "entry: {} -> {}", old.entry, new.entry);
    }
    if fn_lines.is_empty() && str_lines.is_empty() && old.entry == new.entry {
        let _ = writeln!(out, "no differences");
    }
    debug!(
        old_functions = old.functions.len(),
        new_functions = new.functions.len(),
        "module diff rendered"
    );
    out
}

/// Push an empty line onto the accumulator and return it (a `fmt::Write`
/// target for the per-index `writeln!`s — the lines render in index order
/// and are only appended to the output when the section is non-empty).
fn fn_lines_marker(lines: &mut Vec<String>) -> &mut String {
    lines.push(String::new());
    lines
        .last_mut()
        .expect("just pushed, so the list is non-empty")
}

/// The same accumulator for the strings section (kept as a named twin so
/// the two sections read symmetrically at the call sites).
fn str_lines_marker(lines: &mut Vec<String>) -> &mut String {
    fn_lines_marker(lines)
}

/// The `(locals, args, blocks)` metadata pair of two functions, rendered
/// as `a->b` fields for the `~` lines.
fn metadata_pair(old_f: &Function, new_f: &Function) -> (String, String, String) {
    (
        format!("{}->{}", old_f.n_locals, new_f.n_locals),
        format!("{}->{}", old_f.arg_count, new_f.arg_count),
        format!(
            "{}->{}",
            old_f.block_offsets.len(),
            new_f.block_offsets.len()
        ),
    )
}

/// The first byte offset where two code buffers differ; when one buffer is
/// a prefix of the other, the shorter buffer's end (the change is the
/// missing tail).
fn first_diff_pc(old_code: &[u8], new_code: &[u8]) -> usize {
    let common = old_code.len().min(new_code.len());
    for (i, (a, b)) in old_code[..common]
        .iter()
        .zip(new_code[..common].iter())
        .enumerate()
    {
        if a != b {
            return i;
        }
    }
    common
}

#[cfg(test)]
mod diff_tests {
    use crate::diff::render_module_diff;
    use crate::{Function, Module, Opcode, decode_module, encode_module};

    fn i64le(v: i64) -> [u8; 8] {
        v.to_le_bytes()
    }
    fn u32le(v: u32) -> [u8; 4] {
        v.to_le_bytes()
    }
    fn op(o: Opcode) -> u8 {
        o as u8
    }

    /// helper(n) = n * 2 + main returning helper(21): the golden module the
    /// other seams' tests use, as the diff baseline.
    fn golden_module() -> Module {
        let mut helper = Vec::new();
        helper.push(op(Opcode::LoadConst));
        helper.extend_from_slice(&i64le(2));
        helper.push(op(Opcode::LoadLocal));
        helper.extend_from_slice(&u32le(1));
        helper.push(op(Opcode::Mul));
        helper.push(op(Opcode::Return));
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst));
        main.extend_from_slice(&i64le(0));
        main.push(op(Opcode::LoadConst));
        main.extend_from_slice(&i64le(21));
        main.push(op(Opcode::Call));
        main.extend_from_slice(&u32le(1));
        main.extend_from_slice(&u32le(0));
        main.extend_from_slice(&u32le(31));
        main.push(op(Opcode::LoadLocal));
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::Return));
        Module::with_strings(
            vec![Function::new(helper, 2, 1), Function::new(main, 1, 0)],
            vec!["abc".to_string()],
            1,
        )
    }

    /// Two identical modules render the explicit no-differences line, and
    /// the render is deterministic.
    #[test]
    fn identical_modules_render_no_differences() {
        let a = golden_module();
        let b = golden_module();
        let rendered = render_module_diff(&a, &b);
        assert_eq!(
            rendered,
            "module-diff glyim.module-image/2 functions 2->2 strings 1->1 entry 1->1\nno differences\n"
        );
        assert_eq!(rendered, render_module_diff(&a, &b));
    }

    /// A constant change inside fn 0 is a `~` line with the payload's
    /// first-diff pc (the constant byte sits one byte past the opcode) and
    /// the unchanged metadata pair.
    #[test]
    fn code_change_pins_first_diff_pc() {
        let old = golden_module();
        let mut new = golden_module();
        new.functions[0].code[1] = 3; // helper's `2` -> `3` (helper doubles)
        let rendered = render_module_diff(&old, &new);
        assert_eq!(
            rendered,
            "\
module-diff glyim.module-image/2 functions 2->2 strings 1->1 entry 1->1
functions:
~ fn 0 first-diff pc 1 bytes 16->16 (locals 2->2, args 1->1, blocks 0->0)
"
        );
    }

    /// Both sides carry a function at the same index but with different
    /// bodies: the union walk renders a single `~` change line (a same-index
    /// divergence is a change, never a removal+addition pair).
    #[test]
    fn same_index_divergence_is_a_change() {
        let mut old = golden_module();
        let mut new = golden_module();
        old.functions
            .push(Function::new(vec![op(Opcode::Return)], 0, 0));
        new.functions
            .push(Function::new(vec![op(Opcode::Trap)], 1, 0));
        let rendered = render_module_diff(&old, &new);
        assert!(
            rendered.contains("~ fn 2 first-diff pc 0 bytes 1->1"),
            "the same-index divergence is one change line in {rendered}"
        );
        assert!(
            !rendered.contains("- fn 2") && !rendered.contains("+ fn 2"),
            "never a removal/addition pair in {rendered}"
        );
    }

    /// A truly one-sided pair: old has three functions, new has two — the
    /// third is a `- fn 2` removal.
    #[test]
    fn removed_function_is_a_minus_line() {
        let mut old = golden_module();
        let new = golden_module();
        old.functions
            .push(Function::new(vec![op(Opcode::Return)], 0, 0));
        let rendered = render_module_diff(&old, &new);
        assert_eq!(
            rendered,
            "\
module-diff glyim.module-image/2 functions 3->2 strings 1->1 entry 1->1
functions:
- fn 2 (removed, locals 0, args 0, bytes 1)
"
        );
    }

    /// An added function in the same shape renders `+ fn 2`.
    #[test]
    fn added_function_is_a_plus_line() {
        let old = golden_module();
        let mut new = golden_module();
        new.functions
            .push(Function::new(vec![op(Opcode::Trap)], 1, 0));
        let rendered = render_module_diff(&old, &new);
        assert_eq!(
            rendered,
            "\
module-diff glyim.module-image/2 functions 2->3 strings 1->1 entry 1->1
functions:
+ fn 2 (added, locals 1, args 0, bytes 1)
"
        );
    }

    /// Identical code with different metadata renders the `metadata`
    /// variant (no first-diff pc — the bytes agree).
    #[test]
    fn metadata_change_renders_without_pc() {
        let mut old = golden_module();
        let new = golden_module();
        old.functions[1].n_locals = 3;
        old.functions[1].block_offsets = vec![0, 9, 18];
        let rendered = render_module_diff(&old, &new);
        assert_eq!(
            rendered,
            "\
module-diff glyim.module-image/2 functions 2->2 strings 1->1 entry 1->1
functions:
~ fn 1 metadata (locals 3->1, args 0->0, blocks 3->0)
"
        );
    }

    /// One code buffer being a prefix of the other pins the first-diff pc
    /// at the shorter buffer's end (the change is the missing tail).
    #[test]
    fn prefix_code_diffs_at_the_shorter_end() {
        let old = golden_module();
        let mut new = golden_module();
        new.functions[0].code.truncate(15); // drop the final Return byte
        let rendered = render_module_diff(&old, &new);
        assert!(
            rendered.contains("~ fn 0 first-diff pc 15 bytes 16->15"),
            "the tail loss diffs at the shorter end in {rendered}"
        );
    }

    /// Pool entries diff index-wise: a change renders `~`, an added entry
    /// `+`, a removed entry `-`, and the unchanged pool renders nothing.
    #[test]
    fn string_pool_changes_render() {
        let old = golden_module();
        let mut new = golden_module();
        new.strings[0] = "abd".to_string();
        new.strings.push("xyz".to_string());
        let rendered = render_module_diff(&old, &new);
        assert!(
            rendered.contains("~ pool[0] \"abc\" -> \"abd\""),
            "the changed entry in {rendered}"
        );
        assert!(
            rendered.contains("+ pool[1] \"xyz\""),
            "the added entry in {rendered}"
        );
        assert!(
            rendered.contains("strings 1->2"),
            "the count pair in {rendered}"
        );
    }

    /// An entry-index move renders its own line (the header count pair is
    /// not enough — the move is a semantic fact).
    #[test]
    fn entry_move_renders_explicitly() {
        let mut new = golden_module();
        new.entry = 0;
        let rendered = render_module_diff(&golden_module(), &new);
        assert!(
            rendered.contains("entry: 1 -> 0"),
            "the move line in {rendered}"
        );
    }

    /// The image round trip composes with the diff: encode both sides,
    /// decode them back, and the loaded pair still reports no differences —
    /// while a real byte change survives the trip.
    #[test]
    fn diff_composes_with_module_image() {
        let a = golden_module();
        let b = golden_module();
        let loaded_a = decode_module(&encode_module(&a)).expect("decode a");
        let loaded_b = decode_module(&encode_module(&b)).expect("decode b");
        let rendered = render_module_diff(&loaded_a, &loaded_b);
        assert!(rendered.contains("no differences"), "in {rendered}");

        let mut c = golden_module();
        c.strings[0] = "changed".to_string();
        let loaded_c = decode_module(&encode_module(&c)).expect("decode c");
        let rendered = render_module_diff(&loaded_a, &loaded_c);
        assert!(
            rendered.contains("~ pool[0] \"abc\" -> \"changed\""),
            "the pool change survives the round trip in {rendered}"
        );
    }

    /// A diff of a module against itself (the same value) is the
    /// no-differences case — the seam's regression gate for a rebuild that
    /// should not alter bytes.
    #[test]
    fn self_diff_is_no_differences() {
        let module = golden_module();
        let rendered = render_module_diff(&module, &module);
        assert!(rendered.contains("no differences"), "in {rendered}");
    }
}
