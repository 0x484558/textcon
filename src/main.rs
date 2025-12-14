use clap::{Parser, ValueEnum};
use globset::{Glob, GlobSetBuilder};
use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use textcon::{Result, TemplateConfig, TextconError, find_references, process_template};

const LONG_HELP: &str = r#"
Reference:
  {{ @file.txt }}      - Include file contents (max 64KB)
  {{ @!file.txt }}     - Force include large file (>64KB)
  {{ @dirname/ }}      - Include directory tree
  {{ @!dirname/ }}     - Include tree AND all file contents
  {{ @. }} OR {{ @/ }} - Include tree of current directory

Examples:
  # Process a template file
  textcon template.txt
  # Process from stdin
  echo "Code: {{ @main.rs }}" | textcon -
  # Check what would be included (dry run)
  textcon template.txt --dry-run
  # List all references in template
  textcon template.txt --list
  # List with details and check existence
  textcon template.txt --list=detailed
  # Output as JSON for scripting
  textcon template.txt --list=json
  # Use different base directory
  textcon template.txt --base-dir /path/to/project
  # Save output to file
  textcon template.txt -o output.txt

Template example:
  # My Project
  {{ @README.md }}
  ## Structure
  {{ @. }}
  ## All Source Code
  {{ @!src/ }}
  ## Logs (force include even if large)
  {{ @!logs/pod.log }}


For more information, visit: https://github.com/0x484558/textcon
"#;

/// Text concatenation for LLM context building.
///
/// Copyright 2025 0x484558 @ aleph0 s.r.o.
/// Licensed under the EUPL v1.2.
#[derive(Parser, Debug)]
#[command(
    name = "textcon",
    version,
    author = "0x484558 @ aleph0 s.r.o.",
    about = "Text concatenation for LLM context building.",
    after_long_help = LONG_HELP,
    after_help = "For more information, visit: https://github.com/0x484558/textcon"
)]
struct Cli {
    /// Template file to process (use '-' for stdin)
    #[arg(value_name = "TEMPLATE")]
    template: PathBuf,

    /// Base directory for resolving @ references
    #[arg(short, long, value_name = "DIR", env = "TEXTCON_BASE_DIR")]
    base_dir: Option<PathBuf>,

    /// Output file (defaults to stdout)
    #[arg(short, long, value_name = "FILE")]
    output: Option<PathBuf>,

    /// Maximum depth for directory tree generation
    #[arg(short = 'd', long, value_name = "DEPTH", default_value = "5")]
    max_depth: Option<usize>,

    /// Don't add file path comments
    #[arg(long)]
    no_comments: bool,

    /// Perform a dry run - validate references without processing
    #[arg(long, conflicts_with = "list")]
    dry_run: bool,

    /// Exclude glob patterns (repeatable). Patterns are relative to base-dir (default CWD)
    #[arg(short = 'x', long = "exclude", value_name = "GLOB", action = clap::ArgAction::Append)]
    exclude: Vec<String>,

    /// Disable compliance with .gitignore files
    #[arg(long)]
    no_gitignore: bool,

    /// List references in template (optionally with format: plain, detailed, json)
    #[arg(long, value_name = "FORMAT", num_args = 0..=1, default_missing_value = "plain", conflicts_with = "dry_run")]
    list: Option<ListFormat>,

    /// Output format for processed template
    #[arg(short = 'f', long, value_enum, default_value = "plain")]
    format: OutputFormat,

    /// Increase verbosity (can be used multiple times)
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,

    /// Suppress all output except errors
    #[arg(short, long, conflicts_with = "verbose")]
    quiet: bool,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum OutputFormat {
    /// Plain text output
    Plain,
    /// Markdown formatted output
    Markdown,
    /// HTML formatted output
    Html,
}

#[derive(Clone, Copy, Debug, ValueEnum, PartialEq)]
enum ListFormat {
    /// Simple list of references
    Plain,
    /// Detailed information about each reference
    Detailed,
    /// JSON output for scripting
    Json,
}

#[derive(Serialize, Deserialize)]
struct ReferenceInfo {
    reference: String,
    start: usize,
    end: usize,
    force: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exists: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    file_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

fn main() {
    let cli = Cli::parse();

    // Set up logging based on verbosity
    let log_level = match (cli.quiet, cli.verbose) {
        (true, _) => LogLevel::Error,
        (false, 0) => LogLevel::Warn,
        (false, 1) => LogLevel::Info,
        (false, 2) => LogLevel::Debug,
        (false, _) => LogLevel::Trace,
    };

    let result = if cli.dry_run {
        dry_run(&cli.template, cli.base_dir.clone(), log_level)
    } else if let Some(list_format) = cli.list {
        list_references(&cli.template, list_format, cli.base_dir.clone(), log_level)
    } else {
        // Build TemplateConfig from CLI options
        let mut config = TemplateConfig::default();
        if let Some(dir) = cli.base_dir.clone() {
            config.base_dir = dir
                .canonicalize()
                .map_err(TextconError::Io)
                .unwrap_or(config.base_dir);
        }
        config.max_tree_depth = cli.max_depth;
        config.add_path_comments = !cli.no_comments;
        config.use_gitignore = !cli.no_gitignore;
        if !cli.exclude.is_empty() {
            let mut builder = GlobSetBuilder::new();
            for pat in &cli.exclude {
                match Glob::new(pat) {
                    Ok(g) => {
                        builder.add(g);
                    }
                    Err(e) => {
                        eprintln!("[ERROR] Invalid exclude pattern '{pat}': {e}");
                        std::process::exit(2);
                    }
                }
            }
            match builder.build() {
                Ok(set) => {
                    config.exclude = Some(set);
                }
                Err(e) => {
                    eprintln!("[ERROR] Failed to build exclude set: {e}");
                    std::process::exit(2);
                }
            }
        }

        process_template_file(
            &cli.template,
            cli.output.clone(),
            cli.format,
            log_level,
            &config,
        )
    };

    if let Err(e) = result {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

fn process_template_file(
    template: &PathBuf,
    output: Option<PathBuf>,
    format: OutputFormat,
    log_level: LogLevel,
    config: &TemplateConfig,
) -> Result<()> {
    log(
        log_level,
        LogLevel::Debug,
        "Starting template processing...",
    );

    // Read template
    let template_content = if template.as_path() == Path::new("-") {
        log(log_level, LogLevel::Info, "Reading template from stdin...");
        let mut buffer = String::new();
        io::stdin().read_to_string(&mut buffer)?;
        buffer
    } else {
        log(
            log_level,
            LogLevel::Info,
            &format!("Reading template from {}", template.display()),
        );
        std::fs::read_to_string(template)?
    };

    // Process template
    log(log_level, LogLevel::Debug, "Processing references...");
    let processed = process_template(&template_content, config)?;

    // Format output
    let formatted = match format {
        OutputFormat::Plain => processed,
        OutputFormat::Markdown => format_as_markdown(&processed),
        OutputFormat::Html => format_as_html(&processed),
    };

    // Write output
    if let Some(output_path) = output {
        log(
            log_level,
            LogLevel::Info,
            &format!("Writing output to {}", output_path.display()),
        );
        std::fs::write(output_path, formatted)?;
    } else {
        print!("{formatted}");
        io::stdout().flush()?;
    }

    log(log_level, LogLevel::Info, "Processing complete!");
    Ok(())
}

fn dry_run(template: &PathBuf, base_dir: Option<PathBuf>, log_level: LogLevel) -> Result<()> {
    log(
        log_level,
        LogLevel::Info,
        "Performing dry run - validating references...",
    );

    let template_content = if template.as_path() == Path::new("-") {
        let mut buffer = String::new();
        io::stdin().read_to_string(&mut buffer)?;
        buffer
    } else {
        std::fs::read_to_string(template)?
    };

    let references = find_references(&template_content)?;
    let references_count = references.len();

    let base =
        base_dir.unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    let mut all_valid = true;
    let mut valid_count = 0;
    let mut invalid_count = 0;

    for reference in &references {
        let path = textcon::fs_utils::resolve_reference_path(&reference.reference, &base);
        match path {
            Ok(p) => {
                if p.exists() {
                    log(
                        log_level,
                        LogLevel::Info,
                        &format!("✓ {} -> {}", reference.reference, p.display()),
                    );
                    valid_count += 1;
                } else {
                    log(
                        log_level,
                        LogLevel::Warn,
                        &format!("✗ {} -> {} (not found)", reference.reference, p.display()),
                    );
                    invalid_count += 1;
                    all_valid = false;
                }
            }
            Err(e) => {
                log(
                    log_level,
                    LogLevel::Error,
                    &format!("✗ {} -> Error: {}", reference.reference, e),
                );
                invalid_count += 1;
                all_valid = false;
            }
        }
    }

    println!("\nSummary: {references_count} references found");
    if valid_count > 0 {
        println!("  ✓ {valid_count} valid");
    }
    if invalid_count > 0 {
        println!("  ✗ {invalid_count} invalid");
    }

    if !all_valid {
        std::process::exit(1);
    }

    Ok(())
}

fn list_references(
    template: &PathBuf,
    format: ListFormat,
    base_dir: Option<PathBuf>,
    log_level: LogLevel,
) -> Result<()> {
    log(log_level, LogLevel::Debug, "Listing template references...");

    let template_content = if template.as_path() == Path::new("-") {
        let mut buffer = String::new();
        io::stdin().read_to_string(&mut buffer)?;
        buffer
    } else {
        std::fs::read_to_string(template)?
    };

    let references = find_references(&template_content)?;
    let base =
        base_dir.unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    match format {
        ListFormat::Plain => {
            for reference in &references {
                println!("{}", reference.reference);
            }
        }
        ListFormat::Detailed => {
            for reference in &references {
                println!("Reference: {}", reference.reference);
                println!("  Position: {}..{}", reference.start, reference.end);
                println!("  Force: {}", if reference.force { "yes" } else { "no" });

                match textcon::fs_utils::resolve_reference_path(&reference.reference, &base) {
                    Ok(p) => {
                        println!("  Path: {}", p.display());
                        println!("  Exists: {}", if p.exists() { "yes" } else { "no" });
                        if p.exists() {
                            if p.is_file() {
                                if let Ok(metadata) = std::fs::metadata(&p) {
                                    println!("  Type: File ({} bytes)", metadata.len());
                                }
                            } else if p.is_dir() {
                                println!("  Type: Directory");
                            }
                        }
                    }
                    Err(e) => {
                        println!("  Error: {e}");
                    }
                }
                println!();
            }
        }
        ListFormat::Json => {
            let mut ref_infos = Vec::new();

            for reference in &references {
                let mut info = ReferenceInfo {
                    reference: reference.reference.clone(),
                    start: reference.start,
                    end: reference.end,
                    force: reference.force,
                    path: None,
                    exists: None,
                    file_type: None,
                    error: None,
                };

                match textcon::fs_utils::resolve_reference_path(&reference.reference, &base) {
                    Ok(p) => {
                        info.path = Some(p.display().to_string());
                        info.exists = Some(p.exists());
                        if p.exists() {
                            if p.is_file() {
                                info.file_type = Some("file".to_string());
                            } else if p.is_dir() {
                                info.file_type = Some("directory".to_string());
                            }
                        }
                    }
                    Err(e) => {
                        info.error = Some(e.to_string());
                    }
                }

                ref_infos.push(info);
            }

            let json = serde_json::to_string_pretty(&ref_infos)?;
            println!("{json}");
        }
    }

    Ok(())
}

fn format_as_markdown(content: &str) -> String {
    format!("```\n{content}\n```")
}

fn format_as_html(content: &str) -> String {
    let escaped = content
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    format!("<pre><code>{escaped}</code></pre>")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

fn log(current_level: LogLevel, message_level: LogLevel, message: &str) {
    if message_level >= current_level {
        eprintln!(
            "[{}] {}",
            match message_level {
                LogLevel::Trace => "TRACE",
                LogLevel::Debug => "DEBUG",
                LogLevel::Info => "INFO",
                LogLevel::Warn => "WARN",
                LogLevel::Error => "ERROR",
            },
            message
        );
    }
}
