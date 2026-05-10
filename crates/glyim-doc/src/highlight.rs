use syntect::parsing::{SyntaxSet, SyntaxSetBuilder};
use syntect::highlighting::ThemeSet;
use syntect::html::highlighted_html_for_string;
use std::sync::LazyLock;

static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(|| {
    let mut builder = SyntaxSetBuilder::new();
    let grammar_json = include_str!("syntaxes/Glyim.tmLanguage.json");
    let grammar = syntect::parsing::SyntaxDefinition::load_from_str(
        grammar_json,
        true,
        Some("glyim"),
    ).expect("invalid grammar");
    builder.add(grammar);
    builder.build()
});

static THEME_SET: LazyLock<ThemeSet> = LazyLock::new(ThemeSet::load_defaults);

pub fn highlight_code(code: &str) -> String {
    let syntax = SYNTAX_SET.find_syntax_by_extension("g")
        .unwrap_or_else(|| SYNTAX_SET.find_syntax_plain_text());
    let theme = &THEME_SET.themes["base16-ocean.dark"];
    match highlighted_html_for_string(code, &SYNTAX_SET, syntax, theme) {
        Ok(html) => html,
        Err(e) => {
            eprintln!("Highlighting error: {e}");
            html_escape::encode_text(code).to_string()
        }
    }
}
