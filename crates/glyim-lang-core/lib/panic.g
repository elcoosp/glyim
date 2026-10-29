//! Panic support for the Glyim core library.

/// The standard panic macro.
/// Panics the current thread with a message.
macro panic! {
    () => {
        panic_any_str("explicit panic")
    },
    ($msg:literal) => {
        panic_any_str($msg)
    },
    ($fmt:literal, $($arg:expr),+) => {
        panic_any_str(format!($fmt, $($arg),+).as_str())
    },
}

/// Panic with any value.
fn panic_any(msg: impl Display) -> ! {
    // Kept for direct callers and for the format!-args panic arm. Note:
    // `format!("{}", msg)` where msg is an `impl Display` generic emits
    // empty today (§3.5.22-family trait-generic format gap). The
    // literal-only macro arms route through `panic_any_str` instead.
    extern "C" {
        fn glyim_panic(msg: *const u8, len: usize) -> !;
    }
    let formatted = format!("{}", msg);
    let bytes = formatted.as_bytes();
    unsafe { glyim_panic(bytes.as_ptr(), bytes.len()) }
}

/// Diverging panic that takes a `&str` directly, bypassing `format!`.
///
/// Used by the literal-only macro arms (`panic!("msg")`, `unreachable!()`,
/// `unimplemented!()`), whose arguments are already `&str` and would
/// otherwise hit the trait-generic-format gap in `panic_any`.
///
/// §3.5.28 fix: previously `panic_any` had an empty body, so panic!-family
/// call sites returned immediately, continued past the panic, and hit a
/// bare `Unreachable` whose generic message masked the user text. Now
/// `panic_any_str` calls `glyim_panic(ptr, len)` which writes
/// `thread 'main' panicked: <msg>\n` and exits 101.
///
/// Registered in glyim-lang-std's well-known fn list so macro expansion
/// can resolve it (same list `panic_any` is in).
fn panic_any_str(msg: &str) -> ! {
    extern "C" {
        fn glyim_panic(msg: *const u8, len: usize) -> !;
    }
    let bytes = msg.as_bytes();
    unsafe { glyim_panic(bytes.as_ptr(), bytes.len()) }
}

/// Asserts that a boolean expression is true at runtime.
macro assert! {
    ($cond:expr) => {
        if !$cond {
            panic!("assertion failed: {}", stringify!($cond));
        }
    },
    ($cond:expr, $($arg:expr),+) => {
        if !$cond {
            panic!("assertion failed: {}", format!($($arg),+));
        }
    },
}

/// Asserts that two expressions are equal to each other.
macro assert_eq! {
    ($left:expr, $right:expr) => {
        if $left != $right {
            panic!("assertion `left == right` failed\n  left: {}\n right: {}", $left, $right);
        }
    },
}

/// Asserts that two expressions are not equal to each other.
macro assert_ne! {
    ($left:expr, $right:expr) => {
        if $left == $right {
            panic!("assertion `left != right` failed\n  left: {}\n right: {}", $left, $right);
        }
    },
}

/// Indicates unimplemented code by panicking.
macro unimplemented! {
    () => {
        panic_any_str("not yet implemented")
    },
}

/// Indicates unreachable code.
macro unreachable! {
    () => {
        panic_any_str("internal error: entered unreachable code")
    },
}
