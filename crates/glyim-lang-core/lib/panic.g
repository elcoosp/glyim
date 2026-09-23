//! Panic support for the Glyim core library.

/// The standard panic macro.
/// Panics the current thread with a message.
macro panic! {
    () => {
        panic_any("explicit panic")
    },
    ($msg:literal) => {
        panic_any($msg)
    },
    ($fmt:literal, $($arg:expr),+) => {
        panic_any(format!($fmt, $($arg),+))
    },
}

/// Panic with any value.
fn panic_any(msg: impl Display) -> ! {
    // compiler intrinsic - aborts execution
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
        panic!("not yet implemented")
    },
}

/// Indicates unreachable code.
macro unreachable! {
    () => {
        panic!("internal error: entered unreachable code")
    },
}
