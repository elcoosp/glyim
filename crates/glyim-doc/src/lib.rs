use glyim_hir::Hir;
use glyim_interner::Interner;

pub fn generate_html(hir: &Hir, interner: &Interner) -> String {
    let mut html = String::from("<html><head><title>Glyim Docs</title></head><body>\n");
    html.push_str("<h1>Module Documentation</h1>\n");
    for item in &hir.items {
        match item {
            glyim_hir::item::HirItem::Fn(f) => {
                html.push_str(&format!("<h2>fn {}</h2>\n", interner.resolve(f.name)));
                html.push_str("<p>Function</p>\n");
            }
            glyim_hir::item::HirItem::Struct(s) => {
                html.push_str(&format!("<h2>struct {}</h2>\n", interner.resolve(s.name)));
                html.push_str("<ul>\n");
                for field in &s.fields {
                    html.push_str(&format!("  <li>{}</li>\n", interner.resolve(field.name)));
                }
                html.push_str("</ul>\n");
            }
            glyim_hir::item::HirItem::Enum(e) => {
                html.push_str(&format!("<h2>enum {}</h2>\n", interner.resolve(e.name)));
                html.push_str("<ul>\n");
                for variant in &e.variants {
                    html.push_str(&format!("  <li>{}</li>\n", interner.resolve(variant.name)));
                }
                html.push_str("</ul>\n");
            }
            _ => {}
        }
    }
    html.push_str("</body></html>");
    html
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_html_includes_struct_and_fn() {
        let src = "struct Point { x, y }\nfn get_x() -> i64 { 0 }\nmain = () => 42";
        let parse_out = glyim_parse::parse(src);
        let mut interner = parse_out.interner;
        let hir = glyim_hir::lower(&parse_out.ast, &mut interner);
        let html = generate_html(&hir, &interner);
        assert!(html.contains("Point"));
        assert!(html.contains("get_x"));
        assert!(html.contains("</html>"));
    }
}
