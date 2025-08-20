use std::io;
use std::path::PathBuf;
use thiserror::Error;

/// Main error type for textcon operations
#[derive(Error, Debug)]
pub enum TextconError {
    /// IO error when reading files or directories
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    /// File not found error with specific path
    #[error("File not found: {path}")]
    FileNotFound { path: PathBuf },

    /// Directory not found error with specific path  
    #[error("Directory not found: {path}")]
    DirectoryNotFound { path: PathBuf },

    /// Invalid reference format in template
    #[error("Invalid reference format: {reference}")]
    InvalidReference { reference: String },

    /// Template parsing error
    #[error("Template parsing error at position {position}: {message}")]
    TemplateParse { position: usize, message: String },

    /// Path traversal security error
    #[error("Path traversal detected (trying to access files outside working directory): {path}")]
    PathTraversal { path: PathBuf },

    /// File size exceeds limit
    #[error("File size exceeds limit of {max_size} bytes: {path} ({size} bytes). Use @!{} to force inclusion.", .path.file_name().and_then(|n| n.to_str()).unwrap_or("file"))]
    FileSizeExceeded {
        path: PathBuf,
        size: u64,
        max_size: u64,
    },

    /// Regex compilation error
    #[error("Regex error: {0}")]
    Regex(#[from] regex::Error),

    /// `WalkDir` error when traversing directories
    #[error("Directory traversal error: {0}")]
    WalkDir(#[from] walkdir::Error),

    /// JSON serialization error
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, TextconError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = TextconError::FileNotFound {
            path: PathBuf::from("/test/file.txt"),
        };
        assert_eq!(format!("{err}"), "File not found: /test/file.txt");

        let err = TextconError::DirectoryNotFound {
            path: PathBuf::from("/test/dir"),
        };
        assert_eq!(format!("{err}"), "Directory not found: /test/dir");

        let err = TextconError::InvalidReference {
            reference: "bad_ref".to_string(),
        };
        assert_eq!(format!("{err}"), "Invalid reference format: bad_ref");

        let err = TextconError::TemplateParse {
            position: 42,
            message: "unexpected token".to_string(),
        };
        assert_eq!(
            format!("{err}"),
            "Template parsing error at position 42: unexpected token"
        );

        let err = TextconError::PathTraversal {
            path: PathBuf::from("/etc/passwd"),
        };
        assert!(format!("{err}").contains("Path traversal detected"));

        let err = TextconError::FileSizeExceeded {
            path: PathBuf::from("large.txt"),
            size: 100_000,
            max_size: 65_536,
        };
        assert!(format!("{err}").contains("65536"));
        assert!(format!("{err}").contains("100000"));
        assert!(format!("{err}").contains("@!large.txt"));
    }

    #[test]
    fn test_error_from_io() {
        let io_err = io::Error::new(io::ErrorKind::NotFound, "test");
        let err: TextconError = io_err.into();
        assert!(matches!(err, TextconError::Io(_)));
    }

    #[test]
    fn test_error_from_json() {
        let json_err = serde_json::from_str::<String>("invalid").unwrap_err();
        let err: TextconError = json_err.into();
        assert!(matches!(err, TextconError::Json(_)));
    }
}
