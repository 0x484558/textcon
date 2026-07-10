# Changelog

All notable changes to textcon will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.0] - 2026-07-10

### Changed

- **BREAKING**: Replaced allocating template APIs with a payload-streaming `Engine`.
- **BREAKING**: Direct inputs now use explicit Markdown or raw rendering without synthesizing template syntax.
- **BREAKING**: Absolute template references retain operating-system semantics; confinement is now explicit with `--sandbox`.
- Unified directory operands and directory references behind one deterministic gitignore/exclusion/depth/hidden selector.
- Reduced diagnostics to standard exit statuses and one stderr error; downstream BrokenPipe is quiet success.

### Added

- Streaming `raw` and adaptive `markdown` reference processors.
- Capability-based reference sandboxing.
- Production `textcon(1)` man page and a source-only codebase-bundling agent skill.

### Removed

- Output-file, HTML, list, dry-run, verbosity, and custom logging surfaces.
- Regex, whole-template replacement, JSON inspection, and duplicated `WalkDir` traversal.

## [0.2.0] - 2025-12-14

### Changed
- **BREAKING**: CLI now accepts files/directories as positional arguments for stitching.
- **BREAKING**: Template processing now requires `--template` flag (e.g., `textcon --template tpl.txt`).

### Added
- Gitignore support enabled by default.
- `--no-gitignore` flag to disable gitignore support.
- Glob exclusions and .gitignore support

## [0.1.0] - 2025-08-20

### Added
- Initial release of textcon, CLI and library crate
- Template processing with `{{ @file.txt }}` syntax. Force inclusion with `@!` prefix for large files (>64KB).
- Directory tree generation with `{{ @dir/ }}`, deep directory inclusion with `{{ @!dir/ }}`
- CLI with ergonomic interface (`textcon template.txt`). Dry-run mode (`--dry-run`) for validation, list mode (`--list`) with JSON output support.

### Security
- Path traversal protection prevents access outside base directory
- Automatic filtering of hidden files and directories

[Unreleased]: https://github.com/0x484558/textcon/compare/0.4.0...HEAD
[0.4.0]: https://github.com/0x484558/textcon/compare/0.3.0...0.4.0
[0.2.0]: https://github.com/0x484558/textcon/releases/tag/0.2.0
[0.1.0]: https://github.com/0x484558/textcon/releases/tag/0.1.0
