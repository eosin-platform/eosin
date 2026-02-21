use prometheus::{CounterVec, HistogramVec, register_counter_vec, register_histogram_vec};

/// Contains the metrics for a controller. Each controller will use
/// unique metric names, but they will use these same metric types.
pub struct ControllerMetrics {
    /// Number of reconciliations by the controller.
    pub reconcile_counter: CounterVec,

    /// Number of actions taken by the controller.
    pub action_counter: CounterVec,

    /// Read phase latency of the controller.
    pub read_histogram: HistogramVec,

    /// Write phase latency of the controller.
    pub write_histogram: HistogramVec,
}

impl ControllerMetrics {
    /// Creates a new set of metrics for a controller. The tag is used
    /// to associate the metrics with a specific controller.
    pub fn new(tag: &str) -> Self {
        let pre = format!("{}_{}", prefix(), tag);
        let reconcile_counter = register_counter_vec!(
            &format!("{}_reconcile_counter", pre),
            "Number of reconciliations by the controller.",
            &["name", "namespace"]
        )
        .unwrap();
        let action_counter = register_counter_vec!(
            &format!("{}_action_counter", pre),
            "Number of actions taken by the controller.",
            &["name", "namespace", "action"]
        )
        .unwrap();
        let read_histogram = register_histogram_vec!(
            &format!("{}_read_duration_seconds", pre),
            "Read phase latency of the controller.",
            &["name", "namespace", "action"]
        )
        .unwrap();
        let write_histogram = register_histogram_vec!(
            &format!("{}_write_duration_seconds", pre),
            "Write phase latency of the controller.",
            &["name", "namespace", "action"]
        )
        .unwrap();
        ControllerMetrics {
            reconcile_counter,
            action_counter,
            read_histogram,
            write_histogram,
        }
    }
}

/// Returns the metrics prefix, which can be overridden with the
/// METRICS_PREFIX environment variable.
pub fn prefix() -> String {
    std::env::var("METRICS_PREFIX").unwrap_or_else(|_| "vpno".to_string())
}
