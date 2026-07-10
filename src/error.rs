use std::io;
use std::path::PathBuf;

use thiserror::Error;

/// Errors returned by the streaming textcon engine.
#[derive(Debug, Error)]
pub enum TextconError {
    /// Invalid engine or selection configuration.
    #[error("invalid configuration: {0}")]
    Config(String),

    /// Invalid reference syntax in the input template.
    #[error("template byte {offset}: {message}")]
    TemplateSyntax { offset: u64, message: String },

    /// A reference was denied by the configured sandbox.
    #[error("sandbox denied reference {path}: {reason}")]
    SandboxDenied { path: PathBuf, reason: String },

    /// A path had an unsupported filesystem type.
    #[error("unsupported filesystem object: {path}")]
    UnsupportedFileType { path: PathBuf },

    /// A contextual filesystem operation failed.
    #[error("cannot {operation} {path}: {source}")]
    PathIo {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    /// Reading a generic input stream failed.
    #[error("cannot read {name}: {source}")]
    Input {
        name: String,
        #[source]
        source: io::Error,
    },

    /// Writing the caller-provided output stream failed.
    #[error("cannot write output: {0}")]
    Output(#[source] io::Error),

    /// An ignore rule could not be parsed.
    #[error("invalid ignore rule in {origin}: {message}")]
    Ignore { origin: String, message: String },
}

impl TextconError {
    pub(crate) fn path_io(
        operation: &'static str,
        path: impl Into<PathBuf>,
        source: io::Error,
    ) -> Self {
        Self::PathIo {
            operation,
            path: path.into(),
            source,
        }
    }

    pub(crate) const fn output(source: io::Error) -> Self {
        Self::Output(source)
    }

    /// Returns true only for a broken caller-provided output stream.
    #[must_use]
    pub fn is_output_broken_pipe(&self) -> bool {
        matches!(self, Self::Output(error) if error.kind() == io::ErrorKind::BrokenPipe)
    }
}

/// Result type used by the textcon library.
pub type Result<T> = std::result::Result<T, TextconError>;
