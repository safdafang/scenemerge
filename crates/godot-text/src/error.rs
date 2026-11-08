use std::fmt;
use std::path::{Path, PathBuf};

/// An error produced while reading a Godot text file.
///
/// The error carries the 1-based line and column (in characters) of the
/// offending input, the text of that line and, optionally, the file path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// Human-readable description of the problem.
    pub message: String,
    /// Byte offset into the (BOM-less) source text.
    pub offset: usize,
    /// 1-based line number.
    pub line: usize,
    /// 1-based column number, counted in Unicode scalar values.
    pub column: usize,
    /// The full text of the offending line (without the line terminator).
    pub snippet: String,
    /// Path of the file being parsed, if known.
    pub path: Option<PathBuf>,
}

impl ParseError {
    /// Creates an error located at `offset` in `src`.
    pub(crate) fn at(src: &str, offset: usize, message: impl Into<String>) -> Self {
        let offset = clamp_to_char_boundary(src, offset);
        let before = src.get(..offset).unwrap_or("");
        let line = before.matches('\n').count() + 1;
        let line_start = before.rfind('\n').map_or(0, |i| i + 1);
        let column = src.get(line_start..offset).map_or(0, |s| s.chars().count()) + 1;
        let rest = src.get(line_start..).unwrap_or("");
        let line_end = rest.find('\n').unwrap_or(rest.len());
        let snippet = rest
            .get(..line_end)
            .unwrap_or("")
            .trim_end_matches('\r')
            .to_string();
        ParseError {
            message: message.into(),
            offset,
            line,
            column,
            snippet,
            path: None,
        }
    }

    /// Attaches a file path to the error (used in the `Display` output).
    #[must_use]
    pub fn with_path(mut self, path: impl AsRef<Path>) -> Self {
        self.path = Some(path.as_ref().to_path_buf());
        self
    }
}

fn clamp_to_char_boundary(src: &str, mut offset: usize) -> usize {
    if offset > src.len() {
        return src.len();
    }
    while offset > 0 && !src.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.path {
            Some(p) => write!(
                f,
                "{}:{}:{}: {}",
                p.display(),
                self.line,
                self.column,
                self.message
            )?,
            None => write!(f, "{}:{}: {}", self.line, self.column, self.message)?,
        }
        let chars: Vec<char> = self.snippet.chars().collect();
        let col0 = self.column.saturating_sub(1).min(chars.len());
        let start = col0.saturating_sub(60);
        let end = (col0 + 40).min(chars.len());
        let shown: String = chars.get(start..end).unwrap_or(&[]).iter().collect();
        let prefix = if start > 0 { "…" } else { "" };
        let suffix = if end < chars.len() { "…" } else { "" };
        write!(f, "\n  | {prefix}{shown}{suffix}")?;
        let pad = col0 - start + prefix.chars().count();
        write!(f, "\n  | {}^", " ".repeat(pad))
    }
}

impl std::error::Error for ParseError {}
