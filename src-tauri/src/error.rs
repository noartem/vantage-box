use serde::{Serialize, Serializer};

/// Ошибки, которые могут долететь до фронтенда. Сериализуются в строку —
/// UI показывает их как есть, поэтому текст пишем человекочитаемый.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("не удалось прочитать/записать файл {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("{path}: некорректный JSON — {source}")]
    Parse {
        path: String,
        #[source]
        source: serde_json::Error,
    },

    #[error("Clash API недоступен: {0}")]
    Transport(String),

    #[error("Clash API вернул {status}: {message}")]
    Api { status: u16, message: String },

    #[error("не удалось определить директорию настроек для этой ОС")]
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
