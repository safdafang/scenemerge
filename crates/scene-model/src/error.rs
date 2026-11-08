//! Errors produced while building the semantic model.

use std::path::{Path, PathBuf};

use godot_text::ParseError;

/// An error building a [`crate::SceneFile`].
#[derive(Debug, Clone, thiserror::Error)]
pub enum ModelError {
    /// The text could not be parsed.
    #[error("{0}")]
    Parse(ParseError),
    /// The file uses a format version other than 3.
    #[error("{}unsupported file format {format}: {message}", path_prefix(.path).replace(':', ": "))]
    UnsupportedFormat {
        /// The `format=` value found.
        format: i64,
        /// Explanation.
        message: String,
        /// File path, if known.
        path: Option<PathBuf>,
    },
    /// The sections do not form a valid scene/resource.
    #[error("{}{line}: {message}", path_prefix(.path))]
    Structure {
        /// 1-based line of the offending section.
        line: usize,
        /// Explanation.
        message: String,
        /// File path, if known.
        path: Option<PathBuf>,
    },
}

fn path_prefix(p: &Option<PathBuf>) -> String {
    p.as_ref()
        .map(|p| format!("{}:", p.display()))
        .unwrap_or_default()
}

impl ModelError {
    pub(crate) fn structure(line: usize, message: impl Into<String>) -> Self {
        ModelError::Structure {
            line,
            message: message.into(),
            path: None,
        }
    }

    /// Attaches the file path to the error.
    #[must_use]
    pub fn with_path(self, p: impl AsRef<Path>) -> Self {
        let path = p.as_ref().to_path_buf();
        let p = Some(path.clone());
        match self {
            ModelError::Parse(e) => ModelError::Parse(e.with_path(path)),
            ModelError::UnsupportedFormat {
                format, message, ..
            } => ModelError::UnsupportedFormat {
                format,
                message,
                path: p,
            },
            ModelError::Structure { line, message, .. } => ModelError::Structure {
                line,
                message,
                path: p,
            },
        }
    }
}
