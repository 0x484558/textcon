use std::path::PathBuf;

use clap::Parser;
use ignore::gitignore::GitignoreBuilder;

use crate::RenderMode;

const LONG_HELP: &str = r"Examples:
  # Bundle selected files with H1 path labels
  textcon src/main.rs src/lib.rs

  # Bundle a directory with deterministic discovery
  textcon src --exclude '*.generated.rs'

  # Concatenate exact operand bytes
  textcon --render raw src/*.rs

  # Expand a template from a file or stdin
  textcon --template context.md
  printf '{{ @README.md }}' | textcon --template -

References:
  {{ @file }}              Include one file
  {{ @directory }}         Include selected descendants without labels
  {{ @directory | markdown }} Include descendants with H1 path labels
  {{ @path | raw }}        Disable Markdown adaptation for this reference";

/// Streaming text composition for code and LLM context.
#[derive(Debug, Parser)]
#[command(
    name = "textcon",
    version,
    about = "Stream files and template references into one predictable output",
    after_long_help = LONG_HELP
)]
pub struct Cli {
    /// Files and directories to compose; use '-' once for stdin.
    #[arg(
        value_name = "INPUT",
        required_unless_present = "template",
        conflicts_with = "template"
    )]
    pub inputs: Vec<PathBuf>,

    /// Stream-expand a template file; use '-' for stdin.
    #[arg(short, long, value_name = "FILE", conflicts_with = "inputs")]
    pub template: Option<PathBuf>,

    /// Rendering inherited by operands and bare template references.
    #[arg(long, value_enum, default_value_t = RenderMode::Markdown)]
    pub render: RenderMode,

    /// Base directory for relative template references.
    #[arg(short, long, value_name = "DIR", requires = "template")]
    pub base_dir: Option<PathBuf>,

    /// Confine template references beneath the base directory.
    #[arg(long, requires = "template")]
    pub sandbox: bool,

    /// Maximum descendant depth; the requested directory is depth zero.
    #[arg(short = 'd', long, value_name = "N")]
    pub max_depth: Option<usize>,

    /// Gitignore-style selection rule; repeat in precedence order.
    #[arg(
        short = 'x',
        long = "exclude",
        value_name = "PATTERN",
        action = clap::ArgAction::Append,
        value_parser = validate_exclude
    )]
    pub excludes: Vec<String>,

    /// Disable `.gitignore` processing during directory discovery.
    #[arg(long)]
    pub no_gitignore: bool,

    /// Include dot-prefixed descendants during directory discovery.
    #[arg(long)]
    pub hidden: bool,
}

fn validate_exclude(value: &str) -> Result<String, String> {
    let mut builder = GitignoreBuilder::new(".");
    builder
        .add_line(None, value)
        .map_err(|error| error.to_string())?;
    builder.build().map_err(|error| error.to_string())?;
    Ok(value.to_owned())
}
