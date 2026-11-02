use thiserror::Error;

/// Errors that stop a load, a resolve, or a slice.
/// Formula failures are not in this type: those fall back to a setting's
/// `default_value` and are reported as warnings on the resolved profile.
#[derive(Debug, Error)]
pub enum Error {
    #[error("{0}")]
    Message(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error in {path}: {source}")]
    Json {
        path: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("printer '{0}' was not found in the Cura definitions")]
    UnknownPrinter(String),
    #[error("{0}")]
    Engine(String),
    #[error("{0}")]
    Mesh(String),
}

impl Error {
    pub fn message(text: impl Into<String>) -> Self {
        Self::Message(text.into())
    }
}
