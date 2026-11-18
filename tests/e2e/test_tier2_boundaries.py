"""Tier 2 E2E hostile, boundary, and error-handling tests across all 10 features (F1-F10)."""

from __future__ import annotations

import json
import os
import re
import shutil
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
    REPO_ROOT,
    assert_no_panic_or_crash_log,
    collect_image_sequence,
    exe_name,
    feature_tags,
    find_neoveg_bin,
    find_preset_for_format,
    is_clean_mft_unavailable,
    isolated_build_env,
    run_ffprobe_oracle,
    run_neoveg,
    skip_cargo_gates,
    stage_isolated_build_workspace,
    validate_adts_aac,
    validate_attribution_sidecar,
    validate_avi,
    validate_cineon,
    validate_dpx,
    validate_exr,
    validate_flac,
    validate_iso_bmff,
    validate_mkv,
    validate_sfnt_font,
    write_bmp,
    write_neoveg_project,
    write_wav,
)

TIER = 2


def _git(*args: str) -> str:
    return subprocess.run(
        ["git", *args],
        cwd=str(REPO_ROOT),
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=30,
        check=True,
    ).stdout


@feature_tags("F1")
class TestTier2F1HostileHygiene(unittest.TestCase):
    def test_f1_t2_no_unattributed_assets_in_tracked_tree(self) -> None:
        tracked = _git("ls-files").splitlines()
        asset_exts = (".png", ".jpg", ".jpeg", ".svg", ".ttf", ".otf", ".ico", ".webp")
        missing = []
        for rel in tracked:
            if rel.lower().endswith(asset_exts):
                p = REPO_ROOT / rel
                sidecar = p.parent / f"{p.name}.attribution"
                if not sidecar.is_file():
                    missing.append(rel)
        self.assertEqual(missing, [], f"Tracked assets missing .attribution sidecar: {missing}")

    def test_f1_t2_no_forbidden_dirs_tracked(self) -> None:
        tracked = _git("ls-files").splitlines()
        bad_dirs = ("manuais/", "plan/", "test_media/", "design de layouts/", "fixtures/", ".obsidian/")
        bad = [p for p in tracked if p.startswith(bad_dirs)]
        self.assertEqual(bad, [], f"Forbidden local reference/media directories tracked in git: {bad}")

    def test_f1_t2_no_secrets_or_absolute_paths_in_docs(self) -> None:
        doc_files = [
            REPO_ROOT / "README.md",
            REPO_ROOT / "ATTRIBUTION.md",
            REPO_ROOT / "ROADMAP.md",
            *(sorted((REPO_ROOT / "docs").rglob("*.md")) if (REPO_ROOT / "docs").is_dir() else []),
        ]
        bad_patterns = [
            r"[A-Za-z]:\\Users\\",
            r"/home/[a-zA-Z0-9._-]+/",
            r"ghp_[A-Za-z0-9]{20,}",
            r"AIza[0-9A-Za-z\-_]{30,}",
            r"AKIA[0-9A-Z]{16}",
        ]
        hits: list[str] = []
        for df in doc_files:
            if not df.is_file():
                continue
            txt = df.read_text(encoding="utf-8", errors="replace")
            for pat in bad_patterns:
                if re.search(pat, txt):
                    hits.append(f"{df.relative_to(REPO_ROOT)} matched {pat}")
        self.assertEqual(hits, [], f"Host-local paths or secrets found in docs: {hits}")

    def test_f1_t2_check_licenses_rejects_invalid_target_gracefully(self) -> None:
        res = subprocess.run(
            [sys.executable, str(REPO_ROOT / "tools" / "check_licenses.py"), "--target", "invalid-nonexistent-triple"],
            cwd=str(REPO_ROOT),
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=30,
        )
        self.assertNotEqual(res.returncode, 0, "Invalid target triple must fail non-zero")

    def test_f1_t2_neoveg_cli_unknown_flags_and_excess_args_no_panic(self) -> None:
        bin_path = find_neoveg_bin()
        for argv in (
            ["--definitely-unknown-flag"],
            ["--render"],
            ["--encode", "only_one.wav"],
            [f"arg{i}" for i in range(11)],
        ):
            res, snap = run_neoveg(argv, bin_path=bin_path)
            assert_no_panic_or_crash_log(res, snap)
            self.assertNotEqual(res.returncode, 0, f"Invalid CLI args {argv} should fail non-zero")
            self.assertTrue((res.stderr or "").strip(), f"Expected error message on stderr for {argv}")


@feature_tags("F2")
class TestTier2F2SelawikBoundaries(unittest.TestCase):
    def test_f2_t2_selawik_ttf_not_truncated_or_html_404(self) -> None:
        fonts_dir = REPO_ROOT / "assets" / "fonts"
        for fname in ("selawk.ttf", "selawksb.ttf"):
            validate_sfnt_font(fonts_dir / fname, expected_family_substr="Selawik")

    def test_f2_t2_selawik_attribution_no_empty_or_placeholder_fields(self) -> None:
        fonts_dir = REPO_ROOT / "assets" / "fonts"
        for fname in ("selawk.ttf", "selawksb.ttf"):
            validate_attribution_sidecar(
                fonts_dir / f"{fname}.attribution",
                expected_asset=fname,
                required_license="OFL-1.1",
            )

    def test_f2_t2_selawik_sidecars_valid_iso_date_and_license_file(self) -> None:
        fonts_dir = REPO_ROOT / "assets" / "fonts"
        for fname in ("selawk.ttf", "selawksb.ttf"):
            data = validate_attribution_sidecar(fonts_dir / f"{fname}.attribution")
            self.assertRegex(data["added"], r"^20\d{2}-\d{2}-\d{2}\b")

    def test_f2_t2_selawik_no_extra_unlicensed_weights(self) -> None:
        fonts_dir = REPO_ROOT / "assets" / "fonts"
        self.assertTrue(fonts_dir.is_dir(), f"Missing {fonts_dir}")
        for p in fonts_dir.iterdir():
            if p.is_file() and p.name.lower().startswith("selaw") and p.suffix.lower() in (".ttf", ".otf"):
                validate_attribution_sidecar(
                    fonts_dir / f"{p.name}.attribution",
                    expected_asset=p.name,
                    required_license="OFL-1.1",
                )

    def test_f2_t2_selawik_file_size_sanity_bounds(self) -> None:
        fonts_dir = REPO_ROOT / "assets" / "fonts"
        for fname in ("selawk.ttf", "selawksb.ttf"):
            p = fonts_dir / fname
            self.assertTrue(p.is_file(), f"Missing {p}")
            sz = p.stat().st_size
            self.assertGreaterEqual(sz, 30_000, f"{p} unexpectedly small ({sz} bytes)")
            self.assertLessEqual(sz, 5_000_000, f"{p} unexpectedly huge ({sz} bytes)")


@feature_tags("F3")
class TestTier2F3InterNotoBoundaries(unittest.TestCase):
    def _inter_and_noto_files(self) -> tuple[list[Path], list[Path]]:
        fonts_dir = REPO_ROOT / "assets" / "fonts"
        self.assertTrue(fonts_dir.is_dir(), f"Missing {fonts_dir}")
        inters = sorted(
            p for p in fonts_dir.iterdir()
            if p.is_file() and p.suffix.lower() in (".ttf", ".otf") and p.name.lower().startswith("inter")
        )
        notos = sorted(
            p for p in fonts_dir.iterdir()
            if p.is_file() and p.suffix.lower() in (".ttf", ".otf") and p.name.lower().startswith("notosans")
        )
        self.assertTrue(inters, f"No Inter font files in {fonts_dir}")
        self.assertTrue(notos, f"No NotoSans font files in {fonts_dir}")
        return inters, notos

    def test_f3_t2_inter_notosans_not_html_or_corrupt_sfnt(self) -> None:
        inters, notos = self._inter_and_noto_files()
        for p in inters:
            validate_sfnt_font(p, expected_family_substr="Inter")
        for p in notos:
            validate_sfnt_font(p, expected_family_substr="Noto Sans")

    def test_f3_t2_inter_has_digit_and_punctuation_glyphs_for_timecode(self) -> None:
        inters, _ = self._inter_and_noto_files()
        reg = next((p for p in inters if "regular" in p.name.lower()), inters[0])
        validate_sfnt_font(reg, expected_family_substr="Inter", required_chars="0123456789:.;-+ ")

    def test_f3_t2_notosans_and_inter_no_placeholder_attribution(self) -> None:
        inters, notos = self._inter_and_noto_files()
        for p in [*inters, *notos]:
            data = validate_attribution_sidecar(
                p.parent / f"{p.name}.attribution",
                expected_asset=p.name,
                required_license="OFL-1.1",
            )
            self.assertTrue(data["source"].startswith("https://"))

    def test_f3_t2_no_orphan_files_in_assets_fonts(self) -> None:
        fonts_dir = REPO_ROOT / "assets" / "fonts"
        self.assertTrue(fonts_dir.is_dir(), f"Missing {fonts_dir}")
        referenced_licenses: set[Path] = set()
        font_names: set[str] = set()
        for p in fonts_dir.iterdir():
            if not p.is_file():
                continue
            if p.suffix.lower() in (".ttf", ".otf"):
                font_names.add(p.name)
                data = validate_attribution_sidecar(fonts_dir / f"{p.name}.attribution", expected_asset=p.name)
                for lf in data["license-file"].split(","):
                    lf = lf.strip()
                    if not lf:
                        continue
                    for cand in (fonts_dir / lf, REPO_ROOT / lf):
                        if cand.is_file():
                            referenced_licenses.add(cand.resolve())
        self.assertTrue(font_names, "No fonts in assets/fonts/")
        for p in fonts_dir.iterdir():
            if not p.is_file():
                continue
            if p.suffix.lower() in (".ttf", ".otf"):
                continue
            if p.name.endswith(".attribution"):
                target_name = p.name[: -len(".attribution")]
                self.assertIn(target_name, font_names, f"Orphan sidecar without matching font: {p}")
            else:
                self.assertIn(
                    p.resolve(),
                    referenced_licenses,
                    f"Orphan unreferenced file in assets/fonts/: {p}",
                )

    def test_f3_t2_license_txt_files_not_truncated(self) -> None:
        fonts_dir = REPO_ROOT / "assets" / "fonts"
        self.assertTrue(fonts_dir.is_dir(), f"Missing {fonts_dir}")
        lic_files = [p for p in fonts_dir.iterdir() if p.is_file() and "license" in p.name.lower()]
        self.assertTrue(lic_files, f"No LICENSE text files in {fonts_dir}")
        for lf in lic_files:
            txt = lf.read_text(encoding="utf-8", errors="replace")
            self.assertGreaterEqual(len(txt), 500, f"Truncated license file: {lf}")
            self.assertIn("SIL OPEN FONT LICENSE", txt.upper())


@feature_tags("F4")
class TestTier2F4UiFontBoundaries(unittest.TestCase):
    def test_f4_t2_no_disallowed_spdx_in_any_attribution_sidecar(self) -> None:
        sidecars = []
        for sub in ("assets", "docs"):
            d = REPO_ROOT / sub
            if d.is_dir():
                sidecars.extend(d.rglob("*.attribution"))
        self.assertTrue(sidecars, "Expected .attribution sidecars in assets/ and docs/")
        for sc in sidecars:
            asset_name = sc.name[: -len(".attribution")]
            data = validate_attribution_sidecar(sc, expected_asset=asset_name)
            self.assertIn(data["license"], ALLOWED_SPDX_LICENSES)

    def test_f4_t2_neoveg_cli_list_commands_and_probe_integrations_zero_panic(self) -> None:
        bin_path = find_neoveg_bin()
        res_lc, snap_lc = run_neoveg(["--list-commands"], bin_path=bin_path)
        assert_no_panic_or_crash_log(res_lc, snap_lc)
        self.assertEqual(res_lc.returncode, 0, res_lc.stderr)
        self.assertGreater(len(res_lc.stdout.splitlines()), 10)

        res_pi, snap_pi = run_neoveg(["--probe-integrations"], bin_path=bin_path)
        assert_no_panic_or_crash_log(res_pi, snap_pi)
        self.assertEqual(res_pi.returncode, 0, f"--probe-integrations failed:\n{res_pi.stderr}")
        parsed = json.loads(res_pi.stdout)
        self.assertIsInstance(parsed, dict)

    def test_f4_t2_neoveg_headless_command_with_corrupt_project_no_panic(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            bad = td / "corrupt.neoveg"
            bad.write_text("{bad json", encoding="utf-8")
            out = td / "out.neoveg"
            res, snap = run_neoveg(["--command", "insert.marker", str(bad), str(out)])
            assert_no_panic_or_crash_log(res, snap, out_dir=td, out_path=out, expect_failed_cleanup=True)
            self.assertNotEqual(res.returncode, 0)

    def test_f4_t2_neoveg_ui_missing_or_corrupt_prefs_fallback(self) -> None:
        fonts_rs = REPO_ROOT / "crates" / "neoveg_ui" / "src" / "fonts.rs"
        self.assertTrue(fonts_rs.is_file(), f"Missing {fonts_rs}")
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            (td / "preferencias.json").write_text("{not valid json!!!", encoding="utf-8")
            res, snap = run_neoveg(["--list-commands"], cwd=td)
            assert_no_panic_or_crash_log(res, snap)
            self.assertEqual(res.returncode, 0, res.stderr)

    def test_f4_t2_zero_unwrap_expect_panic_in_neoveg_ui(self) -> None:
        fonts_rs = REPO_ROOT / "crates" / "neoveg_ui" / "src" / "fonts.rs"
        self.assertTrue(fonts_rs.is_file(), f"Missing {fonts_rs}")
        lib_rs = (REPO_ROOT / "crates" / "neoveg_ui" / "src" / "lib.rs").read_text(encoding="utf-8")
        for lint in ("clippy::unwrap_used", "clippy::expect_used", "clippy::panic"):
            self.assertIn(lint, lib_rs, f"neoveg_ui/src/lib.rs missing {lint}")
        if skip_cargo_gates():
            return
        res = subprocess.run(
            ["cargo", "clippy", "-p", "neoveg_ui", "--all-targets", "--", "-D", "warnings"],
            cwd=str(REPO_ROOT),
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=240,
        )
        self.assertEqual(res.returncode, 0, f"neoveg_ui clippy failed:\n{res.stderr}")


@feature_tags("F5")
class TestTier2F5FlacAudioBoundaries(unittest.TestCase):
    def test_f5_t2_bva_1frame_silence_flac(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            proj = write_neoveg_project(td / "silent.neoveg", width=32, height=24, fps=(25, 1), duration_s=0.04)
            out = td / "short_silence.flac"
            res, snap = run_neoveg(["--render", str(proj), str(out)])
            assert_no_panic_or_crash_log(res, snap, out_dir=td, out_path=out)
            self.assertEqual(res.returncode, 0, res.stderr)
            validate_flac(out, expected_sr=48000, expected_ch=2)

    def test_f5_t2_hostile_truncated_and_bad_crc_flac_input(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            empty_flac = td / "empty.flac"
            empty_flac.write_bytes(b"")
            trunc_flac = td / "trunc.flac"
            trunc_flac.write_bytes(b"fLaC\x80\x00\x00\x22\x00\x10\x00\x10")
            for idx, bad_in in enumerate((empty_flac, trunc_flac)):
                out = td / f"out_{idx}.wav"
                res, snap = run_neoveg(["--encode", str(bad_in), str(out), "sys.aud_wav_16"])
                assert_no_panic_or_crash_log(res, snap, out_dir=td, out_path=out, expect_failed_cleanup=True)
                self.assertNotEqual(res.returncode, 0, f"Corrupt FLAC {bad_in} should fail cleanly")

    def test_f5_t2_hostile_zero_and_extreme_sr_project(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            for idx, bad_sr in enumerate((0, 99_999_999)):
                proj = write_neoveg_project(
                    td / f"bad_sr_{idx}.neoveg",
                    width=32,
                    height=24,
                    duration_s=0.08,
                    sample_rate=bad_sr,
                )
                out = td / f"out_{idx}.flac"
                res, snap = run_neoveg(["--render", str(proj), str(out)])
                assert_no_panic_or_crash_log(res, snap, out_dir=td, out_path=out)
                # sanitize() clamps invalid sample_rate to 48000, so render succeeds cleanly with 48000 Hz
                self.assertEqual(res.returncode, 0, res.stderr)
                validate_flac(out, expected_sr=48000)

    def test_f5_t2_hostile_extreme_gain_and_nan_envelope_to_flac(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            wav = write_wav(td / "tone.wav", sr=48000, channels=2, duration_s=0.16)
            proj_path = write_neoveg_project(td / "p.neoveg", wav_path=wav, duration_s=0.16)
            data = json.loads(proj_path.read_text(encoding="utf-8"))
            data["tracks"][1]["volume"] = 100.0
            proj_path.write_text(json.dumps(data), encoding="utf-8")
            out = td / "clipped.flac"
            res, snap = run_neoveg(["--render", str(proj_path), str(out)])
            assert_no_panic_or_crash_log(res, snap, out_dir=td, out_path=out)
            self.assertEqual(res.returncode, 0, res.stderr)
            validate_flac(out, expected_sr=48000, expected_ch=2)

    def test_f5_t2_unwritable_output_dir_flac(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            proj = write_neoveg_project(td / "p.neoveg", duration_s=0.08)
            blocker = td / "not_a_dir"
            blocker.write_text("file", encoding="utf-8")
            out = blocker / "out.flac"
            res, snap = run_neoveg(["--render", str(proj), str(out)])
            assert_no_panic_or_crash_log(res, snap, out_dir=td)
            self.assertNotEqual(res.returncode, 0)


@feature_tags("F6")
class TestTier2F6SequenceBoundaries(unittest.TestCase):
    def test_f6_t2_bva_minimum_and_odd_dimensions_cin_dpx_exr(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            # Project::sanitize() clamps >=16 and rounds down to even (16x16, 62x46)
            proj_min = write_neoveg_project(td / "min.neoveg", width=2, height=2, fps=(25, 1), duration_s=0.04)
            for ext, val in (
                ("cin", lambda p: validate_cineon(p, 16, 16)),
                ("dpx", lambda p: validate_dpx(p, 16, 16)),
                ("exr", lambda p: validate_exr(p, 16, 16)),
            ):
                base = td / f"min.{ext}"
                res, snap = run_neoveg(["--render", str(proj_min), str(base)])
                assert_no_panic_or_crash_log(res, snap, out_dir=td)
                self.assertEqual(res.returncode, 0, f"min {ext} failed:\n{res.stderr}")
                frames = collect_image_sequence(base, expected_count=1)
                val(frames[0])

    def test_f6_t2_bva_single_frame_range_cin(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            proj = write_neoveg_project(td / "one.neoveg", width=32, height=24, fps=(25, 1), duration_s=0.04)
            base = td / "single.cin"
            res, snap = run_neoveg(["--render", str(proj), str(base)])
            assert_no_panic_or_crash_log(res, snap, out_dir=td)
            self.assertEqual(res.returncode, 0, res.stderr)
            frames = collect_image_sequence(base, expected_count=1)
            validate_cineon(frames[0], 32, 24)
            self.assertFalse((td / "single_000001.cin").exists())

    def test_f6_t2_hostile_corrupt_cin_dpx_exr_input(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            bad_cin = td / "bad.cin"
            bad_cin.write_bytes(b"\x80\x2a\x5f\xd7" + b"\x00" * 32)
            bad_dpx = td / "bad.dpx"
            # Claim huge width/height to probe OOM guard
            dpx_hdr = bytearray(b"SDPX" + b"\x00" * 1660)
            dpx_hdr[772:780] = b"\x7f\xff\xff\xff\x7f\xff\xff\xff"
            bad_dpx.write_bytes(bytes(dpx_hdr))
            bad_exr = td / "bad.exr"
            bad_exr.write_bytes(b"\x76\x2f\x31\x01\x02\x00\x00\x00corrupt")

            for bad_in in (bad_cin, bad_dpx, bad_exr):
                out = td / f"{bad_in.stem}_out.png"
                res, snap = run_neoveg(["--encode", str(bad_in), str(out), "sys.seq_png"])
                assert_no_panic_or_crash_log(res, snap, out_dir=td)
                self.assertNotEqual(res.returncode, 0, f"Corrupt {bad_in.name} should fail cleanly")

    def test_f6_t2_hostile_zero_duration_project_sequence(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            proj = write_neoveg_project(td / "empty.neoveg", width=32, height=24, duration_s=0.0)
            for ext in ("cin", "dpx", "exr"):
                base = td / f"empty.{ext}"
                res, snap = run_neoveg(["--render", str(proj), str(base)])
                assert_no_panic_or_crash_log(res, snap, out_dir=td)
                self.assertNotEqual(res.returncode, 0, f"Zero-duration {ext} render must fail cleanly")
                self.assertFalse((td / f"empty_000000.{ext}").exists())

    def test_f6_t2_hostile_unwritable_seq_path(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            proj = write_neoveg_project(td / "p.neoveg", width=32, height=24, duration_s=0.04)
            blocker = td / "file_not_dir"
            blocker.write_text("x", encoding="utf-8")
            res, snap = run_neoveg(["--render", str(proj), str(blocker / "seq.cin")])
            assert_no_panic_or_crash_log(res, snap, out_dir=td)
            self.assertNotEqual(res.returncode, 0)


@feature_tags("F7")
class TestTier2F7PureRustContainerBoundaries(unittest.TestCase):
    def test_f7_t2_bva_odd_dimensions_mov_mkv_avi(self) -> None:
        mkv_ffv1 = find_preset_for_format(["MkvFfv1"], required_keywords=["ffv1"], description="MKV FFV1")
        mov_mjpeg = find_preset_for_format(
            ["MovMjpeg", "MovPhotoJpeg", "MovUncompressed"],
            required_keywords=["mov"],
            forbidden_keywords=["h264", "hevc", "av1"],
            description="MOV pure-Rust",
        )
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            bmp_odd = write_bmp(td / "odd_63x47.bmp", width=63, height=47)
            for pid, ext, val in (
                (mkv_ffv1, "mkv", lambda p: validate_mkv(p, expected_video_codec_ids={"V_FFV1"})),
                (mov_mjpeg, "mov", lambda p: validate_iso_bmff(p, require_qt_brand=True)),
                ("sys.bc_avi_mjpeg", "avi", validate_avi),
            ):
                out = td / f"odd.{ext}"
                res, snap = run_neoveg(["--encode", str(bmp_odd), str(out), pid])
                assert_no_panic_or_crash_log(res, snap, out_dir=td, out_path=out)
                if res.returncode == 0:
                    val(out)
                    run_ffprobe_oracle(out)

    def test_f7_t2_bva_video_only_no_audio_stream(self) -> None:
        mkv_ffv1 = find_preset_for_format(["MkvFfv1"], required_keywords=["ffv1"], description="MKV FFV1")
        mov_p = find_preset_for_format(
            ["MovUncompressed", "MovMjpeg", "MovPhotoJpeg"],
            required_keywords=["mov"],
            forbidden_keywords=["h264", "hevc", "av1"],
            description="MOV pure-Rust",
        )
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            proj = write_neoveg_project(td / "video_only.neoveg", width=64, height=48, duration_s=0.12, wav_path=None)
            out_mkv = td / "vo.mkv"
            r1, s1 = run_neoveg(["--encode", str(proj), str(out_mkv), mkv_ffv1])
            assert_no_panic_or_crash_log(r1, s1, out_dir=td, out_path=out_mkv)
            self.assertEqual(r1.returncode, 0, r1.stderr)
            validate_mkv(out_mkv, expected_w=64, expected_h=48, expect_audio=False)
            run_ffprobe_oracle(out_mkv)

            out_mov = td / "vo.mov"
            r2, s2 = run_neoveg(["--encode", str(proj), str(out_mov), mov_p])
            assert_no_panic_or_crash_log(r2, s2, out_dir=td, out_path=out_mov)
            self.assertEqual(r2.returncode, 0, r2.stderr)
            validate_iso_bmff(out_mov, expected_w=64, expected_h=48, require_qt_brand=True, expect_audio=False)
            run_ffprobe_oracle(out_mov)

    def test_f7_t2_hostile_corrupt_mov_and_mkv_input(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            # Cyclic / overflowing ISO-BMFF box
            bad_mov = td / "bad.mov"
            bad_mov.write_bytes(b"\x00\x00\x00\x08ftyp" + b"\x00\x00\x00\x01moov\x00\x00\x00\x00\x00\x00\x00\x00")
            # Invalid EBML VINT leading 0x00 and truncated Segment
            bad_mkv = td / "bad.mkv"
            bad_mkv.write_bytes(b"\x1a\x45\xdf\xa3\x84\x42\x82\x80\x00\xff\xff")

            for bad_in in (bad_mov, bad_mkv):
                out = td / f"{bad_in.stem}_out.wav"
                res, snap = run_neoveg(["--encode", str(bad_in), str(out), "sys.aud_wav_16"])
                assert_no_panic_or_crash_log(res, snap, out_dir=td, out_path=out, expect_failed_cleanup=True)
                self.assertNotEqual(res.returncode, 0)

    def test_f7_t2_hostile_corrupt_avi_input(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            bad_avi = td / "bad.avi"
            bad_avi.write_bytes(b"RIFF\xff\xff\xff\x7fAVI LIST\xff\xff\xff\x7fhdrlavih")
            out = td / "out.wav"
            res, snap = run_neoveg(["--encode", str(bad_avi), str(out), "sys.aud_wav_16"])
            assert_no_panic_or_crash_log(res, snap, out_dir=td, out_path=out, expect_failed_cleanup=True)
            self.assertNotEqual(res.returncode, 0)

    def test_f7_t2_bva_unicode_and_spaces_output_path(self) -> None:
        mkv_ffv1 = find_preset_for_format(["MkvFfv1"], required_keywords=["ffv1"], description="MKV FFV1")
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            wav = write_wav(td / "tone.wav", sr=48000, channels=2, duration_s=0.12)
            proj = write_neoveg_project(td / "p.neoveg", width=64, height=48, duration_s=0.12, wav_path=wav)
            out = td / "saída teste ação.mkv"
            res, snap = run_neoveg(["--encode", str(proj), str(out), mkv_ffv1])
            assert_no_panic_or_crash_log(res, snap, out_dir=td, out_path=out)
            self.assertEqual(res.returncode, 0, res.stderr)
            validate_mkv(out, expected_w=64, expected_h=48, expected_video_codec_ids={"V_FFV1"})
            run_ffprobe_oracle(out)


@feature_tags("F8")
class TestTier2F8MfBoundaries(unittest.TestCase):
    def test_f8_t2_bva_odd_dimensions_mf_containers(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            bmp_odd = write_bmp(td / "odd.bmp", width=63, height=47)
            out = td / "odd.mp4"
            res, snap = run_neoveg(["--encode", str(bmp_odd), str(out), "sys.bc_match_mp4"])
            assert_no_panic_or_crash_log(res, snap, out_dir=td, out_path=out)
            self.assertEqual(res.returncode, 0, res.stderr)
            info = validate_iso_bmff(out, expected_video_fourccs={b"avc1", b"avc3"})
            self.assertEqual(info["video"]["width"] % 2, 0)
            self.assertEqual(info["video"]["height"] % 2, 0)
            run_ffprobe_oracle(out)

    def test_f8_t2_bva_minimum_dimensions_mf_encoder(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            proj = write_neoveg_project(td / "tiny.neoveg", width=16, height=16, fps=(25, 1), duration_s=0.12)
            out = td / "tiny.mp4"
            res, snap = run_neoveg(["--encode", str(proj), str(out), "sys.bc_match_mp4"])
            assert_no_panic_or_crash_log(res, snap, out_dir=td, out_path=out, expect_failed_cleanup=(res.returncode != 0))
            if res.returncode == 0:
                validate_iso_bmff(out, expected_video_fourccs={b"avc1", b"avc3"})
            else:
                self.assertTrue(is_clean_mft_unavailable(res, snap, out_dir=td, out_path=out))

    def test_f8_t2_hostile_corrupt_mp4_mkv_aac_to_decoder(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            for name, payload in (
                ("corrupt.mp4", b"\x00\x00\x00\x18ftypmp42" + b"\xff" * 40),
                ("corrupt.mkv", b"\x1a\x45\xdf\xa3\x01\x00\x00\x00"),
                ("corrupt.aac", b"\xff\xf1\x50\x80\x00\x1f\xfc\xde\xad"),
            ):
                bad = td / name
                bad.write_bytes(payload)
                out = td / f"{bad.stem}_out.wav"
                res, snap = run_neoveg(["--encode", str(bad), str(out), "sys.aud_wav_16"])
                assert_no_panic_or_crash_log(res, snap, out_dir=td, out_path=out, expect_failed_cleanup=True)
                self.assertNotEqual(res.returncode, 0, f"Corrupt {name} should fail cleanly")

    def test_f8_t2_mft_unavailable_cleanup_oracle(self) -> None:
        # Exercise an optional codec (AV1 or VP9) and verify atomic cleanup if unavailable
        av1_p = find_preset_for_format(["Mp4Av1", "MkvAv1", "MovAv1"], required_keywords=["av1"], description="AV1")
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            proj = write_neoveg_project(td / "p.neoveg", width=128, height=72, duration_s=0.12)
            out = td / "probe_av1.mp4"
            res, snap = run_neoveg(["--encode", str(proj), str(out), av1_p])
            assert_no_panic_or_crash_log(res, snap, out_dir=td, out_path=out, expect_failed_cleanup=(res.returncode != 0))
            if res.returncode != 0:
                self.assertTrue(
                    is_clean_mft_unavailable(res, snap, out_dir=td, out_path=out),
                    f"Unavailable optional MFT must exit cleanly without temp leaks:\n{res.stderr}",
                )
                self.assertFalse(out.exists())

    def test_f8_t2_audio_only_to_aac_adts(self) -> None:
        aac_p = find_preset_for_format(["AacAdts"], required_keywords=["aac"], description="AAC ADTS")
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            wav = write_wav(td / "tone.wav", sr=48000, channels=2, duration_s=0.20)
            out = td / "stem.aac"
            res, snap = run_neoveg(["--encode", str(wav), str(out), aac_p])
            assert_no_panic_or_crash_log(res, snap, out_dir=td, out_path=out)
            self.assertEqual(res.returncode, 0, res.stderr)
            validate_adts_aac(out)


@feature_tags("F9")
class TestTier2F9CliPresetBoundaries(unittest.TestCase):
    def test_f9_t2_hostile_unknown_preset_id(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            wav = write_wav(td / "tone.wav", sr=48000, channels=2, duration_s=0.12)
            out = td / "out.wav"
            res, snap = run_neoveg(["--encode", str(wav), str(out), "sys.does_not_exist_xyz"])
            assert_no_panic_or_crash_log(res, snap, out_dir=td, out_path=out, expect_failed_cleanup=True)
            self.assertNotEqual(res.returncode, 0)
            self.assertIn("desconhecido", (res.stderr or "").lower())

    def test_f9_t2_hostile_missing_input_and_bad_cli_args(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            missing = td / "missing.neoveg"
            out = td / "out.avi"
            res1, snap1 = run_neoveg(["--render", str(missing), str(out)])
            assert_no_panic_or_crash_log(res1, snap1, out_dir=td, out_path=out, expect_failed_cleanup=True)
            self.assertNotEqual(res1.returncode, 0)

            res2, snap2 = run_neoveg(["--command", "nonexistent.command.id"])
            assert_no_panic_or_crash_log(res2, snap2)
            self.assertNotEqual(res2.returncode, 0)

    def test_f9_t2_hostile_corrupt_neoveg_project_json(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            # Cyclic parent_track_id + invalid fps 0/0
            bad_proj = td / "cyclic.neoveg"
            bad_proj.write_text(
                json.dumps(
                    {
                        "settings": {
                            "width": 64,
                            "height": 48,
                            "fps": {"num": 0, "den": 0},
                            "sample_rate": 48000,
                        },
                        "media": [{"id": 1, "name": "C", "source": {"Generated": "ColorBars"}, "duration": None, "has_video": True, "has_audio": False}],
                        "tracks": [
                            {"id": 10, "name": "V1", "kind": "Video", "parent_track_id": 10, "events": [{"id": 100, "name": "E", "media_id": 1, "start": {"ticks": 0}, "length": {"ticks": 141120000}}]}
                        ],
                    }
                ),
                encoding="utf-8",
            )
            out = td / "sanitized.avi"
            res, snap = run_neoveg(["--render", str(bad_proj), str(out)])
            assert_no_panic_or_crash_log(res, snap, out_dir=td, out_path=out)
            self.assertEqual(res.returncode, 0, f"Sanitizer should recover cyclic/zero-fps project:\n{res.stderr}")
            validate_avi(out, expected_w=64, expected_h=48)

    def test_f9_t2_hostile_extension_mismatch_path(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            proj = write_neoveg_project(td / "p.neoveg", width=64, height=48, duration_s=0.08)
            out = td / "out.unknown_ext_xyz"
            res, snap = run_neoveg(["--render", str(proj), str(out)])
            assert_no_panic_or_crash_log(res, snap, out_dir=td, out_path=out)
            # RenderFormat::from_path falls back to AviMjpeg cleanly
            self.assertEqual(res.returncode, 0, res.stderr)
            validate_avi(out, expected_w=64, expected_h=48)

    def test_f9_t2_fuzz_tests_cargo_gate(self) -> None:
        # Verify fuzz_tests module exists and covers new formats
        fuzz_rs = (REPO_ROOT / "crates" / "neoveg_engine" / "src" / "fuzz_tests.rs").read_text(encoding="utf-8")
        self.assertIn("fuzz_encoder_parsers", fuzz_rs)
        if skip_cargo_gates():
            return
        res = subprocess.run(
            ["cargo", "test", "-p", "neoveg_engine", "--lib", "fuzz_tests"],
            cwd=str(REPO_ROOT),
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=240,
        )
        assert_no_panic_or_crash_log(res)
        self.assertEqual(res.returncode, 0, f"fuzz_tests failed:\n{res.stdout}\n{res.stderr}")


@feature_tags("F10")
class TestTier2F10PortableBuildBoundaries(unittest.TestCase):
    def test_f10_t2_rejects_target_cpu_native_and_retains_target(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            ws = stage_isolated_build_workspace(Path(tmp), with_release_bin=True, copy_repo_assets=True)
            env = isolated_build_env({"RUSTFLAGS": "-C target-cpu=native"})
            res = subprocess.run(
                [sys.executable, str(ws / "build_portable.py"), "--no-build"],
                cwd=str(ws),
                env=env,
                capture_output=True,
                text=True,
                encoding="utf-8",
                errors="replace",
                timeout=30,
            )
            self.assertNotEqual(res.returncode, 0)
            self.assertIn("target-cpu=native", f"{res.stdout}\n{res.stderr}")
            self.assertTrue((ws / "target").is_dir(), "target/ must be retained when build_portable.py aborts")

    def test_f10_t2_missing_release_binary_no_build_retains_target(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            ws = stage_isolated_build_workspace(Path(tmp), with_release_bin=False, copy_repo_assets=True)
            (ws / "target").mkdir(parents=True, exist_ok=True)
            res = subprocess.run(
                [sys.executable, str(ws / "build_portable.py"), "--no-build"],
                cwd=str(ws),
                env=isolated_build_env(),
                capture_output=True,
                text=True,
                encoding="utf-8",
                errors="replace",
                timeout=30,
            )
            self.assertNotEqual(res.returncode, 0)
            self.assertTrue((ws / "target").is_dir(), "target/ must be retained when release binary is missing")

    def test_f10_t2_proprietary_os_font_in_assets_fonts_aborts_and_retains_target(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            ws = stage_isolated_build_workspace(Path(tmp), with_release_bin=True, copy_repo_assets=True)
            fonts_dir = ws / "assets" / "fonts"
            fonts_dir.mkdir(parents=True, exist_ok=True)
            (fonts_dir / "segoeui.ttf").write_bytes(b"\x00\x01\x00\x00" + b"\x00" * 64)
            (fonts_dir / "segoeui.ttf.attribution").write_text(
                "asset: segoeui.ttf\ntitle: Segoe\nauthor: MS\nsource: local\nlicense: OFL-1.1\nlicense-file: x\nadded: 2026-11-18\nnotes: test\n",
                encoding="utf-8",
            )
            res = subprocess.run(
                [sys.executable, str(ws / "build_portable.py"), "--no-build"],
                cwd=str(ws),
                env=isolated_build_env(),
                capture_output=True,
                text=True,
                encoding="utf-8",
                errors="replace",
                timeout=30,
            )
            self.assertNotEqual(
                res.returncode,
                0,
                "build_portable.py must reject proprietary OS font segoeui.ttf even with .attribution",
            )
            self.assertTrue((ws / "target").is_dir(), "target/ must be retained on proprietary font rejection")

    def test_f10_t2_unattributed_ttf_in_assets_fonts_aborts_and_retains_target(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            ws = stage_isolated_build_workspace(Path(tmp), with_release_bin=True, copy_repo_assets=True)
            fonts_dir = ws / "assets" / "fonts"
            fonts_dir.mkdir(parents=True, exist_ok=True)
            (fonts_dir / "rogue_unattributed.ttf").write_bytes(b"\x00\x01\x00\x00" + b"\x00" * 64)
            res = subprocess.run(
                [sys.executable, str(ws / "build_portable.py"), "--no-build"],
                cwd=str(ws),
                env=isolated_build_env(),
                capture_output=True,
                text=True,
                encoding="utf-8",
                errors="replace",
                timeout=30,
            )
            self.assertNotEqual(res.returncode, 0, "build_portable.py must reject unattributed .ttf")
            self.assertTrue((ws / "target").is_dir(), "target/ must be retained on unattributed font rejection")

    def test_f10_t2_out_inside_target_safety_guard_preserves_out_and_target(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            ws = stage_isolated_build_workspace(Path(tmp), with_release_bin=True, copy_repo_assets=True)
            inside_target = ws / "target" / "portable_test"
            res = subprocess.run(
                [sys.executable, str(ws / "build_portable.py"), "--no-build", "--out", str(inside_target)],
                cwd=str(ws),
                env=isolated_build_env(),
                capture_output=True,
                text=True,
                encoding="utf-8",
                errors="replace",
                timeout=30,
            )
            if res.returncode == 0:
                self.assertTrue(
                    (inside_target / exe_name()).is_file(),
                    "build_portable.py deleted its own --out inside target/!",
                )
            else:
                self.assertTrue((ws / "target").is_dir())

    def test_f10_t2_forbidden_binary_ffmpeg_in_package_aborts_and_retains_target(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            ws = stage_isolated_build_workspace(Path(tmp), with_release_bin=True, copy_repo_assets=True)
            fonts_dir = ws / "assets" / "fonts"
            fonts_dir.mkdir(parents=True, exist_ok=True)
            (fonts_dir / "ffmpeg.exe").write_bytes(b"MZ" + b"\x00" * 32)
            res = subprocess.run(
                [sys.executable, str(ws / "build_portable.py"), "--no-build"],
                cwd=str(ws),
                env=isolated_build_env(),
                capture_output=True,
                text=True,
                encoding="utf-8",
                errors="replace",
                timeout=30,
            )
            self.assertNotEqual(res.returncode, 0, "build_portable.py must reject ffmpeg.exe inside packaged tree")
            self.assertTrue((ws / "target").is_dir())
