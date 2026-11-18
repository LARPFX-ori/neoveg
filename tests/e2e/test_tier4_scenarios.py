"""Tier 4 E2E real-world application scenarios and repository quality gates (5 scenarios)."""

from __future__ import annotations

import platform
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
    REPO_ROOT,
    allow_destructive_target_clean,
    assert_no_panic_or_crash_log,
    collect_image_sequence,
    exe_name,
    feature_tags,
    find_neoveg_bin,
    find_preset_for_format,
    is_clean_mft_unavailable,
    isolated_build_env,
    resolve_vp9_preset_and_validator,
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
    validate_wav,
    validate_web_multi_group_outputs,
    write_neoveg_project,
    write_wav,
)

TIER = 4


class TestTier4Scenarios(unittest.TestCase):
    @feature_tags("F1", "F2", "F3", "F10")
    def test_scenario_1_portable_dist_and_execution(self) -> None:
        """Scenario 1: Portable distribution -> dist/ assets + docs + target/ cleanup -> dist exe encodes."""
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            if allow_destructive_target_clean():
                # Explicit destructive mode (M5 final gate): run build_portable.py directly on repo root.
                res = subprocess.run(
                    [sys.executable, str(REPO_ROOT / "build_portable.py")],
                    cwd=str(REPO_ROOT),
                    capture_output=True,
                    text=True,
                    encoding="utf-8",
                    errors="replace",
                    timeout=1200,
                )
                self.assertEqual(res.returncode, 0, f"build_portable.py failed:\n{res.stdout}\n{res.stderr}")
                self.assertFalse(
                    (REPO_ROOT / "target").exists(),
                    "target/ must be deleted after verified dist/ copy",
                )
                dist_dir = REPO_ROOT / "dist"
                # Copy dist_dir to temp before executing neoveg.exe so runtime logs/autosave don't dirty repo dist/
                run_dist = td / "dist_run"
                shutil.copytree(dist_dir, run_dist)
                target_root = REPO_ROOT
            else:
                # Safe concurrent mode: test build_portable.py --no-build in isolated temp workspace
                self.assertTrue((REPO_ROOT / "assets" / "fonts").is_dir(), "assets/fonts/ missing in repo")
                ws = stage_isolated_build_workspace(td, with_release_bin=True, copy_repo_assets=True)
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
                self.assertFalse((ws / "target").exists(), "ws/target/ must be deleted after dist/ verification")
                dist_dir = ws / "dist"
                run_dist = dist_dir
                target_root = ws

            dist_exe = dist_dir / exe_name()
            self.assertTrue(dist_exe.is_file(), f"Missing {dist_exe}")
            for doc in ("README.md", "ROADMAP.md", "ATTRIBUTION.md", "LICENSE-MIT", "LICENSE-APACHE"):
                self.assertTrue((dist_dir / doc).is_file(), f"Missing {doc} in dist/")
            dist_fonts = dist_dir / "assets" / "fonts"
            self.assertTrue(dist_fonts.is_dir(), "dist/assets/fonts/ missing")
            validate_sfnt_font(dist_fonts / "selawk.ttf", expected_family_substr="Selawik")
            validate_sfnt_font(dist_fonts / "selawksb.ttf", expected_family_substr="Selawik")
            validate_attribution_sidecar(dist_fonts / "selawk.ttf.attribution", base_dir=target_root)

            # Execute the packaged portable binary on a synthetic project
            proj = write_neoveg_project(td / "sc1.neoveg", width=64, height=48, duration_s=0.12)
            out_avi = td / "sc1_out.avi"
            r_enc, snap = run_neoveg(
                ["--encode", str(proj), str(out_avi), "sys.bc_avi_mjpeg"],
                bin_path=run_dist / exe_name(),
                cwd=td,
            )
            assert_no_panic_or_crash_log(r_enc, snap, out_dir=td, out_path=out_avi)
            self.assertEqual(r_enc.returncode, 0, r_enc.stderr)
            validate_avi(out_avi, expected_w=64, expected_h=48)

    @feature_tags("F5", "F6", "F7", "F9")
    def test_scenario_2_mastering_delivery_package(self) -> None:
        """Scenario 2: Multi-format mastering package (.mkv FFV1 + .mov + .cin + .flac)."""
        mkv_ffv1 = find_preset_for_format(["MkvFfv1"], required_keywords=["ffv1"], description="MKV FFV1")
        mov_p = find_preset_for_format(
            ["MovMjpeg", "MovPhotoJpeg", "MovUncompressed"],
            required_keywords=["mov"],
            forbidden_keywords=["h264", "hevc", "av1"],
            description="MOV intermediate",
        )
        flac_p = find_preset_for_format(["Flac"], required_keywords=["flac"], description="FLAC stem")
        cin_p = find_preset_for_format(["CineonSequence", "Cineon"], required_keywords=["cin"], description="Cineon")

        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            wav = write_wav(td / "master_audio.wav", sr=48000, channels=2, duration_s=0.20)
            proj = write_neoveg_project(
                td / "master.neoveg",
                width=64,
                height=48,
                fps=(25, 1),
                duration_s=0.20,
                wav_path=wav,
                markers=[{"id": 500, "position": {"ticks": 0}, "label": "Start"}],
            )

            # 1. Archival lossless MKV FFV1
            out_mkv = td / "master_archive.mkv"
            r1, s1 = run_neoveg(["--encode", str(proj), str(out_mkv), mkv_ffv1])
            assert_no_panic_or_crash_log(r1, s1, out_dir=td, out_path=out_mkv)
            self.assertEqual(r1.returncode, 0, r1.stderr)
            validate_mkv(
                out_mkv, expected_w=64, expected_h=48, expected_video_codec_ids={"V_FFV1"}, expect_audio=True
            )
            run_ffprobe_oracle(out_mkv)

            # 2. Post-production intermediate QuickTime MOV
            out_mov = td / "master_intermediate.mov"
            r2, s2 = run_neoveg(["--encode", str(proj), str(out_mov), mov_p])
            assert_no_panic_or_crash_log(r2, s2, out_dir=td, out_path=out_mov)
            self.assertEqual(r2.returncode, 0, r2.stderr)
            validate_iso_bmff(out_mov, expected_w=64, expected_h=48, require_qt_brand=True, expect_audio=True)
            run_ffprobe_oracle(out_mov)

            # 3. DI/VFX Cineon 10-bit sequence + sidecar WAV
            out_cin = td / "vfx_scan.cin"
            r3, s3 = run_neoveg(["--encode", str(proj), str(out_cin), cin_p])
            assert_no_panic_or_crash_log(r3, s3, out_dir=td)
            self.assertEqual(r3.returncode, 0, r3.stderr)
            cin_frames = collect_image_sequence(out_cin, expected_count=5)
            validate_cineon(cin_frames[0], 64, 48)
            validate_wav(td / "vfx_scan.wav", expected_sr=48000, expected_ch=2)

            # 4. Audio stem master FLAC
            out_flac = td / "audio_stem.flac"
            r4, s4 = run_neoveg(["--encode", str(proj), str(out_flac), flac_p])
            assert_no_panic_or_crash_log(r4, s4, out_dir=td, out_path=out_flac)
            self.assertEqual(r4.returncode, 0, r4.stderr)
            validate_flac(out_flac, expected_sr=48000, expected_ch=2)
            run_ffprobe_oracle(out_flac)

    @feature_tags("F4", "F5", "F7", "F9")
    def test_scenario_3_batch_preset_group_and_multiformat_transcode(self) -> None:
        """Scenario 3: Render intermediate AVI, then batch-encode via preset group + Resolve 20 outputs."""
        flac_p = find_preset_for_format(["Flac"], required_keywords=["flac"], description="FLAC")
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            wav = write_wav(td / "tone.wav", sr=48000, channels=2, duration_s=0.20)
            proj = write_neoveg_project(
                td / "src.neoveg", width=128, height=72, fps=(25, 1), duration_s=0.20, wav_path=wav
            )
            src_avi = td / "source_clip.avi"
            r0, s0 = run_neoveg(["--render", str(proj), str(src_avi)])
            assert_no_panic_or_crash_log(r0, s0, out_dir=td, out_path=src_avi)
            self.assertEqual(r0.returncode, 0, r0.stderr)
            validate_avi(src_avi, expected_w=128, expected_h=72, expect_audio=True)

            # Batch preset group delivery
            r1, s1 = run_neoveg(["--encode", str(src_avi), str(td / "batch.mp4"), "grp.web_multi"])
            assert_no_panic_or_crash_log(r1, s1, out_dir=td)
            self.assertEqual(r1.returncode, 0, r1.stderr)
            validate_web_multi_group_outputs(td, "batch")

            # Re-encode duplicate collision increment check
            r2, s2 = run_neoveg(["--encode", str(src_avi), str(td / "stem.flac"), flac_p])
            assert_no_panic_or_crash_log(r2, s2, out_dir=td)
            self.assertEqual(r2.returncode, 0, r2.stderr)
            validate_flac(td / "stem.flac", expected_sr=48000, expected_ch=2)

            r3, s3 = run_neoveg(["--encode", str(src_avi), str(td / "stem.flac"), flac_p])
            assert_no_panic_or_crash_log(r3, s3, out_dir=td)
            self.assertEqual(r3.returncode, 0, r3.stderr)
            self.assertTrue((td / "stem_1.flac").is_file(), "Incrementing output stem_1.flac missing")
            validate_flac(td / "stem_1.flac", expected_sr=48000, expected_ch=2)

    @feature_tags("F8", "F9")
    def test_scenario_4_mf_capability_matrix(self) -> None:
        """Scenario 4: Windows MF Resolve 20 capability matrix with strict codec verification."""
        matrix = [
            ("Mp4H264", ["mp4", "h264"], "mp4", True, "h264", lambda p: validate_iso_bmff(p, expected_video_fourccs={b"avc1", b"avc3"})),
            ("Mp4Hevc", ["hevc"], "mp4", False, "hevc", lambda p: validate_iso_bmff(p, expected_video_fourccs={b"hvc1", b"hev1"})),
            ("MovHevc", ["mov", "hevc"], "mov", False, "hevc", lambda p: validate_iso_bmff(p, expected_video_fourccs={b"hvc1", b"hev1"})),
            ("MkvH264", ["mkv", "h264"], "mkv", True, "h264", lambda p: validate_mkv(p, expected_video_codec_ids={"V_MPEG4/ISO/AVC"})),
            ("MkvHevc", ["mkv", "hevc"], "mkv", False, "hevc", lambda p: validate_mkv(p, expected_video_codec_ids={"V_MPEGH/ISO/HEVC"})),
            ("Mp4Av1", ["mp4", "av1"], "mp4", False, "av1", lambda p: validate_iso_bmff(p, expected_video_fourccs={b"av01"})),
            ("MovMpeg4", ["mov", "mpeg4"], "mov", False, "mpeg4", lambda p: validate_iso_bmff(p, expected_video_fourccs={b"mp4v"})),
        ]
        resolved = [
            (
                find_preset_for_format([fmt], required_keywords=kws, description=fmt),
                ext,
                req_win,
                ffp_codec,
                val,
            )
            for fmt, kws, ext, req_win, ffp_codec, val in matrix
        ]
        vp9_pid, vp9_ext, vp9_val = resolve_vp9_preset_and_validator()
        resolved.append((vp9_pid, vp9_ext, False, "vp9", vp9_val))
        aac_p = find_preset_for_format(["AacAdts"], required_keywords=["aac"], description="AacAdts")

        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            wav = write_wav(td / "tone.wav", sr=48000, channels=2, duration_s=0.16)
            proj = write_neoveg_project(
                td / "mf.neoveg", width=128, height=72, fps=(25, 1), duration_s=0.16, wav_path=wav
            )
            for pid, ext, req_win, ffp_codec, val in resolved:
                out = td / f"mf_{ffp_codec}.{ext}"
                res, snap = run_neoveg(["--encode", str(proj), str(out), pid])
                assert_no_panic_or_crash_log(
                    res, snap, out_dir=td, out_path=out, expect_failed_cleanup=(res.returncode != 0)
                )
                if res.returncode == 0:
                    val(out)
                    probe = run_ffprobe_oracle(out)
                    if probe:
                        vstreams = [s for s in probe["streams"] if s.get("codec_type") == "video"]
                        self.assertEqual(vstreams[0]["codec_name"], ffp_codec)
                else:
                    if req_win and platform.system() == "Windows":
                        self.fail(f"Required Windows MF target {pid} failed:\n{res.stderr}")
                    self.assertTrue(is_clean_mft_unavailable(res, snap, out_dir=td, out_path=out))

            out_aac = td / "mf_stem.aac"
            r_aac, s_aac = run_neoveg(["--encode", str(wav), str(out_aac), aac_p])
            assert_no_panic_or_crash_log(r_aac, s_aac, out_dir=td, out_path=out_aac)
            self.assertEqual(r_aac.returncode, 0, r_aac.stderr)
            validate_adts_aac(out_aac)

    @feature_tags("F1", "F2", "F3", "F4", "F10")
    def test_scenario_5_governance_and_typography_gate(self) -> None:
        """Scenario 5: Full governance, license, attribution, typography, fmt, clippy, and UI gate."""
        fonts_dir = REPO_ROOT / "assets" / "fonts"
        self.assertTrue(fonts_dir.is_dir(), f"Missing {fonts_dir}")
        validate_sfnt_font(fonts_dir / "selawk.ttf", expected_family_substr="Selawik")
        validate_sfnt_font(fonts_dir / "selawksb.ttf", expected_family_substr="Selawik")
        for sc in fonts_dir.glob("*.attribution"):
            validate_attribution_sidecar(sc, expected_asset=sc.name[: -len(".attribution")], required_license="OFL-1.1")

        res_lic = subprocess.run(
            [sys.executable, str(REPO_ROOT / "tools" / "check_licenses.py"), "--current"],
            cwd=str(REPO_ROOT),
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=60,
        )
        self.assertEqual(res_lic.returncode, 0, f"check_licenses.py failed:\n{res_lic.stdout}\n{res_lic.stderr}")

        if skip_cargo_gates():
            return

        for cmd in (
            ["cargo", "fmt", "--all", "--", "--check"],
            ["cargo", "clippy", "--workspace", "--all-targets", "--", "-D", "warnings"],
            ["cargo", "test", "-p", "neoveg_ui", "--", "--test-threads=1"],
        ):
            r = subprocess.run(
                cmd,
                cwd=str(REPO_ROOT),
                capture_output=True,
                text=True,
                encoding="utf-8",
                errors="replace",
                timeout=900,
            )
            assert_no_panic_or_crash_log(r)
            self.assertEqual(r.returncode, 0, f"Gate {cmd} failed:\n{r.stdout}\n{r.stderr}")
