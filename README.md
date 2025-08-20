# textcon

[![CI](https://github.com/0x484558/textcon/actions/workflows/ci.yml/badge.svg)](https://github.com/0x484558/textcon/actions/workflows/ci.yml)
[![Release](https://github.com/0x484558/textcon/actions/workflows/release.yml/badge.svg)](https://github.com/0x484558/textcon/actions/workflows/release.yml)
[![Security Audit](https://github.com/0x484558/textcon/actions/workflows/audit.yml/badge.svg)](https://github.com/0x484558/textcon/actions/workflows/audit.yml)
[![Documentation](https://github.com/0x484558/textcon/actions/workflows/docs.yml/badge.svg)](https://github.com/0x484558/textcon/actions/workflows/docs.yml)
[![License: EUPL-1.2](https://img.shields.io/badge/License-EUPL--1.2-blue.svg)](LICENSE)
[![Crates.io](https://img.shields.io/crates/v/textcon.svg)](https://crates.io/crates/textcon)

A **text** **con**catenation tool that fills in provided template by expanding file and directory references into a single corpus suitable for Large Language Model (LLM) consumption. Perfect for preparing code, configurations, and project structures for consumption by web versions of AI assistants like ChatGPT, Claude, Gemini, and others.

Can be used as either CLI tool or Rust library. Embeds file contents directly into templates, generates visual directory structures. Resolves relative paths and symlinks within the base directory only, but allows flexible path formatting ([Postel's law](https://en.wikipedia.org/wiki/Robustness_principle)) and uses explicit notation for including large files or deep directory contents.

## Installation

```bash
cargo install textcon
```

Alternatively, download a release from GitHub.

## Quick Start

Create a template file with `{{ @file }}` references:

```text
# My Project

## Source Code

{{ @src/main.rs }}

## Project Structure

{{ @. }}
```

Process the template:

```bash
textcon template.txt
```

## Reference Format

### Basic Syntax

| Pattern | Description | Size Limit |
|---------|-------------|------------|
| `{{ @file.txt }}` | Include file contents | 64KB max |
| `{{ @!file.txt }}` | Force include large file | No limit |
| `{{ @dir/ }}` | Show directory tree only | N/A |
| `{{ @!dir/ }}` | Show tree + all file contents | No limit per file |
| `{{ @. }}` | Current directory tree | N/A |
| `{{ @!. }}` | Current dir tree + all files | No limit per file |

### Path Formats

All these formats are equivalent for `file.txt` in the current directory:
- `{{ @file.txt }}`
- `{{ @/file.txt }}`
- `{{ @./file.txt }}`

Leading slashes are ignored - all paths are relative to the base directory.

## CLI Usage

```bash
# Process a template file
textcon template.txt

# Process from stdin
echo "Code: {{ @main.rs }}" | textcon -

# Write to file
textcon template.txt -o output.txt

# Use different base directory
textcon template.txt --base-dir /path/to/project

# Limit directory tree depth
textcon template.txt --max-depth 3

# Remove file path comments
textcon template.txt --no-comments

# Check validity of references
textcon template.txt --dry-run

# List references found in the template
textcon template.txt --list
# Detailed information
textcon template.txt --list=detailed
# JSON output for scripting
textcon template.txt --list=json

# View help with examples
textcon --help
```

## Library Usage

```rust
use textcon::{process_template, TemplateConfig};

fn main() -> textcon::Result<()> {
    let template = "Project files:\n{{ @src/ }}\n\nMain code:\n{{ @src/main.rs }}";
    let config = TemplateConfig {
        base_dir: PathBuf::from("/my/project"),
        max_tree_depth: Some(3),
        add_path_comments: true,
        max_file_size: 128 * 1024, // 128KB limit
        inline_contents: true,
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
| `File size exceeds limit` | File larger than 64KB | Use `@!filename` to force |
| `Path traversal detected` | Trying to access outside base dir | Use only relative paths |
| `Invalid reference format` | Malformed template syntax | Check `{{ @... }}` format |

## Copyright & License

© [Vladyslav "Hex" Yamkovyi](https://github.com/0x484558) @ aleph0 s.r.o. 2025 - Licensed under the EUPL. See [LICENSE](LICENSE) for more info.

### Acknowledgments

- [clap](https://github.com/clap-rs/clap) - CLI parsing (Apache-2.0/MIT)
- [regex](https://github.com/rust-lang/regex) - Pattern matching (Apache-2.0/MIT)
- [thiserror](https://github.com/dtolnay/thiserror) - Error handling (Apache-2.0/MIT)
- [walkdir](https://github.com/BurntSushi/walkdir) - Directory traversal (MIT/Unlicense)
- [serde](https://github.com/serde-rs/serde) - Serialization framework (Apache-2.0/MIT)
