/// names.
pub mod names {
    /// OPS_READY_RECEIVED.
    pub const OPS_READY_RECEIVED: &str = "ops_ready_received";
    /// OPS_APPLIED.
    pub const OPS_APPLIED: &str = "ops_applied";
    /// TURN_PROCESSED.
    pub const TURN_PROCESSED: &str = "turn_processed";
    /// TURN_PANIC.
    pub const TURN_PANIC: &str = "turn_panic";
    /// ORCHESTRATOR_ERROR.
    pub const ORCHESTRATOR_ERROR: &str = "orchestrator_error";
    /// STREAM_COMPLETE.
    pub const STREAM_COMPLETE: &str = "stream_complete";
    /// EXTENSION_ERROR.
    pub const EXTENSION_ERROR: &str = "extension_error";
    /// COMMIT_DECISION.
    pub const COMMIT_DECISION: &str = "commit_decision";
    /// DONE_PIPELINE.
    pub const DONE_PIPELINE: &str = "done_pipeline";
    /// PR_CREATED.
    pub const PR_CREATED: &str = "pr_created";
}

/// Metrics.
pub trait Metrics: Send + Sync {
    /// increment_counter.
    fn increment_counter(&self, name: &str, labels: &[(&str, &str)]);
    /// record_histogram.
    fn record_histogram(&self, name: &str, value: f64, labels: &[(&str, &str)]);
}

/// NoOpMetrics.
pub struct NoOpMetrics;
impl Metrics for NoOpMetrics {
    fn increment_counter(&self, _name: &str, _labels: &[(&str, &str)]) {}
    fn record_histogram(&self, _name: &str, _value: f64, _labels: &[(&str, &str)]) {}
}

/// LoggingMetrics.
pub struct LoggingMetrics;
impl Metrics for LoggingMetrics {
    fn increment_counter(&self, name: &str, labels: &[(&str, &str)]) {
        tracing::debug!(metric = name, labels = ?labels, "counter incremented");
    }
    fn record_histogram(&self, name: &str, value: f64, labels: &[(&str, &str)]) {
        tracing::debug!(metric = name, value, labels = ?labels, "histogram recorded");
    }
}

#[cfg(feature = "prometheus")]
pub mod prometheus_impl {
    use super::Metrics;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// PrometheusMetrics caches all counters and histograms by name.
    /// First call for a name: create, register, cache, increment.
    /// Subsequent calls: cache hit, increment only. No allocation.
    pub struct PrometheusMetrics {
        counters: Mutex<HashMap<String, prometheus::IntCounter>>,
        histograms: Mutex<HashMap<String, prometheus::Histogram>>,
    }

    impl Default for PrometheusMetrics {
        fn default() -> Self {
            Self::new()
        }
    }

    impl PrometheusMetrics {
        pub fn new() -> Self {
            Self {
                counters: Mutex::new(HashMap::new()),
                histograms: Mutex::new(HashMap::new()),
            }
        }
    }

    impl Metrics for PrometheusMetrics {
        fn increment_counter(&self, name: &str, labels: &[(&str, &str)]) {
            let mut cache = self.counters.lock().unwrap();

            if let Some(counter) = cache.get(name) {
                counter.inc();
                return;
            }

            let opts = prometheus::Opts::new(name, name).const_labels(make_const_labels(labels));
            let counter =
                prometheus::IntCounter::with_opts(opts).expect("failed to create counter opts");

            let _ = prometheus::default_registry().register(Box::new(counter.clone()));

            cache.insert(name.to_string(), counter.clone());
            counter.inc();
        }

        fn record_histogram(&self, name: &str, value: f64, labels: &[(&str, &str)]) {
            let mut cache = self.histograms.lock().unwrap();

            if let Some(histo) = cache.get(name) {
                histo.observe(value);
                return;
            }

            let opts =
                prometheus::HistogramOpts::new(name, name).const_labels(make_const_labels(labels));
            let histo =
                prometheus::Histogram::with_opts(opts).expect("failed to create histogram opts");

            let _ = prometheus::default_registry().register(Box::new(histo.clone()));

            cache.insert(name.to_string(), histo.clone());
            histo.observe(value);
        }
    }

    fn make_const_labels(labels: &[(&str, &str)]) -> std::collections::HashMap<String, String> {
        labels
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }
}

/// production_metrics.
pub fn production_metrics() -> Box<dyn Metrics> {
    #[cfg(feature = "prometheus")]
    {
        Box::new(prometheus_impl::PrometheusMetrics::new())
    }
    #[cfg(not(feature = "prometheus"))]
    {
        Box::new(LoggingMetrics)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_noop_and_logging_dont_panic() {
        let n = NoOpMetrics;
        n.increment_counter("x", &[]);
        n.record_histogram("x", 1.0, &[]);
        let l = LoggingMetrics;
        l.increment_counter("x", &[("k", "v")]);
        l.record_histogram("x", 1.0, &[("k", "v")]);
    }
}
