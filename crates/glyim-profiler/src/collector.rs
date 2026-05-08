use crate::profile::{
    CompilationProfile, IncrementalProfile, MemoryProfile, StageName, StageProfile,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

thread_local! {
    static COLLECTOR: RefCell<ProfileCollector> = RefCell::new(ProfileCollector::new());
}

pub struct ProfileCollector {
    profile: CompilationProfile,
    stage_starts: HashMap<StageName, Instant>,
    enabled: bool,
}

impl Default for ProfileCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl ProfileCollector {
    pub fn new() -> Self {
        Self {
            profile: CompilationProfile {
                id: 0,
                started_at: chrono::Utc::now(),
                total_duration: Duration::ZERO,
                stages: HashMap::new(),
                memory: MemoryProfile {
                    peak_rss: 0,
                    total_allocated: 0,
                    total_freed: 0,
                    allocation_count: 0,
                    deallocation_count: 0,
                },
                incremental: IncrementalProfile {
                    red_items: 0,
                    green_items: 0,
                    total_items: 0,
                    cache_hit_ratio: 0.0,
                },
            },
            stage_starts: HashMap::new(),
            enabled: false,
        }
    }

    pub fn enable() {
        COLLECTOR.with(|c| c.borrow_mut().enabled = true);
    }

    pub fn disable() {
        COLLECTOR.with(|c| c.borrow_mut().enabled = false);
    }

    #[inline]
    pub fn enter_stage(stage: StageName) {
        COLLECTOR.with(|c| {
            let mut c = c.borrow_mut();
            if !c.enabled {
                return;
            }
            c.stage_starts.insert(stage, Instant::now());
        });
    }

    #[inline]
    pub fn exit_stage(stage: StageName, items: usize, hits: usize, misses: usize) {
        COLLECTOR.with(|c| {
            let mut c = c.borrow_mut();
            if !c.enabled {
                return;
            }
            if let Some(start) = c.stage_starts.remove(&stage) {
                let duration: Duration = start.elapsed();
                c.profile.stages.insert(
                    stage,
                    StageProfile {
                        duration,
                        items_processed: items,
                        cache_hits: hits,
                        cache_misses: misses,
                        bytes_allocated: 0,
                        skipped: false,
                    },
                );
            }
        });
    }

    pub fn skip_stage(stage: StageName) {
        COLLECTOR.with(|c| {
            let mut c = c.borrow_mut();
            if !c.enabled {
                return;
            }
            c.profile.stages.insert(
                stage,
                StageProfile {
                    duration: Duration::ZERO,
                    items_processed: 0,
                    cache_hits: 0,
                    cache_misses: 0,
                    bytes_allocated: 0,
                    skipped: true,
                },
            );
        });
    }

    pub fn finish() -> CompilationProfile {
        COLLECTOR.with(|c| {
            let mut c = c.borrow_mut();
            c.profile.total_duration =
                Instant::now().duration_since(Instant::now() - c.profile.total_duration);
            c.profile.clone()
        })
    }

    pub fn set_incremental(red: usize, green: usize) {
        COLLECTOR.with(|c| {
            let mut c = c.borrow_mut();
            let total = red + green;
            c.profile.incremental.red_items = red;
            c.profile.incremental.green_items = green;
            c.profile.incremental.total_items = total;
            c.profile.incremental.cache_hit_ratio = if total > 0 {
                green as f64 / total as f64
            } else {
                0.0
            };
        });
    }
}
