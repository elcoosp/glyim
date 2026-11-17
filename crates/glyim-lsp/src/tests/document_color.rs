//! Policy-level tests for the document-color engine (LSP-COLOR).
//!
//! Mirrors `document_link.rs` and `inline_value.rs`: the *policy* (which
//! recorded string literals are color sites, and what channels each
//! spells) is pinned through an in-memory [`MockColorSource`] implementing
//! [`crate::document_color::ColorLexSource`], decoupled from any database,
//! parse tree, or lock. The stored-tree production path and both full wire
//! requests are covered end-to-end in `document_color_tests.rs`.

use crate::document_color::{
    ColorLexSource, ColorLit, compute_color_presentations, compute_document_colors,
    spell_hex_payload, wire_color_information,
};

/// In-memory literal store: the policy-side test double. The policy owns
/// every quote-strip, charset, and digit-width decision on top.
struct MockColorSource<'a> {
    literals: Vec<&'a str>,
}

impl ColorLexSource for MockColorSource<'_> {
    fn string_literals(&self) -> Vec<ColorLit> {
        let mut out = Vec::new();
        let mut lo = 0;
        for text in &self.literals {
            out.push(ColorLit {
                lo,
                hi: lo + text.len(),
                text: (*text).to_string(),
            });
            lo += text.len();
        }
        out
    }
}

fn channels_of(literal: &str) -> Option<(f32, f32, f32, f32)> {
    let mock = MockColorSource {
        literals: vec![literal],
    };
    let sites = compute_document_colors(&mock);
    let site = sites.first()?;
    Some((site.red, site.green, site.blue, site.alpha))
}

/// The core six-digit literal answers its channels as fractions of 255.
#[test]
fn a_six_digit_literal_answers_its_byte_fractions() {
    let (r, g, b, a) = channels_of("\"#ff00aa\"").expect("`#ff00aa` is a color site");
    assert_eq!(r, 255.0 / 255.0);
    assert_eq!(g, 0.0);
    assert_eq!(b, 170.0 / 255.0);
    assert_eq!(a, 1.0, "a missing digit spells full opacity");
}

/// The three-digit shorthand expands each nibble over 15.
#[test]
fn a_three_digit_literal_expands_its_nibbles() {
    let (r, g, b, a) = channels_of("\"#f0a\"").expect("`#f0a` is a color site");
    assert_eq!(r, 15.0 / 15.0);
    assert_eq!(g, 0.0);
    assert_eq!(b, 10.0 / 15.0);
    assert_eq!(a, 1.0);
}

/// The eight-digit form carries its alpha; the four-digit shorthand too.
#[test]
fn digit_forms_with_alpha_carry_it() {
    let (_, _, _, a8) = channels_of("\"#ffffff80\"").expect("`#ffffff80` is a color site");
    assert_eq!(a8, 128.0 / 255.0);
    let (_, _, _, a4) = channels_of("\"#fff8\"").expect("`#fff8` is a color site");
    assert_eq!(a4, 8.0 / 15.0);
}

/// Hex digits answer case-insensitively — the CSS widths are case-blind
/// and so is the policy.
#[test]
fn the_payload_is_case_insensitive() {
    let upper = channels_of("\"#FF00AA\"").expect("upper case is a color site");
    let lower = channels_of("\"#ff00aa\"").expect("lower case is a color site");
    assert_eq!(upper, lower);
}

/// Prose around a color, wrong digit counts, and non-hex payloads answer
/// nothing: the server would have to guess which slice of the string the
/// parser owns, and a guessed site is a picker edit over user text.
#[test]
fn non_color_strings_answer_nothing() {
    for literal in [
        "\"hello world\"",
        "\"bg: #ff00aa\"",
        "\"#\"",
        "\"#ff\"",
        "\"#ff00a\"",
        "\"#ff00aa0\"",
        "\"#ff00aa00ff\"",
        "\"#zz00aa\"",
        "\"\"",
    ] {
        assert!(
            channels_of(literal).is_none(),
            "`{literal}` must not be a color site"
        );
    }
}

/// Sites answer in source order and are *not* deduplicated: two identical
/// literals are two distinct sites with two swatches.
#[test]
fn sites_answer_in_source_order_and_are_not_deduplicated() {
    let mock = MockColorSource {
        literals: vec!["\"#000000\"", "fn main() -> i32 { 0 }", "\"#ffffff\""],
    };
    let sites = compute_document_colors(&mock);
    assert_eq!(sites.len(), 2, "one site per color literal");
    assert!(sites[0].red < sites[1].red, "source order is kept");
}

/// The payload extent is the literal without its quotes — sound because a
/// matching body is escape-free by construction (an escaped body can
/// never survive the hex charset).
#[test]
fn the_site_extent_is_the_payload_without_quotes() {
    let mock = MockColorSource {
        literals: vec!["\"#ff00aa\""],
    };
    let site = &compute_document_colors(&mock)[0];
    assert_eq!(site.payload_lo, 1, "one byte past the opening quote");
    assert_eq!(site.payload_hi, 8, "one byte before the closing quote");
}

/// The wire mapping the entry point performs: the payload extent maps
/// through the injected span→position mapping into the `ColorInformation`
/// range, and the channels carry over unchanged. An unmappable site
/// answers `None` — never a guessed position.
#[test]
fn the_site_maps_to_the_wire_contract() {
    let site = recorded_site_for_wire();
    let wire = wire_color_information(&site, &|lo, hi| Some(((0, lo), (0, hi))));
    let Some(info) = wire else {
        panic!("a mappable site maps to ColorInformation");
    };
    assert_eq!(info.range.start.character, 1);
    assert_eq!(info.range.end.character, 8);
    assert_eq!(info.color.red, 1.0);
    assert_eq!(info.color.alpha, 1.0);

    let unmappable = wire_color_information(&site, &|_, _| None);
    assert!(unmappable.is_none(), "an unmappable site answers None");
}

/// A recorded site for the wire-mapping test: `"#ff00aa"` payload at
/// bytes 1..8.
fn recorded_site_for_wire() -> crate::document_color::RecordedColor {
    crate::document_color::RecordedColor {
        payload_lo: 1,
        payload_hi: 8,
        red: 1.0,
        green: 0.0,
        blue: 170.0 / 255.0,
        alpha: 1.0,
    }
}

/// The presentation policy answers exactly one presentation — glyim
/// carries one color notation, the hex literal — labeled with the
/// canonical lowercase spelling.
#[test]
fn the_presentation_policy_answers_one_hex_spelling() {
    let facts = compute_color_presentations(1.0, 0.0, 170.0 / 255.0, 1.0);
    assert_eq!(facts.len(), 1, "one notation, one presentation");
    assert_eq!(facts[0].label, "#ff00aa");
}

/// The spelling quantizes to 8-bit channels and picks the width by
/// opacity: `#rrggbb` at full opacity, `#rrggbbaa` otherwise.
#[test]
fn the_spelling_picks_its_width_by_opacity() {
    assert_eq!(spell_hex_payload(1.0, 0.0, 170.0 / 255.0, 1.0), "#ff00aa");
    assert_eq!(
        spell_hex_payload(1.0, 0.0, 170.0 / 255.0, 128.0 / 255.0),
        "#ff00aa80"
    );
    // Out-of-range components clamp into [0-1] — the hex notation has no
    // spelling for them, and clamping is the honest normalization.
    assert_eq!(spell_hex_payload(2.0, -1.0, 0.5, 1.0), "#ff0080");
}

/// Spelling round-trips through the parser: the channels a literal spells
/// re-spell to the same payload.
#[test]
fn spelling_round_trips_through_the_parser() {
    let mock = MockColorSource {
        literals: vec!["\"#3fa7d6\""],
    };
    let site = &compute_document_colors(&mock)[0];
    let spelled = spell_hex_payload(site.red, site.green, site.blue, site.alpha);
    let re_parsed = channels_of(&format!("\"{spelled}\"")).expect("the spelling is a color site");
    assert_eq!(
        (site.red, site.green, site.blue, site.alpha),
        re_parsed,
        "the spelling round-trips"
    );
}
