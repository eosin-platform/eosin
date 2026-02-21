#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Kubernetes reported error: {source}")]
    Kube {
        #[from]
        source: kube::Error,
    },

    #[error("Invalid user input: {0}")]
    UserInput(String),

    #[error("Failed to parse DateTime: {source}")]
    Chrono {
        #[from]
        source: chrono::ParseError,
    },

    #[error("Out of range: {source}")]
    OutOfRange {
        #[from]
        source: chrono::OutOfRangeError,
    },

    #[error("Json error: {source}")]
    Json {
        #[from]
        source: serde_json::Error,
    },

    #[error("Parse duration: {source}")]
    ParseDuration {
        #[from]
        source: parse_duration::parse::Error,
    },
}
