"""Tier 3 E2E pairwise cross-feature combination tests across F1-F10 (>= 10 tests)."""

from __future__ import annotations

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
    assert_no_panic_or_crash_log,
    collect_image_sequence,
    exe_name,
    feature_tags,
    find_neoveg_bin,
    find_preset_for_format,
    isolated_build_env,
    run_ffprobe_oracle,
    run_neoveg,
    sha256_file,
    skip_cargo_gates,
    stage_isolated_build_workspace,
    validate_adts_aac,
    validate_avi,
    validate_cineon,
    validate_dpx,
    validate_flac,
    validate_iso_bmff,
    validate_mkv,
    validate_wav,
    write_neoveg_project,
    write_wav,
)

TIER = 3


class TestTier3Pairwise(unittest.TestCase):
    @feature_tags("F2", "F3", "F10")
    def test_t3_all_fonts_and_sidecars_survive_portable_dist(self) -> None:
        src_fonts = REPO_ROOT / "assets" / "fonts"
        self.assertTrue(src_fonts.is_dir(), f"Missing {src_fonts}")
        with tempfile.TemporaryDirectory() as tmp:
            ws = stage_isolated_build_workspace(Path(tmp), with_release_bin=True, copy_repo_assets=True)
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
            self.assertEqual(res.returncode, 0, res.stderr)
            dist_fonts = ws / "dist" / "assets" / "fonts"
            self.assertTrue(dist_fonts.is_dir(), "dist/assets/fonts/ missing")
            for required in ("selawk.ttf", "selawksb.ttf"):
                self.assertTrue((dist_fonts / required).is_file(), f"Missing {required} in dist")
                self.assertEqual(sha256_file(src_fonts / required), sha256_file(dist_fonts / required))
                self.assertTrue((dist_fonts / f"{required}.attribution").is_file())
            inter_files = list(dist_fonts.glob("Inter*.ttf"))
            noto_files = list(dist_fonts.glob("NotoSans*.ttf"))
            self.assertTrue(inter_files, "Inter*.ttf missing from dist/assets/fonts/")
            self.assertTrue(noto_files, "NotoSans*.ttf missing from dist/assets/fonts/")

    @feature_tags("F1", "F10")
    def test_t3_portable_dist_ignores_dev_forbidden_dirs(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            ws = stage_isolated_build_workspace(Path(tmp), with_release_bin=True, copy_repo_assets=True)
            (ws / "plan").mkdir()
            (ws / "plan" / "secret_notes.txt").write_text("do not bundle", encoding="utf-8")
            (ws / "test_media").mkdir()
            (ws / "test_media" / "clip.mp4").write_bytes(b"0000")

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
            self.assertEqual(res.returncode, 0, res.stderr)
            dist_dir = ws / "dist"
            self.assertTrue(dist_dir.is_dir())
            self.assertFalse((dist_dir / "plan").exists())
            self.assertFalse((dist_dir / "test_media").exists())
            self.assertFalse((ws / "target").exists())

    @feature_tags("F4", "F10")
    def test_t3_dist_exe_runs_cli_with_empty_cwd(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            ws = stage_isolated_build_workspace(td, with_release_bin=True, copy_repo_assets=True)
            res_pkg = subprocess.run(
                [sys.executable, str(ws / "build_portable.py"), "--no-build"],
                cwd=str(ws),
                env=isolated_build_env(),
                capture_output=True,
                text=True,
                encoding="utf-8",
                errors="replace",
                timeout=60,
            )
            self.assertEqual(res_pkg.returncode, 0, res_pkg.stderr)
            dist_exe = ws / "dist" / exe_name()
            self.assertTrue(dist_exe.is_file(), f"Missing packaged binary in dist/: {dist_exe}")
            empty_cwd = td / "empty_cwd"
            empty_cwd.mkdir()

            r_lc, s_lc = run_neoveg(["--list-commands"], bin_path=dist_exe, cwd=empty_cwd)
            assert_no_panic_or_crash_log(r_lc, s_lc)
            self.assertEqual(r_lc.returncode, 0, r_lc.stderr)

            r_pi, s_pi = run_neoveg(["--probe-integrations"], bin_path=dist_exe, cwd=empty_cwd)
            assert_no_panic_or_crash_log(r_pi, s_pi)
            self.assertEqual(r_pi.returncode, 0, r_pi.stderr)

    @feature_tags("F2", "F3", "F4")
    def test_t3_ui_fonts_and_themes_smoke(self) -> None:
        fonts_rs = REPO_ROOT / "crates" / "neoveg_ui" / "src" / "fonts.rs"
        self.assertTrue(fonts_rs.is_file(), f"Missing {fonts_rs}")
        fonts_dir = REPO_ROOT / "assets" / "fonts"
        self.assertTrue((fonts_dir / "selawk.ttf").is_file())
        if skip_cargo_gates():
            return
        res = subprocess.run(
            ["cargo", "test", "-p", "neoveg_ui", "ui_smoke", "--", "--test-threads=1"],
            cwd=str(REPO_ROOT),
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=240,
        )
        assert_no_panic_or_crash_log(res)
        self.assertEqual(res.returncode, 0, f"{res.stdout}\n{res.stderr}")

    @feature_tags("F5", "F7")
    def test_t3_f5_f7_embedded_flac_and_pcm_in_mkv_mov(self) -> None:
        mkv_ffv1 = find_preset_for_format(["MkvFfv1"], required_keywords=["ffv1"], description="MKV FFV1")
        mov_p = find_preset_for_format(
            ["MovMjpeg", "MovPhotoJpeg", "MovUncompressed"],
            required_keywords=["mov"],
            forbidden_keywords=["h264", "hevc", "av1"],
            description="MOV pure-Rust",
        )
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            wav = write_wav(td / "tone.wav", sr=48000, channels=2, duration_s=0.20)
            proj = write_neoveg_project(
                td / "p.neoveg", width=64, height=48, fps=(25, 1), duration_s=0.20, wav_path=wav
            )
            out_mkv = td / "pair.mkv"
            r1, s1 = run_neoveg(["--encode", str(proj), str(out_mkv), mkv_ffv1])
            assert_no_panic_or_crash_log(r1, s1, out_dir=td, out_path=out_mkv)
            self.assertEqual(r1.returncode, 0, r1.stderr)
            validate_mkv(
                out_mkv,
                expected_w=64,
                expected_h=48,
                expected_video_codec_ids={"V_FFV1"},
                expect_audio=True,
                expected_sr=48000,
                expected_ch=2,
            )
            run_ffprobe_oracle(out_mkv)

            out_mov = td / "pair.mov"
            r2, s2 = run_neoveg(["--encode", str(proj), str(out_mov), mov_p])
            assert_no_panic_or_crash_log(r2, s2, out_dir=td, out_path=out_mov)
            self.assertEqual(r2.returncode, 0, r2.stderr)
            validate_iso_bmff(out_mov, expected_w=64, expected_h=48, require_qt_brand=True, expect_audio=True)
            run_ffprobe_oracle(out_mov)

    @feature_tags("F5", "F6")
    def test_t3_f6_f5_cineon_dpx_with_51_sidecar_and_flac_stem(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            wav51 = write_wav(td / "tone51.wav", sr=48000, channels=6, duration_s=0.16)
            proj = write_neoveg_project(
                td / "p51.neoveg",
                width=48,
                height=32,
                fps=(25, 1),
                duration_s=0.16,
                wav_path=wav51,
                channel_layout="Surround51",
            )
            seq_cin = td / "film.cin"
            r1, s1 = run_neoveg(["--render", str(proj), str(seq_cin)])
            assert_no_panic_or_crash_log(r1, s1, out_dir=td)
            self.assertEqual(r1.returncode, 0, r1.stderr)
            frames = collect_image_sequence(seq_cin, expected_count=4)
            validate_cineon(frames[0], 48, 32)
            validate_wav(td / "film.wav", expected_sr=48000, expected_ch=6)

            stem_flac = td / "stem51.flac"
            r2, s2 = run_neoveg(["--render", str(proj), str(stem_flac)])
            assert_no_panic_or_crash_log(r2, s2, out_dir=td, out_path=stem_flac)
            self.assertEqual(r2.returncode, 0, r2.stderr)
            validate_flac(stem_flac, expected_sr=48000, expected_ch=6)

    @feature_tags("F7", "F9")
    def test_t3_f7_f9_different_project_settings_into_mov_mkv_avi(self) -> None:
        mkv_ffv1 = find_preset_for_format(["MkvFfv1"], required_keywords=["ffv1"], description="MKV FFV1")
        mov_p = find_preset_for_format(
            ["MovMjpeg", "MovPhotoJpeg", "MovUncompressed"],
            required_keywords=["mov"],
            forbidden_keywords=["h264", "hevc", "av1"],
            description="MOV pure-Rust",
        )
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            wav = write_wav(td / "tone.wav", sr=48000, channels=2, duration_s=0.16)
            proj_a = write_neoveg_project(
                td / "solid_red.neoveg",
                width=64,
                height=48,
                duration_s=0.16,
                wav_path=wav,
                gen_spec={"Generated": {"SolidColor": {"color": [1.0, 0.0, 0.0, 1.0]}}},
            )
            proj_b = write_neoveg_project(
                td / "colorbars.neoveg",
                width=64,
                height=48,
                duration_s=0.16,
                wav_path=wav,
                gen_spec={"Generated": "ColorBars"},
            )
            out_a = td / "a.mkv"
            out_b = td / "b.mkv"
            for proj, out in ((proj_a, out_a), (proj_b, out_b)):
                res, snap = run_neoveg(["--encode", str(proj), str(out), mkv_ffv1])
                assert_no_panic_or_crash_log(res, snap, out_dir=td, out_path=out)
                self.assertEqual(res.returncode, 0, res.stderr)
                validate_mkv(out, expected_w=64, expected_h=48, expected_video_codec_ids={"V_FFV1"})
            self.assertNotEqual(sha256_file(out_a), sha256_file(out_b))

            out_mov = td / "b.mov"
            r_mov, s_mov = run_neoveg(["--encode", str(proj_b), str(out_mov), mov_p])
            assert_no_panic_or_crash_log(r_mov, s_mov, out_dir=td, out_path=out_mov)
            self.assertEqual(r_mov.returncode, 0, r_mov.stderr)
            validate_iso_bmff(out_mov, expected_w=64, expected_h=48, require_qt_brand=True)

            out_avi = td / "b.avi"
            r_avi, s_avi = run_neoveg(["--encode", str(proj_b), str(out_avi), "sys.bc_avi_mjpeg"])
            assert_no_panic_or_crash_log(r_avi, s_avi, out_dir=td, out_path=out_avi)
            self.assertEqual(r_avi.returncode, 0, r_avi.stderr)
            validate_avi(out_avi, expected_w=64, expected_h=48)

    @feature_tags("F8", "F9")
    def test_t3_f8_f9_preset_group_and_parallel_mf_pure_rust(self) -> None:
        mkv_ffv1 = find_preset_for_format(["MkvFfv1"], required_keywords=["ffv1"], description="MKV FFV1")
        flac_p = find_preset_for_format(["Flac"], required_keywords=["flac"], description="FLAC")
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            wav = write_wav(td / "tone.wav", sr=48000, channels=2, duration_s=0.16)
            proj = write_neoveg_project(
                td / "p.neoveg", width=128, height=72, fps=(25, 1), duration_s=0.16, wav_path=wav
            )
            # Execute MF group + pure-Rust targets consecutively on same source
            r_grp, s_grp = run_neoveg(["--encode", str(proj), str(td / "web.mp4"), "grp.web_multi"])
            assert_no_panic_or_crash_log(r_grp, s_grp, out_dir=td)
            self.assertEqual(r_grp.returncode, 0, r_grp.stderr)
            validate_iso_bmff(td / "web.mp4", expected_video_fourccs={b"avc1", b"avc3"})

            r_mkv, s_mkv = run_neoveg(["--encode", str(proj), str(td / "arch.mkv"), mkv_ffv1])
            assert_no_panic_or_crash_log(r_mkv, s_mkv, out_dir=td)
            self.assertEqual(r_mkv.returncode, 0, r_mkv.stderr)
            validate_mkv(td / "arch.mkv", expected_w=128, expected_h=72, expected_video_codec_ids={"V_FFV1"})

            r_flc, s_flc = run_neoveg(["--encode", str(proj), str(td / "stem.flac"), flac_p])
            assert_no_panic_or_crash_log(r_flc, s_flc, out_dir=td)
            self.assertEqual(r_flc.returncode, 0, r_flc.stderr)
            validate_flac(td / "stem.flac", expected_sr=48000, expected_ch=2)

    @feature_tags("F6", "F7")
    def test_t3_f6_f7_transcode_cineon_to_mov_and_mkv(self) -> None:
        mkv_ffv1 = find_preset_for_format(["MkvFfv1"], required_keywords=["ffv1"], description="MKV FFV1")
        mov_p = find_preset_for_format(
            ["MovMjpeg", "MovPhotoJpeg", "MovUncompressed"],
            required_keywords=["mov"],
            forbidden_keywords=["h264", "hevc", "av1"],
            description="MOV pure-Rust",
        )
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            proj = write_neoveg_project(td / "p.neoveg", width=64, height=48, fps=(25, 1), duration_s=0.08)
            seq_cin = td / "plate.cin"
            r1, s1 = run_neoveg(["--render", str(proj), str(seq_cin)])
            assert_no_panic_or_crash_log(r1, s1, out_dir=td)
            self.assertEqual(r1.returncode, 0, r1.stderr)
            cin_frames = collect_image_sequence(seq_cin, expected_count=2)
            validate_cineon(cin_frames[0], 64, 48)

            out_mov = td / "from_cin.mov"
            r2, s2 = run_neoveg(["--encode", str(cin_frames[0]), str(out_mov), mov_p])
            assert_no_panic_or_crash_log(r2, s2, out_dir=td, out_path=out_mov)
            self.assertEqual(r2.returncode, 0, r2.stderr)
            validate_iso_bmff(out_mov, expected_w=64, expected_h=48, require_qt_brand=True)

            out_mkv = td / "from_cin.mkv"
            r3, s3 = run_neoveg(["--encode", str(cin_frames[0]), str(out_mkv), mkv_ffv1])
            assert_no_panic_or_crash_log(r3, s3, out_dir=td, out_path=out_mkv)
            self.assertEqual(r3.returncode, 0, r3.stderr)
            validate_mkv(out_mkv, expected_w=64, expected_h=48, expected_video_codec_ids={"V_FFV1"})

    @feature_tags("F5", "F8")
    def test_t3_f5_f8_flac_input_to_aac_adts_and_m4a(self) -> None:
        aac_p = find_preset_for_format(["AacAdts"], required_keywords=["aac"], description="AAC ADTS")
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            wav = write_wav(td / "tone.wav", sr=48000, channels=2, duration_s=0.25)
            proj = write_neoveg_project(td / "p.neoveg", wav_path=wav, duration_s=0.25)
            flac_mid = td / "mid.flac"
            r1, s1 = run_neoveg(["--render", str(proj), str(flac_mid)])
            assert_no_panic_or_crash_log(r1, s1, out_dir=td, out_path=flac_mid)
            self.assertEqual(r1.returncode, 0, r1.stderr)
            validate_flac(flac_mid, expected_sr=48000, expected_ch=2)

            out_aac = td / "out.aac"
            r2, s2 = run_neoveg(["--encode", str(flac_mid), str(out_aac), aac_p])
            assert_no_panic_or_crash_log(r2, s2, out_dir=td, out_path=out_aac)
            self.assertEqual(r2.returncode, 0, r2.stderr)
            validate_adts_aac(out_aac)

            out_m4a = td / "out.m4a"
            r3, s3 = run_neoveg(["--encode", str(flac_mid), str(out_m4a), "sys.aud_m4a_256"])
            assert_no_panic_or_crash_log(r3, s3, out_dir=td, out_path=out_m4a)
            self.assertEqual(r3.returncode, 0, r3.stderr)
            self.assertGreater(out_m4a.stat().st_size, 100)
            run_ffprobe_oracle(out_m4a)

    @feature_tags("F6", "F9")
    def test_t3_f6_f9_dpx_and_cineon_via_cli_presets(self) -> None:
        cin_p = find_preset_for_format(["CineonSequence", "Cineon"], required_keywords=["cin"], description="Cineon")
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            proj = write_neoveg_project(td / "p.neoveg", width=32, height=24, fps=(25, 1), duration_s=0.08)
            r1, s1 = run_neoveg(["--encode", str(proj), str(td / "seq.cin"), cin_p])
            assert_no_panic_or_crash_log(r1, s1, out_dir=td)
            self.assertEqual(r1.returncode, 0, r1.stderr)
            validate_cineon(collect_image_sequence(td / "seq.cin", 2)[0], 32, 24)

            r2, s2 = run_neoveg(["--encode", str(proj), str(td / "seq.dpx"), "sys.seq_dpx"])
            assert_no_panic_or_crash_log(r2, s2, out_dir=td)
            self.assertEqual(r2.returncode, 0, r2.stderr)
            validate_dpx(collect_image_sequence(td / "seq.dpx", 2)[0], 32, 24)

    @feature_tags("F1", "F4", "F10")
    def test_t3_f1_f4_f10_build_portable_propagates_attribution_and_no_forbidden_fonts(self) -> None:
        fonts_dir = REPO_ROOT / "assets" / "fonts"
        self.assertTrue(fonts_dir.is_dir(), f"Missing {fonts_dir}")
        with tempfile.TemporaryDirectory() as tmp:
            ws = stage_isolated_build_workspace(Path(tmp), with_release_bin=True, copy_repo_assets=True)
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
            self.assertEqual(res.returncode, 0, res.stderr)
            dist_fonts = ws / "dist" / "assets" / "fonts"
            self.assertTrue(dist_fonts.is_dir())
            lic_texts = list(dist_fonts.glob("*LICENSE*.txt"))
            self.assertTrue(lic_texts, "dist/assets/fonts/ must preserve font LICENSE.txt files")
            attr_md = (ws / "dist" / "ATTRIBUTION.md").read_text(encoding="utf-8")
            self.assertIn("selawk.ttf", attr_md)
