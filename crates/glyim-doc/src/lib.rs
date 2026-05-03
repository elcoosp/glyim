use glyim_hir::Hir;
use glyim_interner::Interner;
use pulldown_cmark::{Parser, html};

/// Generate HTML documentation from HIR, including doc comments rendered as GitHub Flavored Markdown.
pub fn generate_html(hir: &Hir, interner: &Interner) -> String {
    let mut html = String::from(
        "<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\"><title>Glyim Docs</title>\
         <style>body{font-family:sans-serif;max-width:900px;margin:0 auto;padding:20px;}\
         .doc-comment{background:#f8f8f8;border-left:4px solid #0366d6;padding:8px 16px;margin:12px 0;}\
         pre{background:#f0f0f0;padding:10px;border-radius:4px;overflow-x:auto;}\
         code{background:#f0f0f0;padding:2px 4px;border-radius:3px;}\
         h2{border-bottom:1px solid #ddd;margin-top:32px;}\
         ul{margin:4px 0;}</style>\
         </head><body>\n",
    );
    html.push_str("<h1>Module Documentation</h1>\n");

    for item in &hir.items {
        match item {
            glyim_hir::item::HirItem::Fn(f) => {
                if let Some(ref doc) = f.doc {
                    html.push_str("<div class=\"doc-comment\">");
                    let parser = Parser::new(doc);
                    html::push_html(&mut html, parser);
                    html.push_str("</div>\n");
                }
                html.push_str(&format!("<h2>fn {}</h2>\n", interner.resolve(f.name)));
                html.push_str("<p>Function</p>\n");
            }
            glyim_hir::item::HirItem::Struct(s) => {
                if let Some(ref doc) = s.doc {
                    html.push_str("<div class=\"doc-comment\">");
                    let parser = Parser::new(doc);
                    html::push_html(&mut html, parser);
                    html.push_str("</div>\n");
                }
                html.push_str(&format!("<h2>struct {}</h2>\n", interner.resolve(s.name)));
                html.push_str("<ul>\n");
                for field in &s.fields {
                    html.push_str(&format!("  <li>{}</li>\n", interner.resolve(field.name)));
                }
                html.push_str("</ul>\n");
            }
            glyim_hir::item::HirItem::Enum(e) => {
                if let Some(ref doc) = e.doc {
                    html.push_str("<div class=\"doc-comment\">");
                    let parser = Parser::new(doc);
                    html::push_html(&mut html, parser);
                    html.push_str("</div>\n");
                }
                html.push_str(&format!("<h2>enum {}</h2>\n", interner.resolve(e.name)));
                html.push_str("<ul>\n");
                for variant in &e.variants {
                    html.push_str(&format!("  <li>{}</li>\n", interner.resolve(variant.name)));
                }
                html.push_str("</ul>\n");
            }
            glyim_hir::item::HirItem::Impl(i) => {
                if let Some(ref doc) = i.doc {
                    html.push_str("<div class=\"doc-comment\">");
                    let parser = Parser::new(doc);
                    html::push_html(&mut html, parser);
                    html.push_str("</div>\n");
                }
                html.push_str(&format!("<h2>impl {}</h2>\n", interner.resolve(i.target_name)));
            }
            glyim_hir::item::HirItem::Extern(e) => {
                if let Some(ref doc) = e.doc {
                    html.push_str("<div class=\"doc-comment\">");
                    let parser = Parser::new(doc);
                    html::push_html(&mut html, parser);
                    html.push_str("</div>\n");
                }
                html.push_str("<h2>extern block</h2>\n");
            }
        }
    }
    html.push_str("</body></html>");
    html
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_paragraph() {
        let html = render_doc_comment("hello world");
        assert!(html.contains("<p>hello world</p>"));
    }

    #[test]
    fn renders_code_block() {
        let html = render_doc_comment("```\nlet x = 1;\n```");
        assert!(html.contains("<code>let x = 1;"));
    }

    fn render_doc_comment(doc: &str) -> String {
        let mut buf = String::new();
        let parser = Parser::new(doc);
        html::push_html(&mut buf, parser);
        buf
    }
}
