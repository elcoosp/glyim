use glyim_doc::manifest::{DocManifest, DocItem, HighlightedExample};
use glyim_doc::{extract_code_blocks, highlight_code};
use crate::pipeline::{compile_source_to_hir, PipelineConfig};
use glyim_hir::{HirItem, HirFn};
use glyim_interner::Interner;
use glyim_pkg::manifest::load_manifest;
use std::path::Path;
use walkdir::WalkDir;
use sha2::{Digest, Sha256};
use hex;

pub fn generate_manifest(package_dir: &Path) -> Result<DocManifest, String> {
    if !package_dir.is_dir() {
        return Err(format!(
            "manifest: IO error: Not a directory (os error 20) — expected directory, got {:?}",
            package_dir
        ));
    }
    let manifest_path = package_dir.join("glyim.toml");
    let manifest = load_manifest(&manifest_path).map_err(|e| format!("manifest: {e}"))?;
    let package_name = manifest.package.name;
    let version = manifest.package.version;

    let src_dir = package_dir.join("src");
    let mut file_paths = Vec::new();
    for entry in WalkDir::new(&src_dir).into_iter().filter_map(|e| e.ok()) {
        if entry.path().extension().map_or(false, |ext| ext == "g") {
            file_paths.push(entry.path().to_path_buf());
        }
    }

    let mut items = Vec::new();
    let config = PipelineConfig::default();

    for file_path in &file_paths {
        let source = std::fs::read_to_string(file_path)
            .map_err(|e| format!("read {:?}: {e}", file_path))?;
        let compiled = compile_source_to_hir(source, file_path, &config)
            .map_err(|e| format!("compile {:?}: {e}", file_path))?;

        eprintln!("HIR items count: {}", compiled.hir.items.len());
        for hir_item in &compiled.hir.items {
            eprintln!("  item: {:?}", hir_item);
            if let HirItem::Fn(f) = hir_item {
                eprintln!("    doc: {:?}", f.doc);
            }
        }

        let file_name = file_path.to_string_lossy().to_string();
        eprintln!("[docgen] HIR items count: {}", compiled.hir.items.len());
        for (idx, hir_item) in compiled.hir.items.iter().enumerate() {
            if let HirItem::Fn(f) = hir_item {
                let name = compiled.interner.resolve(f.name);
                eprintln!("[docgen] item {}: Fn '{}' doc={:?}", idx, name, f.doc);
            }
            let doc_item = hir_item_to_doc_item(
                hir_item,
                &compiled.interner,
                file_name.clone(),
                &package_name,
            )?;
            items.push(doc_item);
        }
    }

    Ok(DocManifest {
        package_name,
        version,
        items,
    })
}

fn hir_item_to_doc_item(
    item: &HirItem,
    interner: &Interner,
    file_name: String,
    package_name: &str,
) -> Result<DocItem, String> {
    let (kind, name, doc, signature_html, source_line) = match item {
        HirItem::Fn(f) => {
            let name_str = interner.resolve(f.name).to_string();
            let sig = format_fn_signature(f, interner);
            let doc = f.doc.clone();
            ("fn".into(), name_str, doc, sig, 0)
        }
        HirItem::Struct(s) => {
            let name_str = interner.resolve(s.name).to_string();
            let doc = s.doc.clone();
            let sig = format!("struct {} {{ /* fields */ }}", name_str);
            ("struct".into(), name_str, doc, sig, 0)
        }
        HirItem::Enum(e) => {
            let name_str = interner.resolve(e.name).to_string();
            let doc = e.doc.clone();
            let sig = format!("enum {} {{ /* variants */ }}", name_str);
            ("enum".into(), name_str, doc, sig, 0)
        }
        HirItem::Impl(i) => {
            let target = interner.resolve(i.target_name).to_string();
            let doc = i.doc.clone();
            let sig = format!("impl {} {{ /* methods */ }}", target);
            ("impl".into(), target, doc, sig, 0)
        }
        HirItem::Extern(e) => {
            let doc = e.doc.clone();
            let sig = "extern { ... }".to_string();
            ("extern".into(), "extern".into(), doc, sig, 0)
        }
    };

    let qualified_name = format!("{}::{}", package_name, name);

    let mut examples = Vec::new();
    if let Some(ref doc_str) = doc {
        for (_, code) in extract_code_blocks(doc_str) {
            let highlighted = highlight_code(&code);
            let mut hasher = Sha256::new();
            hasher.update(code.as_bytes());
            let hash = hex::encode(hasher.finalize());
            examples.push(HighlightedExample { code, html: highlighted, hash });
        }
    }

    Ok(DocItem {
        kind,
        name,
        qualified_name,
        doc,
        signature_html,
        source_file: file_name,
        source_line,
        highlighted_examples: examples,
        doc_test_results: Vec::new(),
        is_pub: true,
    })
}

fn format_fn_signature(f: &HirFn, interner: &Interner) -> String {
    let params: Vec<String> = f.params.iter().map(|(sym, _ty)| {
        format!("{}: ...", interner.resolve(*sym))
    }).collect();
    let ret = f.ret.as_ref().map(|_| " -> ...").unwrap_or("");
    format!("fn {}({}){}", interner.resolve(f.name), params.join(", "), ret)
}
