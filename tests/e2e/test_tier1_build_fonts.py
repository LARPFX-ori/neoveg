"""Tier 1 E2E tests for F1 (Branch/Hygiene), F2 (Selawik), F3 (Inter/NotoSans), F4 (UI Fonts), and F10 (Portable Dist)."""

from __future__ import annotations

import os
import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

_REPO_ROOT = Path(__file__).resolve().parents[2]
if str(_REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(_REPO_ROOT))

from tests.e2e.helpers import (
    ALLOWED_SPDX_LICENSES,
    PROPRIETARY_FONT_STEMS,
    REPO_ROOT,
    assert_no_panic_or_crash_log,
    exe_name,
    feature_tags,
    find_neoveg_bin,
    isolated_build_env,
    sha256_file,
    skip_cargo_gates,
    snapshot_crash_logs,
    stage_isolated_build_workspace,
    validate_attribution_sidecar,
    validate_sfnt_font,
)

TIER = 1


def _git(*args: str) -> str:
    res = subprocess.run(
        ["git", *args],
        cwd=str(REPO_ROOT),
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=30,
        check=True,
    )
    return res.stdout


def _find_inter_fonts(fonts_dir: Path) -> list[Path]:
    if not fonts_dir.is_dir():
        return []
    return sorted(
        p for p in fonts_dir.iterdir()
        if p.is_file() and p.suffix.lower() in (".ttf", ".otf") and p.name.lower().startswith("inter")
    )


def _find_notosans_fonts(fonts_dir: Path) -> list[Path]:
    if not fonts_dir.is_dir():
        return []
    return sorted(
        p for p in fonts_dir.iterdir()
        if p.is_file() and p.suffix.lower() in (".ttf", ".otf") and p.name.lower().startswith("notosans")
    )


@feature_tags("F1")
class TestTier1F1BranchAndHygiene(unittest.TestCase):
    def test_f1_branch_is_descriptive_non_main(self) -> None:
        branch = _git("branch", "--show-current").strip()
        self.assertTrue(branch, "Active git branch must not be empty (detached HEAD)")
        self.assertNotEqual(branch, "main", "Development must happen on a descriptive branch, not main")
        self.assertRegex(
            branch,
            r"^(feat|fix|docs|chore|refactor|test)/[a-z0-9._-]+$",
            f"Branch name {branch!r} does not follow descriptive prefix convention",
        )

    def test_f1_clean_worktree_no_tracked_build_or_media_artifacts(self) -> None:
        tracked = _git("ls-files").splitlines()
        forbidden_prefixes = (
            "target/",
            "dist/",
            "fixtures/",
            "test_media/",
            "logs/",
            "autosave/",
            "plan/",
            "manuais/",
            "design de layouts/",
        )
        forbidden_exts = (".mp4", ".mov", ".mkv", ".avi", ".wav", ".mp3", ".flac", ".cin", ".dpx", ".exr")
        bad_paths = [
            p for p in tracked
            if p.startswith(forbidden_prefixes) or p.lower().endswith(forbidden_exts)
        ]
        self.assertEqual(bad_paths, [], f"Forbidden tracked build/media artifacts found: {bad_paths}")

    def test_f1_gitignore_excludes_dist_target_and_local_refs(self) -> None:
        gi = (REPO_ROOT / ".gitignore").read_text(encoding="utf-8", errors="replace")
        lines = {ln.strip() for ln in gi.splitlines() if ln.strip() and not ln.strip().startswith("#")}
        self.assertTrue(
            "/target" in lines or "target" in lines or "/target/" in lines,
            ".gitignore must exclude /target",
        )
        self.assertTrue(
            "/dist" in lines or "dist" in lines or "/dist/" in lines or "dist/" in lines,
            ".gitignore must exclude /dist",
        )
        for required in ("/logs/", "/autosave/", "/plan/", "/manuais/", "/test_media/"):
            self.assertIn(required, lines, f".gitignore missing {required}")

    def test_f1_workspace_crates_have_license_and_lints(self) -> None:
        root_toml = (REPO_ROOT / "Cargo.toml").read_text(encoding="utf-8")
        self.assertIn('license = "MIT OR Apache-2.0"', root_toml)
        self.assertIn("unsafe_code", root_toml)

        crates_dir = REPO_ROOT / "crates"
        crate_tomls = sorted(crates_dir.glob("*/Cargo.toml"))
        self.assertEqual(len(crate_tomls), 5, f"Expected 5 workspace crates, found {crate_tomls}")
        for ct in crate_tomls:
            txt = ct.read_text(encoding="utf-8")
            self.assertIn("license", txt, f"{ct} missing license")
            crate_name = ct.parent.name
            if crate_name in ("neoveg_engine", "neoveg_scripting"):
                has_ws_lints = "[lints]" in txt and "workspace = true" in txt
                has_ffi_lints = "[lints.rust]" in txt and 'unsafe_code = "deny"' in txt
                self.assertTrue(
                    has_ws_lints or has_ffi_lints,
                    f"{ct} must declare [lints] workspace = true or [lints.rust] unsafe_code = \"deny\"",
                )
            else:
                self.assertIn("[lints]", txt, f"{ct} missing [lints]")
                self.assertIn("workspace = true", txt, f"{ct} missing workspace = true under [lints]")
            src_root = ct.parent / "src" / ("main.rs" if (ct.parent / "src" / "main.rs").is_file() else "lib.rs")
            src_txt = src_root.read_text(encoding="utf-8")
            self.assertIn("clippy::unwrap_used", src_txt, f"{src_root} missing crate-level #![deny(clippy::unwrap_used, ...)]")

    def test_f1_repo_has_required_root_governance_files(self) -> None:
        required = (
            "AGENTS.md",
            "ATTRIBUTION.md",
            "LICENSE-MIT",
            "LICENSE-APACHE",
            "README.md",
            "ROADMAP.md",
            "tools/check_licenses.py",
            "build_portable.py",
        )
        missing = [rel for rel in required if not (REPO_ROOT / rel).is_file()]
        self.assertEqual(missing, [], f"Missing required root governance files: {missing}")


@feature_tags("F2")
class TestTier1F2SelawikFonts(unittest.TestCase):
    def test_f2_selawik_ttf_files_exist_and_valid_sfnt(self) -> None:
        fonts_dir = REPO_ROOT / "assets" / "fonts"
        for fname in ("selawk.ttf", "selawksb.ttf"):
            fpath = fonts_dir / fname
            info = validate_sfnt_font(fpath, expected_family_substr="Selawik")
            self.assertIn("head", info["tables"])

    def test_f2_selawik_sfnt_name_table_matches_regular_and_semibold(self) -> None:
        fonts_dir = REPO_ROOT / "assets" / "fonts"
        reg = validate_sfnt_font(
            fonts_dir / "selawk.ttf",
            expected_family_substr="Selawik",
            expected_subfamily_substr="Regular",
        )
        semi = validate_sfnt_font(
            fonts_dir / "selawksb.ttf",
            expected_family_substr="Selawik",
            expected_subfamily_substr="SemiBold",
        )
        self.assertTrue(reg["families"])
        self.assertTrue(semi["families"])

    def test_f2_selawik_attribution_sidecars_valid(self) -> None:
        fonts_dir = REPO_ROOT / "assets" / "fonts"
        for fname in ("selawk.ttf", "selawksb.ttf"):
            data = validate_attribution_sidecar(
                fonts_dir / f"{fname}.attribution",
                expected_asset=fname,
                required_license="OFL-1.1",
            )
            self.assertTrue(
                data["source"].startswith("https://"),
                f"{fname}.attribution source must be https:// URL, got {data['source']!r}",
            )

    def test_f2_selawik_license_file_present_and_ofl(self) -> None:
        fonts_dir = REPO_ROOT / "assets" / "fonts"
        data = validate_attribution_sidecar(fonts_dir / "selawk.ttf.attribution")
        lic_rel = data["license-file"].split(",")[0].strip()
        lic_path = (fonts_dir / lic_rel) if (fonts_dir / lic_rel).is_file() else (REPO_ROOT / lic_rel)
        txt = lic_path.read_text(encoding="utf-8", errors="replace")
        self.assertIn("SIL OPEN FONT LICENSE", txt.upper())
        self.assertIn("1.1", txt)

    def test_f2_selawik_indexed_in_attribution_md(self) -> None:
        txt = (REPO_ROOT / "ATTRIBUTION.md").read_text(encoding="utf-8")
        self.assertIn("selawk.ttf", txt)
        self.assertIn("selawksb.ttf", txt)
        self.assertIn("OFL-1.1", txt)


@feature_tags("F3")
class TestTier1F3InterNotoComparison(unittest.TestCase):
    def test_f3_inter_font_and_sfnt_valid(self) -> None:
        fonts_dir = REPO_ROOT / "assets" / "fonts"
        inter_files = _find_inter_fonts(fonts_dir)
        self.assertTrue(
            inter_files,
            f"Expected Inter font (.ttf) in {fonts_dir}, found none",
        )
        reg = fonts_dir / "Inter-Regular.ttf"
        target = reg if reg.is_file() else inter_files[0]
        validate_sfnt_font(
            target,
            expected_family_substr="Inter",
            required_chars="0123456789:.;-",
        )

    def test_f3_notosans_fallback_font_and_glyph_coverage(self) -> None:
        fonts_dir = REPO_ROOT / "assets" / "fonts"
        noto_files = _find_notosans_fonts(fonts_dir)
        self.assertTrue(
            noto_files,
            f"Expected NotoSans-Regular.ttf in {fonts_dir}, found none",
        )
        reg = fonts_dir / "NotoSans-Regular.ttf"
        target = reg if reg.is_file() else noto_files[0]
        info = validate_sfnt_font(
            target,
            expected_family_substr="Noto Sans",
            required_chars="ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789",
        )
        self.assertGreater(info["cmap_count"], 100)

    def test_f3_inter_and_notosans_attribution_sidecars_valid(self) -> None:
        fonts_dir = REPO_ROOT / "assets" / "fonts"
        inter_files = _find_inter_fonts(fonts_dir)
        noto_files = _find_notosans_fonts(fonts_dir)
        self.assertTrue(inter_files, f"No Inter font files in {fonts_dir}")
        self.assertTrue(noto_files, f"No NotoSans font files in {fonts_dir}")
        for fpath in [*inter_files, *noto_files]:
            validate_attribution_sidecar(
                fonts_dir / f"{fpath.name}.attribution",
                expected_asset=fpath.name,
                required_license="OFL-1.1",
            )

    def test_f3_inter_and_notosans_license_files_and_attribution_md(self) -> None:
        fonts_dir = REPO_ROOT / "assets" / "fonts"
        inter_files = _find_inter_fonts(fonts_dir)
        noto_files = _find_notosans_fonts(fonts_dir)
        self.assertTrue(inter_files and noto_files, "Inter and NotoSans fonts must exist in assets/fonts/")
        attr_txt = (REPO_ROOT / "ATTRIBUTION.md").read_text(encoding="utf-8")
        self.assertIn("Inter", attr_txt)
        self.assertIn("Noto Sans", attr_txt)
        for fpath in [inter_files[0], noto_files[0]]:
            data = validate_attribution_sidecar(fonts_dir / f"{fpath.name}.attribution")
            lic_rel = data["license-file"].split(",")[0].strip()
            lic_path = (fonts_dir / lic_rel) if (fonts_dir / lic_rel).is_file() else (REPO_ROOT / lic_rel)
            self.assertIn("SIL OPEN FONT LICENSE", lic_path.read_text(encoding="utf-8", errors="replace").upper())

    def test_f3_font_comparison_documented(self) -> None:
        candidate_docs = [
            REPO_ROOT / "ATTRIBUTION.md",
            REPO_ROOT / "README.md",
            *(sorted((REPO_ROOT / "docs").rglob("*.md")) if (REPO_ROOT / "docs").is_dir() else []),
        ]
        combined = "\n".join(p.read_text(encoding="utf-8", errors="replace") for p in candidate_docs if p.is_file())
        for family in ("Selawik", "Noto Sans", "Inter", "DejaVu Sans", "PT Sans"):
            self.assertIn(
                family,
                combined,
                f"Font comparison documentation missing evaluation of {family!r} across ATTRIBUTION.md/README.md/docs/",
            )


@feature_tags("F4")
class TestTier1F4UiFontIntegration(unittest.TestCase):
    def test_f4_no_proprietary_os_fonts_anywhere_in_repo(self) -> None:
        tracked = _git("ls-files").splitlines()
        assets_files = (
            [str(p.relative_to(REPO_ROOT)) for p in (REPO_ROOT / "assets").rglob("*") if p.is_file()]
            if (REPO_ROOT / "assets").is_dir()
            else []
        )
        for rel in set(tracked + assets_files):
            low = rel.lower()
            if low.endswith((".ttf", ".otf", ".ttc", ".woff", ".woff2")):
                stem = Path(low).name
                for bad in PROPRIETARY_FONT_STEMS:
                    self.assertNotIn(
                        bad,
                        stem,
                        f"Proprietary OS font file detected in repo/assets: {rel}",
                    )

    def test_f4_every_font_asset_has_sidecar_and_allowlisted_spdx(self) -> None:
        fonts_dir = REPO_ROOT / "assets" / "fonts"
        self.assertTrue(fonts_dir.is_dir(), f"Missing fonts directory: {fonts_dir}")
        font_files = [
            p for p in fonts_dir.rglob("*")
            if p.is_file() and p.suffix.lower() in (".ttf", ".otf")
        ]
        self.assertGreaterEqual(len(font_files), 4, f"Expected >=4 bundled fonts in {fonts_dir}, found {font_files}")
        for ff in font_files:
            data = validate_attribution_sidecar(
                ff.parent / f"{ff.name}.attribution",
                expected_asset=ff.name,
            )
            self.assertIn(data["license"], ALLOWED_SPDX_LICENSES)

    def test_f4_neoveg_ui_fonts_unit_and_smoke_tests_pass(self) -> None:
        fonts_rs = REPO_ROOT / "crates" / "neoveg_ui" / "src" / "fonts.rs"
        self.assertTrue(fonts_rs.is_file(), f"Missing neoveg_ui font module: {fonts_rs}")
        app_rs = (REPO_ROOT / "crates" / "neoveg_ui" / "src" / "app.rs").read_text(encoding="utf-8")
        self.assertIn("fonts::", app_rs, "app.rs must wire neoveg_ui::fonts")
        if skip_cargo_gates():
            return
        res = subprocess.run(
            ["cargo", "test", "-p", "neoveg_ui", "--lib", "--", "--test-threads=1"],
            cwd=str(REPO_ROOT),
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=300,
        )
        assert_no_panic_or_crash_log(res)
        self.assertEqual(res.returncode, 0, f"neoveg_ui lib tests failed:\n{res.stdout}\n{res.stderr}")

    def test_f4_neoveg_ui_kittest_passes_sequential(self) -> None:
        fonts_rs = REPO_ROOT / "crates" / "neoveg_ui" / "src" / "fonts.rs"
        self.assertTrue(fonts_rs.is_file(), f"Missing neoveg_ui font module: {fonts_rs}")
        timeline_rs = (REPO_ROOT / "crates" / "neoveg_ui" / "src" / "timeline.rs").read_text(encoding="utf-8")
        self.assertTrue(
            "visor" in timeline_rs.lower() and "ruler" in timeline_rs.lower(),
            "timeline.rs must wire Inter visor & ruler font families",
        )
        if skip_cargo_gates():
            return
        res = subprocess.run(
            ["cargo", "test", "-p", "neoveg_ui", "kittest_", "--", "--test-threads=1"],
            cwd=str(REPO_ROOT),
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=300,
        )
        assert_no_panic_or_crash_log(res)
        self.assertEqual(res.returncode, 0, f"neoveg_ui kittest failed:\n{res.stdout}\n{res.stderr}")

    def test_f4_compiled_binary_embeds_open_fonts_not_proprietary(self) -> None:
        bin_path = find_neoveg_bin()
        raw = bin_path.read_bytes()
        for family in ("Selawik", "Inter", "Noto Sans"):
            self.assertTrue(
                family.encode("ascii") in raw or family.encode("utf-16-be") in raw,
                f"Compiled binary {bin_path} does not embed open font {family} (stale or unwired)",
            )
        # Ensure Microsoft proprietary font copyright strings are not embedded
        self.assertFalse(b"Segoe UI\x00" in raw, "Proprietary Segoe UI font string found in binary")


@feature_tags("F10")
class TestTier1F10PortableBuildAndDocs(unittest.TestCase):
    def test_f10_cli_help_and_check_licenses_succeed(self) -> None:
        res_bp = subprocess.run(
            [sys.executable, str(REPO_ROOT / "build_portable.py"), "--help"],
            cwd=str(REPO_ROOT),
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=30,
        )
        self.assertEqual(res_bp.returncode, 0, res_bp.stderr)
        self.assertIn("--no-build", res_bp.stdout)
        self.assertIn("--out", res_bp.stdout)

        res_lic = subprocess.run(
            [sys.executable, str(REPO_ROOT / "tools" / "check_licenses.py"), "--current"],
            cwd=str(REPO_ROOT),
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=60,
        )
        self.assertEqual(res_lic.returncode, 0, res_lic.stderr)

    def test_f10_build_portable_defaults_to_repo_dist(self) -> None:
        bp_txt = (REPO_ROOT / "build_portable.py").read_text(encoding="utf-8")
        self.assertFalse(
            'cargo_target_dir(repo_root) / "portable"' in bp_txt,
            "build_portable.py still defaults to <target-dir>/portable/ instead of <repo_root>/dist",
        )
        self.assertRegex(bp_txt, r"""repo_root\s*/\s*["']dist["']""")

    def test_f10_isolated_package_creates_dist_with_exe_docs_and_fonts(self) -> None:
        fonts_dir = REPO_ROOT / "assets" / "fonts"
        self.assertTrue(fonts_dir.is_dir(), "assets/fonts/ must exist before packaging fonts into dist/")
        with tempfile.TemporaryDirectory() as tmp:
            ws = stage_isolated_build_workspace(Path(tmp), with_release_bin=True, copy_repo_assets=True)
            src_bin = ws / "target" / "release" / exe_name()
            expected_sha = sha256_file(src_bin)

            res = subprocess.run(
                [sys.executable, str(ws / "build_portable.py"), "--no-build"],
                cwd=str(ws),
                env=isolated_build_env(),
                capture_output=True,
                text=True,
                encoding="utf-8",
                errors="replace",
                timeout=60,
            )
            self.assertEqual(res.returncode, 0, f"build_portable.py failed:\n{res.stdout}\n{res.stderr}")
            dist_dir = ws / "dist"
            self.assertTrue(dist_dir.is_dir(), "build_portable.py default output must be <ws>/dist/")
            dist_bin = dist_dir / exe_name()
            self.assertTrue(dist_bin.is_file(), f"Missing packaged binary: {dist_bin}")
            self.assertEqual(sha256_file(dist_bin), expected_sha)
            for doc in ("README.md", "ROADMAP.md", "ATTRIBUTION.md", "LICENSE-MIT", "LICENSE-APACHE"):
                self.assertTrue((dist_dir / doc).is_file(), f"Missing doc in dist/: {doc}")
            for rdir in ("logs", "autosave", "layouts"):
                self.assertTrue((dist_dir / rdir).is_dir(), f"Missing runtime dir in dist/: {rdir}")
            dist_fonts = dist_dir / "assets" / "fonts"
            self.assertTrue(dist_fonts.is_dir(), "dist/assets/fonts/ missing in portable package")
            ttfs = list(dist_fonts.glob("*.ttf"))
            self.assertGreaterEqual(len(ttfs), 4, f"Expected >=4 .ttf files in dist/assets/fonts/, got {ttfs}")
            for ttf in ttfs:
                self.assertTrue((dist_fonts / f"{ttf.name}.attribution").is_file())

    def test_f10_isolated_package_deletes_target_after_verified_copy(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            ws = stage_isolated_build_workspace(Path(tmp), with_release_bin=True, copy_repo_assets=True)
            self.assertTrue((ws / "target").is_dir())
            res = subprocess.run(
                [sys.executable, str(ws / "build_portable.py"), "--no-build"],
                cwd=str(ws),
                env=isolated_build_env(),
                capture_output=True,
                text=True,
                encoding="utf-8",
                errors="replace",
                timeout=60,
            )
            self.assertEqual(res.returncode, 0, f"build_portable.py failed:\n{res.stdout}\n{res.stderr}")
            self.assertTrue((ws / "dist" / exe_name()).is_file(), "dist/ binary must exist")
            self.assertFalse((ws / "target").exists(), "build_portable.py must delete target/ after verified copy")

    def test_f10_custom_out_outside_target_succeeds_and_cleans_target(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            ws = stage_isolated_build_workspace(Path(tmp), with_release_bin=True, copy_repo_assets=True)
            custom_out = ws / "custom_pkg"
            res = subprocess.run(
                [sys.executable, str(ws / "build_portable.py"), "--no-build", "--out", str(custom_out)],
                cwd=str(ws),
                env=isolated_build_env(),
                capture_output=True,
                text=True,
                encoding="utf-8",
                errors="replace",
                timeout=60,
            )
            self.assertEqual(res.returncode, 0, f"build_portable.py --out failed:\n{res.stdout}\n{res.stderr}")
            self.assertTrue((custom_out / exe_name()).is_file())
            self.assertFalse((ws / "target").exists(), "target/ must be deleted when --out is outside target/")

    def test_f10_readme_documents_dist_and_target_cleanup(self) -> None:
        readme = (REPO_ROOT / "README.md").read_text(encoding="utf-8")
        self.assertIn("dist/", readme, "README.md must document dist/ portable output")
        self.assertRegex(
            readme.lower(),
            r"(apaga|remove|limpa|delet).{0,40}target",
            "README.md must document target/ cleanup after portable packaging",
        )
