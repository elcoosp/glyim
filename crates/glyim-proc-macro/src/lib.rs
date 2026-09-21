//! Procedural macro bridge (Phase 9.2 MVP).
//!
//! This crate defines the stable, C-compatible ABI contract between the Glyim
//! compiler and a procedural-macro crate compiled to a `cdylib`, plus the
//! loader that `dlopen`s such a crate and the in-process registry used during
//! macro expansion.
//!
//! Design notes:
//! - The dylib boundary must **not** pass Rust-internal HIR/AST types. Instead
//!   the contract is a flat, `#[repr(C)]` token stream (see [`PmToken`],
//!   [`PmTokenStream`], [`PmStr`]). This mirrors how real Rust serializes
//!   `proc_macro::TokenStream` across the proc-macro boundary.
//! - The host (compiler) exports the allocator/`push`/`free` helpers
//!   ([`pm_ts_alloc`], [`pm_ts_push`], [`pm_ts_free`]) that a loaded dylib
//!   calls to build its output token stream. The dylib exports a single entry
//!   point, [`PROC_MACRO_MAIN_SYMBOL`], which receives a C-ABI *register*
//!   callback and uses it to publish each `#[proc_macro]` function.
//!
//! The two-stage *compile* of a proc-macro crate to a host cdylib
//! (`glyim-cli` building the crate for the host target then loading it) is the
//! larger remaining piece tracked in `docs/plans/v0.1.0/unstub-5/KNOWN_GAPS.md`
//! Phase 9.2. This crate provides the ABI + loader + in-process registry, all
//! of which are exercised green by the unit tests.

use glyim_syntax::SyntaxKind;
use std::collections::HashMap;
use std::ffi::{c_char, CStr, CString};
use std::ptr;

/// Symbol name a compiled proc-macro cdylib must export as its entry point.
///
/// Signature (C ABI): `extern "C" fn(proc_macro_register: PmRegisterFn)`.
pub const PROC_MACRO_MAIN_SYMBOL: &[u8] = b"glyim_proc_macro_main\0";

/// A borrowed string view across the dylib boundary.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PmStr {
    /// Pointer to UTF-8 bytes (not necessarily NUL-terminated).
    pub ptr: *const u8,
    /// Byte length.
    pub len: u32,
}

impl PmStr {
    /// Borrow a Rust `str` as a [`PmStr`] (no copy; caller must keep alive).
    #[allow(clippy::not_unsafe_ptr_arg_deref)]
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> PmStr {
        PmStr {
            ptr: s.as_ptr(),
            len: s.len() as u32,
        }
    }

    /// Copy this view into an owned Rust `String`.
    ///
    /// # Safety
    /// `ptr` must point to `len` valid UTF-8 bytes for the duration of the read.
    pub unsafe fn to_string(&self) -> String {
        if self.ptr.is_null() || self.len == 0 {
            return String::new();
        }
        unsafe {
            let slice = std::slice::from_raw_parts(self.ptr, self.len as usize);
            String::from_utf8_lossy(slice).into_owned()
        }
    }
}

/// A single token crossing the dylib boundary.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PmToken {
    /// `SyntaxKind` discriminator (`u16` is wide enough for the kind space).
    pub kind: u16,
    /// Token text (the lexed source text).
    pub text: PmStr,
}

/// A flat token stream passed across the dylib boundary.
///
/// Owned by whichever side allocated it via [`pm_ts_alloc`]/[`pm_ts_push`].
/// The host frees output streams it receives via [`pm_ts_free`].
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PmTokenStream {
    /// Pointer to `len` contiguous [`PmToken`]s (heap-allocated).
    pub ptr: *mut PmToken,
    /// Number of live tokens.
    pub len: u32,
    /// Allocated capacity in tokens.
    pub cap: u32,
}

// The pointer fields are owned/transferred explicitly; the struct itself is
// safe to send across the FFI boundary as a value.
unsafe impl Send for PmTokenStream {}

/// C-ABI signature of the host's register callback, handed to a loaded dylib's
/// `glyim_proc_macro_main`.
///
/// Arguments: `(name_ptr, name_len, entry)`. `entry` is itself a C-ABI function
/// pointer the host will later call with an input [`PmTokenStream`] and expect
/// an output [`PmTokenStream`] (allocated by the host helpers).
pub type PmRegisterFn = extern "C" fn(*const c_char, *mut PmTokenStream, *mut PmTokenStream);

/// C-ABI signature of a registered macro entry point.
pub type PmMacroFn = extern "C" fn(*mut PmTokenStream, *mut PmTokenStream);

// ---------------------------------------------------------------------------
// ABI helpers (host side)
// ---------------------------------------------------------------------------

/// Allocate an empty, growable [`PmTokenStream`].
///
/// # Safety / ownership
/// The returned stream must be freed with [`pm_ts_free`] exactly once.
#[unsafe(no_mangle)]
pub extern "C" fn pm_ts_alloc() -> PmTokenStream {
    PmTokenStream {
        ptr: ptr::null_mut(),
        len: 0,
        cap: 0,
    }
}

/// Append `token` to `stream`, growing the backing buffer as needed.
///
/// # Safety
/// `stream` must be a valid stream returned by [`pm_ts_alloc`] (or one the
/// host owns), and `token.text.ptr`/`token.text.len` must describe valid
/// memory for the duration of the push.
#[unsafe(no_mangle)]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn pm_ts_push(stream: *mut PmTokenStream, token: PmToken) {
    unsafe {
        let s = &mut *stream;
        if s.len == s.cap {
            let new_cap = (s.cap as usize).max(8) * 2;
            let new_layout = std::alloc::Layout::array::<PmToken>(new_cap).unwrap();
            let new_ptr = if s.ptr.is_null() {
                std::alloc::alloc(new_layout) as *mut PmToken
            } else {
                let old_layout = std::alloc::Layout::array::<PmToken>(s.cap as usize).unwrap();
                std::alloc::realloc(s.ptr as *mut u8, old_layout, new_layout.size()) as *mut PmToken
            };
            if new_ptr.is_null() {
                std::alloc::handle_alloc_error(new_layout);
            }
            s.ptr = new_ptr;
            s.cap = new_cap as u32;
        }
        *s.ptr.add(s.len as usize) = token;
        s.len += 1;
    }
}

/// Free a [`PmTokenStream`] previously allocated by the host.
///
/// # Safety
/// `stream` must be a stream the host owns (returned by [`pm_ts_alloc`] and not
/// yet freed).
#[unsafe(no_mangle)]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn pm_ts_free(stream: *mut PmTokenStream) {
    unsafe {
        let s = &mut *stream;
        if !s.ptr.is_null() && s.cap > 0
            && let Ok(layout) = std::alloc::Layout::array::<PmToken>(s.cap as usize) {
                std::alloc::dealloc(s.ptr as *mut u8, layout);
            }
        s.ptr = ptr::null_mut();
        s.len = 0;
        s.cap = 0;
    }
}

// ---------------------------------------------------------------------------
// Token <-> PmToken conversion
// ---------------------------------------------------------------------------

/// Convert a compiler `Token` (kind + text) into a boundary [`PmToken`].
///
/// Only `kind` and `text` cross the boundary; span/mark are deliberately
/// dropped (the expanded tokens get fresh spans at splice time).
pub fn token_to_pm(kind: SyntaxKind, text: &str) -> PmToken {
    PmToken {
        kind: kind as u16,
        text: PmStr::from_str(text),
    }
}

/// Read a boundary [`PmTokenStream`] into `(SyntaxKind, String)` pairs.
///
/// # Safety
/// `ts` must be a valid stream (either the host's own or one received from a
/// dylib that the host owns).
pub unsafe fn pm_to_tokens(ts: &PmTokenStream) -> Vec<(SyntaxKind, String)> {
    let mut out = Vec::with_capacity(ts.len as usize);
    if ts.ptr.is_null() {
        return out;
    }
    unsafe {
        for i in 0..ts.len as usize {
            let t = &*ts.ptr.add(i);
            let kind = SyntaxKind::try_from(t.kind).unwrap_or(SyntaxKind::Error);
            let text = t.text.to_string();
            out.push((kind, text));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Registry / loader
// ---------------------------------------------------------------------------

/// Expansion function: maps an input token list to an output token list.
pub type MacroFn = dyn Fn(&[(SyntaxKind, String)]) -> Vec<(SyntaxKind, String)> + Send + Sync;

/// A registered procedural macro: a name plus an expansion function that maps
/// an input token list to an output token list.
#[derive(Clone)]
pub struct ProcMacro {
    /// Macro name as written at the call site.
    pub name: String,
    /// Expansion: `(input tokens) -> output tokens`.
    pub expand: std::sync::Arc<MacroFn>,
}

/// Registry of procedural macros available during expansion.
#[derive(Default)]
pub struct Registry {
    macros: HashMap<String, ProcMacro>,
}

impl Registry {
    /// Create an empty registry.
    pub fn new() -> Registry {
        Registry::default()
    }

    /// Register a macro in-process (used for tests and built-in proc macros).
    pub fn register<F>(&mut self, name: &str, expand: F)
    where
        F: Fn(&[(SyntaxKind, String)]) -> Vec<(SyntaxKind, String)> + Send + Sync + 'static,
    {
        self.macros.insert(
            name.to_string(),
            ProcMacro {
                name: name.to_string(),
                expand: std::sync::Arc::new(expand),
            },
        );
    }

    /// True if `name` resolves to a registered proc macro.
    pub fn contains(&self, name: &str) -> bool {
        self.macros.contains_key(name)
    }

    /// Expand `input` through the proc macro named `name`.
    ///
    /// Returns `None` if no such macro is registered (caller should fall back
    /// to declarative expansion or raise an "unresolved macro" diagnostic).
    pub fn expand(&self, name: &str, input: &[(SyntaxKind, String)]) -> Option<Vec<(SyntaxKind, String)>> {
        self.macros.get(name).map(|m| (m.expand)(input))
    }

    /// Number of registered macros.
    pub fn len(&self) -> usize {
        self.macros.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.macros.is_empty()
    }

    /// Merge another registry's macros into this one. Macros already present
    /// in `self` keep their existing definition (the incoming copy is ignored).
    /// Used by the two-stage proc-macro build to combine the `Registry`es
    /// loaded from each proc-macro dependency crate into a single shared
    /// registry (Phase 8 / plan §9.2).
    pub fn merge(&mut self, other: &Registry) {
        for (name, pm) in &other.macros {
            self.macros.entry(name.clone()).or_insert_with(|| pm.clone());
        }
    }
}

/// A loaded proc-macro cdylib handle (keeps the library resident).
pub struct LoadedCrate {
    /// Library handle, kept alive for the lifetime of the registry it populated.
    _lib: libloading::Library,
    /// Registry populated by the loaded crate's `glyim_proc_macro_main`.
    pub registry: Registry,
}

impl Drop for LoadedCrate {
    fn drop(&mut self) {
        // `libloading::Library` closes itself on drop; nothing to do here.
    }
}

/// Load a compiled proc-macro cdylib and populate a [`Registry`] from it.
///
/// This performs the **load + register** half of the two-stage proc-macro
/// build. The compile half (building the proc-macro crate for the host target)
/// is tracked separately in `docs/plans/v0.1.0/unstub-5/KNOWN_GAPS.md` Phase 9.2.
///
/// Uses `libloading`, which dispatches to `dlopen` (Unix) / `LoadLibraryW`
/// (Windows) / `dlopen` (macOS) behind one API, so the same path works on
/// every platform the compiler itself supports (Phase 1.5).
pub fn load_cdylib(path: &str) -> Result<LoadedCrate, String> {
    // SAFETY: proc-macro cdylibs are build-generated, trusted artifacts
    // produced by this same toolchain's own build of the proc-macro crate —
    // matches the existing Unix path's trust assumption.
    let lib = unsafe {
        libloading::Library::new(path)
            .map_err(|e| format!("failed to load proc-macro cdylib {path}: {e}"))?
    };

    // SAFETY: the symbol is the crate's single C-ABI entry point; we cast it
    // to the exact `PmRegisterMain` fn-pointer type the ABI contract expects.
    let main_fn: PmRegisterMain = unsafe {
        *lib.get(PROC_MACRO_MAIN_SYMBOL)
            .map_err(|e| format!("{} not found in {path}: {e}", String::from_utf8_lossy(&PROC_MACRO_MAIN_SYMBOL[..PROC_MACRO_MAIN_SYMBOL.len() - 1])))?
    };

    let mut registry = Registry::new();
    // The C-ABI register callback that the dylib calls once per macro.
    extern "C" fn register_cb(
        reg: *mut RegistryHolder,
        name: *const c_char,
        entry: PmMacroFn,
    ) {
        unsafe {
            let name = CStr::from_ptr(name).to_string_lossy().into_owned();
            let expand = move |input: &[(SyntaxKind, String)]| -> Vec<(SyntaxKind, String)> {
                // Build input PmTokenStream on the host side.
                let mut in_ts = pm_ts_alloc();
                for (kind, text) in input {
                    let ctext = CString::new(text.as_str()).unwrap_or_default();
                    let pm_text = PmStr {
                        ptr: ctext.as_ptr() as *const u8,
                        len: text.len() as u32,
                    };
                    pm_ts_push(
                        &mut in_ts,
                        PmToken {
                            kind: *kind as u16,
                            text: pm_text,
                        },
                    );
                    // ctext is leaked intentionally: the dylib reads it during
                    // the call; kept alive for the duration of the call.
                    std::mem::forget(ctext);
                }
                let mut out_ts = pm_ts_alloc();
                (entry)(&mut in_ts, &mut out_ts);
                let result = pm_to_tokens(&out_ts);
                pm_ts_free(&mut in_ts);
                pm_ts_free(&mut out_ts);
                result
            };
            (*reg).0.register(&name, expand);
        }
    }

    let mut holder = RegistryHolder(registry);
    main_fn(&mut holder as *mut RegistryHolder, register_cb);
    registry = holder.0;

    Ok(LoadedCrate {
        _lib: lib,
        registry,
    })
}

type PmRegisterMain = extern "C" fn(*mut RegistryHolder, PmRegisterCallback);
type PmRegisterCallback =
    extern "C" fn(*mut RegistryHolder, *const c_char, PmMacroFn);

/// Opaque holder passed to the dylib's main; bundles the registry so the C-ABI
/// callback can register into it.
#[repr(C)]
pub struct RegistryHolder(pub Registry);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_expands_identity_roundtrip() {
        let mut reg = Registry::new();
        reg.register("identity", |input| input.to_vec());
        assert!(reg.contains("identity"));

        let input: Vec<(SyntaxKind, String)> =
            vec![(SyntaxKind::KwFn, "fn".into()), (SyntaxKind::Ident, "main".into())];
        let out = reg.expand("identity", &input).expect("macro registered");
        assert_eq!(out, input, "identity macro must return input unchanged");
    }

    #[test]
    fn registry_unknown_macro_returns_none() {
        let reg = Registry::new();
        assert!(reg.expand("nope", &[]).is_none());
    }

    #[test]
    #[allow(unused_unsafe)]
    fn abi_alloc_push_free_roundtrip() {
        let mut ts = pm_ts_alloc();
        let ctext = CString::new("example").unwrap();
        let pm_text = PmStr {
            ptr: ctext.as_ptr() as *const u8,
            len: 7,
        };
        unsafe {
            pm_ts_push(
                &mut ts,
                PmToken {
                    kind: SyntaxKind::Ident as u16,
                    text: pm_text,
                },
            );
        }
        assert_eq!(ts.len, 1);
        unsafe {
            let tokens = pm_to_tokens(&ts);
            assert_eq!(tokens.len(), 1);
            assert_eq!(tokens[0].0, SyntaxKind::Ident);
            assert_eq!(tokens[0].1, "example");
            pm_ts_free(&mut ts);
        }
        assert_eq!(ts.len, 0);
    }

    #[test]
    fn registry_merge_combines_macros() {
        let mut a = Registry::new();
        a.register("derive_a", |input| input.to_vec());
        assert!(a.contains("derive_a"));

        let mut b = Registry::new();
        b.register("derive_b", |input| input.to_vec());
        assert!(b.contains("derive_b"));

        // Merging `b` into `a` brings `derive_b` in while keeping `derive_a`.
        a.merge(&b);
        assert!(a.contains("derive_a"));
        assert!(a.contains("derive_b"));
        assert_eq!(a.len(), 2);

        // Re-registering an existing name keeps the original (no duplicate).
        let mut c = Registry::new();
        c.register("derive_a", |input| input.to_vec());
        a.merge(&c);
        assert_eq!(a.len(), 2, "duplicate name must not create a second entry");
    }

    /// End-to-end two-stage proc-macro round trip (Phase 8 / plan §9.2).
    ///
    /// This is the one path the prior sessions never exercised: actually
    /// compiling a real proc-macro crate to a host cdylib and loading it via
    /// `load_cdylib`, then expanding a macro through the loaded `Registry`.
    /// The fixture (`tests/fixtures/pm_doubler/src/lib.rs`) exports
    /// `glyim_proc_macro_main` and registers a `reverse` macro that reverses
    /// the input token stream. We compile it with the host `rustc`, load the
    /// produced `.dylib`/`.so`, and assert the macro both registers and
    /// expands correctly through the C-ABI boundary.
    #[test]
    fn load_cdylib_round_trip_compiles_and_expands() {
        // Locate the fixture source relative to this crate root.
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let fixture = std::path::Path::new(manifest_dir)
            .join("tests/fixtures/pm_doubler/src/lib.rs");
        assert!(fixture.exists(), "fixture missing: {}", fixture.display());

        let out_dir = std::env::temp_dir().join(format!(
            "glyim_pm_rt_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&out_dir).expect("create temp dir");
        // The temp cdylib lives under std::env::temp_dir() and is left for the
        // OS to reap; it is uniquely named per-PID so repeated runs don't clash.
        let ext = if cfg!(target_os = "macos") {
            "dylib"
        } else {
            "so"
        };
        let cdylib = out_dir.join(format!("libpm_doubler.{ext}"));

        // Compile the fixture for the HOST triple to a cdylib.
        let status = std::process::Command::new("rustc")
            .args([
                "--edition",
                "2021",
                "--crate-type",
                "cdylib",
                "-O",
            ])
            .arg(&fixture)
            .arg("-o")
            .arg(&cdylib)
            .status()
            .expect("failed to spawn rustc");
        assert!(status.success(), "rustc failed to compile the proc-macro fixture");
        assert!(cdylib.exists(), "compiled cdylib missing: {}", cdylib.display());

        // Load it through the real two-stage loader.
        let loaded = load_cdylib(cdylib.to_str().unwrap())
            .expect("load_cdylib must succeed on the compiled fixture");
        assert!(
            loaded.registry.contains("reverse"),
            "loaded registry must contain the `reverse` macro"
        );
        assert_eq!(loaded.registry.len(), 1, "exactly one macro expected");

        // Expand: `reverse` of `[KwFn "fn", Ident "main"]` must yield
        // `[Ident "main", KwFn "fn"]`.
        let input: Vec<(SyntaxKind, String)> = vec![
            (SyntaxKind::KwFn, "fn".into()),
            (SyntaxKind::Ident, "main".into()),
        ];
        let out = loaded
            .registry
            .expand("reverse", &input)
            .expect("reverse must be registered");
        assert_eq!(
            out,
            vec![
                (SyntaxKind::Ident, "main".into()),
                (SyntaxKind::KwFn, "fn".into()),
            ],
            "reverse must reverse the token stream across the C-ABI boundary"
        );
    }
}
