TEXTCON(1)

# NAME

textcon - stream files and template references into one output

# SYNOPSIS

*textcon* [options] _INPUT_...

*textcon* *--template* _TEMPLATE_ [options]

# DESCRIPTION

*textcon* is a payload-streaming context composer. Operand mode writes selected files in argument order. Template mode copies literal template bytes and substitutes explicit file or directory references as they are encountered.

Results are written only to standard output. Successful execution writes no status messages. A failure after earlier output leaves that prefix visible; callers must check the exit status.

# MODES

Operand mode accepts files, directories, and one *-* stdin operand. A second *-* is a usage error. Directories use the shared selection policy described below.

Template mode requires *--template*. The value *-* reads template bytes from stdin. The template source and direct operands are explicit caller authority and are not constrained by *--sandbox*.

Supplying neither operands nor *--template* is a usage error.

# RENDERING

*--render markdown* is the default. Every direct input file and every descendant of a direct directory is emitted as:

```
# `encoded/logical/path`

unwrapped body
```

No code fence is synthesized. Source Markdown, raw HTML, remote images, or comment lines can therefore affect a Markdown preview. Treat generated bundles as untrusted Markdown and use a sanitizing viewer when needed.

Path labels use a lossless single-line encoding. A labelled record is terminated with the minimum LF bytes needed for one blank line. Body bytes and existing line endings are otherwise preserved.

For paths ending in *.md* or *.markdown* (ASCII case-insensitive), effective Markdown rendering shifts top-level ATX H1 through H5 by one level. H6, Setext headings, indented code, fenced code, line endings, NUL, invalid UTF-8, and all other bytes remain unchanged. The transformation is lexical, streaming, per-source, and applied once.

*--render raw* concatenates exact bytes without labels, separators, or Markdown adaptation.

# TEMPLATE REFERENCES

References have these forms:

```
{{ @path }}
{{ @path | markdown }}
{{ @path | raw }}
```

In inherited Markdown mode, a bare file reference emits an unlabelled adaptive body. A bare directory concatenates adaptive descendant bodies in selector order without separators. In inherited raw mode, both are byte-exact.

For a file, *| markdown* re-enables extension-based Markdown adaptation but does not add a label. For a directory it emits an H1-labelled record per descendant. *| raw* always disables labels, adaptation, and separators.

Processor names are lowercase and case-sensitive. At least one ASCII whitespace byte must precede the processor pipe, so _a|b_ remains a path. Use *\\|* and *\\}* for literal delimiter bytes in a path. Leading and trailing syntax whitespace is not part of the path. Paths must be nonempty UTF-8 without NUL.

*\\{{* emits a literal *{{*. Odd and even runs of preceding backslashes are handled predictably. Unrelated brace constructs remain literal, while malformed, oversized, or unterminated reference-like constructs fail with their original byte offset. A reference candidate is limited to 256 KiB.

Included bytes are never reparsed. Expansion is one-pass and cannot recurse or cycle through placeholder text contained in a referenced file.

# PATH RESOLUTION AND SANDBOX

Relative references resolve beneath *--base-dir*, defaulting to the current directory. Without *--sandbox*, absolute paths and *..* use normal operating-system semantics. On Unix, _{{ @/etc/fstab }}_ therefore addresses _/etc/fstab_. Positional operands always resolve normally from the process working directory and are never retargeted by *--base-dir*.

*--sandbox* opens the base directory as a filesystem capability and confines reference targets, descendant traversal, and ignore-file reads beneath it. Absolute references are accepted only when they map beneath that root. Escaping *..* components and symlinks that resolve outside the root fail; explicitly referenced symlinks that resolve inside the root may succeed. Discovered symlinks are skipped. The template source and direct operands remain outside this policy.

Sandboxing constrains namespace access; it does not create a filesystem snapshot, prevent concurrent mutation, or limit output size.

# DIRECTORY SELECTION

Top-level operands/references retain their order and duplicates. Each directory is traversed depth-first with siblings sorted by native path order. The default depth is unlimited; the requested directory is depth zero, so *--max-depth 0* selects no descendants and *--max-depth 1* selects immediate files.

Dot-prefixed descendants are hidden unless *--hidden* is supplied. Discovered symlinks, reparse points, and special files are skipped. Explicitly named regular files and symlinks bypass hidden, gitignore, and exclusion filtering.

*.gitignore* files are applied from the selection-policy root through nested selected directories unless *--no-gitignore* is supplied. Global Git excludes, _.git/info/exclude_, and _.ignore_ are not consulted.

Each *--exclude* value is a gitignore-style rule. Rules are evaluated in command-line order after *.gitignore*; later matches win and a leading *!* re-includes. A descendant cannot be reached through a pruned parent unless the parent is also re-included. Hidden and symlink policies cannot be overridden by negation.

Traversal, metadata, ignore parsing, selected-file read, and output errors are fatal. Already-written output remains visible.

Source and template payload memory is independent of payload size. Deterministic sorting retains one directory's entries at a time, and active ignore rules remain resident during their subtree traversal, so total memory also depends on maximum directory width and ignore-rule size.

# OPTIONS

*-t, --template* _FILE_
	Expand a template file. Use *-* for standard input. Conflicts with operands.

*--render* _markdown|raw_
	Select the inherited renderer. The default is *markdown*.

*-b, --base-dir* _DIR_
	Resolve relative template references beneath _DIR_. Valid only in template mode.

*--sandbox*
	Confine template references beneath the base directory. Valid only in template mode.

*-d, --max-depth* _N_
	Limit descendant depth for both directory forms.

*-x, --exclude* _PATTERN_
	Append an ordered gitignore-style selection rule. May be repeated.

*--no-gitignore*
	Disable *.gitignore* processing.

*--hidden*
	Include dot-prefixed descendants.

*-h, --help*
	Show command help.

*-V, --version*
	Show the textcon version.

# STANDARD INPUT AND OUTPUT

Use positional *-* for direct stdin or *--template -* for template stdin. Standard output contains result bytes only. Shell redirection replaces an output-file option.

When output is redirected to a file inside a selected directory, textcon identifies and skips that file so recursive discovery cannot ingest its own growing output. Explicitly naming the same file as stdout is an error.

A closed downstream pipe is quiet successful termination. Other write failures are fatal.

# EXIT STATUS

*0*
	Success, including downstream BrokenPipe.

*1*
	Template, filesystem, sandbox, input, rendering, or output failure.

*2*
	Invalid command usage or invalid option value.

# ENVIRONMENT

No environment variable changes path resolution or selection semantics.

# EXAMPLES

Bundle shell-expanded sources with path labels:

```
textcon src/*.rs > CODE.md
```

Concatenate exact bytes:

```
textcon --render raw part1 part2 > combined
```

Expand a template from stdin with ambient absolute references:

```
printf 'Host mounts:\n{{ @/etc/fstab }}\n' | textcon --template -
```

Expand a directory as labelled Markdown while confining references:

```
textcon --template context.md --base-dir ./project --sandbox
```

Re-include a file beneath an excluded directory by re-including its parent first:

```
textcon . -x generated/ -x '!generated/' -x 'generated/*' -x '!generated/keep.rs'
```

# SEE ALSO

*gitignore*(5), *pandoc*(1), *sh*(1)
