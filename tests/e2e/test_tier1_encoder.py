"""Tier 1 E2E tests for F5 (Audio), F6 (Sequences), F7 (MOV/MKV/AVI), F8 (MF Codecs), and F9 (Presets/CLI)."""

from __future__ import annotations

import platform
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
    feature_tags,
    ffmpeg_has_decoder,
    find_neoveg_bin,
    find_preset_for_format,
    is_clean_mft_unavailable,
    parse_system_presets_catalog,
    resolve_vp9_preset_and_validator,
    run_ffprobe_oracle,
    run_neoveg,
    skip_cargo_gates,
    validate_adts_aac,
    validate_au,
    validate_avi,
    validate_caf,
    validate_cineon,
    validate_dpx,
    validate_exr,
    validate_flac,
    validate_iso_bmff,
    validate_mkv,
    validate_png,
    validate_wav,
    validate_web_multi_group_outputs,
    write_bmp,
    write_neoveg_project,
    write_wav,
)

TIER = 1


@feature_tags("F5")
class TestTier1F5AudioEncoders(unittest.TestCase):
    def test_f5_flac_render_16bit_stereo(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            wav = write_wav(td / "tone.wav", sr=48000, channels=2, duration_s=0.25)
            proj = write_neoveg_project(td / "p.neoveg", wav_path=wav, duration_s=0.25)
            out = td / "out.flac"
            res, snap = run_neoveg(["--render", str(proj), str(out)])
            assert_no_panic_or_crash_log(res, snap, out_dir=td, out_path=out)
            self.assertEqual(res.returncode, 0, f"--render out.flac failed:\n{res.stderr}")

            info = validate_flac(out, expected_sr=48000, expected_ch=2)
            self.assertIn(info["bps"], (16, 24))
            probe = run_ffprobe_oracle(out)
            if probe:
                astreams = [s for s in probe["streams"] if s.get("codec_type") == "audio"]
                self.assertEqual(astreams[0]["codec_name"], "flac")
                self.assertEqual(int(astreams[0]["channels"]), 2)

    def test_f5_flac_encode_24bit_preset(self) -> None:
        preset_id = find_preset_for_format(
            ["Flac"],
            required_keywords=["flac"],
            description="FLAC 24-bit audio preset",
        )
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            wav = write_wav(td / "tone.wav", sr=48000, channels=2, duration_s=0.25)
            out = td / "out_preset.flac"
            res, snap = run_neoveg(["--encode", str(wav), str(out), preset_id])
            assert_no_panic_or_crash_log(res, snap, out_dir=td, out_path=out)
            self.assertEqual(res.returncode, 0, f"--encode FLAC failed:\n{res.stderr}")
            info = validate_flac(out, expected_sr=48000, expected_ch=2)
            self.assertIn(info["bps"], (16, 24))
            run_ffprobe_oracle(out)

    def test_f5_flac_51_surround(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            wav51 = write_wav(td / "tone51.wav", sr=48000, channels=6, duration_s=0.24)
            proj = write_neoveg_project(
                td / "p51.neoveg",
                wav_path=wav51,
                channel_layout="Surround51",
                duration_s=0.24,
            )
            out = td / "surround.flac"
            res, snap = run_neoveg(["--render", str(proj), str(out)])
            assert_no_panic_or_crash_log(res, snap, out_dir=td, out_path=out)
            self.assertEqual(res.returncode, 0, f"--render 5.1 FLAC failed:\n{res.stderr}")
            validate_flac(out, expected_sr=48000, expected_ch=6)
            probe = run_ffprobe_oracle(out)
            if probe:
                astreams = [s for s in probe["streams"] if s.get("codec_type") == "audio"]
                self.assertEqual(int(astreams[0]["channels"]), 6)

    def test_f5_au_and_caf_51_surround_parity(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            wav51 = write_wav(td / "tone51.wav", sr=48000, channels=6, duration_s=0.24)
            proj = write_neoveg_project(
                td / "p51.neoveg",
                wav_path=wav51,
                channel_layout="Surround51",
                duration_s=0.24,
            )
            out_au = td / "surround.au"
            res_au, snap_au = run_neoveg(["--render", str(proj), str(out_au)])
            assert_no_panic_or_crash_log(res_au, snap_au, out_dir=td, out_path=out_au)
            self.assertEqual(res_au.returncode, 0, res_au.stderr)
            validate_au(out_au, expected_sr=48000, expected_ch=6)

            out_caf = td / "surround.caf"
            res_caf, snap_caf = run_neoveg(["--render", str(proj), str(out_caf)])
            assert_no_panic_or_crash_log(res_caf, snap_caf, out_dir=td, out_path=out_caf)
            self.assertEqual(res_caf.returncode, 0, res_caf.stderr)
            validate_caf(out_caf, expected_sr=48000, expected_ch=6)

    def test_f5_flac_roundtrip_reimport(self) -> None:
        wav_preset = find_preset_for_format(["Wav"], required_keywords=["wav"], description="WAV preset")
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            wav_in = write_wav(td / "tone.wav", sr=48000, channels=2, duration_s=0.25)
            proj = write_neoveg_project(td / "p.neoveg", wav_path=wav_in, duration_s=0.25)
            flac_out = td / "mid.flac"
            res1, snap1 = run_neoveg(["--render", str(proj), str(flac_out)])
            assert_no_panic_or_crash_log(res1, snap1, out_dir=td, out_path=flac_out)
            self.assertEqual(res1.returncode, 0, res1.stderr)
            validate_flac(flac_out, expected_sr=48000, expected_ch=2)

            wav_rt = td / "roundtrip.wav"
            res2, snap2 = run_neoveg(["--encode", str(flac_out), str(wav_rt), wav_preset])
            assert_no_panic_or_crash_log(res2, snap2, out_dir=td, out_path=wav_rt)
            self.assertEqual(res2.returncode, 0, res2.stderr)
            validate_wav(wav_rt, expected_sr=48000, expected_ch=2)

    def test_f5_sidecar_wav_51_on_image_seq(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            wav51 = write_wav(td / "tone51.wav", sr=48000, channels=6, duration_s=0.16)
            proj = write_neoveg_project(
                td / "p51.neoveg",
                width=32,
                height=24,
                fps=(25, 1),
                duration_s=0.16,
                wav_path=wav51,
                channel_layout="Surround51",
            )
            seq_base = td / "frame.png"
            res, snap = run_neoveg(["--render", str(proj), str(seq_base)])
            assert_no_panic_or_crash_log(res, snap, out_dir=td)
            self.assertEqual(res.returncode, 0, res.stderr)
            collect_image_sequence(seq_base, expected_count=4)
            sidecar_wav = td / "frame.wav"
            validate_wav(sidecar_wav, expected_sr=48000, expected_ch=6)


@feature_tags("F6")
class TestTier1F6ImageSequences(unittest.TestCase):
    def test_f6_cineon_sequence_render(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            wav = write_wav(td / "tone.wav", sr=48000, channels=2, duration_s=0.20)
            proj = write_neoveg_project(
                td / "p.neoveg", width=64, height=48, fps=(25, 1), duration_s=0.20, wav_path=wav
            )
            seq_base = td / "scan.cin"
            res, snap = run_neoveg(["--render", str(proj), str(seq_base)])
            assert_no_panic_or_crash_log(res, snap, out_dir=td)
            self.assertEqual(res.returncode, 0, f"--render scan.cin failed:\n{res.stderr}")
            frames = collect_image_sequence(seq_base, expected_count=5)
            for f in frames:
                info = validate_cineon(f, expected_w=64, expected_h=48)
                self.assertEqual(info["bits"], 10)
            validate_wav(td / "scan.wav", expected_sr=48000, expected_ch=2)
            if ffmpeg_has_decoder("cineon"):
                probe = run_ffprobe_oracle(frames[0])
                if probe:
                    self.assertEqual(probe["streams"][0]["codec_name"], "cineon")

    def test_f6_cineon_encode_preset(self) -> None:
        preset_id = find_preset_for_format(
            ["CineonSequence", "Cineon"],
            required_keywords=["cin"],
            description="Cineon 10-bit image sequence preset",
        )
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            proj = write_neoveg_project(td / "p.neoveg", width=64, height=48, fps=(25, 1), duration_s=0.12)
            seq_base = td / "enc.cin"
            res, snap = run_neoveg(["--encode", str(proj), str(seq_base), preset_id])
            assert_no_panic_or_crash_log(res, snap, out_dir=td)
            self.assertEqual(res.returncode, 0, res.stderr)
            frames = collect_image_sequence(seq_base, expected_count=3)
            validate_cineon(frames[0], expected_w=64, expected_h=48)

    def test_f6_dpx_10bit_and_8bit_sequences(self) -> None:
        dpx_8_or_default = find_preset_for_format(
            ["DpxSequence"], required_keywords=["dpx"], description="DPX sequence preset"
        )
        dpx_10 = find_preset_for_format(
            ["DpxSequence", "Dpx10Sequence", "Dpx10BitSequence"],
            required_keywords=["dpx", "10"],
            description="DPX 10-bit sequence preset (Resolve 20)",
        )
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            proj = write_neoveg_project(td / "p.neoveg", width=64, height=48, fps=(25, 1), duration_s=0.12)

            base_def = td / "dpx_def.dpx"
            res1, snap1 = run_neoveg(["--encode", str(proj), str(base_def), dpx_8_or_default])
            assert_no_panic_or_crash_log(res1, snap1, out_dir=td)
            self.assertEqual(res1.returncode, 0, res1.stderr)
            f1 = collect_image_sequence(base_def, expected_count=3)
            validate_dpx(f1[0], expected_w=64, expected_h=48)

            base_10 = td / "dpx_10.dpx"
            res2, snap2 = run_neoveg(["--encode", str(proj), str(base_10), dpx_10])
            assert_no_panic_or_crash_log(res2, snap2, out_dir=td)
            self.assertEqual(res2.returncode, 0, res2.stderr)
            f2 = collect_image_sequence(base_10, expected_count=3)
            validate_dpx(f2[0], expected_w=64, expected_h=48, expected_bits=10)
            run_ffprobe_oracle(f2[0])

    def test_f6_exr_sequence_hdr_and_compression(self) -> None:
        exr_preset = find_preset_for_format(
            ["ExrSequence"], required_keywords=["exr"], description="OpenEXR sequence preset"
        )
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            proj = write_neoveg_project(td / "p.neoveg", width=64, height=48, fps=(25, 1), duration_s=0.12)
            base_exr = td / "hdr.exr"
            res, snap = run_neoveg(["--encode", str(proj), str(base_exr), exr_preset])
            assert_no_panic_or_crash_log(res, snap, out_dir=td)
            self.assertEqual(res.returncode, 0, res.stderr)
            frames = collect_image_sequence(base_exr, expected_count=3)
            info = validate_exr(frames[0], expected_w=64, expected_h=48)
            self.assertIn(info["compression"], range(0, 10))
            probe = run_ffprobe_oracle(frames[0])
            if probe:
                self.assertEqual(probe["streams"][0]["codec_name"], "exr")

    def test_f6_cineon_roundtrip_reimport(self) -> None:
        png_preset = find_preset_for_format(["PngSequence"], required_keywords=["png"], description="PNG seq preset")
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            proj = write_neoveg_project(td / "p.neoveg", width=48, height=32, fps=(25, 1), duration_s=0.08)
            seq_cin = td / "rt.cin"
            res1, snap1 = run_neoveg(["--render", str(proj), str(seq_cin)])
            assert_no_panic_or_crash_log(res1, snap1, out_dir=td)
            self.assertEqual(res1.returncode, 0, res1.stderr)
            cin_frames = collect_image_sequence(seq_cin, expected_count=2)
            validate_cineon(cin_frames[0], expected_w=48, expected_h=32)

            out_png = td / "reimport.png"
            res2, snap2 = run_neoveg(["--encode", str(cin_frames[0]), str(out_png), png_preset])
            assert_no_panic_or_crash_log(res2, snap2, out_dir=td)
            self.assertEqual(res2.returncode, 0, res2.stderr)
            png_frames = collect_image_sequence(out_png)
            validate_png(png_frames[0], expected_w=48, expected_h=32)


@feature_tags("F7")
class TestTier1F7PureRustContainers(unittest.TestCase):
    def test_f7_mov_uncompressed_rgb_and_yuv422(self) -> None:
        rgb_preset = find_preset_for_format(
            ["MovUncompressed", "MovUncompressedRgb", "MovRgb", "MovArgb", "MovBgra"],
            required_keywords=["mov"],
            forbidden_keywords=["h264", "hevc", "av1", "mpeg4", "vp9", "jpeg", "mjpeg", "png", "rle", "ffv1"],
            description="QuickTime MOV Uncompressed RGB/ARGB preset",
        )
        yuv_preset = find_preset_for_format(
            ["MovUncompressedYuv422", "MovYuv422", "Mov2vuy", "MovV210"],
            required_keywords=["mov", "yuv"],
            description="QuickTime MOV Uncompressed YUV422 preset",
        )
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            wav = write_wav(td / "tone.wav", sr=48000, channels=2, duration_s=0.20)
            proj = write_neoveg_project(
                td / "p.neoveg", width=64, height=48, fps=(25, 1), duration_s=0.20, wav_path=wav
            )
            for pid, fourccs, tag in (
                (rgb_preset, {b"raw ", b"BGRA", b"ABGR", b"r210", b"R10k", b"b64a", b"RGBA"}, "rgb"),
                (yuv_preset, {b"2vuy", b"uyvy", b"yuvs", b"v210"}, "yuv"),
            ):
                out = td / f"uncomp_{tag}.mov"
                res, snap = run_neoveg(["--encode", str(proj), str(out), pid])
                assert_no_panic_or_crash_log(res, snap, out_dir=td, out_path=out)
                self.assertEqual(res.returncode, 0, f"MOV {tag} encode failed:\n{res.stderr}")
                validate_iso_bmff(
                    out,
                    expected_w=64,
                    expected_h=48,
                    expected_video_fourccs=fourccs,
                    require_qt_brand=True,
                    expect_audio=True,
                )
                run_ffprobe_oracle(out)

    def test_f7_mov_photojpeg_and_png_rle(self) -> None:
        mjpeg_preset = find_preset_for_format(
            ["MovMjpeg", "MovPhotoJpeg"],
            required_keywords=["mov"],
            forbidden_keywords=["h264", "hevc", "av1"],
            description="QuickTime MOV PhotoJPEG/MJPEG preset",
        )
        png_preset = find_preset_for_format(
            ["MovPng", "MovRle"],
            required_keywords=["mov"],
            forbidden_keywords=["h264", "hevc", "av1"],
            description="QuickTime MOV PNG/RLE preset",
        )
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            wav = write_wav(td / "tone.wav", sr=48000, channels=2, duration_s=0.20)
            proj = write_neoveg_project(
                td / "p.neoveg", width=64, height=48, fps=(25, 1), duration_s=0.20, wav_path=wav
            )
            for pid, fourccs, tag in (
                (mjpeg_preset, {b"jpeg", b"mjpa", b"mjpb"}, "mjpeg"),
                (png_preset, {b"png ", b"rle "}, "png_rle"),
            ):
                out = td / f"qt_{tag}.mov"
                res, snap = run_neoveg(["--encode", str(proj), str(out), pid])
                assert_no_panic_or_crash_log(res, snap, out_dir=td, out_path=out)
                self.assertEqual(res.returncode, 0, res.stderr)
                validate_iso_bmff(
                    out,
                    expected_w=64,
                    expected_h=48,
                    expected_video_fourccs=fourccs,
                    require_qt_brand=True,
                    expect_audio=True,
                )
                run_ffprobe_oracle(out)

    def test_f7_mkv_ffv1_lossless_and_alpha(self) -> None:
        ffv1_preset = find_preset_for_format(
            ["MkvFfv1"],
            required_keywords=["ffv1"],
            description="Matroska MKV FFV1 lossless preset",
        )
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            wav = write_wav(td / "tone.wav", sr=48000, channels=2, duration_s=0.20)
            proj = write_neoveg_project(
                td / "p.neoveg", width=64, height=48, fps=(25, 1), duration_s=0.20, wav_path=wav
            )
            out = td / "archive_ffv1.mkv"
            res, snap = run_neoveg(["--encode", str(proj), str(out), ffv1_preset])
            assert_no_panic_or_crash_log(res, snap, out_dir=td, out_path=out)
            self.assertEqual(res.returncode, 0, f"MKV FFV1 encode failed:\n{res.stderr}")
            validate_mkv(
                out,
                expected_w=64,
                expected_h=48,
                expected_video_codec_ids={"V_FFV1"},
                expect_audio=True,
                expected_sr=48000,
                expected_ch=2,
            )
            probe = run_ffprobe_oracle(out)
            if probe:
                vstreams = [s for s in probe["streams"] if s.get("codec_type") == "video"]
                self.assertEqual(vstreams[0]["codec_name"], "ffv1")

    def test_f7_mkv_mjpeg_or_uncompressed_with_pcm_flac(self) -> None:
        mkv_preset = find_preset_for_format(
            ["MkvMjpeg", "MkvUncompressed", "MkvPng"],
            required_keywords=["mkv"],
            forbidden_keywords=["h264", "hevc", "av1", "vp9", "ffv1"],
            description="Matroska MKV MJPEG/Uncompressed/PNG pure-Rust preset",
        )
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            wav = write_wav(td / "tone.wav", sr=48000, channels=2, duration_s=0.20)
            proj = write_neoveg_project(
                td / "p.neoveg", width=64, height=48, fps=(25, 1), duration_s=0.20, wav_path=wav
            )
            out = td / "pure_rust.mkv"
            res, snap = run_neoveg(["--encode", str(proj), str(out), mkv_preset])
            assert_no_panic_or_crash_log(res, snap, out_dir=td, out_path=out)
            self.assertEqual(res.returncode, 0, res.stderr)
            validate_mkv(
                out,
                expected_w=64,
                expected_h=48,
                expected_video_codec_ids={"V_MJPEG", "V_PNG", "V_UNCOMPRESSED", "V_MS/VFW/FOURCC"},
                expect_audio=True,
                expected_audio_codec_ids={"A_PCM/INT/LIT", "A_PCM/FLOAT/IEEE", "A_FLAC"},
                expected_sr=48000,
                expected_ch=2,
            )
            run_ffprobe_oracle(out)

    def test_f7_avi_uncompressed_yuv422_and_bgra(self) -> None:
        yuv_preset = find_preset_for_format(
            ["AviUncompressedYuv422", "AviYuv422", "AviUyvy", "AviYuy2"],
            required_keywords=["avi", "yuv"],
            description="AVI Uncompressed YUV422 preset",
        )
        bgra_preset = find_preset_for_format(
            ["AviUncompressedBgra", "AviBgra", "AviRgba"],
            required_keywords=["avi"],
            forbidden_keywords=["mjpeg", "rgb24", "yuv", "10"],
            description="AVI Uncompressed BGRA 32-bit preset",
        )
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            wav = write_wav(td / "tone.wav", sr=48000, channels=2, duration_s=0.20)
            proj = write_neoveg_project(
                td / "p.neoveg", width=64, height=48, fps=(25, 1), duration_s=0.20, wav_path=wav
            )

            out_yuv = td / "out_yuv.avi"
            res1, snap1 = run_neoveg(["--encode", str(proj), str(out_yuv), yuv_preset])
            assert_no_panic_or_crash_log(res1, snap1, out_dir=td, out_path=out_yuv)
            self.assertEqual(res1.returncode, 0, res1.stderr)
            validate_avi(
                out_yuv,
                expected_w=64,
                expected_h=48,
                expected_compressions={b"UYVY", b"YUY2", b"YUYV", b"v210"},
                expect_audio=True,
                expected_sr=48000,
                expected_ch=2,
            )
            run_ffprobe_oracle(out_yuv)

            out_bgra = td / "out_bgra.avi"
            res2, snap2 = run_neoveg(["--encode", str(proj), str(out_bgra), bgra_preset])
            assert_no_panic_or_crash_log(res2, snap2, out_dir=td, out_path=out_bgra)
            self.assertEqual(res2.returncode, 0, res2.stderr)
            validate_avi(
                out_bgra,
                expected_w=64,
                expected_h=48,
                expected_compressions={b"\x00\x00\x00\x00", b"BGRA", b"RGBA", b"DIB "},
                expected_bit_count=32,
                expect_audio=True,
                expected_sr=48000,
                expected_ch=2,
            )
            run_ffprobe_oracle(out_bgra)

    def test_f7_avi_uncompressed_10bit_rgb(self) -> None:
        rgb10_preset = find_preset_for_format(
            ["AviUncompressedRgb10", "AviRgb10"],
            required_keywords=["avi", "10"],
            description="AVI Uncompressed RGB 10-bit preset",
        )
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            wav = write_wav(td / "tone.wav", sr=48000, channels=2, duration_s=0.20)
            proj = write_neoveg_project(
                td / "p.neoveg", width=64, height=48, fps=(25, 1), duration_s=0.20, wav_path=wav
            )
            out = td / "out_rgb10.avi"
            res, snap = run_neoveg(["--encode", str(proj), str(out), rgb10_preset])
            assert_no_panic_or_crash_log(res, snap, out_dir=td, out_path=out)
            self.assertEqual(res.returncode, 0, res.stderr)
            validate_avi(
                out,
                expected_w=64,
                expected_h=48,
                expected_compressions={b"r210", b"R10k", b"b64a", b"RGB\x0a"},
                expect_audio=True,
            )
            run_ffprobe_oracle(out)


@feature_tags("F8")
class TestTier1F8MediaFoundationFormats(unittest.TestCase):
    def _assert_mf_or_clean_unavailable(
        self,
        res: subprocess.CompletedProcess[str],
        snap,
        out: Path,
        required_on_windows: bool,
        validator_fn,
        expected_ffprobe_codec: str,
    ) -> None:
        assert_no_panic_or_crash_log(
            res, snap, out_dir=out.parent, out_path=out, expect_failed_cleanup=(res.returncode != 0)
        )
        if res.returncode == 0:
            validator_fn(out)
            probe = run_ffprobe_oracle(out)
            if probe:
                vstreams = [s for s in probe["streams"] if s.get("codec_type") == "video"]
                if vstreams:
                    self.assertEqual(
                        vstreams[0]["codec_name"],
                        expected_ffprobe_codec,
                        f"Expected exact codec {expected_ffprobe_codec}, got {vstreams[0]['codec_name']}",
                    )
                else:
                    astreams = [s for s in probe["streams"] if s.get("codec_type") == "audio"]
                    self.assertEqual(astreams[0]["codec_name"], expected_ffprobe_codec)
        else:
            if required_on_windows and platform.system() == "Windows":
                self.fail(
                    f"Baseline Windows MF codec {expected_ffprobe_codec} failed unexpectedly:\n{res.stderr}"
                )
            self.assertTrue(
                is_clean_mft_unavailable(res, snap, out_dir=out.parent, out_path=out),
                f"Non-zero MF failure must return clean unavailable message, got rc={res.returncode}:\n{res.stderr}",
            )

    def test_f8_mp4_h264_and_hevc_baseline(self) -> None:
        h264_p = find_preset_for_format(["Mp4H264"], required_keywords=["mp4", "h264"], description="MP4 H.264")
        hevc_p = find_preset_for_format(["Mp4Hevc"], required_keywords=["hevc"], description="MP4 HEVC")
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            wav = write_wav(td / "tone.wav", sr=48000, channels=2, duration_s=0.20)
            proj = write_neoveg_project(
                td / "p.neoveg", width=128, height=72, fps=(25, 1), duration_s=0.20, wav_path=wav
            )
            out_h264 = td / "h264.mp4"
            res1, snap1 = run_neoveg(["--encode", str(proj), str(out_h264), h264_p])
            self._assert_mf_or_clean_unavailable(
                res1,
                snap1,
                out_h264,
                required_on_windows=True,
                validator_fn=lambda p: validate_iso_bmff(p, expected_video_fourccs={b"avc1", b"avc3"}),
                expected_ffprobe_codec="h264",
            )

            out_hevc = td / "hevc.mp4"
            res2, snap2 = run_neoveg(["--encode", str(proj), str(out_hevc), hevc_p])
            self._assert_mf_or_clean_unavailable(
                res2,
                snap2,
                out_hevc,
                required_on_windows=False,
                validator_fn=lambda p: validate_iso_bmff(p, expected_video_fourccs={b"hvc1", b"hev1"}),
                expected_ffprobe_codec="hevc",
            )

    def test_f8_mov_hevc_and_mpeg4(self) -> None:
        mov_hevc = find_preset_for_format(["MovHevc"], required_keywords=["mov", "hevc"], description="MOV HEVC")
        mov_mp4v = find_preset_for_format(["MovMpeg4"], required_keywords=["mov", "mpeg4"], description="MOV MPEG4")
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            wav = write_wav(td / "tone.wav", sr=48000, channels=2, duration_s=0.20)
            proj = write_neoveg_project(
                td / "p.neoveg", width=128, height=72, fps=(25, 1), duration_s=0.20, wav_path=wav
            )
            out_hevc = td / "hevc.mov"
            res1, snap1 = run_neoveg(["--encode", str(proj), str(out_hevc), mov_hevc])
            self._assert_mf_or_clean_unavailable(
                res1,
                snap1,
                out_hevc,
                required_on_windows=False,
                validator_fn=lambda p: validate_iso_bmff(p, expected_video_fourccs={b"hvc1", b"hev1"}),
                expected_ffprobe_codec="hevc",
            )

            out_mp4v = td / "mpeg4.mov"
            res2, snap2 = run_neoveg(["--encode", str(proj), str(out_mp4v), mov_mp4v])
            self._assert_mf_or_clean_unavailable(
                res2,
                snap2,
                out_mp4v,
                required_on_windows=False,
                validator_fn=lambda p: validate_iso_bmff(p, expected_video_fourccs={b"mp4v"}),
                expected_ffprobe_codec="mpeg4",
            )

    def test_f8_mkv_h264_and_hevc(self) -> None:
        mkv_h264 = find_preset_for_format(["MkvH264"], required_keywords=["mkv", "h264"], description="MKV H.264")
        mkv_hevc = find_preset_for_format(["MkvHevc"], required_keywords=["mkv", "hevc"], description="MKV HEVC")
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            wav = write_wav(td / "tone.wav", sr=48000, channels=2, duration_s=0.20)
            proj = write_neoveg_project(
                td / "p.neoveg", width=128, height=72, fps=(25, 1), duration_s=0.20, wav_path=wav
            )
            out1 = td / "h264.mkv"
            res1, snap1 = run_neoveg(["--encode", str(proj), str(out1), mkv_h264])
            self._assert_mf_or_clean_unavailable(
                res1,
                snap1,
                out1,
                required_on_windows=True,
                validator_fn=lambda p: validate_mkv(p, expected_video_codec_ids={"V_MPEG4/ISO/AVC"}),
                expected_ffprobe_codec="h264",
            )

            out2 = td / "hevc.mkv"
            res2, snap2 = run_neoveg(["--encode", str(proj), str(out2), mkv_hevc])
            self._assert_mf_or_clean_unavailable(
                res2,
                snap2,
                out2,
                required_on_windows=False,
                validator_fn=lambda p: validate_mkv(p, expected_video_codec_ids={"V_MPEGH/ISO/HEVC"}),
                expected_ffprobe_codec="hevc",
            )

    def test_f8_av1_mp4_mov_mkv(self) -> None:
        mp4_av1 = find_preset_for_format(["Mp4Av1"], required_keywords=["mp4", "av1"], description="MP4 AV1")
        mov_av1 = find_preset_for_format(["MovAv1"], required_keywords=["mov", "av1"], description="MOV AV1")
        mkv_av1 = find_preset_for_format(["MkvAv1"], required_keywords=["mkv", "av1"], description="MKV AV1")
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            proj = write_neoveg_project(td / "p.neoveg", width=128, height=72, fps=(25, 1), duration_s=0.16)
            for pid, fname, val in (
                (mp4_av1, "out.mp4", lambda p: validate_iso_bmff(p, expected_video_fourccs={b"av01"})),
                (mov_av1, "out.mov", lambda p: validate_iso_bmff(p, expected_video_fourccs={b"av01"})),
                (mkv_av1, "out.mkv", lambda p: validate_mkv(p, expected_video_codec_ids={"V_AV1"})),
            ):
                out = td / fname
                res, snap = run_neoveg(["--encode", str(proj), str(out), pid])
                self._assert_mf_or_clean_unavailable(
                    res, snap, out, required_on_windows=False, validator_fn=val, expected_ffprobe_codec="av1"
                )

    def test_f8_vp9_and_aac_adts(self) -> None:
        vp9_p, vp9_ext, vp9_val = resolve_vp9_preset_and_validator()
        aac_p = find_preset_for_format(["AacAdts"], required_keywords=["aac"], description="AAC ADTS (.aac)")
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            wav = write_wav(td / "tone.wav", sr=48000, channels=2, duration_s=0.20)
            proj = write_neoveg_project(
                td / "p.neoveg", width=128, height=72, fps=(25, 1), duration_s=0.20, wav_path=wav
            )
            out_vp9 = td / f"out_vp9.{vp9_ext}"
            res_vp9, snap_vp9 = run_neoveg(["--encode", str(proj), str(out_vp9), vp9_p])
            self._assert_mf_or_clean_unavailable(
                res_vp9,
                snap_vp9,
                out_vp9,
                required_on_windows=False,
                validator_fn=vp9_val,
                expected_ffprobe_codec="vp9",
            )

            out_aac = td / "out.aac"
            res_aac, snap_aac = run_neoveg(["--encode", str(wav), str(out_aac), aac_p])
            self._assert_mf_or_clean_unavailable(
                res_aac,
                snap_aac,
                out_aac,
                required_on_windows=True,
                validator_fn=validate_adts_aac,
                expected_ffprobe_codec="aac",
            )


@feature_tags("F9")
class TestTier1F9PresetsAndCli(unittest.TestCase):
    def test_f9_cli_encode_all_new_system_presets_resolvable(self) -> None:
        catalog = parse_system_presets_catalog()
        formats = {e["format"] for e in catalog}
        required_formats = {
            "Flac",
            "CineonSequence",
            "AacAdts",
            "Mp3",
            "Y4m",
            "Wave64",
            "Aiff",
            "Au",
            "Caf",
            "JpegSequence",
            "TiffSequence",
        }
        missing = sorted(required_formats - formats)
        self.assertEqual(
            missing,
            [],
            f"presets.rs system_presets() missing RenderFormat entries: {missing}",
        )

    def test_f9_cli_render_extension_inference(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            wav = write_wav(td / "tone.wav", sr=48000, channels=2, duration_s=0.16)
            proj = write_neoveg_project(
                td / "p.neoveg", width=64, height=48, fps=(25, 1), duration_s=0.16, wav_path=wav
            )
            checks = [
                ("ext.flac", lambda p: validate_flac(p, expected_sr=48000, expected_ch=2)),
                ("ext.cin", lambda p: validate_cineon(collect_image_sequence(p, 4)[0], 64, 48)),
                ("ext.mkv", lambda p: validate_mkv(p, expected_w=64, expected_h=48)),
                ("ext.avi", lambda p: validate_avi(p, expected_w=64, expected_h=48)),
                ("ext.aac", validate_adts_aac),
            ]
            for fname, val in checks:
                out = td / fname
                res, snap = run_neoveg(["--render", str(proj), str(out)])
                assert_no_panic_or_crash_log(res, snap, out_dir=td, out_path=out)
                self.assertEqual(res.returncode, 0, f"--render {fname} failed:\n{res.stderr}")
                val(out)

    def test_f9_cli_preset_group_multi_output(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            wav = write_wav(td / "tone.wav", sr=48000, channels=2, duration_s=0.20)
            proj = write_neoveg_project(
                td / "p.neoveg", width=128, height=72, fps=(25, 1), duration_s=0.20, wav_path=wav
            )
            out = td / "bundle.mp4"
            res, snap = run_neoveg(["--encode", str(proj), str(out), "grp.web_multi"])
            assert_no_panic_or_crash_log(res, snap, out_dir=td)
            self.assertEqual(res.returncode, 0, f"grp.web_multi encode failed:\n{res.stderr}")
            validate_web_multi_group_outputs(td, "bundle")

    def test_f9_list_commands_and_render_command(self) -> None:
        bin_path = find_neoveg_bin()
        res, snap = run_neoveg(["--list-commands"], bin_path=bin_path)
        assert_no_panic_or_crash_log(res, snap)
        self.assertEqual(res.returncode, 0, res.stderr)
        self.assertIn("file.render", res.stdout)
        self.assertIn("file.send_to_encoder", res.stdout)

        with tempfile.TemporaryDirectory() as tmp:
            td = Path(tmp)
            proj = write_neoveg_project(td / "p.neoveg", width=64, height=48, fps=(25, 1), duration_s=0.12)
            out_proj = td / "out.neoveg"
            res_cmd, snap_cmd = run_neoveg(
                ["--command", "insert.marker", str(proj), str(out_proj)], bin_path=bin_path
            )
            assert_no_panic_or_crash_log(res_cmd, snap_cmd)
            self.assertEqual(res_cmd.returncode, 0, res_cmd.stderr)
            self.assertTrue(out_proj.is_file())

    def test_f9_render_formats_cargo_test_parity(self) -> None:
        dialogs_rs = (REPO_ROOT / "crates" / "neoveg_ui" / "src" / "dialogs.rs").read_text(encoding="utf-8")
        self.assertIn("AacAdts", dialogs_rs, "dialogs.rs RENDER_TEMPLATES must include AacAdts")
        self.assertIn("Flac", dialogs_rs, "dialogs.rs RENDER_TEMPLATES must include Flac")
        self.assertIn("CineonSequence", dialogs_rs, "dialogs.rs RENDER_TEMPLATES must include CineonSequence")
        if skip_cargo_gates():
            return
        res = subprocess.run(
            ["cargo", "test", "-p", "neoveg_engine", "--lib", "encoder::presets"],
            cwd=str(REPO_ROOT),
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=180,
        )
        assert_no_panic_or_crash_log(res)
        self.assertEqual(res.returncode, 0, f"{res.stdout}\n{res.stderr}")
