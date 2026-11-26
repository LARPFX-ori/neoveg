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

    def test_extract_audited_portable_ignores_user_files_in_dist(self) -> None:
        # Issue #175: um pré-render do usuário em dist/prerender/ não é mídia do
        # pacote e não pode reprovar a atualização do dist/.
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dist = root / "dist"
            (dist / "prerender").mkdir(parents=True)
            user_avi = dist / "prerender" / "prerender_1_2.avi"
            user_avi.write_bytes(b"RIFF-usuario")
            staging = root / "staging"
            staging.mkdir()

            good_zip = root / "good.zip"
            with zipfile.ZipFile(good_zip, "w") as zf:
                zf.writestr("Neoveg-0.4.1-portable/neoveg.exe", b"MZ-bin")

            count = build_portable.extract_audited_portable(good_zip, dist, staging)
            self.assertEqual(count, 1)
            self.assertEqual((dist / "neoveg.exe").read_bytes(), b"MZ-bin")
            self.assertEqual(user_avi.read_bytes(), b"RIFF-usuario")

    def test_gh_fallback_updates_dist_with_user_prerender(self) -> None:
        # Issue #175, pelo fluxo inteiro do fallback (gh simulado): com um
        # pré-render do usuário em dist/, o fallback tem de terminar com 0.
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dist = root / "dist"
            (dist / "prerender").mkdir(parents=True)
            (dist / "prerender" / "prerender_1_2.avi").write_bytes(b"RIFF-usuario")
            installer_out = root / "dist-installer"

            src_zip = root / "Neoveg-0.4.1-windows-x86_64-portable.zip"
            with zipfile.ZipFile(src_zip, "w") as zf:
                zf.writestr("Neoveg-0.4.1-portable/neoveg.exe", b"MZ-bin")
            portable = src_zip.read_bytes()
            digest = hashlib.sha256(portable).hexdigest()

            def fake_run(cmd, **_kwargs):  # type: ignore[no-untyped-def]
                if "download" in cmd:
                    out = Path(cmd[cmd.index("--dir") + 1]) / "neoveg-windows-x86_64"
                    out.mkdir(parents=True)
                    (out / src_zip.name).write_bytes(portable)
                    (out / "SHA256SUMS.txt").write_text(
                        f"{digest}  {src_zip.name}\n", encoding="utf-8"
                    )
                return subprocess.CompletedProcess(cmd, 0, "", "")

            with (
                mock.patch.object(build_portable.shutil, "which", return_value="gh"),
                mock.patch.object(build_portable, "_list_workflow_run_ids", return_value=[]),
                mock.patch.object(build_portable, "_wait_for_new_run", return_value=(1, "")),
                mock.patch.object(build_portable, "_wait_until_run_visible"),
                mock.patch.object(build_portable.subprocess, "run", side_effect=fake_run),
            ):
                rc = build_portable.build_via_github_actions(root, dist, installer_out)

            self.assertEqual(rc, 0)
            self.assertEqual((dist / "neoveg.exe").read_bytes(), b"MZ-bin")
            self.assertTrue((dist / "prerender" / "prerender_1_2.avi").is_file())

    def test_extract_audited_portable_rejects_media_in_package(self) -> None:
        # O caso inverso continua barrado, e o pacote reprovado não toca o dist/.
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dist = root / "dist"
            dist.mkdir()
            staging = root / "staging"
            staging.mkdir()

            bad_zip = root / "bad.zip"
            with zipfile.ZipFile(bad_zip, "w") as zf:
                zf.writestr("Neoveg-0.4.1-portable/neoveg.exe", b"MZ-bin")
                zf.writestr("Neoveg-0.4.1-portable/demo.mp4", b"video")

            with self.assertRaises(SystemExit):
                build_portable.extract_audited_portable(bad_zip, dist, staging)
            self.assertFalse((dist / "neoveg.exe").exists())
            self.assertFalse((dist / "demo.mp4").exists())

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
