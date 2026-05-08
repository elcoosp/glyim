use dashmap::DashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CacheGranularity {
    FineGrained,
    Module,
    CoarseGrained,
}

#[derive(Clone, Debug, Default)]
pub struct EditHistory {
    pub recent_edits: Vec<(Instant, std::ops::Range<usize>)>,
    pub edit_count: u32,
    pub edit_concentration: f64,
}

const MAX_RECENT_EDITS: usize = 30;
const CONCENTRATION_FINE_THRESHOLD: f64 = 0.7;
const CONCENTRATION_COARSE_THRESHOLD: f64 = 0.05;
const HIGH_CHURN_THRESHOLD: u32 = 5;

pub struct GranularityMonitor {
    edit_history: DashMap<PathBuf, EditHistory>,
    granularity: DashMap<PathBuf, CacheGranularity>,
}

impl GranularityMonitor {
    pub fn new() -> Self {
        Self {
            edit_history: DashMap::new(),
            granularity: DashMap::new(),
        }
    }

    pub fn observe_edit(
        &self,
        path: &Path,
        range: std::ops::Range<usize>,
    ) {
        let mut history = self.edit_history.entry(path.to_path_buf()).or_default();
        history.recent_edits.push((Instant::now(), range));
        history.edit_count += 1;
        if history.recent_edits.len() > MAX_RECENT_EDITS {
            let excess = history.recent_edits.len() - MAX_RECENT_EDITS;
            history.recent_edits.drain(..excess);
        }
        history.edit_concentration =
            compute_concentration(&history.recent_edits);
        let new_granularity =
            if history.edit_count > HIGH_CHURN_THRESHOLD
                && history.edit_concentration < CONCENTRATION_COARSE_THRESHOLD
            {
                CacheGranularity::CoarseGrained
            } else if history.edit_concentration > CONCENTRATION_FINE_THRESHOLD {
                CacheGranularity::FineGrained
            } else {
                CacheGranularity::Module
            };
        self.granularity
            .insert(path.to_path_buf(), new_granularity);
    }

    pub fn granularity(&self, path: &Path) -> CacheGranularity {
        self.granularity
            .get(path)
            .map(|g| *g.value())
            .unwrap_or(CacheGranularity::Module)
    }

    pub fn edit_history(&self, path: &Path) -> Option<EditHistory> {
        self.edit_history.get(path).map(|h| h.clone())
    }

    pub fn reset(&self, path: &Path) {
        self.edit_history.remove(path);
        self.granularity.remove(path);
    }

    pub fn reset_all(&self) {
        self.edit_history.clear();
        self.granularity.clear();
    }
}

impl Default for GranularityMonitor {
    fn default() -> Self {
        Self::new()
    }
}

fn compute_concentration(
    edits: &[(Instant, std::ops::Range<usize>)],
) -> f64 {
    if edits.len() < 2 {
        return 0.0;
    }
    let centers: Vec<f64> = edits
        .iter()
        .map(|(_, range)| (range.start as f64 + range.end as f64) / 2.0)
        .collect();
    let min_center = centers.iter().cloned().fold(f64::INFINITY, f64::min);
    let max_center = centers.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let span = max_center - min_center;
    if span < 1.0 {
        return 1.0;
    }
    let mean_center = centers.iter().sum::<f64>() / centers.len() as f64;
    let avg_distance: f64 = centers
        .iter()
        .map(|c| (c - mean_center).abs())
        .sum::<f64>()
        / centers.len() as f64;
    let half_span = span / 2.0;
    if half_span < 1.0 {
        return 1.0;
    }
    let conc = 1.0 - (avg_distance / half_span).min(1.0);
    eprintln!("compute_concentration: edges={}, span={:.1}, avg_dist={:.1}, half_span={:.1}, conc={:.3}", edits.len(), span, avg_distance, half_span, conc);
    conc
}
