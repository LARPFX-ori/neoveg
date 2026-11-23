#!/usr/bin/env python3
"""Testes unitários do fallback via GitHub Actions em build_portable.py e build_installer.py."""

from __future__ import annotations

import hashlib
import os
import subprocess
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest import mock

REPO_ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO_ROOT))

import build_installer  # noqa: E402
import build_portable  # noqa: E402


class BuildGhFallbackTests(unittest.TestCase):
    def test_workflow_cmd_windows_and_linux(self) -> None:
        self.assertEqual(
            build_portable.build_gh_workflow_cmd(system="Windows"),
            [
                "gh",
                "workflow",
                "run",
                "build-packages.yml",
                "-R",
                "LARPFX-ori/neoveg",
                "-f",
                "windows=true",
                "-f",
                "linux=false",
            ],
        )
        self.assertEqual(
            build_portable.build_gh_workflow_cmd(system="Linux"),
            [
                "gh",
                "workflow",
                "run",
                "build-packages.yml",
                "-R",
                "LARPFX-ori/neoveg",
                "-f",
                "windows=false",
                "-f",
                "linux=true",
            ],
        )

    def test_should_use_gh_fallback_guards(self) -> None:
        with mock.patch.dict(os.environ, {"GITHUB_ACTIONS": ""}, clear=False):
            self.assertTrue(build_portable.should_use_gh_fallback(False, False))
            self.assertFalse(build_portable.should_use_gh_fallback(True, False))
            self.assertFalse(build_portable.should_use_gh_fallback(False, True))
        with mock.patch.dict(os.environ, {"GITHUB_ACTIONS": "true"}, clear=False):
            self.assertFalse(build_portable.should_use_gh_fallback(False, False))

    def test_verify_sha256sums_valid_and_tampered(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            d = Path(tmp)
            f = d / "Neoveg-Setup-0.4.1.exe"
            f.write_bytes(b"payload-de-teste")
            digest = hashlib.sha256(b"payload-de-teste").hexdigest()
            sums = d / "SHA256SUMS.txt"
            sums.write_text(f"{digest}  {f.name}\n", encoding="utf-8")

            verified = build_portable.verify_sha256sums(sums)
            self.assertEqual(verified, [f.resolve()])

            # Conteúdo adulterado falha
            f.write_bytes(b"adulterado")
            with self.assertRaises(RuntimeError):
                build_portable.verify_sha256sums(sums)

            # Path traversal falha
            sums.write_text(f"{digest}  ../fora.exe\n", encoding="utf-8")
            with self.assertRaises(RuntimeError):
                build_portable.verify_sha256sums(sums)

    def test_extract_portable_zip_preserves_runtime_and_blocks_zip_slip(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dist = root / "dist"
            dist.mkdir()
            (dist / "preferencias.json").write_text('{"lang":"pt-BR"}', encoding="utf-8")

            good_zip = root / "good.zip"
            with zipfile.ZipFile(good_zip, "w") as zf:
                zf.writestr("Neoveg-0.4.1-portable/neoveg.exe", b"MZ-bin")
                zf.writestr("Neoveg-0.4.1-portable/i18n/pt-BR.json", b"{}")

            count = build_portable.extract_portable_zip(good_zip, dist)
            self.assertEqual(count, 2)
            self.assertEqual((dist / "neoveg.exe").read_bytes(), b"MZ-bin")
            self.assertTrue((dist / "preferencias.json").is_file())

            evil_zip = root / "evil.zip"
            with zipfile.ZipFile(evil_zip, "w") as zf:
                zf.writestr("../escape.txt", b"boom")
            with self.assertRaises(RuntimeError):
                build_portable.extract_portable_zip(evil_zip, dist)

    def test_build_portable_and_installer_trigger_fallback_on_compile_error(self) -> None:
        err = subprocess.CalledProcessError(101, ["cargo", "build", "--release", "-p", "neoveg"])
        with (
            mock.patch.dict(os.environ, {"GITHUB_ACTIONS": ""}, clear=False),
            mock.patch.object(build_portable, "build_and_package", side_effect=err),
            mock.patch.object(build_portable, "build_via_github_actions", return_value=0) as gh_mock,
            mock.patch.object(sys, "argv", ["build_portable.py"]),
        ):
            rc = build_portable.main()
            self.assertEqual(rc, 0)
            self.assertEqual(gh_mock.call_count, 1)

        with (
            mock.patch.dict(os.environ, {"GITHUB_ACTIONS": ""}, clear=False),
            mock.patch.object(build_portable, "build_and_package", side_effect=err),
            mock.patch.object(build_portable, "build_via_github_actions", return_value=0) as gh_mock,
            mock.patch.object(sys, "argv", ["build_installer.py"]),
        ):
            rc = build_installer.main()
            self.assertEqual(rc, 0)
            self.assertEqual(gh_mock.call_count, 1)


if __name__ == "__main__":
    unittest.main()
