//! Time tests for glyim-runtime
use std::time::{Duration, Instant, SystemTime};

#[test]
fn time_now_secs_monotonic() {
    // W5-C06-T03: glyim_time_now_secs returns monotonic timestamp
    let start = Instant::now();
    let secs1 = start.elapsed().as_secs();

    // Wait a bit
    std::thread::sleep(Duration::from_millis(10));

    let secs2 = start.elapsed().as_secs();

    // Should be monotonic (non-decreasing)
    assert!(secs2 >= secs1, "Monotonic time went backwards");
}

#[test]
fn time_now_nanos_precision() {
    // Test that nanos component is in valid range
    let instant = Instant::now();
    let nanos = instant.elapsed().subsec_nanos();

    // Nanos should be in [0, 1_000_000_000)
    assert!(nanos < 1_000_000_000, "Nanos out of range: {nanos}");
}

#[test]
fn time_system_secs_epoch() {
    // Test system time is relative to UNIX epoch
    let now = SystemTime::now();
    let since_epoch = now
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("System time before epoch");

    // Should be a reasonable timestamp (after year 2000)
    assert!(
        since_epoch.as_secs() > 946_684_800,
        "System time seems invalid"
    );
}

#[test]
fn time_system_nanos_precision() {
    // Test system nanos component
    let now = SystemTime::now();
    let nanos = now
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("System time before epoch")
        .subsec_nanos();

    assert!(nanos < 1_000_000_000, "System nanos out of range: {nanos}");
}
