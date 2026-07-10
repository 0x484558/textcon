---
name: textcon-bundle-codebase
description: Bundle a local source-code repository into a timestamped CODE Markdown file with textcon. Use when an AI agent needs to package, snapshot, export, or prepare a complete codebase as one Markdown context artifact for review, handoff, or sharing.
---

# Bundle a codebase

1. Identify the intended codebase root. Do not assume a parent monorepo when the user selected a subproject.
2. Consider repository-specific secret or generated-file exclusions. Gitignore filtering reduces noise but is not a security boundary.
3. Resolve the directory containing this `SKILL.md` as `SKILL_DIR`, then run the bundled helper instead of reconstructing a shell pipeline:

   ```bash
   python3 "${SKILL_DIR}/scripts/bundle_codebase.py" /path/to/codebase
   ```

   Replace `${SKILL_DIR}` with this skill directory. Pass repeatable `--exclude PATTERN`, `--hidden`, `--max-depth N`, or `--max-bytes N` only when the request or repository warrants them.
4. If the helper reports invalid UTF-8, identify the offending binary/non-text selection and derive or request a precise exclusion. Never silently omit a selected file.
5. On success, report the emitted `CODE-YYYY-MM-DD_HH-MM-SS.md` path and its byte size. Do not load the potentially large artifact into agent context unless the user explicitly asks.
6. Keep the bundle local. Do not upload, attach, or send it without separate authorization.

The helper requires `textcon 0.4.x` on `PATH` (or `--textcon PATH`). It respects textcon's default gitignore and hidden-file policy, excludes existing `CODE-*.md` bundles and its own temporary file, validates UTF-8 incrementally, and atomically replaces a same-second filename only after successful completion.
