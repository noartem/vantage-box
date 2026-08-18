use serde::{Serialize, Serializer};

/// Errors that can reach the frontend. Serialized to a string — the UI shows
/// them as-is, so the text is written to be human-readable.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed to read/write file {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("{path}: invalid JSON — {source}")]
    Parse {
        path: String,
        #[source]
        source: serde_json::Error,
    },

    #[error("Clash API unavailable: {0}")]
    Transport(String),

    #[error("Clash API returned {status}: {message}")]
    Api { status: u16, message: String },

    #[error("could not determine the settings directory for this OS")]
    NoConfigDir,

    #[error("{0}")]
    Other(String),
}

impl Error {
    pub fn io(path: impl Into<String>, source: std::io::Error) -> Self {
        Error::Io {
            path: path.into(),
            source,
        }
    }

    pub fn parse(path: impl Into<String>, source: serde_json::Error) -> Self {
        Error::Parse {
            path: path.into(),
            source,
        }
    }
}

impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        Error::Transport(e.to_string())
    }
}

impl Serialize for Error {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
