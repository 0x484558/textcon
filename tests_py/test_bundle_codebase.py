from __future__ import annotations

from datetime import datetime
import importlib.util
import io
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


SCRIPT = (
    Path(__file__).parents[1]
    / "skills"
    / "textcon-bundle-codebase"
    / "scripts"
    / "bundle_codebase.py"
)
REPOSITORY = Path(__file__).parents[1]
SPEC = importlib.util.spec_from_file_location("bundle_codebase", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
bundle = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = bundle
SPEC.loader.exec_module(bundle)


NOW = datetime(2026, 7, 10, 12, 34, 56)


class FakeProcess:
    def __init__(self, output: bytes, returncode: int = 0):
        self.stdout = io.BytesIO(output)
        self.returncode = returncode
        self.terminated = False
        self.killed = False

    def poll(self):
        return self.returncode

    def wait(self, timeout=None):
        return self.returncode

    def terminate(self):
        self.terminated = True
        self.returncode = 0

    def kill(self):
        self.killed = True
        self.returncode = -9


class RunningFakeProcess(FakeProcess):
    def __init__(self, output: bytes, masked_status: int = 0):
        super().__init__(output, returncode=None)
        self.masked_status = masked_status

    def wait(self, timeout=None):
        if self.returncode is None:
            self.returncode = self.masked_status
        return self.returncode

    def terminate(self):
        self.terminated = True
        self.returncode = self.masked_status


def version_result(output=b"textcon 0.4.7\n", status=0):
    return subprocess.CompletedProcess(["textcon", "--version"], status, output, b"")


class PureHelperTests(unittest.TestCase):
    def test_filename_has_local_second_precision_only(self):
        self.assertEqual(
            bundle.make_bundle_filename(NOW), "CODE-2026-07-10_12-34-56.md"
        )

    def test_version_acceptance_and_rejection(self):
        accepted = ("textcon 0.4.0", "textcon 0.4.123\n", "textcon 0.4.9\r\n")
        rejected = (
            "textcon 0.3.9\n",
            "textcon 0.5.0\n",
            " textcon 0.4.1\n",
            "textcon 0.4.1 \n",
            "textcon 0.4.x\n",
            "textcon 0.4.1\nextra",
        )
        for value in accepted:
            self.assertTrue(bundle.is_supported_version(value), value)
        for value in rejected:
            self.assertFalse(bundle.is_supported_version(value), value)

    def test_command_order_and_protected_exclusions_are_last(self):
        options = bundle.SelectorOptions(
            excludes=("target", "!target/keep"),
            hidden=True,
            max_depth=4,
            no_gitignore=True,
        )
        self.assertEqual(
            bundle.build_command("/bin/textcon", options),
            [
                "/bin/textcon",
                "--render",
                "markdown",
                "--hidden",
                "--max-depth",
                "4",
                "--no-gitignore",
                "--exclude",
                "target",
                "--exclude",
                "!target/keep",
                "--exclude",
                "CODE-*.md",
                "--exclude",
                ".textcon-code-*.tmp",
                "--",
                ".",
            ],
        )

    def test_path_with_spaces_remains_one_argv_element(self):
        binary = "/tmp/textcon tools/textcon"
        command = bundle.build_command(binary, bundle.SelectorOptions(excludes=()))
        self.assertEqual(command[0], binary)
        self.assertEqual(command[-2:], ["--", "."])


class BundleIntegrationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name).resolve()
        self.options = bundle.SelectorOptions(excludes=())

    def tearDown(self):
        self.temp.cleanup()

    def create(self, process, max_bytes=None):
        with mock.patch.object(bundle.subprocess, "Popen", return_value=process) as popen:
            path = bundle.create_bundle(
                self.root, "/tmp/textcon tools/textcon", self.options, max_bytes, NOW
            )
        return path, popen

    def assert_no_temps(self):
        self.assertEqual(list(self.root.glob(".textcon-code-*.tmp")), [])

    def test_success_atomically_overwrites_collision(self):
        final = self.root / bundle.make_bundle_filename(NOW)
        final.write_text("old", encoding="utf-8")
        path, popen = self.create(FakeProcess(b"# `src/lib.rs`\n\nnew\n"))
        self.assertEqual(path, final)
        self.assertEqual(final.read_bytes(), b"# `src/lib.rs`\n\nnew\n")
        self.assert_no_temps()
        self.assertEqual(popen.call_args.kwargs["cwd"], str(self.root))
        self.assertIs(popen.call_args.kwargs["stdout"], subprocess.PIPE)
        self.assertNotIn("shell", popen.call_args.kwargs)

    def test_invalid_utf8_split_across_reads_fails_and_preserves_final(self):
        final = self.root / bundle.make_bundle_filename(NOW)
        final.write_bytes(b"existing")

        class SplitStream:
            def __init__(self):
                self.parts = iter((b"valid \xe2", b"(invalid"))

            def read(self, size):
                return next(self.parts, b"")

            def close(self):
                pass

        process = FakeProcess(b"")
        process.stdout = SplitStream()
        with mock.patch.object(bundle.subprocess, "Popen", return_value=process):
            with self.assertRaisesRegex(bundle.BundleError, "not valid UTF-8"):
                bundle.create_bundle(self.root, "textcon", self.options, None, NOW)
        self.assertEqual(final.read_bytes(), b"existing")
        self.assert_no_temps()

    def test_max_bytes_failure_survives_child_success_after_pipe_close(self):
        final = self.root / bundle.make_bundle_filename(NOW)
        final.write_bytes(b"existing")
        process = RunningFakeProcess(b"12345", masked_status=0)
        with mock.patch.object(bundle.subprocess, "Popen", return_value=process):
            with self.assertRaisesRegex(bundle.BundleError, "exceeds --max-bytes"):
                bundle.create_bundle(self.root, "textcon", self.options, 4, NOW)
        self.assertTrue(process.terminated)
        self.assertEqual(final.read_bytes(), b"existing")
        self.assert_no_temps()

    def test_child_nonzero_fails_without_publication(self):
        final = self.root / bundle.make_bundle_filename(NOW)
        final.write_bytes(b"existing")
        with mock.patch.object(
            bundle.subprocess, "Popen", return_value=FakeProcess(b"partial", returncode=7)
        ):
            with self.assertRaisesRegex(bundle.BundleError, "status 7"):
                bundle.create_bundle(self.root, "textcon", self.options, None, NOW)
        self.assertEqual(final.read_bytes(), b"existing")
        self.assert_no_temps()

    def test_run_rejects_version_before_temp_creation(self):
        fake_binary = self.root / "textcon"
        fake_binary.write_bytes(b"")
        fake_binary.chmod(0o700)
        stderr = io.StringIO()
        with mock.patch.object(bundle.subprocess, "run", return_value=version_result(b"textcon 0.5.0\n")), mock.patch.object(
            bundle.tempfile, "mkstemp"
        ) as mkstemp, mock.patch("sys.stderr", stderr):
            status = bundle.run([str(self.root), "--textcon", str(fake_binary)])
        self.assertEqual(status, 1)
        mkstemp.assert_not_called()
        self.assertIn("required textcon 0.4.x", stderr.getvalue())

    def test_run_prints_only_absolute_path_after_success(self):
        fake_binary = self.root / "textcon tool"
        fake_binary.write_bytes(b"")
        fake_binary.chmod(0o700)
        stdout = io.StringIO()
        with mock.patch.object(bundle.subprocess, "run", return_value=version_result()), mock.patch.object(
            bundle.subprocess, "Popen", return_value=FakeProcess(b"bundle\n")
        ) as popen, mock.patch("sys.stdout", stdout):
            status = bundle.run(
                [str(self.root), "--textcon", str(fake_binary)], now_factory=lambda: NOW
            )
        self.assertEqual(status, 0)
        expected = self.root / bundle.make_bundle_filename(NOW)
        self.assertEqual(stdout.getvalue(), f"{expected}\n")
        self.assertEqual(popen.call_args.args[0][0], str(fake_binary.resolve()))


class RealCliEndToEndTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        subprocess.run(
            ["cargo", "build", "--locked", "--bin", "textcon"],
            cwd=REPOSITORY,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=True,
        )
        executable = "textcon.exe" if sys.platform == "win32" else "textcon"
        cls.binary = (REPOSITORY / "target" / "debug" / executable).resolve()

    def test_real_helper_and_cli_overwrite_without_self_ingestion(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            (root / "src").mkdir()
            (root / "README.md").write_text("# Project\n", encoding="utf-8")
            source = root / "src" / "lib.rs"
            source.write_text("const VALUE: &str = \"ONE\";\n", encoding="utf-8")
            (root / "secret.txt").write_text("SECRET", encoding="utf-8")
            (root / ".gitignore").write_text("secret.txt\n", encoding="utf-8")
            (root / "CODE-old.md").write_text("OLD_BUNDLE", encoding="utf-8")
            final = root / bundle.make_bundle_filename(NOW)
            final.write_text("COLLISION", encoding="utf-8")

            first_stdout = io.StringIO()
            with mock.patch("sys.stdout", first_stdout):
                first_status = bundle.run(
                    [str(root), "--textcon", str(self.binary)],
                    now_factory=lambda: NOW,
                )
            self.assertEqual(first_status, 0)
            self.assertEqual(first_stdout.getvalue(), f"{final}\n")
            first = final.read_text(encoding="utf-8")
            self.assertIn("# `README.md`", first)
            self.assertIn('const VALUE: &str = "ONE";', first)
            self.assertNotIn("SECRET", first)
            self.assertNotIn("OLD_BUNDLE", first)
            self.assertNotIn("COLLISION", first)

            source.write_text("const VALUE: &str = \"TWO\";\n", encoding="utf-8")
            second_stdout = io.StringIO()
            with mock.patch("sys.stdout", second_stdout):
                second_status = bundle.run(
                    [str(root), "--textcon", str(self.binary)],
                    now_factory=lambda: NOW,
                )
            self.assertEqual(second_status, 0)
            self.assertEqual(second_stdout.getvalue(), f"{final}\n")
            second = final.read_text(encoding="utf-8")
            self.assertIn('const VALUE: &str = "TWO";', second)
            self.assertNotIn('const VALUE: &str = "ONE";', second)
            self.assertEqual(list(root.glob(".textcon-code-*.tmp")), [])
            if os.name == "posix":
                self.assertEqual(final.stat().st_mode & 0o777, 0o600)


if __name__ == "__main__":
    unittest.main()
