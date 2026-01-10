//! # textcon
//!
//! A template processing library and CLI tool for expanding file and directory references
//! in text templates. Designed to prepare content for Large Language Models (LLMs) by
//! embedding file contents and directory structures directly into templates.
//!
//! ## Features
//!
//! - Process templates containing `{{ @file.txt }}` references
//! - Expand file contents inline
//! - Generate directory tree representations
//! - Security: prevents path traversal attacks
//! - Flexible configuration options
//!
//! ## Usage
//!
//! ### As a Library
//!
//! ```no_run
//! use textcon::{process_template, TemplateConfig};
//!
//! let template = "Here's my code:\n{{ @src/main.rs }}\n\nProject structure:\n{{ @. }}";
//! let config = TemplateConfig::default();
//!
//! match textcon::process_template(template, &config) {
//!     Ok(result) => println!("{}", result),
//!     Err(e) => eprintln!("Error: {}", e),
//! }
//! ```
//!
//! ### As a CLI Tool
//!
//! ```bash
//! # Process a template file
//! textcon template.txt
//!
//! # Process template from stdin
//! echo "Code: {{ @main.rs }}" | textcon
//!
//! # Process with custom base directory
//! textcon template.txt -b /path/to/project
//! ```

pub mod error;
pub mod fs_utils;
pub mod template;

// Re-export main types and functions for convenience
pub use error::{Result, TextconError};
pub use template::{
    TemplateConfig, TemplateReference, find_references, process_reference, process_template,
    process_template_file,
};
