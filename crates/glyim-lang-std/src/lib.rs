//! Glyim Language Standard Library
//!
//! This crate contains the standard library source files for the Glyim language.
//! The actual library code is in `.g` files under `lib/`, written in Glyim syntax.
//! This Rust crate provides access to those source files and testing infrastructure.
//!
//! The std library builds on top of `glyim-lang-core` and provides:
//! - `std::io` — I/O primitives (Read, Write, stdin/stdout/stderr)
//! - `std::fs` — Filesystem operations (File, OpenOptions, directory operations)
//! - `std::net` — Networking (TCP, UDP, IP addresses)
//! - `std::thread` — Native thread spawning and management
//! - `std::sync` — Synchronization primitives (Mutex, RwLock, Arc, atomics)
//! - `std::env` — Environment variables, process arguments, current directory
//! - `std::time` — Time measurement (Duration, Instant, SystemTime)
//! - `std::process` — Child process spawning and management

/// Returns the source code of a standard library module by name.
pub fn std_source(name: &str) -> Option<&'static str> {
    match name {
        "io" => Some(include_str!("../lib/io.g")),
        "fs" => Some(include_str!("../lib/fs.g")),
        "net" => Some(include_str!("../lib/net.g")),
        "thread" => Some(include_str!("../lib/thread.g")),
        "sync" => Some(include_str!("../lib/sync.g")),
        "env" => Some(include_str!("../lib/env.g")),
        "time" => Some(include_str!("../lib/time.g")),
        "process" => Some(include_str!("../lib/process.g")),
        // `future` lives in the core library (`glyim-lang-core/lib/future.g`);
        // the stdlib's async futures (`net.g`'s `WriteFuture`/`ReadFuture`)
        // depend on `Future`/`Poll`/`Context`/`Waker`, so it must be assembled
        // alongside the std modules.
        "future" => Some(include_str!("../../glyim-lang-core/lib/future.g")),
        // Core extension-method impls. `impl str` / `impl<T> [T]` define the
        // `as_ptr`/`len`/`to_string`/`iter_mut` methods the std sources call
        // on primitive receivers. Emitted FLAT (below, via `flat_modules`) so
        // the `impl str` self-type binds to the primitive, not a `str` module.
        // `cmp.g` (core). Supplies the free functions `min`/`max` that
        // `io.g` calls by bare name, plus the comparison traits
        // (`Ord`/`PartialOrd`/`Eq`/`PartialEq`).
        "cmp" => Some(include_str!("../../glyim-lang-core/lib/cmp.g")),
        // Panic-family macros (`panic!`, `assert!`, `assert_eq!`, …) live in
        // core's `panic.g`. Without this arm they are not part of the
        // assembled stdlib, so `panic!(..)` in e.g. time.g's
        // `Instant::duration_since` fails to expand and its match arm is
        // dropped ("non-exhaustive match: missing variants None").
        "panic" => Some(include_str!("../../glyim-lang-core/lib/panic.g")),
        // Core extension-method impls. `impl str` / `impl<T> [T]` define the
        // `as_ptr`/`len`/`to_string`/`iter_mut` methods the std sources call
        // on primitive receivers. Emitted FLAT (below, via `flat_modules`).
        "str" => Some(include_str!("../../glyim-lang-core/lib/str.g")),
        "slice" => Some(include_str!("../../glyim-lang-core/lib/slice.g")),
        // --- core module additions (prelude surface) ---
        "option" => Some(include_str!("../../glyim-lang-core/lib/option.g")),
        "result" => Some(include_str!("../../glyim-lang-core/lib/result.g")),
        "iter" => Some(include_str!("../../glyim-lang-core/lib/iter.g")),
        "ops" => Some(include_str!("../../glyim-lang-core/lib/ops.g")),
        "default" => Some(include_str!("../../glyim-lang-core/lib/default.g")),
        "mem" => Some(include_str!("../../glyim-lang-core/lib/mem.g")),
        "ptr" => Some(include_str!("../../glyim-lang-core/lib/ptr.g")),
        "cell" => Some(include_str!("../../glyim-lang-core/lib/cell.g")),
        "marker" => Some(include_str!("../../glyim-lang-core/lib/marker.g")),
        "convert" => Some(include_str!("../../glyim-lang-core/lib/convert.g")),
        "hint" => Some(include_str!("../../glyim-lang-core/lib/hint.g")),
        // --- alloc module additions (prelude surface) ---
        "vec" => Some(include_str!("../../glyim-lang-alloc/lib/vec.g")),
        "boxed" => Some(include_str!("../../glyim-lang-alloc/lib/boxed.g")),
        "rc" => Some(include_str!("../../glyim-lang-alloc/lib/rc.g")),
        "string" => Some(include_str!("../../glyim-lang-alloc/lib/string.g")),
        "raw_vec" => Some(include_str!("../../glyim-lang-alloc/lib/raw_vec.g")),
        "alloc" => Some(include_str!("../../glyim-lang-alloc/lib/alloc.g")),

        _ => None,
    }
}

/// Returns the names of all standard library modules.
pub fn std_modules() -> &'static [&'static str] {
    &[
        "io", "fs", "net", "thread", "sync", "env", "time", "process",
    ]
}

/// Returns the combined source of all standard library modules.
pub fn std_source_all() -> String {
    let mut out = String::new();
    for name in std_modules() {
        if let Some(src) = std_source(name) {
            out.push_str(&format!("// === module: {} ===\n", name));
            out.push_str(src);
            out.push('\n');
        }
    }
    out
}

/// Assemble the standard library as a *modular* crate (Option A of the
/// stdlib-compile plan): each `.g` module becomes an inline `pub mod X { … }`
/// with its items made `pub`, and the crate root re-exports every public item
/// via `pub use X::name;` so that both the bare references the stdlib uses
/// internally (`Read`, `Write`, `Instant`, …) and the qualified `std::`-style
/// paths (`time::Instant`, `io::Read`) resolve.
///
/// This is the structure the language expects: the crate doc comments refer to
/// `std::io` / `std::time` / `std::net`, and the modules reference each other
/// both by bare name and by `module::Item` paths. A flat concatenation (what
/// `std_source_all` produced) cannot satisfy the qualified paths, and wrapping
/// modules without root re-exports cannot satisfy the bare names — so both
/// mechanisms are required.
///
/// Glob re-exports (`use io::*`) are not supported by the resolver, so each
/// module's public items are listed explicitly in `MODULE_PUBS`.
pub fn std_source_assembled() -> String {
    // (name, [item, ...]) — emitted as `pub mod name { … }` + `pub use name::item;`
    // per item, so bare names resolve at the crate root.
    let pubs: &[(&str, &[&str])] = &[
        // Script 259: dependency order. The typeck walker visits modules in
        // source order and registers an impl method's `body_owner_map`
        // entry only when it *reaches* that impl's module. `boxed`/`vec`/
        // `raw_vec`/`string`/`rc` reference `alloc::Layout` methods and
        // must therefore appear AFTER `alloc` and `raw_vec` in the emitted
        // source. Before this reorder, `Box::new`'s body ran before
        // `alloc::Layout::from_size_align` had been registered, producing
        // cascading `.expect()` "no method" errors on the resulting
        // `Error` receiver.
        ("option", &["Option"]),
        ("result", &["Result"]),
        ("iter", &[
            "Iterator", "IntoIterator", "FromIterator",
            "Map", "Filter", "Enumerate", "Skip", "Take", "Zip", "Chain",
        ]),
        ("ops", &[
            "Deref", "DerefMut", "Drop",
            "Fn", "FnMut", "FnOnce",
            "Add", "Sub", "Mul", "Div", "Rem", "Neg", "Not",
            "Index", "IndexMut",
        ]),
        ("default", &["Default"]),
        ("mem", &[
            "size_of", "size_of_val", "align_of", "align_of_val",
            "replace", "swap", "take", "forget", "drop",
        ]),
        ("ptr", &["read", "write", "drop_in_place"]),
        ("cell", &["Cell", "RefCell", "UnsafeCell"]),
        ("marker", &["Sized", "Send", "Sync", "Unpin", "Copy", "PhantomData"]),
        ("convert", &["From", "Into", "TryFrom", "TryInto", "AsRef", "AsMut"]),
        ("hint", &["black_box", "spin_loop"]),
        // --- allocator layer (dependencies of vec/boxed/rc/string) ---
        ("alloc", &["GlobalAlloc", "Layout", "GLOBAL", "handle_alloc_error"]),
        ("raw_vec", &["RawVec"]),
        // --- allocator consumers ---
        ("vec", &["Vec"]),
        ("boxed", &["Box"]),
        ("rc", &["Rc"]),
        ("string", &["String"]),
        ("io", &[
            "Read", "Write", "BufRead", "Error", "ErrorKind",
            "Stdin", "Stdout", "Stderr",
            "empty_reader", "stdin", "stdout", "stderr",
            "println", "print", "eprintln", "eprint",
                "RepeatBytes",
            ]),
        ("fs", &[
            "File", "OpenOption", "read_to_string", "write_to_file",
            "FileType", "Metadata", "DirEntry", "read_dir",
        ]),
        ("net", &[
            "TcpStream", "TcpListener", "UdpSocket",
            "IpAddr", "Ipv4Addr", "Ipv6Addr",
            "SocketAddr", "SocketAddrV4", "SocketAddrV6",
            "ToSocketAddrs", "resolve", "connect", "bind",
        ]),
        ("thread", &[
            "Thread", "ThreadId", "spawn", "sleep", "JoinHandle", "yield_now",
        ]),
        ("sync", &[
            "Mutex", "RwLock", "Arc",
            "AtomicBool", "AtomicI32", "AtomicU32", "AtomicUsize",
            "Condvar", "Barrier",
                "OnceLock",
            ]),
        ("env", &[
            "args", "var", "set_var", "current_dir",
            "temp_dir", "home_dir", "args_os",
        ]),
        ("time", &["Duration", "Instant", "SystemTime", "UNIX_EPOCH"]),
        ("process", &["Command", "Child", "Stdio", "exit", "id"]),
        ("future", &["Poll", "Waker", "Context", "Future"]),
        ("cmp", &[
            "min", "max",
            "Ord", "PartialOrd", "Eq", "PartialEq",
            "Reverse",
        ]),
        ("panic", &["panic_any"]),
    ];
    // `str` and `slice` are emitted FLAT: their `impl str { .. }` /
    // `impl<T> [T] { .. }` extension blocks must bind to the primitive types,
    // not a module named `str` / `slice`.
    let flat_modules: &[&str] = &["str", "slice"];
    let mut out = String::new();
    out.push_str("// Assembled modular glyim standard library (Option A).\n");
    for name in flat_modules {
        if let Some(src) = std_source(name) {
            let pub_src = make_pub(src);
            out.push_str(&pub_src);
            out.push('\n');
        }
    }
    for (name, _items) in pubs {
        if let Some(src) = std_source(name) {
            let pub_src = make_pub(src);
            out.push_str(&format!("pub mod {name} {{\n"));
            out.push_str(&pub_src);
            out.push_str("\n}\n\n");
        }
    }
    out.push_str("// Root re-exports so bare names resolve crate-wide.\n");
    for (name, items) in pubs {
        for item in *items {
            out.push_str(&format!("pub use {name}::{item};\n"));
        }
    }
    out.push('\n');
    out
}

/// Assemble a **minimal** standard library crate containing only the modules
/// required for a "hello world" program (and basic collections + I/O). This
/// excludes `net`, `fs`, `thread`, `sync`, `env`, `time`, `process`, which
/// currently have compiler-feature gaps that block the full assembled stdlib
/// from compiling cleanly. `--with-stdlib` uses this variant by default so a
/// user's first program type-checks and runs; the full stdlib is opt-in via
/// `--with-full-stdlib` until those remaining gaps are closed.
pub fn std_source_assembled_minimal() -> String {
    let pubs: &[(&str, &[&str])] = &[
        ("option", &["Option"]),
        ("result", &["Result"]),
        ("iter", &[
            "Iterator", "IntoIterator", "FromIterator",
            "Map", "Filter", "Enumerate", "Skip", "Take", "Zip", "Chain",
        ]),
        ("ops", &[
            "Deref", "DerefMut", "Drop",
            "Fn", "FnMut", "FnOnce",
            "Add", "Sub", "Mul", "Div", "Rem", "Neg", "Not",
            "Index", "IndexMut",
        ]),
        ("default", &["Default"]),
        ("mem", &[
            "size_of", "size_of_val", "align_of", "align_of_val",
            "replace", "swap", "take", "forget", "drop",
        ]),
        ("ptr", &["read", "write", "drop_in_place", "null", "null_mut"]),
        ("cell", &["Cell", "RefCell", "UnsafeCell"]),
        ("marker", &["Sized", "Send", "Sync", "Unpin", "Copy", "PhantomData"]),
        ("convert", &["From", "Into", "TryFrom", "TryInto", "AsRef", "AsMut"]),
        ("hint", &["black_box", "spin_loop"]),
        // Script 434: dependency order — same reorder as the FULL
        // assembler got in Script 259. `alloc`/`raw_vec` must come before
        // their consumers (`vec`, `boxed`, `rc`, `string`) or the typeck
        // walker registers `Layout`'s impl after `Box::new`'s body, which
        // produces the 7 `no method expect` errors the CLI test hits.
        ("alloc", &["GlobalAlloc", "Layout", "GLOBAL", "handle_alloc_error"]),
        ("raw_vec", &["RawVec"]),
        ("vec", &["Vec"]),
        ("boxed", &["Box"]),
        ("rc", &["Rc"]),
        ("string", &["String"]),
        ("io", &[
            "Read", "Write", "BufRead", "Error", "ErrorKind",
            "Stdin", "Stdout", "Stderr",
            "empty_reader", "stdin", "stdout", "stderr",
            "println", "print", "eprintln", "eprint",
            "RepeatBytes",
        ]),
        ("future", &["Poll", "Waker", "Context", "Future"]),
        ("cmp", &[
            "min", "max",
            "Ord", "PartialOrd", "Eq", "PartialEq",
            "Reverse",
        ]),
        ("panic", &["panic_any"]),
    ];
    // `str` and `slice` emitted FLAT.
    let flat_modules: &[&str] = &["str", "slice"];
    let mut out = String::new();
    out.push_str("// Assembled MINIMAL glyim standard library (Script 82).\n");
    for name in flat_modules {
        if let Some(src) = std_source(name) {
            let pub_src = make_pub(src);
            out.push_str(&pub_src);
            out.push('\n');
        }
    }
    for (name, _items) in pubs {
        if let Some(src) = std_source(name) {
            let pub_src = make_pub(src);
            out.push_str(&format!("pub mod {name} {{\n"));
            out.push_str(&pub_src);
            out.push_str("\n}\n\n");
        }
    }
    out.push_str("// Root re-exports so bare names resolve crate-wide.\n");
    for (name, items) in pubs {
        for item in *items {
            out.push_str(&format!("pub use {name}::{item};\n"));
        }
    }
    out.push('\n');
    out
}

/// Make every top-level (module-level, brace-depth-0) item declaration in
/// `src` `pub`. Items nested inside `trait`/`impl`/`struct`/`enum`/`extern`
/// bodies are at a deeper brace depth and are left untouched — only the
/// leading keyword of a module-level `fn`/`struct`/`enum`/`trait`/`type`/`const`/
/// `static` item gains a `pub` if it does not already have one.
fn make_pub(src: &str) -> String {
    let item_kws = [
        "fn ", "struct ", "enum ", "trait ", "type ", "const ", "static ",
    ];
    let mut out = String::with_capacity(src.len());
    let mut depth: i32 = 0;
    // `impl_depth`: when > 0 we are inside an `impl { ... }` block. `pub` on
    // impl methods is a Rust-ism that Glyim's parser does not accept
    // ("expected impl item, found KwPub"), so we never add `pub` there.
    let mut impl_depth: i32 = -1;
    for line in src.lines() {
        let trimmed = line.trim_start();
        let is_item = item_kws.iter().any(|kw| trimmed.starts_with(kw));
        let already_pub = trimmed.starts_with("pub ") || trimmed.starts_with("pub(");
        // Detect the start of an `impl` block on this line (the `impl` may
        // carry generics and a trait path before the opening `{`).
        let line_starts_impl = trimmed.starts_with("impl ")
            || trimmed.starts_with("impl<")
            || trimmed.starts_with("impl{");
        // Determine whether `pub` should be injected: only when we are at the
        // top of the module (depth <= 0) AND not inside an impl block.
        let inside_impl = impl_depth >= 0 && depth > impl_depth;
        if depth <= 0 && is_item && !already_pub && !trimmed.starts_with("//") {
            let indent = line.len() - line.trim_start_matches([' ', '\t']).len();
            out.push_str(&line[..indent]);
            out.push_str("pub ");
            out.push_str(&line[indent..]);
        } else {
            out.push_str(line);
        }
        // Update brace depth.
        for ch in line.chars() {
            if ch == '{' {
                depth += 1;
            } else if ch == '}' {
                depth -= 1;
                // If we just closed the impl block, clear the marker.
                if impl_depth >= 0 && depth == impl_depth {
                    impl_depth = -1;
                }
            }
        }
        // If this line opened an `impl` block, record its depth (the depth
        // *after* processing this line's braces).
        if line_starts_impl && impl_depth < 0 {
            impl_depth = depth;
        }
        let _ = inside_impl;
        out.push('\n');
    }
    out
}

/// Returns the total number of standard library modules.
pub fn std_module_count() -> usize {
    std_modules().len()
}

#[cfg(test)]
mod tests;
