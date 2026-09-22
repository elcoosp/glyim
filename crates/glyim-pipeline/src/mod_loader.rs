//! External module loading (`mod foo;` → `foo.g`).
//!
//! # Design
//!
//! `mod foo;` (no body) is a *declaration* that the module's items live in
//! another file. The parser already produces a bodyless `Module` node for
//! it; nothing downstream knows to look for the file, so `helper::value()`
//! resolves to nothing and codegen ICEs on the resulting `TyKind::Error`.
//!
//! This pass **flattens** a crate into a single source string: every
//! `mod foo;` is replaced by `mod foo { <contents of foo.g> }`, recursively,
//! before parsing. Flattening (rather than splicing a rowan tree) means the
//! parser, def-map, HIR, typeck, and codegen are all unchanged — they see
//! one ordinary source with inline modules. That is exactly how the language
//! semantics define `mod foo;`: an inline module whose contents come from
//! another file.
//!
//! # File resolution
//!
//! For `mod foo;` declared in file `dir/parent.g`, the loader looks for, in
//! order:
//!   1. `dir/foo.g`
//!   2. `dir/foo/mod.g`
//!
//! (Rust's `dir/foo/mod.rs` convention, with `.g` in place of `.rs`.)
//!
//! # Cycle detection
//!
//! The loader tracks the absolute paths currently being expanded. A cycle
//! (`a.g` → `b.g` → `a.g`) is reported as a diagnostic and the offending
//! `mod` is left unexpanded, so the user gets a real error rather than a
//! stack overflow.
//!
//! # Diagnostic locations
//!
//! Flattening to one string means the whole crate shares a single `FileId`;
//! a diagnostic from a loaded file points into the *flattened* source, where
//! the loaded contents are visible inline. A future refinement is an offset
//! map from flattened to (file, offset) so diagnostics point at the original
//! file — tracked, not required for correctness.

use glyim_diag::{DiagSeverity, ErrorCategory, ErrorCode, GlyimDiagnostic, MultiSpan};
use glyim_span::Span;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Flatten every bodyless `mod foo;` in `source` into an inline
/// `mod foo { ... }`, loading `foo.g` (or `foo/mod.g`) relative to
/// `source_path`.
///
/// Returns the flattened source and any diagnostics produced (missing file,
/// cycle). The flattened source parses and lowers exactly like a hand-written
/// single-file crate with inline modules.
pub fn flatten_modules(
    source: &str,
    source_path: &Path,
    file_id: glyim_span::FileId,
) -> (String, Vec<GlyimDiagnostic>) {
    let mut diags = Vec::new();
    let mut active: HashSet<PathBuf> = HashSet::new();
    let canonical = source_path
        .canonicalize()
        .unwrap_or_else(|_| source_path.to_path_buf());
    active.insert(canonical);
    let mut ctx = LoadCtx { file_id };
    let out = flatten(source, source_path, &mut active, &mut diags, &mut ctx);
    (out, diags)
}

/// Loader bookkeeping: the crate's `FileId` so module-file diagnostics carry
/// real spans into the source the pipeline is compiling.
struct LoadCtx {
    file_id: glyim_span::FileId,
}

/// Recursive worker. `active` is the set of file paths currently being
/// expanded (for cycle detection); a path is inserted on entry and removed
/// on exit so a diamond dependency (`a` uses `b` and `c`, both use `d`) is
/// not falsely flagged as a cycle.
fn flatten(
    source: &str,
    source_path: &Path,
    active: &mut HashSet<PathBuf>,
    diags: &mut Vec<GlyimDiagnostic>,
    ctx: &mut LoadCtx,
) -> String {
    let mods = find_bodyless_mods(source);
    if mods.is_empty() {
        return source.to_string();
    }

    let dir = source_path.parent().unwrap_or_else(|| Path::new("."));
    let mut out = String::with_capacity(source.len());
    let mut cursor = 0usize;

    for m in &mods {
        // Copy everything before this `mod foo;`.
        out.push_str(&source[cursor..m.start]);
        out.push_str(&format!("mod {} {{ ", m.name));

        match resolve_module_file(dir, &m.name) {
            Some(mod_path) => {
                let canonical = mod_path
                    .canonicalize()
                    .unwrap_or_else(|_| mod_path.clone());
                if active.contains(&canonical) {
                    diags.push(module_error(
                        m.start,
                        m.end,
                        source,
                        ctx,
                        &format!(
                            "module `{}` forms a cycle: `{}` is already being expanded",
                            m.name,
                            mod_path.display()
                        ),
                    ));
                } else {
                    match std::fs::read_to_string(&mod_path) {
                        Ok(mod_src) => {
                            active.insert(canonical.clone());
                            out.push_str(&flatten(&mod_src, &mod_path, active, diags, ctx));
                            active.remove(&canonical);
                        }
                        Err(e) => diags.push(module_error(
                            m.start,
                            m.end,
                            source,
                            ctx,
                            &format!(
                                "failed to read module file `{}` for `mod {};`: {}",
                                mod_path.display(),
                                m.name,
                                e
                            ),
                        )),
                    }
                }
            }
            None => diags.push(module_error(
                m.start,
                m.end,
                source,
                ctx,
                &format!(
                    "cannot find module file for `mod {};` (looked for `{}/{}.g` and `{}/{}/mod.g`)",
                    m.name,
                    dir.display(),
                    m.name,
                    dir.display(),
                    m.name
                ),
            )),
        }

        out.push_str(" }");
        cursor = m.end;
    }

    out.push_str(&source[cursor..]);
    out
}

/// A bodyless `mod <ident>;` discovered in the source text, with the byte
/// range covering `mod <ident>;` (including the semicolon).
struct ModDecl {
    name: String,
    start: usize,
    end: usize,
}

/// Scan `source` for top-level `mod <ident>;` declarations.
///
/// A line-oriented scan is deliberate and sufficient: `mod foo;` is an *item*
/// declaration and, like every item, occupies its own logical unit; the only
/// thing that can precede it on a line is trivia (`//` comment, whitespace).
/// We skip `//` line comments and string/char literals so a `mod` inside a
/// comment or string is not misdetected. Module content is later merged by
/// the parser, so a mis-scan here cannot produce a wrong tree — at worst it
/// loads a file that the parser then rejects.
fn find_bodyless_mods(source: &str) -> Vec<ModDecl> {
    let bytes = source.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    let n = bytes.len();

    while i < n {
        let c = bytes[i];

        // Skip line comments.
        if c == b'/' && i + 1 < n && bytes[i + 1] == b'/' {
            while i < n && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        // Skip block comments.
        if c == b'/' && i + 1 < n && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < n && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(n);
            continue;
        }
        // Skip string and char literals.
        if c == b'"' || c == b'\'' {
            let quote = c;
            i += 1;
            while i < n && bytes[i] != quote {
                if bytes[i] == b'\\' && i + 1 < n {
                    i += 1;
                }
                i += 1;
            }
            i = (i + 1).min(n);
            continue;
        }

        // Is `mod` a whole identifier here?
        if c == b'm'
            && source[i..].starts_with("mod")
            && is_ident_boundary_before(source, i)
        {
            let mut j = i + 3;
            // skip whitespace
            while j < n && (bytes[j] as char).is_whitespace() {
                j += 1;
            }
            // read identifier
            let name_start = j;
            while j < n && (bytes[j] == b'_' || (bytes[j] as char).is_alphanumeric()) {
                j += 1;
            }
            let name = &source[name_start..j];
            if !name.is_empty() {
                // skip whitespace, require `;`
                while j < n && (bytes[j] as char).is_whitespace() {
                    j += 1;
                }
                if j < n && bytes[j] == b';' {
                    out.push(ModDecl {
                        name: name.to_string(),
                        start: i,
                        end: j + 1,
                    });
                    i = j + 1;
                    continue;
                }
            }
        }

        i += 1;
    }

    out
}

fn is_ident_boundary_before(source: &str, i: usize) -> bool {
    if i == 0 {
        return true;
    }
    let prev = source.as_bytes()[i - 1];
    !(prev == b'_' || (prev as char).is_alphanumeric())
}

/// Resolve `mod name;` relative to `dir`, trying `name.g` then `name/mod.g`.
fn resolve_module_file(dir: &Path, name: &str) -> Option<PathBuf> {
    let direct = dir.join(format!("{name}.g"));
    if direct.is_file() {
        return Some(direct);
    }
    let mod_dir = dir.join(name).join("mod.g");
    if mod_dir.is_file() {
        return Some(mod_dir);
    }
    None
}

/// Build a diagnostic pointing at the `mod name;` range in the *original*
/// (un-flattened) source. The span is a byte range within `source`; the
/// pipeline has already registered `source` as the crate's `FileId`.
fn module_error(
    start: usize,
    end: usize,
    _source: &str,
    ctx: &LoadCtx,
    message: &str,
) -> GlyimDiagnostic {
    use glyim_span::{ByteIdx, SyntaxContext};
    let span = Span::new(
        ctx.file_id,
        ByteIdx::from_raw(start as u32),
        ByteIdx::from_raw(end as u32),
        SyntaxContext::ROOT,
    );
    GlyimDiagnostic::new(
        ErrorCode {
            category: ErrorCategory::Type,
            number: 0,
        },
        DiagSeverity::Error,
        message.to_string(),
        MultiSpan::from_span(span),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_bodyless_mod() {
        let mods = find_bodyless_mods("mod a;\nfn main() {}\n");
        assert_eq!(mods.len(), 1);
        assert_eq!(mods[0].name, "a");
    }

    #[test]
    fn ignores_inline_mod() {
        let mods = find_bodyless_mods("mod a { fn f() {} }\n");
        assert!(mods.is_empty());
    }

    #[test]
    fn ignores_mod_in_comment() {
        let mods = find_bodyless_mods("// mod hidden;\nfn main() {}\n");
        assert!(mods.is_empty());
    }

    #[test]
    fn ignores_mod_in_string() {
        let mods = find_bodyless_mods(r#"fn f() { let s = "mod hidden;"; }"#);
        assert!(mods.is_empty());
    }

    #[test]
    fn finds_multiple() {
        let mods = find_bodyless_mods("mod a;\nmod b;\n");
        assert_eq!(mods.len(), 2);
    }
}
