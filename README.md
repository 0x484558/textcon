# textcon

[![CI](https://github.com/0x484558/textcon/actions/workflows/ci.yml/badge.svg)](https://github.com/0x484558/textcon/actions/workflows/ci.yml)
[![Release](https://github.com/0x484558/textcon/actions/workflows/release.yml/badge.svg)](https://github.com/0x484558/textcon/actions/workflows/release.yml)
[![Documentation](https://github.com/0x484558/textcon/actions/workflows/docs.yml/badge.svg)](https://github.com/0x484558/textcon/actions/workflows/docs.yml)
[![License: EUPL-1.2](https://img.shields.io/badge/License-EUPL--1.2-blue.svg)](LICENSE)
[![Crates.io](https://img.shields.io/crates/v/textcon.svg)](https://crates.io/crates/textcon)

Simple **text** **con**catenation tool that stitches files together into a body of text suitable for consumption by Large Language Models (LLMs). Perfect for preparing code, configurations, and project structures for consumption by web versions of AI assistants like ChatGPT, Claude, Gemini, and others.

Can be used as either CLI tool or Rust library. Embeds file contents directly into templates by resolving references recursively. Resolves relative paths and symlinks within the base directory only, following best security practices to prevent path traversal.

## Installation

```bash
cargo install textcon
```

Alternatively, download a release from GitHub.

## Quick Start

The simplest way to use `textcon` is to stitch files and directories together:

```bash
# Combine specific files
textcon src/main.rs src/lib.rs

# Recursively include all files in a directory
textcon src/
```

For more control, create a template file with `{{ @file }}` references:

```text
# My Project

## Source Code

{{ @src/main.rs }}

## Project Source

{{ @. }}
```

Process the template using the `--template` flag:

```bash
textcon --template template.txt
```

## Reference Format

### Basic Syntax

| Pattern | Description |
|---------|-------------|
| `{{ @file.txt }}` | Include file contents |
| `{{ @dir/ }}` | Include all file contents in directory |
| `{{ @. }}` | Include all files from current directory |

All these formats are equivalent for `file.txt` in the current directory:
- `{{ @file.txt }}`
- `{{ @/file.txt }}`
- `{{ @./file.txt }}`


### Exclusions

- **.gitignore**: Respected by default. Use `--no-gitignore` to disable.
- **Manual exclusions**: Use `--exclude "PATTERN"` to exclude specific paths.

Patterns without slashes match at any depth, leading slashes anchor to the base directory, and negation (e.g., `--exclude "!important.txt"`) is supported to explicitly include files.

## CLI Usage

```bash
# Process a template file
textcon --template template.txt

# Stitch files and directories together
textcon src/main.rs src/lib.rs

# Process from stdin
echo "Code: {{ @main.rs }}" | textcon --template -

# Write to file
textcon --template template.txt -o output.txt

# Use different base directory
textcon --template template.txt --base-dir /path/to/project

# Limit directory recursion depth
textcon src/ --max-depth 3

# Exclude specific files/patterns (glob)
textcon src/ --exclude "*.log" --exclude "secrets/**"

# Disable .gitignore compliance (enabled by default)
textcon src/ --no-gitignore

# Check validity of references
textcon --template template.txt --dry-run
# (Works with inputs too)
textcon src/ --dry-run

# List references found in the template
textcon --template template.txt --list
# Detailed information
textcon --template template.txt --list=detailed
# JSON output for scripting
textcon --template template.txt --list=json

# View help with examples
textcon --help
```

## Library Usage

```rust
use std::path::PathBuf;
use textcon::{process_template, TemplateConfig};

fn main() -> textcon::Result<()> {
    let template = "Project files:\n{{ @src/ }}\n\nMain code:\n{{ @src/main.rs }}";
    let config = TemplateConfig {
        base_dir: PathBuf::from("/my/project"),
        max_tree_depth: Some(3),
        inline_contents: true,
        ..TemplateConfig::default()
    };
    // Alternatively, `TemplateConfig::default();`
    
    let output = process_template(template, &config)?;
    println!("{}", output);
    
    Ok(())
}
```

## Error Handling

Common errors and solutions:

| Error | Cause | Solution |
|-------|-------|----------|
| `File not found` | Reference to non-existent file | Check file path and spelling |
| `Directory not found` | Reference to non-existent directory | Verify directory exists |
| `Path traversal detected` | Trying to access outside base dir | Use only relative paths |
| `Invalid reference format` | Malformed template syntax | Check `{{ @... }}` format |

## Copyright & License

© [Hex](https://github.com/0x484558) @ aleph0 s.r.o. 2025 - Licensed under the EUPL. See [LICENSE](LICENSE) for more info.

### Acknowledgments

- [clap](https://github.com/clap-rs/clap) - CLI parsing (Apache-2.0/MIT)
- [regex](https://github.com/rust-lang/regex) - Pattern matching (Apache-2.0/MIT)
- [thiserror](https://github.com/dtolnay/thiserror) - Error handling (Apache-2.0/MIT)
- [walkdir](https://github.com/BurntSushi/walkdir) - Directory traversal (MIT/Unlicense)
- [ignore](https://github.com/BurntSushi/ripgrep/tree/master/crates/ignore) - Fast file ignore (MIT/Unlicense)
- [serde](https://github.com/serde-rs/serde) - Serialization framework (Apache-2.0/MIT)
