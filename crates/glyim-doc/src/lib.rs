use glyim_hir::Hir;
use glyim_interner::Interner;
use pulldown_cmark::{Parser, html};

/// Generate HTML documentation from HIR, including doc comments rendered as GitHub Flavored Markdown.
fn format_fn_signature(f: &glyim_hir::node::HirFn, interner: &Interner) -> String {
    let params: Vec<String> = f
        .params
        .iter()
        .map(|(sym, ty)| {
            format!(
                "{}: {}",
                interner.resolve(*sym),
                type_to_string(ty, interner)
            )
        })
        .collect();
    let ret = f
        .ret
        .as_ref()
        .map(|ty| format!(" -> {}", type_to_string(ty, interner)))
        .unwrap_or_default();
    format!(
        "fn {}({}){}",
        interner.resolve(f.name),
        params.join(", "),
        ret
    )
}

fn type_name_to_string(sym: glyim_interner::Symbol, interner: &Interner) -> String {
    match interner.resolve(sym) {
        "Int" | "i64" => "i64".to_string(),
        "Float" | "f64" => "f64".to_string(),
        "Bool" | "bool" => "bool".to_string(),
        "Str" | "str" => "str".to_string(),
        other => other.to_string(),
    }
}

fn type_to_string(ty: &glyim_hir::types::HirType, interner: &Interner) -> String {
    match ty {
        glyim_hir::types::HirType::Named(sym) => type_name_to_string(*sym, interner),
        glyim_hir::types::HirType::Generic(sym, args) => {
            let args_str: Vec<String> = args.iter().map(|a| type_to_string(a, interner)).collect();
            format!("{}<{}>", interner.resolve(*sym), args_str.join(", "))
        }
        glyim_hir::types::HirType::Int => "i64".to_string(),
        glyim_hir::types::HirType::Float => "f64".to_string(),
        glyim_hir::types::HirType::Bool => "bool".to_string(),
        glyim_hir::types::HirType::Str => "str".to_string(),
        glyim_hir::types::HirType::Unit => "()".to_string(),
        glyim_hir::types::HirType::Tuple(elems) => {
            let inner: Vec<String> = elems.iter().map(|e| type_to_string(e, interner)).collect();
            format!("({})", inner.join(", "))
        }
        glyim_hir::types::HirType::RawPtr(inner) => {
            format!("*mut {}", type_to_string(inner, interner))
        }
        glyim_hir::types::HirType::Func(params, ret) => {
            let p: Vec<String> = params.iter().map(|t| type_to_string(t, interner)).collect();
            format!("fn({}) -> {}", p.join(", "), type_to_string(ret, interner))
        }
        _ => format!("{:?}", ty),
    }
}

/// Extract Glyim code blocks from a Markdown doc string.
/// Returns a list of (optional title, code) extracted from ```glyim fences.
pub fn extract_code_blocks(doc: &str) -> Vec<(Option<String>, String)> {
    let mut blocks = Vec::new();
    let mut in_glyim_block = false;
    let mut block_title = None;
    let mut block_lines = Vec::new();
    let mut in_fence = false;
    let mut lang = String::new();

    for line in doc.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            if in_fence {
                // closing fence
                if in_glyim_block {
                    let code = block_lines.join("\n");
                    if !code.trim().is_empty() {
                        blocks.push((block_title.take(), code));
                    }
                    in_glyim_block = false;
                    block_lines.clear();
                }
                in_fence = false;
                lang.clear();
            } else {
                // opening fence
                in_fence = true;
                lang = trimmed.strip_prefix("```").unwrap_or("").trim().to_string();
                block_title = None;
                in_glyim_block = lang == "glyim";
            }
        } else if in_fence && in_glyim_block {
            block_lines.push(line.to_string());
        }
    }
    blocks
}

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
                let sig = format_fn_signature(f, interner);
                html.push_str(&format!("<h2>{}</h2>\n", sig));
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
                    if let Some(ref doc) = field.doc {
                        html.push_str("<div class=\"doc-comment\">");
                        let parser = Parser::new(doc);
                        html::push_html(&mut html, parser);
                        html.push_str("</div>\n");
                    }
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
                    if let Some(ref doc) = variant.doc {
                        html.push_str("<div class=\"doc-comment\">");
                        let parser = Parser::new(doc);
                        html::push_html(&mut html, parser);
                        html.push_str("</div>\n");
                    }
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
                html.push_str(&format!(
                    "<h2>impl {}</h2>\n",
                    interner.resolve(i.target_name)
                ));
                for method in &i.methods {
                    if let Some(ref doc) = method.doc {
                        html.push_str("<div class=\"doc-comment\">");
                        let parser = Parser::new(doc);
                        html::push_html(&mut html, parser);
                        html.push_str("</div>\n");
                    }
                    let sig = format_fn_signature(method, interner);
                    html.push_str(&format!("<h3>{}</h3>\n", sig));
                }
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

    #[test]
    fn extract_simple_block() {
        let doc = "```glyim\nlet x = 1;\n```\n";
        let blocks = extract_code_blocks(doc);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].1, "let x = 1;");
    }

    #[test]
    fn extract_multiple_blocks() {
        let doc = "```glyim\n1 + 1\n```\nbar\n```glyim\n2 + 2\n```\n";
        let blocks = extract_code_blocks(doc);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[1].1, "2 + 2");
    }

    #[test]
    fn ignore_non_glyim_blocks() {
        let doc = "```\nnot glyim\n```\n```glyim\n42\n```\n";
        let blocks = extract_code_blocks(doc);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].1, "42");
    }
}
