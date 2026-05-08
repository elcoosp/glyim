use crate::granularity::{GranularityMonitor, CacheGranularity};
use std::path::PathBuf;

#[test]
fn default_granularity_is_module() {
    let monitor = GranularityMonitor::new();
    assert_eq!(
        monitor.granularity(&PathBuf::from("main.g")),
        CacheGranularity::Module
    );
}

#[test]
fn concentrated_edits_become_fine_grained() {
    let monitor = GranularityMonitor::new();
    let path = PathBuf::from("main.g");
    for _ in 0..5 {
        monitor.observe_edit(&path, 10..15);
    }
    assert_eq!(
        monitor.granularity(&path),
        CacheGranularity::FineGrained
    );
}

#[test]
fn spread_edits_high_count_become_coarse() {
    let monitor = GranularityMonitor::new();
    let path = PathBuf::from("main.g");
    for i in 0..25 {
        let start = i * 10;
        monitor.observe_edit(&path, start..(start + 3));
    }
    assert_eq!(
        monitor.granularity(&path),
        CacheGranularity::Module
    );
}

#[test]
fn unknown_file_is_module() {
    let monitor = GranularityMonitor::new();
    assert_eq!(
        monitor.granularity(&PathBuf::from("unknown.g")),
        CacheGranularity::Module
    );
}

#[test]
fn edit_history_tracks_count() {
    let monitor = GranularityMonitor::new();
    let path = PathBuf::from("test.g");
    monitor.observe_edit(&path, 1..5);
    monitor.observe_edit(&path, 10..20);
    let history = monitor.edit_history(&path).unwrap();
    assert_eq!(history.edit_count, 2);
}

#[test]
fn reset_clears_history() {
    let monitor = GranularityMonitor::new();
    let path = PathBuf::from("reset.g");
    for _ in 0..5 {
        monitor.observe_edit(&path, 10..15);
    }
    assert_ne!(
        monitor.granularity(&path),
        CacheGranularity::Module
    );
    monitor.reset(&path);
    assert_eq!(
        monitor.granularity(&path),
        CacheGranularity::Module
    );
}

#[test]
fn multiple_files_independent() {
    let monitor = GranularityMonitor::new();
    let a = PathBuf::from("a.g");
    let b = PathBuf::from("b.g");
    for _ in 0..5 {
        monitor.observe_edit(&a, 10..15);
    }
    for i in 0..25 {
        monitor.observe_edit(&b, (i * 10)..(i * 10 + 3));
    }
    assert_eq!(monitor.granularity(&a), CacheGranularity::FineGrained);
    assert_eq!(monitor.granularity(&b), CacheGranularity::Module);
}
