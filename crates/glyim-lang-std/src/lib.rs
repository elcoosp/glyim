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
    let pubs: &[(&str, &[&str])] = &[
        ("io", &["Read", "Write", "BufRead", "Error", "ErrorKind", "Stdin", "Stdout", "Stderr", "empty_reader", "stdin", "stdout", "stderr"]),
        ("fs", &["File", "OpenOption", "read_to_string", "write_to_file", "FileType", "Metadata", "DirEntry", "read_dir"]),
        ("net", &["TcpStream", "TcpListener", "UdpSocket", "IpAddr", "Ipv4Addr", "Ipv6Addr", "SocketAddr", "SocketAddrV4", "SocketAddrV6", "ToSocketAddrs", "resolve", "connect", "bind"]),
        ("thread", &["Thread", "ThreadId", "spawn", "sleep", "JoinHandle", "yield_now"]),
        ("sync", &["Mutex", "RwLock", "Arc", "AtomicBool", "AtomicI32", "AtomicU32", "AtomicUsize", "Condvar", "Barrier"]),
        ("env", &["args", "var", "set_var", "current_dir", "temp_dir", "home_dir", "args_os"]),
        ("time", &["Duration", "Instant", "SystemTime", "UNIX_EPOCH"]),
        ("process", &["Command", "Child", "Stdio", "exit", "id"]),
        (
            "future",
            &["Poll", "Waker", "Context", "Future"],
        ),
    ];
    let mut out = String::new();
    out.push_str("// Assembled modular glyim standard library (Option A).\n");
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
    let item_kws = ["fn ", "struct ", "enum ", "trait ", "type ", "const ", "static "];
    let mut out = String::with_capacity(src.len());
    let mut depth: i32 = 0;
    for line in src.lines() {
        // Depth at the *start* of the line determines whether this is a
        // module-level item. Count this line's own braces afterwards so the
        // opening brace of `trait X {` is not counted before the keyword.
        let trimmed = line.trim_start();
        let is_item = item_kws.iter().any(|kw| trimmed.starts_with(kw));
        let already_pub = trimmed.starts_with("pub ") || trimmed.starts_with("pub(");
        if depth <= 0 && is_item && !already_pub && !trimmed.starts_with("//") {
            let indent = line.len() - line.trim_start_matches([' ', '\t']).len();
            out.push_str(&line[..indent]);
            out.push_str("pub ");
            out.push_str(&line[indent..]);
        } else {
            out.push_str(line);
        }
        // Update brace depth for the next line. A `{` after the item keyword
        // opens a body; a `}` closes one.
        for ch in line.chars() {
            if ch == '{' {
                depth += 1;
            } else if ch == '}' {
                depth -= 1;
            }
        }
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