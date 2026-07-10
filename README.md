# textcon

`textcon` is a streaming context composer for code, documentation, logs, and other text-oriented sources. It can bundle shell-selected files or expand explicit `{{ @path }}` references without retaining the complete input or output in memory.

The core works on bytes. Markdown rendering adds predictable path headings and performs one bounded, lexical heading adjustment for included Markdown documents; it is not a general formatter or sanitizer.

## Installation

Install only the CLI from crates.io:

```sh
cargo install --locked textcon
```

Custom GitHub release archives also contain `share/man/man1/textcon.1`, README, and license. Cargo cannot install ancillary man pages.

## Operand mode

Bundle files in shell argument order:

```sh
textcon src/main.rs src/lib.rs > CODE.md
textcon src/*.rs > CODE.md
```

Markdown is the default renderer. Every selected file starts with an H1 path heading followed by its unwrapped body. `.md` and `.markdown` bodies have top-level ATX H1–H5 shifted down one level so their headings remain beneath the file heading.

Use raw mode for exact concatenation:

```sh
textcon --render raw part1 part2 > combined
```

Directories are traversed in deterministic depth-first order. `.gitignore` files and hidden descendants are respected by default:

```sh
textcon . --exclude 'target/' --exclude '!target/' \
  --exclude 'target/*' --exclude '!target/keep.txt'
textcon src --max-depth 3 --hidden --no-gitignore
```

Exclusions use gitignore syntax, are evaluated in command-line order, and override `.gitignore`. Explicit files bypass discovery filters. Discovered symlinks and special files are skipped.

Source and template payloads are streamed with memory independent of their size. Deterministic directory sorting retains one directory's entries at a time, and active ignore rules remain resident while traversing their subtree, so total memory also depends on maximum directory width and ignore-rule size.

Use positional `-` once for direct stdin. Supplying no input is a usage error; stdin is always explicit.

## Template mode

```text
# Project

{{ @README.md }}

{{ @src | markdown }}
```

Expand it with:

```sh
textcon --template context.md > expanded.md
printf 'Config: {{ @config.toml }}' | textcon --template -
```

Reference behavior follows the inherited `--render` mode:

| Reference | File | Directory |
|---|---|---|
| Bare, Markdown | Unlabelled adaptive body | Unlabelled adaptive bodies without separators |
| Bare, raw | Exact bytes | Exact bytes without separators |
| `\| markdown` | Adaptive body, still unlabelled | H1-labelled adaptive records |
| `\| raw` | Exact bytes | Exact bytes without labels or separators |

References are expanded once. Placeholder-looking text inside an included file is copied literally and cannot recurse.

Relative paths resolve beneath `--base-dir`, defaulting to the current directory. Absolute paths remain absolute, so `{{ @/etc/fstab }}` addresses `/etc/fstab` on Unix. Add `--sandbox` to confine reference reads beneath the base directory using capability-relative filesystem access:

```sh
textcon --template context.md --base-dir ./project --sandbox
```

The template source and positional operands are explicit authority and are not sandboxed.

Use `\{{` for a literal opener. Reference processors are lowercase `raw` or `markdown`; malformed or unterminated reference-like tokens fail with a byte offset. Literal template and included content may contain arbitrary bytes, while reference paths must be UTF-8 without NUL.

## Pipeline behavior

- stdout contains result bytes only.
- successful execution writes nothing to stderr.
- exit 0 means success, including a downstream BrokenPipe.
- operational and template failures exit 1; usage errors exit 2.
- a late streaming failure can leave a valid prefix on stdout.
- shell redirection replaces the removed output-file option.

When stdout is a regular file inside a traversed directory, textcon skips that file to avoid ingesting its own growing output.

See [`textcon(1)`](docs/man/textcon.1.scd) for the complete grammar and selection contract.

## Rust API

```rust
use std::io;
use textcon::{Engine, EngineOptions};

fn main() -> textcon::Result<()> {
    let engine = Engine::new(EngineOptions::default())?;
    engine.expand_template(&mut io::stdin().lock(), &mut io::stdout().lock())
}
```

`Engine::render_inputs`, `Engine::render_reader`, and `Engine::expand_template` are streaming operations over caller-provided readers and writers. The library propagates BrokenPipe; only the CLI maps stdout BrokenPipe to success.

## AI-agent skill

The repository contains a source-only, ready-to-copy skill at `skills/textcon-bundle-codebase`. It creates a local, atomically published `CODE-YYYY-MM-DD_HH-MM-SS.md` bundle through a deterministic Python helper.

For Codex on Unix-like systems:

```sh
mkdir -p "${CODEX_HOME:-$HOME/.codex}/skills"
cp -R skills/textcon-bundle-codebase "${CODEX_HOME:-$HOME/.codex}/skills/"
```

For Codex in PowerShell:

```powershell
$Skills = if ($env:CODEX_HOME) { Join-Path $env:CODEX_HOME 'skills' } else { Join-Path $HOME '.codex/skills' }
New-Item -ItemType Directory -Force $Skills | Out-Null
Copy-Item -Recurse skills/textcon-bundle-codebase $Skills
```

Copy the same directory into another agent's skill directory when it supports `SKILL.md` packages. The skill is intentionally excluded from Cargo packages and project-built binary release archives and is never installed into a user home automatically.

## Development

```sh
just verify
```

Man-page checks require `scdoc` and `mandoc`. The project tests formatting, strict Clippy lints, library/CLI behavior, the generated man page, and the source-only skill helper.

## License

Copyright 2025–2026 0x484558 @ aleph0 s.r.o. Licensed under the EUPL-1.2; see [LICENSE](LICENSE).
