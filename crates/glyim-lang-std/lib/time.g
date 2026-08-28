//! Temporal quantification for the Glyim standard library.
//!
//! This module provides types for measuring and working with time.

/// A `Duration` type to represent a span of time.
struct Duration {
    secs: u64,
    nanos: u32,
}

impl Duration {
    /// The minimum duration.
    const MIN: Duration = Duration { secs: 0, nanos: 0 };

    /// The maximum duration.
    const MAX: Duration = Duration { secs: u64::MAX, nanos: 999_999_999 };

    /// Create a new `Duration` from the specified number of seconds and nanoseconds.
    fn new(secs: u64, nanos: u32) -> Duration {
        let secs = secs + (nanos / 1_000_000_000) as u64;
        let nanos = nanos % 1_000_000_000;
        Duration { secs, nanos }
    }

    /// Create a `Duration` from the specified number of seconds.
    fn from_secs(secs: u64) -> Duration {
        Duration { secs, nanos: 0 }
    }

    /// Create a `Duration` from the specified number of milliseconds.
    fn from_millis(millis: u64) -> Duration {
        Duration {
            secs: millis / 1_000,
            nanos: ((millis % 1_000) * 1_000_000) as u32,
        }
    }

    /// Create a `Duration` from the specified number of microseconds.
    fn from_micros(micros: u64) -> Duration {
        Duration {
            secs: micros / 1_000_000,
            nanos: ((micros % 1_000_000) * 1_000) as u32,
        }
    }

    /// Create a `Duration` from the specified number of nanoseconds.
    fn from_nanos(nanos: u64) -> Duration {
        Duration {
            secs: nanos / 1_000_000_000,
            nanos: (nanos % 1_000_000_000) as u32,
        }
    }

    /// Returns the number of whole seconds contained by this `Duration`.
    fn as_secs(&self) -> u64 {
        self.secs
    }

    /// Returns the fractional part of this `Duration` in milliseconds.
    fn subsec_millis(&self) -> u32 {
        self.nanos / 1_000_000
    }

    /// Returns the fractional part of this `Duration` in microseconds.
    fn subsec_micros(&self) -> u32 {
        self.nanos / 1_000
    }

    /// Returns the fractional part of this `Duration` in nanoseconds.
    fn subsec_nanos(&self) -> u32 {
        self.nanos
    }

    /// Returns the total number of whole milliseconds contained by this `Duration`.
    fn as_millis(&self) -> u128 {
        self.secs as u128 * 1_000 + self.nanos as u128 / 1_000_000
    }

    /// Returns the total number of whole microseconds contained by this `Duration`.
    fn as_micros(&self) -> u128 {
        self.secs as u128 * 1_000_000 + self.nanos as u128 / 1_000
    }

    /// Returns the total number of nanoseconds contained by this `Duration`.
    fn as_nanos(&self) -> u128 {
        self.secs as u128 * 1_000_000_000 + self.nanos as u128
    }

    /// Checked `Duration` addition. Computes `self + other`, returning `None` if overflow occurred.
    fn checked_add(self, other: Duration) -> Option<Duration> {
        let secs = self.secs.checked_add(other.secs)?;
        let nanos = self.nanos + other.nanos;
        if nanos >= 1_000_000_000 {
            let secs = secs.checked_add(1)?;
            Option::Some(Duration { secs, nanos: nanos - 1_000_000_000 })
        } else {
            Option::Some(Duration { secs, nanos })
        }
    }

    /// Checked `Duration` subtraction. Computes `self - other`, returning `None` if overflow occurred.
    fn checked_sub(self, other: Duration) -> Option<Duration> {
        if self.secs < other.secs || (self.secs == other.secs && self.nanos < other.nanos) {
            Option::None
        } else if self.nanos >= other.nanos {
            Option::Some(Duration { secs: self.secs - other.secs, nanos: self.nanos - other.nanos })
        } else {
            Option::Some(Duration { secs: self.secs - other.secs - 1, nanos: self.nanos + 1_000_000_000 - other.nanos })
        }
    }

    /// Multiply `Duration` by a scalar.
    fn mul(self, rhs: u32) -> Duration {
        let total_nanos = self.as_nanos() * rhs as u128;
        Duration {
            secs: (total_nanos / 1_000_000_000) as u64,
            nanos: (total_nanos % 1_000_000_000) as u32,
        }
    }

    /// Divide `Duration` by a scalar.
    fn div(self, rhs: u32) -> Duration {
        let total_nanos = self.as_nanos() / rhs as u128;
        Duration {
            secs: (total_nanos / 1_000_000_000) as u64,
            nanos: (total_nanos % 1_000_000_000) as u32,
        }
    }

    /// Returns `true` if this `Duration` spans no time.
    fn is_zero(&self) -> bool {
        self.secs == 0 && self.nanos == 0
    }
}

impl Default for Duration {
    fn default() -> Duration {
        Duration { secs: 0, nanos: 0 }
    }
}

/// A measurement of a monotonically non-decreasing clock.
struct Instant {
    secs: u64,
    nanos: u32,
}

impl Instant {
    /// Returns an instant corresponding to "now".
    fn now() -> Instant {
        extern "C" {
            fn glyim_time_now_secs() -> u64;
            fn glyim_time_now_nanos() -> u32;
        }
        let secs = unsafe { glyim_time_now_secs() };
        let nanos = unsafe { glyim_time_now_nanos() };
        Instant { secs, nanos }
    }

    /// Returns the amount of time elapsed since this instant was created.
    fn elapsed(&self) -> Duration {
        self.diff(&Instant::now())
    }

    /// Returns the amount of time elapsed from another instant to this one,
    /// or zero if that instant is later than this one.
    fn duration_since(&self, earlier: &Instant) -> Duration {
        self.diff(earlier)
    }

    /// Returns `Some(t)` where `t` is the time `self + duration` if `t` can be represented.
    fn checked_add(&self, duration: Duration) -> Option<Instant> {
        let secs = self.secs.checked_add(duration.secs)?;
        Option::Some(Instant { secs, nanos: self.nanos + duration.nanos })
    }

    /// Returns `Some(t)` where `t` is the time `self - duration` if `t` can be represented.
    fn checked_sub(&self, duration: Duration) -> Option<Instant> {
        let secs = self.secs.checked_sub(duration.secs)?;
        Option::Some(Instant { secs, nanos: self.nanos.saturating_sub(duration.nanos) })
    }

    fn diff(&self, later: &Instant) -> Duration {
        if later.secs > self.secs || (later.secs == self.secs && later.nanos >= self.nanos) {
            if later.nanos >= self.nanos {
                Duration { secs: later.secs - self.secs, nanos: later.nanos - self.nanos }
            } else {
                Duration { secs: later.secs - self.secs - 1, nanos: later.nanos + 1_000_000_000 - self.nanos }
            }
        } else {
            Duration::default()
        }
    }
}

/// A measurement of the system clock, useful for talking to external entities
/// like the file system or other processes.
struct SystemTime {
    secs: u64,
    nanos: u32,
}

impl SystemTime {
    /// Returns the system time corresponding to "now".
    fn now() -> SystemTime {
        extern "C" {
            fn glyim_time_system_secs() -> u64;
            fn glyim_time_system_nanos() -> u32;
        }
        let secs = unsafe { glyim_time_system_secs() };
        let nanos = unsafe { glyim_time_system_nanos() };
        SystemTime { secs, nanos }
    }

    /// Create a `SystemTime` from secs and nanos since the Unix epoch.
    fn from_secs_nanos(secs: u64, nanos: u32) -> SystemTime {
        SystemTime { secs, nanos }
    }

    /// Returns the amount of time elapsed since this time was created.
    fn elapsed(&self) -> Result<Duration, Duration> {
        let now = SystemTime::now();
        if now.secs > self.secs || (now.secs == self.secs && now.nanos >= self.nanos) {
            Result::Ok(now.diff(self))
        } else {
            Result::Err(self.diff(&now))
        }
    }

    /// Returns `self + duration`.
    fn checked_add(&self, duration: Duration) -> Option<SystemTime> {
        let secs = self.secs.checked_add(duration.secs)?;
        Option::Some(SystemTime { secs, nanos: self.nanos + duration.nanos })
    }

    /// Returns `self - duration`.
    fn checked_sub(&self, duration: Duration) -> Option<SystemTime> {
        let secs = self.secs.checked_sub(duration.secs)?;
        Option::Some(SystemTime { secs, nanos: self.nanos.saturating_sub(duration.nanos) })
    }

    fn diff(&self, earlier: &SystemTime) -> Duration {
        if self.nanos >= earlier.nanos {
            Duration { secs: self.secs - earlier.secs, nanos: self.nanos - earlier.nanos }
        } else {
            Duration { secs: self.secs - earlier.secs - 1, nanos: self.nanos + 1_000_000_000 - earlier.nanos }
        }
    }
}

/// A measurement of a monotonically non-decreasing clock.
///
/// Instants are opaque, ordered values suitable only for measuring the
/// duration between two events. `now` reads a monotonic nanosecond counter
/// from the runtime (`glyim_time_now`), so it never goes backwards even
/// across wall-clock adjustments.
pub struct Instant {
    nanos: u64,
}

impl Instant {
    /// Return an instant corresponding to "now".
    fn now() -> Instant {
        extern "C" {
            fn glyim_time_now() -> u64;
        }
        Instant { nanos: unsafe { glyim_time_now() } }
    }

    /// Seconds component of this instant's raw monotonic counter.
    fn as_secs(&self) -> u64 {
        self.nanos / 1_000_000_000
    }

    /// Nanoseconds component (sub-second) of this instant's raw counter.
    fn subsec_nanos(&self) -> u32 {
        (self.nanos % 1_000_000_000) as u32
    }

    /// Duration elapsed since `earlier`, or `None` if `earlier` is later than
    /// `self` (clock went backwards).
    fn checked_duration_since(&self, earlier: Instant) -> Option<Duration> {
        if self.nanos >= earlier.nanos {
            let diff = self.nanos - earlier.nanos;
            Option::Some(Duration { secs: diff / 1_000_000_000, nanos: (diff % 1_000_000_000) as u32 })
        } else {
            Option::None
        }
    }

    /// Duration elapsed since `earlier`; panics if `earlier` is in the future.
    fn duration_since(&self, earlier: Instant) -> Duration {
        match self.checked_duration_since(earlier) {
            Option::Some(d) => d,
            Option::None => panic!("`Instant::duration_since` called with a later instant"),
        }
    }

    /// Duration elapsed since this instant was created (i.e. `Instant::now() - self`).
    fn elapsed(&self) -> Duration {
        Instant::now().duration_since(*self)
    }
}

const UNIX_EPOCH: SystemTime = SystemTime { secs: 0, nanos: 0 };

/// Returns the current time as a Duration since the UNIX epoch.
fn system_time_since_epoch() -> Duration {
    let now = SystemTime::now();
    Duration { secs: now.secs, nanos: now.nanos }
}
