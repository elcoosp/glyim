use syntect::highlighting::ThemeSet;
use syntect::html::highlighted_html_for_string;
use syntect::parsing::SyntaxSet;

pub fn highlight_code(code: &str) -> String {
    let ss = SyntaxSet::load_defaults_newlines();
    let ts = ThemeSet::load_defaults();
    // fall back to plain text if somehow no syntax matches
    let syntax = ss
        .find_syntax_by_extension("g")
        .unwrap_or_else(|| ss.find_syntax_plain_text());
    let theme = &ts.themes["base16-ocean.dark"];
    match highlighted_html_for_string(code, &ss, syntax, theme) {
        Ok(html) => html,
        Err(e) => {
            eprintln!("highlight error: {e}");
            html_escape::encode_text(code).to_string()
        }
    }
}
