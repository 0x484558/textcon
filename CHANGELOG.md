# Changelog

All notable changes to textcon will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2025-08-20

### Added
- Initial release of textcon, CLI and library crate
- Template processing with `{{ @file.txt }}` syntax. Force inclusion with `@!` prefix for large files (>64KB).
- Directory tree generation with `{{ @dir/ }}`, deep directory inclusion with `{{ @!dir/ }}`
- CLI with ergonomic interface (`textcon template.txt`). Dry-run mode (`--dry-run`) for validation, list mode (`--list`) with JSON output support.

### Security
- Path traversal protection prevents access outside base directory
- Automatic filtering of hidden files and directories

[Unreleased]: https://github.com/0x484558/textcon/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/0x484558/textcon/releases/tag/v0.1.0
