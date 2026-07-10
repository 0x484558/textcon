#![warn(clippy::all, clippy::nursery, clippy::pedantic)]
//! Payload-streaming text composition and one-pass file-reference expansion.
//!
//! [`Engine::render_inputs`] emits ordered file or directory bundles, while
//! [`Engine::expand_template`] streams a template and substitutes `{{ @path }}`
//! references without buffering the complete input or output.

pub mod cli;
mod engine;
pub mod error;
mod parser;
mod render;
mod selector;

pub use engine::{Engine, EngineOptions, RenderMode, SelectionOptions};
pub use error::{Result, TextconError};
