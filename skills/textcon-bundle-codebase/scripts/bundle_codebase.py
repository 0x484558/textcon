#!/usr/bin/env python3
"""Create a local, atomic Markdown codebase bundle with textcon 0.4.x."""

from __future__ import annotations

import argparse
import codecs
from dataclasses import dataclass
from datetime import datetime
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
from typing import Callable, Optional, Sequence


CHUNK_SIZE = 64 * 1024
_VERSION_RE = re.compile(r"textcon 0\.4\.\d+(?:\r?\n)?\Z")


class BundleError(Exception):
    """An expected command failure suitable for a concise diagnostic."""


@dataclass(frozen=True)
class SelectorOptions:
    excludes: Sequence[str]
    hidden: bool = False
    max_depth: Optional[int] = None
    no_gitignore: bool = False


def make_bundle_filename(now: datetime) -> str:
    """Return the collision-overwritable local-time output filename."""
    return now.strftime("CODE-%Y-%m-%d_%H-%M-%S.md")


def is_supported_version(output: str) -> bool:
    """Whether output is exactly a textcon 0.4.x version line."""
    return _VERSION_RE.fullmatch(output) is not None


def build_command(binary: str, options: SelectorOptions) -> list[str]:
    """Build textcon argv, keeping protected exclusions after user rules."""
    command = [binary, "--render", "markdown"]
    if options.hidden:
        command.append("--hidden")
    if options.max_depth is not None:
        command.extend(("--max-depth", str(options.max_depth)))
    if options.no_gitignore:
        command.append("--no-gitignore")
    for pattern in options.excludes:
        command.extend(("--exclude", pattern))
    command.extend(
        (
            "--exclude",
            "CODE-*.md",
            "--exclude",
            ".textcon-code-*.tmp",
            "--",
            ".",
        )
    )
    return command


def _positive_int(value: str) -> int:
    try:
        parsed = int(value)
    except ValueError as exc:
        raise argparse.ArgumentTypeError("must be a positive integer") from exc
    if parsed <= 0:
        raise argparse.ArgumentTypeError("must be a positive integer")
    return parsed


def _nonnegative_int(value: str) -> int:
    try:
        parsed = int(value)
    except ValueError as exc:
        raise argparse.ArgumentTypeError("must be a non-negative integer") from exc
    if parsed < 0:
        raise argparse.ArgumentTypeError("must be a non-negative integer")
    return parsed


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Create an atomic CODE-{datetime}.md codebase bundle with textcon."
    )
    parser.add_argument("ROOT", nargs="?", help="codebase root (default: current directory)")
    parser.add_argument("--textcon", metavar="PATH", help="textcon executable path or name")
    parser.add_argument(
        "--exclude", action="append", default=[], metavar="PATTERN", help="exclude pattern"
    )
    parser.add_argument("--hidden", action="store_true", help="include hidden entries")
    parser.add_argument("--max-depth", type=_nonnegative_int, metavar="N")
    parser.add_argument("--max-bytes", type=_positive_int, metavar="N")
    parser.add_argument("--no-gitignore", action="store_true")
    return parser


def _resolve_root(value: Optional[str]) -> Path:
    candidate = Path.cwd() if value is None else Path(value).expanduser()
    try:
        root = candidate.resolve(strict=True)
    except OSError as exc:
        raise BundleError(f"cannot resolve root {candidate}: {exc}") from exc
    if not root.is_dir():
        raise BundleError(f"root is not a directory: {root}")
    return root


def resolve_textcon(requested: Optional[str]) -> str:
    """Resolve an explicit executable path, or find textcon on PATH."""
    if requested:
        candidate = Path(requested).expanduser()
        if candidate.is_file():
            if not os.access(str(candidate), os.X_OK):
                raise BundleError(f"textcon is not executable: {candidate}")
            return str(candidate.resolve())
        found = shutil.which(requested)
        if found:
            return str(Path(found).resolve())
        raise BundleError(f"textcon executable not found: {requested}")

    found = shutil.which("textcon")
    if not found:
        raise BundleError("textcon executable not found on PATH")
    return str(Path(found).resolve())


def verify_textcon(binary: str) -> None:
    """Require an exact successful `textcon 0.4.x` version response."""
    try:
        completed = subprocess.run(
            [binary, "--version"],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
    except OSError as exc:
        raise BundleError(f"cannot run textcon --version: {exc}") from exc
    if completed.returncode != 0:
        raise BundleError(f"textcon --version exited with status {completed.returncode}")
    try:
        output = completed.stdout.decode("utf-8", errors="strict")
    except UnicodeDecodeError as exc:
        raise BundleError("textcon --version returned non-UTF-8 output") from exc
    if not is_supported_version(output):
        raise BundleError("unsupported textcon version; required textcon 0.4.x")


def _stop_process(process: subprocess.Popen[bytes]) -> None:
    stdout = process.stdout
    if stdout is not None:
        try:
            stdout.close()
        except OSError:
            pass
    if process.poll() is None:
        try:
            process.terminate()
        except OSError:
            pass
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            try:
                process.kill()
            except OSError:
                pass
            process.wait()


def _fsync_directory_after_publication(root: Path, final_path: Path) -> None:
    if os.name != "posix":
        return
    descriptor: Optional[int] = None
    try:
        flags = os.O_RDONLY | getattr(os, "O_DIRECTORY", 0)
        descriptor = os.open(str(root), flags)
        os.fsync(descriptor)
    except OSError as exc:
        print(
            "textcon-bundle-codebase: warning: "
            f"published {final_path}, but could not fsync directory {root}: {exc}",
            file=sys.stderr,
        )
    finally:
        if descriptor is not None:
            try:
                os.close(descriptor)
            except OSError:
                pass


def create_bundle(
    root: Path,
    binary: str,
    options: SelectorOptions,
    max_bytes: Optional[int],
    now: datetime,
) -> Path:
    """Stream textcon output to a private temp and atomically publish it."""
    final_path = root / make_bundle_filename(now)
    temp_path: Optional[Path] = None
    temp_file = None
    raw_descriptor: Optional[int] = None
    process: Optional[subprocess.Popen[bytes]] = None
    published = False

    try:
        raw_descriptor, temp_name = tempfile.mkstemp(
            prefix=".textcon-code-", suffix=".tmp", dir=str(root)
        )
        temp_path = Path(temp_name)
        if os.name == "posix":
            os.fchmod(raw_descriptor, 0o600)
        temp_file = os.fdopen(raw_descriptor, "wb")
        raw_descriptor = None

        try:
            process = subprocess.Popen(
                build_command(binary, options),
                cwd=str(root),
                stdout=subprocess.PIPE,
            )
        except OSError as exc:
            raise BundleError(f"cannot start textcon: {exc}") from exc

        if process.stdout is None:  # Defensive: PIPE above guarantees this.
            raise BundleError("cannot read textcon output")

        decoder = codecs.getincrementaldecoder("utf-8")("strict")
        total = 0
        try:
            while True:
                chunk = process.stdout.read(CHUNK_SIZE)
                if not chunk:
                    break
                new_total = total + len(chunk)
                if max_bytes is not None and new_total > max_bytes:
                    raise BundleError(f"bundle exceeds --max-bytes limit of {max_bytes}")
                decoder.decode(chunk, final=False)
                temp_file.write(chunk)
                total = new_total
            decoder.decode(b"", final=True)
        except UnicodeDecodeError as exc:
            raise BundleError("textcon output is not valid UTF-8") from exc
        except OSError as exc:
            raise BundleError(f"cannot stream bundle: {exc}") from exc

        process.stdout.close()
        status = process.wait()
        if status != 0:
            raise BundleError(f"textcon exited with status {status}")

        try:
            temp_file.flush()
            os.fsync(temp_file.fileno())
            temp_file.close()
            temp_file = None
            os.replace(str(temp_path), str(final_path))
        except OSError as exc:
            raise BundleError(f"cannot publish bundle: {exc}") from exc
        published = True
    except KeyboardInterrupt as exc:
        raise BundleError("interrupted") from exc
    except OSError as exc:
        raise BundleError(f"cannot create temporary bundle: {exc}") from exc
    finally:
        if process is not None and (not published or process.poll() is None):
            _stop_process(process)
        if raw_descriptor is not None:
            try:
                os.close(raw_descriptor)
            except OSError:
                pass
        if temp_file is not None:
            try:
                temp_file.close()
            except OSError:
                pass
        if not published and temp_path is not None:
            try:
                temp_path.unlink()
            except FileNotFoundError:
                pass
            except OSError:
                pass

    _fsync_directory_after_publication(root, final_path)
    return final_path.resolve()


def run(argv: Optional[Sequence[str]] = None, now_factory: Callable[[], datetime] = datetime.now) -> int:
    args = _parser().parse_args(argv)
    try:
        root = _resolve_root(args.ROOT)
        binary = resolve_textcon(args.textcon)
        # Compatibility is checked before create_bundle can create a temp file.
        verify_textcon(binary)
        options = SelectorOptions(
            excludes=tuple(args.exclude),
            hidden=args.hidden,
            max_depth=args.max_depth,
            no_gitignore=args.no_gitignore,
        )
        final_path = create_bundle(root, binary, options, args.max_bytes, now_factory())
        print(final_path)
        return 0
    except BundleError as exc:
        print(f"textcon-bundle-codebase: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(run())
