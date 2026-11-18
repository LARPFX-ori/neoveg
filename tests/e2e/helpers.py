"""Shared opaque-box test helpers, fixture generators, and binary validators."""

from __future__ import annotations

import datetime
import hashlib
import json
import math
import os
import platform
import re
import shutil
import struct
import subprocess
import tempfile
import wave
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable, Optional

REPO_ROOT = Path(__file__).resolve().parents[2]
TICKS_PER_SEC = 1_411_200_000

ALLOWED_SPDX_LICENSES = {
    "MIT",
    "Apache-2.0",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "OFL-1.1",
    "Ubuntu-font-1.0",
    "Bitstream-Vera",
    "CC0-1.0",
    "CC-BY-4.0",
    "CC-BY-SA-4.0",
    "LicenseRef-Public-Domain",
    "MIT OR Apache-2.0",
    "Apache-2.0 OR MIT",
}

REQUIRED_ATTRIBUTION_KEYS = (
    "asset",
    "title",
    "author",
    "source",
    "license",
    "license-file",
    "added",
    "notes",
)

PROPRIETARY_FONT_STEMS = (
    "segoe",
    "tahoma",
    "arial",
    "calibri",
    "consola",
    "times",
    "verdana",
    "helvetica",
    "wingding",
    "cambria",
    "candara",
    "corbel",
    "georgia",
    "trebuc",
)

PANIC_MARKERS = (
    "panicked at",
    "RUST_BACKTRACE",
    "erro interno na thread",
    "falha interna isolada",
)

MFT_UNAVAILABLE_MARKERS = (
    "indisponível",
    "indisponivel",
    "media foundation",
    "mft",
    "codec",
    "hresult",
    "0xc00d",
    "disponível apenas no windows",
    "disponivel apenas no windows",
    "não suportado",
    "nao suportado",
    "not supported",
    "unavailable",
)


def feature_tags(*tags: str):
    """Attach feature tags (e.g. 'F1', 'F5') to a test method or TestCase class."""

    def decorator(obj):
        existing = tuple(getattr(obj, "_feature_tags", ()))
        merged = tuple(dict.fromkeys((*existing, *tags)))
        obj._feature_tags = merged
        return obj

    return decorator


def exe_name() -> str:
    return "neoveg.exe" if platform.system() == "Windows" else "neoveg"


def find_neoveg_bin() -> Path:
    """Resolve the neoveg binary via NEOVEG_BIN or newest built candidate."""
    env_bin = os.environ.get("NEOVEG_BIN")
    if env_bin:
        p = Path(env_bin)
        if not p.is_file():
            raise AssertionError(f"NEOVEG_BIN points to non-existent file: {p}")
        return p

    name = exe_name()
    candidates = [
        REPO_ROOT / "dist" / name,
        REPO_ROOT / "target" / "release" / name,
        REPO_ROOT / "target" / "debug" / name,
    ]
    existing = [p for p in candidates if p.is_file()]
    if not existing:
        raise AssertionError(
            "No neoveg binary found. Checked NEOVEG_BIN and: "
            + ", ".join(str(c) for c in candidates)
        )
    existing.sort(key=lambda p: p.stat().st_mtime, reverse=True)
    return existing[0]


def allow_destructive_target_clean() -> bool:
    return os.environ.get("NEOVEG_E2E_ALLOW_TARGET_CLEAN") == "1"


def skip_cargo_gates() -> bool:
    return os.environ.get("NEOVEG_E2E_SKIP_CARGO_GATES") == "1"


@dataclass
class CrashLogSnapshot:
    paths: dict[Path, int]


def snapshot_crash_logs(bin_path: Optional[Path] = None) -> CrashLogSnapshot:
    candidates: list[Path] = [
        REPO_ROOT / "logs" / "crash.log",
        Path(tempfile.gettempdir()) / "neoveg" / "logs" / "crash.log",
    ]
    if bin_path is not None:
        candidates.append(bin_path.parent / "logs" / "crash.log")
    sizes: dict[Path, int] = {}
    for p in candidates:
        sizes[p] = p.stat().st_size if p.is_file() else 0
    return CrashLogSnapshot(paths=sizes)


def _is_crash_returncode(rc: int) -> bool:
    if rc < 0:
        return True
    if rc == 101:
        return True
    if rc >= 0xC0000000:
        return True
    return False


def assert_no_panic_or_crash_log(
    res: subprocess.CompletedProcess[str],
    snap: Optional[CrashLogSnapshot] = None,
    out_dir: Optional[Path] = None,
    out_path: Optional[Path] = None,
    expect_failed_cleanup: bool = False,
) -> None:
    """Assert subprocess did not panic, crash with NTSTATUS/signal, or grow crash.log."""
    combined = f"{res.stdout or ''}\n{res.stderr or ''}"
    if _is_crash_returncode(res.returncode):
        raise AssertionError(
            f"Process crashed with exit code {res.returncode} (0x{res.returncode & 0xFFFFFFFF:08X}).\n"
            f"stderr:\n{res.stderr}"
        )
    for marker in PANIC_MARKERS:
        if marker.lower() in combined.lower():
            raise AssertionError(
                f"Detected panic marker {marker!r} in output (rc={res.returncode}).\n"
                f"stderr:\n{res.stderr}"
            )
    if snap is not None:
        for log_path, prev_size in snap.paths.items():
            if log_path.is_file() and log_path.stat().st_size > prev_size:
                raise AssertionError(
                    f"crash.log grew during command execution: {log_path} ({prev_size} -> {log_path.stat().st_size})"
                )
    if out_dir is not None and out_dir.is_dir():
        leftovers = list(out_dir.glob("*.neoveg_part_*"))
        if leftovers:
            raise AssertionError(
                f"Leftover partial temp files after run: {[str(x) for x in leftovers]}"
            )
    if expect_failed_cleanup and out_path is not None and out_path.exists():
        if out_path.stat().st_size == 0:
            raise AssertionError(f"Zero-byte partial output file left behind: {out_path}")


def is_clean_mft_unavailable(
    res: subprocess.CompletedProcess[str],
    snap: CrashLogSnapshot,
    out_dir: Optional[Path] = None,
    out_path: Optional[Path] = None,
) -> bool:
    """Return True iff failure is a clean, non-panicking MFT/codec unavailability error."""
    assert_no_panic_or_crash_log(
        res, snap, out_dir=out_dir, out_path=out_path, expect_failed_cleanup=True
    )
    if res.returncode != 1:
        return False
    combined = f"{res.stdout or ''}\n{res.stderr or ''}".lower()
    if any(m in combined for m in MFT_UNAVAILABLE_MARKERS):
        return True
    return False


def run_neoveg(
    args: list[str],
    bin_path: Optional[Path] = None,
    cwd: Optional[Path] = None,
    timeout: int = 120,
) -> tuple[subprocess.CompletedProcess[str], CrashLogSnapshot]:
    exe = bin_path or find_neoveg_bin()
    snap = snapshot_crash_logs(exe)
    res = subprocess.run(
        [str(exe), *args],
        cwd=str(cwd) if cwd else None,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=timeout,
    )
    return res, snap


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        while True:
            chunk = f.read(65536)
            if not chunk:
                break
            h.update(chunk)
    return h.hexdigest()


# =============================================================================
# Synthetic Fixture Generators (Pure Python, Zero External Dependencies)
# =============================================================================


def write_wav(
    path: Path,
    sr: int = 48000,
    channels: int = 2,
    duration_s: float = 0.25,
    freq: float = 440.0,
    bits: int = 16,
) -> Path:
    """Write a valid PCM WAV (standard 2ch or WAVE_FORMAT_EXTENSIBLE 5.1 6ch)."""
    path.parent.mkdir(parents=True, exist_ok=True)
    n_frames = max(1, int(round(sr * duration_s)))
    if bits != 16:
        raise ValueError("write_wav supports 16-bit PCM")

    samples = bytearray()
    for i in range(n_frames):
        t = i / sr
        val = int(round(math.sin(2.0 * math.pi * freq * t) * 16000.0))
        val = max(-32768, min(32767, val))
        for ch in range(channels):
            # Vary phase slightly per channel so channels are non-identical.
            s = val if ch % 2 == 0 else (val // 2)
            samples.extend(struct.pack("<h", s))

    if channels <= 2:
        with wave.open(str(path), "wb") as wf:
            wf.setnchannels(channels)
            wf.setsampwidth(2)
            wf.setframerate(sr)
            wf.writeframes(bytes(samples))
        return path

    # WAVE_FORMAT_EXTENSIBLE (0xFFFE) for 5.1 / multi-channel audio
    block_align = channels * 2
    byte_rate = sr * block_align
    channel_mask = 0x3F if channels == 6 else ((1 << channels) - 1)
    ksdata_pcm = (
        struct.pack("<IHH", 0x00000001, 0x0000, 0x0010)
        + b"\x80\x00\x00\xAA\x00\x38\x9B\x71"
    )
    fmt_chunk = struct.pack(
        "<HHIIHHHHI",
        0xFFFE,
        channels,
        sr,
        byte_rate,
        block_align,
        16,
        22,
        16,
        channel_mask,
    ) + ksdata_pcm
    data_size = len(samples)
    riff_size = 4 + (8 + len(fmt_chunk)) + (8 + data_size)
    with path.open("wb") as f:
        f.write(b"RIFF")
        f.write(struct.pack("<I", riff_size))
        f.write(b"WAVE")
        f.write(b"fmt ")
        f.write(struct.pack("<I", len(fmt_chunk)))
        f.write(fmt_chunk)
        f.write(b"data")
        f.write(struct.pack("<I", data_size))
        f.write(samples)
    return path


def write_bmp(path: Path, width: int = 64, height: int = 48) -> Path:
    """Write a minimal valid 24-bit uncompressed BMP image in pure Python."""
    path.parent.mkdir(parents=True, exist_ok=True)
    row_bytes = width * 3
    pad = (4 - (row_bytes % 4)) % 4
    stride = row_bytes + pad
    pixel_size = stride * height
    file_size = 14 + 40 + pixel_size

    file_hdr = struct.pack("<2sIHHI", b"BM", file_size, 0, 0, 54)
    dib_hdr = struct.pack(
        "<IIIHHIIIIII",
        40,
        width,
        height,
        1,
        24,
        0,
        pixel_size,
        2835,
        2835,
        0,
        0,
    )
    rows = bytearray()
    for y in range(height):
        for x in range(width):
            b = (x * 4) % 256
            g = (y * 5) % 256
            r = ((x + y) * 3) % 256
            rows.extend((b, g, r))
        rows.extend(b"\x00" * pad)
    path.write_bytes(file_hdr + dib_hdr + bytes(rows))
    return path


def write_neoveg_project(
    path: Path,
    width: int = 64,
    height: int = 48,
    fps: tuple[int, int] = (25, 1),
    duration_s: float = 0.24,
    wav_path: Optional[Path] = None,
    channel_layout: str = "Stereo",
    sample_rate: int = 48000,
    gen_spec: Optional[dict] = None,
    include_video: bool = True,
    extra_settings: Optional[dict] = None,
    markers: Optional[list[dict]] = None,
) -> Path:
    """Write a minimal valid UTF-8 .neoveg JSON project using exact integer Ticks."""
    path.parent.mkdir(parents=True, exist_ok=True)
    ticks_dur = int(round(duration_s * TICKS_PER_SEC))
    settings: dict = {
        "width": width,
        "height": height,
        "fps": {"num": fps[0], "den": fps[1]},
        "sample_rate": sample_rate,
        "channel_layout": channel_layout,
    }
    if extra_settings:
        settings.update(extra_settings)

    media: list[dict] = []
    tracks: list[dict] = []

    if include_video:
        source_spec = gen_spec if gen_spec is not None else {"Generated": "ColorBars"}
        media.append(
            {
                "id": 1,
                "name": "ColorBars",
                "source": source_spec,
                "duration": None,
                "has_video": True,
                "has_audio": False,
            }
        )
        tracks.append(
            {
                "id": 10,
                "name": "Video 1",
                "kind": "Video",
                "events": (
                    [
                        {
                            "id": 100,
                            "name": "ColorBars",
                            "media_id": 1,
                            "start": {"ticks": 0},
                            "length": {"ticks": ticks_dur},
                        }
                    ]
                    if ticks_dur > 0
                    else []
                ),
            }
        )

    if wav_path is not None:
        abs_wav = str(wav_path.resolve())
        media.append(
            {
                "id": 2,
                "name": wav_path.name,
                "source": {"File": {"path": abs_wav}},
                "duration": {"ticks": max(ticks_dur, 1)},
                "has_video": False,
                "has_audio": True,
            }
        )
        tracks.append(
            {
                "id": 11,
                "name": "Audio 1",
                "kind": "Audio",
                "events": (
                    [
                        {
                            "id": 101,
                            "name": wav_path.name,
                            "media_id": 2,
                            "start": {"ticks": 0},
                            "length": {"ticks": ticks_dur},
                        }
                    ]
                    if ticks_dur > 0
                    else []
                ),
            }
        )

    proj: dict = {
        "settings": settings,
        "media": media,
        "tracks": tracks,
    }
    if markers:
        proj["markers"] = markers

    path.write_text(json.dumps(proj, indent=2), encoding="utf-8")
    return path


# =============================================================================
# Preset Discovery Helper (Read-Only against crates/neoveg_engine/src/encoder/presets.rs)
# =============================================================================


def parse_system_presets_catalog() -> list[dict[str, str]]:
    """Parse system_presets() from presets.rs read-only to discover current preset IDs."""
    presets_rs = REPO_ROOT / "crates" / "neoveg_engine" / "src" / "encoder" / "presets.rs"
    if not presets_rs.is_file():
        return []
    text = presets_rs.read_text(encoding="utf-8", errors="replace")
    # Match SysSpec("id", "label", PresetCategory::..., RenderFormat::Variant, ...)
    pattern = re.compile(
        r'SysSpec\(\s*"(?P<id>[^"]+)"\s*,\s*"(?P<label>[^"]+)"\s*,\s*'
        r'PresetCategory::(?P<cat>[A-Za-z0-9_]+)\s*,\s*'
        r'RenderFormat::(?P<format>[A-Za-z0-9_]+)',
        re.MULTILINE,
    )
    matches = list(pattern.finditer(text))
    out: list[dict[str, str]] = []
    for idx, m in enumerate(matches):
        # Bound snippet strictly to this single SysSpec(...) block (never bleeding into the next entry)
        start = m.start()
        limit = matches[idx + 1].start() if idx + 1 < len(matches) else len(text)
        paren_pos = text.find("(", start)
        end = limit
        if paren_pos != -1 and paren_pos < limit:
            depth = 0
            in_str = False
            escaped = False
            for pos in range(paren_pos, limit):
                ch = text[pos]
                if in_str:
                    if escaped:
                        escaped = False
                    elif ch == "\\":
                        escaped = True
                    elif ch == '"':
                        in_str = False
                    continue
                if ch == '"':
                    in_str = True
                elif ch == "(":
                    depth += 1
                elif ch == ")":
                    depth -= 1
                    if depth == 0:
                        end = pos + 1
                        break
        snippet = text[start:end]
        out.append(
            {
                "id": m.group("id"),
                "label": m.group("label"),
                "category": m.group("cat"),
                "format": m.group("format"),
                "snippet": snippet.lower(),
            }
        )
    return out


def find_preset_entry_for_format(
    format_variants: Iterable[str],
    required_keywords: Iterable[str] = (),
    forbidden_keywords: Iterable[str] = (),
    description: str = "target format",
) -> dict[str, str]:
    """Locate a matching system preset entry from presets.rs or fail loudly if absent."""
    catalog = parse_system_presets_catalog()
    variant_set = {v.lower() for v in format_variants}
    req_kw = [k.lower() for k in required_keywords]
    forb_kw = [k.lower() for k in forbidden_keywords]

    fmt_matches = [e for e in catalog if e["format"].lower() in variant_set]

    # Pass 1: exact RenderFormat variant match + required/forbidden keywords across full SysSpec snippet
    for entry in fmt_matches:
        haystack = f"{entry['id']} {entry['label']} {entry['format']} {entry['snippet']}".lower()
        if all(k in haystack for k in req_kw) and not any(k in haystack for k in forb_kw):
            return entry

    # Pass 2: unambiguous exact RenderFormat match (handles Portuguese labels without English keywords
    # or prose in notes mentioning a forbidden codec name), while refusing generic multi-variant formats
    # when a sub-variant keyword ("10", "yuv") is specifically discriminating.
    for entry in fmt_matches:
        fmt_low = entry["format"].lower()
        if "10" in req_kw and fmt_low in {"dpxsequence", "aviuncompressed"}:
            continue
        if "yuv" in req_kw and fmt_low in {"movuncompressed", "aviuncompressed"}:
            continue
        id_label_fmt = f"{entry['id']} {entry['label']} {entry['format']}".lower()
        if not any(k in id_label_fmt for k in forb_kw):
            return entry

    raise AssertionError(
        f"No system preset found in presets.rs for {description} "
        f"(variants={list(format_variants)}, keywords={list(required_keywords)}). "
        f"Available RenderFormats in presets.rs: "
        f"{sorted({e['format'] for e in catalog})}"
    )


def find_preset_for_format(
    format_variants: Iterable[str],
    required_keywords: Iterable[str] = (),
    forbidden_keywords: Iterable[str] = (),
    description: str = "target format",
) -> str:
    """Locate a matching system preset ID from presets.rs or fail loudly if absent."""
    return find_preset_entry_for_format(
        format_variants,
        required_keywords=required_keywords,
        forbidden_keywords=forbidden_keywords,
        description=description,
    )["id"]


def resolve_vp9_preset_and_validator() -> tuple[str, str, object]:
    """Resolve VP9 system preset, expected extension, and container-aware validator."""
    entry = find_preset_entry_for_format(
        ["Vp9", "Mp4Vp9", "MovVp9", "MkvVp9", "WebmVp9"],
        required_keywords=["vp9"],
        description="VP9",
    )
    fmt = entry["format"].lower()
    hint = f"{fmt} {entry['id']} {entry['label']} {entry['snippet']}".lower()
    if "webm" in hint:
        ext = "webm"
    elif "mkv" in hint or "matroska" in hint:
        ext = "mkv"
    elif "mov" in hint:
        ext = "mov"
    else:
        ext = "mp4"

    def _validate_vp9_output(p: Path) -> dict:
        head = p.read_bytes()[:4] if p.is_file() else b""
        if head == b"\x1a\x45\xdf\xa3" or ext in ("mkv", "webm"):
            allowed = {"webm", "matroska"} if ext == "webm" else {"matroska", "webm"}
            return validate_mkv(
                p,
                expected_video_codec_ids={"V_VP9"},
                allowed_doc_types=allowed,
            )
        return validate_iso_bmff(p, expected_video_fourccs={b"vp09"})

    return entry["id"], ext, _validate_vp9_output


def validate_web_multi_group_outputs(out_dir: Path, stem: str) -> list[Path]:
    """Validate grp.web_multi outputs for either 3-output replacement or 4-output append semantics."""
    four_set = [
        out_dir / f"{stem}.mp4",
        out_dir / f"{stem}_1.mp4",
        out_dir / f"{stem}_2.mp4",
        out_dir / f"{stem}_3.m4a",
    ]
    three_set = [
        out_dir / f"{stem}.mp4",
        out_dir / f"{stem}_1.mp4",
        out_dir / f"{stem}_2.m4a",
    ]
    if all(p.is_file() for p in four_set):
        chosen = four_set
    elif all(p.is_file() for p in three_set) and not (out_dir / f"{stem}_3.m4a").exists():
        chosen = three_set
    else:
        found = sorted(p.name for p in out_dir.glob(f"{stem}*"))
        raise AssertionError(
            f"grp.web_multi did not produce expected 3-file or 4-file bundle in {out_dir}; found {found}"
        )

    for p in chosen:
        if p.suffix.lower() == ".mp4":
            validate_iso_bmff(p, expected_video_fourccs={b"avc1", b"avc3"})
        else:
            data = p.read_bytes()
            boxes = {fcc for fcc, _s, _e in _iter_bmff_boxes(data, 0, len(data))}
            if not ({b"ftyp", b"moov", b"mdat"} <= boxes):
                raise AssertionError(f"Group M4A output {p} missing ISO-BMFF boxes (found {boxes})")
    return chosen


# =============================================================================
# Attribution, License & SFNT Font Validators (F1-F4, F10)
# =============================================================================


def parse_attribution_sidecar(path: Path) -> dict[str, str]:
    if not path.is_file():
        raise AssertionError(f"Missing attribution sidecar: {path}")
    text = path.read_text(encoding="utf-8")
    data: dict[str, str] = {}
    last_key: Optional[str] = None
    for line in text.splitlines():
        if not line.strip():
            continue
        if line[0].isspace():
            if last_key is not None:
                data[last_key] = f"{data[last_key]} {line.strip()}".strip()
            continue
        if ":" not in line:
            continue
        key, val = line.split(":", 1)
        last_key = key.strip().lower()
        data[last_key] = val.strip()
    return data


def validate_attribution_sidecar(
    sidecar_path: Path,
    expected_asset: Optional[str] = None,
    required_license: Optional[str] = None,
    base_dir: Optional[Path] = None,
) -> dict[str, str]:
    data = parse_attribution_sidecar(sidecar_path)
    missing = [k for k in REQUIRED_ATTRIBUTION_KEYS if k not in data]
    if missing:
        raise AssertionError(f"{sidecar_path} missing required keys: {missing}")

    for k in REQUIRED_ATTRIBUTION_KEYS:
        val = data[k]
        if not val:
            raise AssertionError(f"{sidecar_path} has empty field '{k}'")
        if val.upper() in {"TBD", "TODO", "UNKNOWN", "N/A", "FIXME"}:
            raise AssertionError(f"{sidecar_path} has placeholder '{val}' in '{k}'")

    if expected_asset is not None and data["asset"] != expected_asset:
        raise AssertionError(
            f"{sidecar_path} asset={data['asset']!r}, expected {expected_asset!r}"
        )

    lic = data["license"]
    if lic not in ALLOWED_SPDX_LICENSES:
        raise AssertionError(
            f"{sidecar_path} license={lic!r} is NOT in AGENTS.md SPDX allowlist"
        )
    if required_license is not None and lic != required_license:
        raise AssertionError(
            f"{sidecar_path} license={lic!r}, expected {required_license!r}"
        )

    m_date = re.match(r"^(\d{4}-\d{2}-\d{2})(?:\b|$)", data["added"])
    if not m_date:
        raise AssertionError(
            f"{sidecar_path} added={data['added']!r} does not start with YYYY-MM-DD"
        )
    try:
        datetime.date.fromisoformat(m_date.group(1))
    except ValueError as exc:
        raise AssertionError(
            f"{sidecar_path} added={data['added']!r} has invalid calendar date: {exc}"
        ) from exc

    # Resolve license-file (comma-separated paths allowed)
    root = base_dir or REPO_ROOT
    lic_files = [x.strip() for x in data["license-file"].split(",") if x.strip()]
    if not lic_files:
        raise AssertionError(f"{sidecar_path} has empty license-file")
    for lf in lic_files:
        cand1 = sidecar_path.parent / lf
        cand2 = root / lf
        if not cand1.is_file() and not cand2.is_file():
            raise AssertionError(
                f"{sidecar_path} references missing license-file '{lf}' "
                f"(checked {cand1} and {cand2})"
            )
    return data


def _decode_sfnt_name_record(raw: bytes, platform_id: int) -> str:
    if platform_id in (0, 3):
        return raw.decode("utf-16-be", errors="replace")
    return raw.decode("latin-1", errors="replace")


def _parse_cmap_subtable(data: bytes, offset: int) -> dict[int, int]:
    if offset + 4 > len(data):
        raise AssertionError(f"SFNT cmap subtable truncated at {offset}")
    fmt = struct.unpack(">H", data[offset : offset + 2])[0]
    mapping: dict[int, int] = {}
    if fmt == 4:
        if offset + 14 > len(data):
            raise AssertionError("SFNT cmap format 4 header truncated")
        length, _lang, seg_count_x2 = struct.unpack(">HHH", data[offset + 2 : offset + 8])
        subtable_end = offset + length
        if length < 14 or subtable_end > len(data):
            raise AssertionError("SFNT cmap format 4 exceeds file bounds")
        seg_count = seg_count_x2 // 2
        end_codes_off = offset + 14
        start_codes_off = end_codes_off + seg_count * 2 + 2
        id_delta_off = start_codes_off + seg_count * 2
        id_range_off = id_delta_off + seg_count * 2
        if id_range_off + seg_count * 2 > subtable_end:
            raise AssertionError(
                f"SFNT cmap format 4 segment arrays exceed subtable length {length}"
            )
        for i in range(seg_count):
            end_c = struct.unpack(">H", data[end_codes_off + i * 2 : end_codes_off + i * 2 + 2])[0]
            start_c = struct.unpack(
                ">H", data[start_codes_off + i * 2 : start_codes_off + i * 2 + 2]
            )[0]
            id_delta = struct.unpack(
                ">h", data[id_delta_off + i * 2 : id_delta_off + i * 2 + 2]
            )[0]
            range_offset = struct.unpack(
                ">H", data[id_range_off + i * 2 : id_range_off + i * 2 + 2]
            )[0]
            if start_c == 0xFFFF and end_c == 0xFFFF:
                break
            # Bound iteration for safety
            if end_c - start_c > 65536:
                continue
            for cp in range(start_c, end_c + 1):
                if range_offset == 0:
                    gid = (cp + id_delta) & 0xFFFF
                else:
                    addr = id_range_off + i * 2 + range_offset + (cp - start_c) * 2
                    if addr + 2 > subtable_end:
                        continue
                    gid = struct.unpack(">H", data[addr : addr + 2])[0]
                    if gid != 0:
                        gid = (gid + id_delta) & 0xFFFF
                if gid != 0:
                    mapping[cp] = gid
    elif fmt == 12:
        if offset + 16 > len(data):
            raise AssertionError("SFNT cmap format 12 truncated")
        num_groups = struct.unpack(">I", data[offset + 12 : offset + 16])[0]
        pos = offset + 16
        for _ in range(min(num_groups, 10000)):
            if pos + 12 > len(data):
                break
            start_c, end_c, start_gid = struct.unpack(">III", data[pos : pos + 12])
            pos += 12
            for cp in range(start_c, min(end_c + 1, start_c + 65536)):
                gid = start_gid + (cp - start_c)
                if gid != 0:
                    mapping[cp] = gid
    return mapping


def validate_sfnt_font(
    path: Path,
    expected_family_substr: Optional[str] = None,
    expected_subfamily_substr: Optional[str] = None,
    required_chars: str = "",
    min_bytes: int = 20_000,
    max_bytes: int = 10_000_000,
) -> dict:
    """Validate TTF/OTF SFNT header, table bounds, `name` records, and `cmap` glyphs."""
    if not path.is_file():
        raise AssertionError(f"Font file does not exist: {path}")
    data = path.read_bytes()
    if len(data) < min_bytes or len(data) > max_bytes:
        raise AssertionError(
            f"Font {path} size {len(data)} outside [{min_bytes}, {max_bytes}]"
        )
    if data[:1] in (b"<", b"{") or data[:9].lower() == b"<!doctype":
        raise AssertionError(f"Font {path} looks like HTML/JSON stub instead of SFNT")

    sfnt_version = data[0:4]
    if sfnt_version not in (b"\x00\x01\x00\x00", b"OTTO", b"true"):
        raise AssertionError(f"Font {path} invalid SFNT magic: {sfnt_version!r}")

    if len(data) < 12:
        raise AssertionError(f"Font {path} SFNT header truncated")
    num_tables = struct.unpack(">H", data[4:6])[0]
    if num_tables == 0 or 12 + num_tables * 16 > len(data):
        raise AssertionError(f"Font {path} invalid numTables={num_tables}")

    tables: dict[str, tuple[int, int]] = {}
    for i in range(num_tables):
        rec = data[12 + i * 16 : 12 + (i + 1) * 16]
        tag = rec[0:4].decode("ascii", errors="replace")
        _csum, off, length = struct.unpack(">III", rec[4:16])
        if off + length > len(data):
            raise AssertionError(
                f"Font {path} table '{tag}' ({off}+{length}) exceeds file size {len(data)}"
            )
        tables[tag] = (off, length)

    for req_tbl in ("head", "name", "cmap"):
        if req_tbl not in tables:
            raise AssertionError(f"Font {path} missing required SFNT table '{req_tbl}'")

    # Parse `name` table
    name_off, name_len = tables["name"]
    if name_len < 6:
        raise AssertionError(f"Font {path} `name` table truncated")
    _fmt, count, str_off = struct.unpack(">HHH", data[name_off : name_off + 6])
    storage_base = name_off + str_off
    names: dict[int, list[str]] = {}
    for i in range(count):
        rec_off = name_off + 6 + i * 12
        if rec_off + 12 > name_off + name_len:
            break
        plat_id, _enc_id, _lang_id, name_id, length, off = struct.unpack(
            ">HHHHHH", data[rec_off : rec_off + 12]
        )
        s_start = storage_base + off
        s_end = s_start + length
        if s_end <= len(data):
            decoded = _decode_sfnt_name_record(data[s_start:s_end], plat_id)
            names.setdefault(name_id, []).append(decoded)

    family_names = names.get(1, []) + names.get(16, [])
    subfamily_names = names.get(2, []) + names.get(17, [])
    full_names = names.get(4, [])
    all_name_text = " | ".join(family_names + subfamily_names + full_names)

    if expected_family_substr:
        if not any(expected_family_substr.lower() in n.lower() for n in family_names + full_names):
            raise AssertionError(
                f"Font {path} `name` table family {family_names!r} does not contain {expected_family_substr!r}"
            )
    if expected_subfamily_substr:
        if not any(
            expected_subfamily_substr.lower() in n.lower() for n in subfamily_names + full_names
        ):
            raise AssertionError(
                f"Font {path} `name` table subfamily {subfamily_names!r} / full {full_names!r} "
                f"does not contain {expected_subfamily_substr!r}"
            )

    # Parse `cmap` table
    cmap_off, cmap_len = tables["cmap"]
    if cmap_len < 4:
        raise AssertionError(f"Font {path} `cmap` table truncated")
    _cmap_ver, num_sub = struct.unpack(">HH", data[cmap_off : cmap_off + 4])
    glyph_map: dict[int, int] = {}
    for i in range(num_sub):
        sub_rec = cmap_off + 4 + i * 8
        if sub_rec + 8 > cmap_off + cmap_len:
            break
        plat_id, _enc_id, sub_off = struct.unpack(">HHI", data[sub_rec : sub_rec + 8])
        if plat_id in (0, 3):
            parsed = _parse_cmap_subtable(data, cmap_off + sub_off)
            glyph_map.update(parsed)

    missing_chars = [ch for ch in required_chars if glyph_map.get(ord(ch), 0) == 0]
    if missing_chars:
        raise AssertionError(
            f"Font {path} missing cmap glyphs for characters: {missing_chars!r}"
        )

    return {
        "tables": list(tables.keys()),
        "families": family_names,
        "subfamilies": subfamily_names,
        "full_names": full_names,
        "summary": all_name_text,
        "cmap_count": len(glyph_map),
    }


# =============================================================================
# Audio, Image Sequence & Video Container Validators (F5-F9)
# =============================================================================


def validate_wav(
    path: Path,
    expected_sr: Optional[int] = None,
    expected_ch: Optional[int] = None,
    expected_bits: Optional[int] = None,
) -> dict:
    if not path.is_file():
        raise AssertionError(f"WAV missing: {path}")
    data = path.read_bytes()
    if len(data) < 44 or data[0:4] != b"RIFF" or data[8:12] != b"WAVE":
        raise AssertionError(f"Invalid WAV header in {path}")
    pos = 12
    fmt_info = None
    data_size = 0
    while pos + 8 <= len(data):
        cid = data[pos : pos + 4]
        csz = struct.unpack("<I", data[pos + 4 : pos + 8])[0]
        if pos + 8 + csz > len(data):
            raise AssertionError(
                f"WAV {path} chunk {cid!r} size {csz} exceeds EOF at {pos} (len={len(data)})"
            )
        payload = data[pos + 8 : pos + 8 + csz]
        if cid == b"fmt " and len(payload) >= 16:
            tag, ch, sr, _br, _ba, bps = struct.unpack("<HHIIHH", payload[:16])
            fmt_info = {"format_tag": tag, "channels": ch, "sample_rate": sr, "bits": bps}
        elif cid == b"data":
            data_size = csz
        pos += 8 + csz + (csz & 1)

    if fmt_info is None:
        raise AssertionError(f"WAV {path} missing 'fmt ' chunk")
    if data_size == 0:
        raise AssertionError(f"WAV {path} has empty 'data' chunk")
    if expected_sr is not None and fmt_info["sample_rate"] != expected_sr:
        raise AssertionError(
            f"WAV {path} sample_rate={fmt_info['sample_rate']}, expected {expected_sr}"
        )
    if expected_ch is not None and fmt_info["channels"] != expected_ch:
        raise AssertionError(
            f"WAV {path} channels={fmt_info['channels']}, expected {expected_ch}"
        )
    if expected_bits is not None and fmt_info["bits"] != expected_bits:
        raise AssertionError(f"WAV {path} bits={fmt_info['bits']}, expected {expected_bits}")
    fmt_info["data_size"] = data_size
    return fmt_info


def validate_flac(
    path: Path,
    expected_sr: Optional[int] = None,
    expected_ch: Optional[int] = None,
    expected_bps: Optional[int] = None,
) -> dict:
    """Validate FLAC magic, STREAMINFO metadata block, and first audio frame sync."""
    if not path.is_file():
        raise AssertionError(f"FLAC file missing: {path}")
    data = path.read_bytes()
    if len(data) < 42:
        raise AssertionError(f"FLAC {path} too short ({len(data)} bytes)")
    if data[0:4] != b"fLaC":
        raise AssertionError(f"FLAC {path} invalid magic {data[0:4]!r}")

    pos = 4
    streaminfo = None
    last_block = False
    while pos + 4 <= len(data):
        b0 = data[pos]
        is_last = bool((b0 >> 7) & 1)
        btype = b0 & 0x7F
        blen = int.from_bytes(data[pos + 1 : pos + 4], "big")
        if btype == 127:
            raise AssertionError(f"FLAC {path} invalid metadata block type 127 at {pos}")
        if pos + 4 + blen > len(data):
            raise AssertionError(f"FLAC {path} metadata block {btype} exceeds EOF at {pos}")
        payload = data[pos + 4 : pos + 4 + blen]
        if pos == 4:
            if btype != 0 or blen != 34:
                raise AssertionError(
                    f"FLAC {path} first block must be STREAMINFO (type=0, len=34), got type={btype}, len={blen}"
                )
            min_bs, max_bs = struct.unpack(">HH", payload[0:4])
            if not (16 <= min_bs <= max_bs <= 65535):
                raise AssertionError(
                    f"FLAC {path} invalid block sizes min={min_bs}, max={max_bs}"
                )
            min_fs = int.from_bytes(payload[4:7], "big")
            max_fs = int.from_bytes(payload[7:10], "big")
            if min_fs > 0 and max_fs > 0 and max_fs < min_fs:
                raise AssertionError(
                    f"FLAC {path} max_frame_size ({max_fs}) < min_frame_size ({min_fs})"
                )
            packed = int.from_bytes(payload[10:18], "big")
            sr = (packed >> 44) & 0xFFFFF
            ch = ((packed >> 41) & 0x7) + 1
            bps = ((packed >> 36) & 0x1F) + 1
            total_samples = packed & 0xFFFFFFFFF
            if sr == 0:
                raise AssertionError(f"FLAC {path} STREAMINFO sample_rate=0")
            if total_samples == 0:
                raise AssertionError(f"FLAC {path} STREAMINFO total_samples=0")
            streaminfo = {
                "sample_rate": sr,
                "channels": ch,
                "bps": bps,
                "total_samples": total_samples,
            }
        pos += 4 + blen
        if is_last:
            last_block = True
            break

    if not last_block or streaminfo is None:
        raise AssertionError(f"FLAC {path} metadata block chain did not terminate cleanly")
    if pos + 9 > len(data):
        raise AssertionError(f"FLAC {path} has no complete audio frame after metadata")
    sync = int.from_bytes(data[pos : pos + 2], "big")
    if (sync & 0xFFFE) != 0xFFF8:
        raise AssertionError(
            f"FLAC {path} invalid audio frame sync 0x{sync:04X} at offset {pos}"
        )
    bs_code = (data[pos + 2] >> 4) & 0x0F
    sr_code = data[pos + 2] & 0x0F
    if bs_code == 0 or sr_code == 0x0F:
        raise AssertionError(
            f"FLAC {path} invalid frame header byte2 0x{data[pos + 2]:02X}"
        )
    ch_assign = (data[pos + 3] >> 4) & 0x0F
    bps_code = (data[pos + 3] >> 1) & 0x07
    if ch_assign > 10 or bps_code == 3 or (data[pos + 3] & 0x01) != 0:
        raise AssertionError(
            f"FLAC {path} invalid frame header byte3 0x{data[pos + 3]:02X}"
        )

    # Parse UTF-8 coded frame/sample number and optional bs/sr extensions, then verify CRC-8 (poly 0x07)
    cur = pos + 4
    first_u = data[cur]
    if first_u < 0x80:
        utf8_len = 1
    else:
        utf8_len = 0
        for bit in range(7, -1, -1):
            if (first_u >> bit) & 1:
                utf8_len += 1
            else:
                break
        if utf8_len < 2 or utf8_len > 7:
            raise AssertionError(
                f"FLAC {path} invalid UTF-8 coded number lead byte 0x{first_u:02X}"
            )
    if cur + utf8_len > len(data):
        raise AssertionError(f"FLAC {path} truncated frame UTF-8 coded number")
    for cont_b in data[cur + 1 : cur + utf8_len]:
        if (cont_b & 0xC0) != 0x80:
            raise AssertionError(f"FLAC {path} invalid UTF-8 continuation byte 0x{cont_b:02X}")
    cur += utf8_len

    if bs_code == 6:
        cur += 1
    elif bs_code == 7:
        cur += 2
    if sr_code == 12:
        cur += 1
    elif sr_code in (13, 14):
        cur += 2

    # Must have CRC-8 + at least 1 subframe byte + 2-byte CRC-16 footer
    if cur + 4 > len(data):
        raise AssertionError(f"FLAC {path} truncated audio frame payload/footer at {cur}")
    crc = 0
    for b in data[pos:cur]:
        crc ^= b
        for _ in range(8):
            crc = ((crc << 1) ^ 0x07) & 0xFF if (crc & 0x80) else (crc << 1) & 0xFF
    if crc != data[cur]:
        raise AssertionError(
            f"FLAC {path} frame header CRC-8 mismatch: computed 0x{crc:02X}, got 0x{data[cur]:02X}"
        )

    if expected_sr is not None and streaminfo["sample_rate"] != expected_sr:
        raise AssertionError(
            f"FLAC {path} sample_rate={streaminfo['sample_rate']}, expected {expected_sr}"
        )
    if expected_ch is not None and streaminfo["channels"] != expected_ch:
        raise AssertionError(
            f"FLAC {path} channels={streaminfo['channels']}, expected {expected_ch}"
        )
    if expected_bps is not None and streaminfo["bps"] != expected_bps:
        raise AssertionError(
            f"FLAC {path} bps={streaminfo['bps']}, expected {expected_bps}"
        )
    return streaminfo


def validate_au(
    path: Path, expected_sr: Optional[int] = None, expected_ch: Optional[int] = None
) -> dict:
    if not path.is_file():
        raise AssertionError(f"AU file missing: {path}")
    data = path.read_bytes()
    if len(data) < 24 or data[0:4] != b".snd":
        raise AssertionError(f"Invalid AU magic in {path}")
    data_off, data_sz, enc, sr, ch = struct.unpack(">IIIII", data[4:24])
    if data_off < 24 or data_off >= len(data):
        raise AssertionError(f"AU {path} invalid data offset {data_off}")
    if data_sz == 0:
        raise AssertionError(f"AU {path} has empty data_size=0")
    if data_sz != 0xFFFFFFFF and data_off + data_sz > len(data):
        raise AssertionError(
            f"AU {path} data payload ({data_off}+{data_sz}) exceeds file size {len(data)}"
        )
    if expected_sr is not None and sr != expected_sr:
        raise AssertionError(f"AU {path} sample_rate={sr}, expected {expected_sr}")
    if expected_ch is not None and ch != expected_ch:
        raise AssertionError(f"AU {path} channels={ch}, expected {expected_ch}")
    return {"encoding": enc, "sample_rate": sr, "channels": ch, "data_size": data_sz}


def validate_caf(
    path: Path, expected_sr: Optional[int] = None, expected_ch: Optional[int] = None
) -> dict:
    if not path.is_file():
        raise AssertionError(f"CAF file missing: {path}")
    data = path.read_bytes()
    if len(data) < 52 or data[0:4] != b"caff":
        raise AssertionError(f"Invalid CAF magic in {path}")
    pos = 8
    desc = None
    has_data = False
    while pos + 12 <= len(data):
        ctype = data[pos : pos + 4]
        csize = struct.unpack(">q", data[pos + 4 : pos + 12])[0]
        body_start = pos + 12
        if ctype == b"desc" and body_start + 32 <= len(data):
            sr = struct.unpack(">d", data[body_start : body_start + 8])[0]
            fmt_id = data[body_start + 8 : body_start + 12]
            flags, bpp, fpp, ch, bps = struct.unpack(
                ">IIIII", data[body_start + 12 : body_start + 32]
            )
            desc = {
                "sample_rate": sr,
                "format_id": fmt_id.decode("ascii", errors="replace"),
                "flags": flags,
                "bytes_per_packet": bpp,
                "frames_per_packet": fpp,
                "channels": ch,
                "bits_per_channel": bps,
            }
        elif ctype == b"data":
            if csize == -1:
                has_data = len(data) > body_start + 4
            else:
                has_data = csize > 4 and body_start + csize <= len(data)
        if csize < 0:
            break
        pos = body_start + csize
    if desc is None or not has_data:
        raise AssertionError(f"CAF {path} missing desc or audio data")
    if expected_sr is not None and int(round(desc["sample_rate"])) != expected_sr:
        raise AssertionError(
            f"CAF {path} sample_rate={desc['sample_rate']}, expected {expected_sr}"
        )
    if expected_ch is not None and desc["channels"] != expected_ch:
        raise AssertionError(
            f"CAF {path} channels={desc['channels']}, expected {expected_ch}"
        )
    return desc


def validate_adts_aac(path: Path) -> dict:
    """Walk ADTS frames from offset 0 to EOF verifying 0xFFF syncwords and frame lengths."""
    if not path.is_file():
        raise AssertionError(f"ADTS AAC file missing: {path}")
    data = path.read_bytes()
    if len(data) < 7:
        raise AssertionError(f"ADTS {path} too short ({len(data)} bytes)")
    pos = 0
    frames = 0
    sr_table = [
        96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350
    ]
    sr = 0
    ch = 0
    while pos < len(data):
        if pos + 7 > len(data):
            raise AssertionError(f"ADTS {path} truncated frame header at {pos}")
        if data[pos] != 0xFF or (data[pos + 1] & 0xF6) != 0xF0:
            raise AssertionError(f"ADTS {path} invalid syncword at {pos}")
        prot_abs = data[pos + 1] & 0x01
        hdr_len = 7 if prot_abs else 9
        sf_idx = (data[pos + 2] >> 2) & 0x0F
        if sf_idx >= len(sr_table):
            raise AssertionError(f"ADTS {path} invalid sampling_frequency_index={sf_idx}")
        sr = sr_table[sf_idx]
        ch = ((data[pos + 2] & 0x01) << 2) | ((data[pos + 3] >> 6) & 0x03)
        if not (1 <= ch <= 7):
            raise AssertionError(f"ADTS {path} invalid channel_configuration={ch}")
        frame_len = (
            ((data[pos + 3] & 0x03) << 11)
            | (data[pos + 4] << 3)
            | ((data[pos + 5] >> 5) & 0x07)
        )
        if frame_len <= hdr_len or pos + frame_len > len(data):
            raise AssertionError(
                f"ADTS {path} invalid frame_length={frame_len} (hdr_len={hdr_len}) at {pos}/{len(data)}"
            )
        pos += frame_len
        frames += 1
    if frames == 0:
        raise AssertionError(f"ADTS {path} contained 0 frames")
    return {"frames": frames, "sample_rate": sr, "channels": ch}


def validate_png(
    path: Path, expected_w: Optional[int] = None, expected_h: Optional[int] = None
) -> dict:
    if not path.is_file():
        raise AssertionError(f"PNG missing: {path}")
    data = path.read_bytes()
    if len(data) < 33 or data[0:8] != b"\x89PNG\r\n\x1a\n" or data[12:16] != b"IHDR":
        raise AssertionError(f"Invalid PNG header in {path}")
    w, h = struct.unpack(">II", data[16:24])
    pos = 8
    has_idat = False
    has_iend = False
    while pos + 12 <= len(data):
        clen = struct.unpack(">I", data[pos : pos + 4])[0]
        ctype = data[pos + 4 : pos + 8]
        if pos + 12 + clen > len(data):
            raise AssertionError(
                f"PNG {path} chunk {ctype!r} length {clen} exceeds EOF at {pos}"
            )
        if ctype == b"IDAT" and clen > 0:
            has_idat = True
        elif ctype == b"IEND":
            has_iend = True
            break
        pos += 12 + clen
    if not has_idat:
        raise AssertionError(f"PNG {path} missing non-empty IDAT chunk")
    if not has_iend:
        raise AssertionError(f"PNG {path} missing IEND terminator chunk")
    if expected_w is not None and w != expected_w:
        raise AssertionError(f"PNG {path} width={w}, expected {expected_w}")
    if expected_h is not None and h != expected_h:
        raise AssertionError(f"PNG {path} height={h}, expected {expected_h}")
    return {"width": w, "height": h}


def validate_cineon(
    path: Path, expected_w: Optional[int] = None, expected_h: Optional[int] = None
) -> dict:
    """Validate Kodak Cineon v4.5 header and 10-bit RGB payload."""
    if not path.is_file():
        raise AssertionError(f"Cineon file missing: {path}")
    data = path.read_bytes()
    if len(data) < 1024:
        raise AssertionError(f"Cineon {path} too short ({len(data)} bytes)")
    magic = data[0:4]
    if magic == b"\x80\x2a\x5f\xd7":
        e = ">"
    elif magic == b"\xd7\x5f\x2a\x80":
        e = "<"
    else:
        raise AssertionError(f"Cineon {path} invalid magic {magic!r}")
    data_off = struct.unpack(f"{e}I", data[4:8])[0]
    if data_off < 712 or data_off >= len(data):
        raise AssertionError(f"Cineon {path} invalid data_off={data_off}")
    total_len = struct.unpack(f"{e}I", data[20:24])[0]
    if total_len != 0 and total_len != len(data):
        raise AssertionError(f"Cineon {path} total_len={total_len} != {len(data)}")
    if data[24:28] != b"V4.5":
        raise AssertionError(f"Cineon {path} version {data[24:32]!r} != V4.5")
    channels = data[193]
    if not (1 <= channels <= 4):
        raise AssertionError(f"Cineon {path} invalid channels={channels}")
    bits = data[198]
    w, h = struct.unpack(f"{e}II", data[200:208])
    if expected_w is not None and w != expected_w:
        raise AssertionError(f"Cineon {path} width={w}, expected {expected_w}")
    if expected_h is not None and h != expected_h:
        raise AssertionError(f"Cineon {path} height={h}, expected {expected_h}")
    if bits == 10 and len(data) - data_off < w * h * 4:
        raise AssertionError(
            f"Cineon {path} payload {len(data) - data_off} < {w * h * 4} for 10-bit RGB"
        )
    return {"width": w, "height": h, "channels": channels, "bits": bits}


def validate_dpx(
    path: Path,
    expected_w: Optional[int] = None,
    expected_h: Optional[int] = None,
    expected_bits: Optional[int] = None,
) -> dict:
    """Validate SMPTE 268M DPX header and uncompressed payload."""
    if not path.is_file():
        raise AssertionError(f"DPX file missing: {path}")
    data = path.read_bytes()
    if len(data) < 1664:
        raise AssertionError(f"DPX {path} too short ({len(data)} bytes)")
    magic = data[0:4]
    if magic == b"SDPX":
        e = ">"
    elif magic == b"XPDS":
        e = "<"
    else:
        raise AssertionError(f"DPX {path} invalid magic {magic!r}")
    data_off = struct.unpack(f"{e}I", data[4:8])[0]
    if data_off < 852 or data_off >= len(data):
        raise AssertionError(f"DPX {path} invalid data_off={data_off}")
    if data[8:12] not in (b"V2.0", b"V1.0"):
        raise AssertionError(f"DPX {path} invalid version {data[8:16]!r}")
    file_sz = struct.unpack(f"{e}I", data[16:20])[0]
    if file_sz != 0 and file_sz != len(data):
        raise AssertionError(f"DPX {path} file_size={file_sz} != {len(data)}")
    w, h = struct.unpack(f"{e}II", data[772:780])
    descriptor = data[800]
    bit_depth = data[803]
    packing = struct.unpack(f"{e}H", data[804:806])[0]
    encoding = struct.unpack(f"{e}H", data[806:808])[0]
    if w == 0 or h == 0:
        raise AssertionError(f"DPX {path} has zero dimension {w}x{h}")
    if expected_w is not None and w != expected_w:
        raise AssertionError(f"DPX {path} width={w}, expected {expected_w}")
    if expected_h is not None and h != expected_h:
        raise AssertionError(f"DPX {path} height={h}, expected {expected_h}")
    if bit_depth not in (8, 10, 12, 16):
        raise AssertionError(f"DPX {path} invalid bit_depth={bit_depth}")
    if expected_bits is not None and bit_depth != expected_bits:
        raise AssertionError(f"DPX {path} bit_depth={bit_depth}, expected {expected_bits}")

    # Verify uncompressed pixel payload size (SMPTE 268M descriptor & 10-bit packing)
    payload_len = len(data) - data_off
    if encoding == 0:
        if descriptor in (51, 52, 103):
            n_comp = 4
        elif descriptor in (50, 102):
            n_comp = 3
        elif descriptor == 100:
            n_comp = 2
        else:
            n_comp = 1
        if bit_depth == 10 and packing in (1, 2) and n_comp == 3:
            min_payload = w * h * 4
        else:
            min_payload = (w * h * n_comp * bit_depth) // 8
        if payload_len < min_payload:
            raise AssertionError(
                f"DPX {path} uncompressed payload {payload_len} < minimum {min_payload} "
                f"for {w}x{h} ({bit_depth}-bit, desc={descriptor}, packing={packing})"
            )
    elif payload_len < 16:
        raise AssertionError(f"DPX {path} RLE payload truncated ({payload_len} bytes)")
    return {
        "width": w,
        "height": h,
        "descriptor": descriptor,
        "bit_depth": bit_depth,
        "packing": packing,
        "encoding": encoding,
    }


def validate_exr(
    path: Path,
    expected_w: Optional[int] = None,
    expected_h: Optional[int] = None,
) -> dict:
    """Validate OpenEXR magic, attributes (channels/compression/dataWindow), and offset table."""
    if not path.is_file():
        raise AssertionError(f"EXR file missing: {path}")
    data = path.read_bytes()
    if len(data) < 64 or data[0:4] != b"\x76\x2f\x31\x01":
        raise AssertionError(f"EXR {path} invalid magic {data[0:4]!r}")
    ver_flags = struct.unpack("<I", data[4:8])[0]
    if (ver_flags & 0xFF) != 2:
        raise AssertionError(f"EXR {path} unexpected version {ver_flags & 0xFF}")
    pos = 8
    attrs: dict[str, tuple[str, bytes]] = {}
    while pos < len(data):
        if data[pos] == 0:
            pos += 1
            break
        n_end = data.find(b"\x00", pos)
        if n_end < 0:
            raise AssertionError(f"EXR {path} unterminated attr name at {pos}")
        aname = data[pos:n_end].decode("ascii", errors="replace")
        pos = n_end + 1
        t_end = data.find(b"\x00", pos)
        if t_end < 0:
            raise AssertionError(f"EXR {path} unterminated attr type for {aname}")
        atype = data[pos:t_end].decode("ascii", errors="replace")
        pos = t_end + 1
        if pos + 4 > len(data):
            raise AssertionError(f"EXR {path} truncated attr size for {aname}")
        asz = struct.unpack("<I", data[pos : pos + 4])[0]
        pos += 4
        if pos + asz > len(data):
            raise AssertionError(f"EXR {path} attr {aname} exceeds EOF")
        attrs[aname] = (atype, data[pos : pos + asz])
        pos += asz

    for req in ("channels", "compression", "dataWindow", "displayWindow"):
        if req not in attrs:
            raise AssertionError(f"EXR {path} missing required attribute '{req}'")

    comp_bytes = attrs["compression"][1]
    if len(comp_bytes) < 1:
        raise AssertionError(f"EXR {path} 'compression' attribute is empty")
    comp = comp_bytes[0]
    dw_bytes = attrs["dataWindow"][1]
    if len(dw_bytes) != 16:
        raise AssertionError(
            f"EXR {path} 'dataWindow' attribute has length {len(dw_bytes)}, expected 16"
        )
    xmin, ymin, xmax, ymax = struct.unpack("<iiii", dw_bytes)
    w = xmax - xmin + 1
    h = ymax - ymin + 1
    if w <= 0 or h <= 0:
        raise AssertionError(f"EXR {path} invalid dataWindow dimensions {w}x{h}")
    if expected_w is not None and w != expected_w:
        raise AssertionError(f"EXR {path} width={w}, expected {expected_w}")
    if expected_h is not None and h != expected_h:
        raise AssertionError(f"EXR {path} height={h}, expected {expected_h}")

    # Parse chlist
    ch_bytes = attrs["channels"][1]
    cpos = 0
    channels: list[tuple[str, int]] = []
    while cpos < len(ch_bytes) and ch_bytes[cpos] != 0:
        cend = ch_bytes.find(b"\x00", cpos)
        if cend < 0 or cend + 17 > len(ch_bytes):
            break
        cname = ch_bytes[cpos:cend].decode("ascii", errors="replace")
        ptype = struct.unpack("<i", ch_bytes[cend + 1 : cend + 5])[0]
        channels.append((cname, ptype))
        cpos = cend + 17

    if not channels:
        raise AssertionError(f"EXR {path} chlist has 0 channels")
    if pos + 8 > len(data):
        raise AssertionError(f"EXR {path} missing scanline offset table")
    first_chunk_off = struct.unpack("<Q", data[pos : pos + 8])[0]
    if not (pos < first_chunk_off < len(data)):
        raise AssertionError(
            f"EXR {path} first chunk offset {first_chunk_off} out of [{pos}, {len(data)})"
        )
    return {"width": w, "height": h, "compression": comp, "channels": channels}


def collect_image_sequence(
    base_path: Path, expected_count: Optional[int] = None
) -> list[Path]:
    """Locate 0-indexed 6-digit sequence files `{stem}_{:06}.{ext}`."""
    parent = base_path.parent
    stem = base_path.stem
    ext = base_path.suffix.lstrip(".")
    files = sorted(parent.glob(f"{stem}_[0-9][0-9][0-9][0-9][0-9][0-9].{ext}"))
    if not files:
        raise AssertionError(
            f"No sequence files matching {parent / f'{stem}_??????.{ext}'}"
        )
    if expected_count is not None and len(files) != expected_count:
        raise AssertionError(
            f"Sequence {base_path} produced {len(files)} frames, expected {expected_count}"
        )
    for idx, f in enumerate(files):
        expected_name = f"{stem}_{idx:06d}.{ext}"
        if f.name != expected_name:
            raise AssertionError(
                f"Sequence gap/mismatch at index {idx}: got {f.name}, expected {expected_name}"
            )
    return files


def _iter_bmff_boxes(data: bytes, start: int, end: int):
    pos = start
    while pos + 8 <= end:
        sz32 = struct.unpack(">I", data[pos : pos + 4])[0]
        fcc = data[pos + 4 : pos + 8]
        hdr = 8
        if sz32 == 1:
            if pos + 16 > end:
                raise AssertionError(f"ISO-BMFF extended box truncated at {pos}")
            sz = struct.unpack(">Q", data[pos + 8 : pos + 16])[0]
            hdr = 16
        elif sz32 == 0:
            sz = end - pos
        else:
            sz = sz32
        if sz < hdr or pos + sz > end:
            raise AssertionError(
                f"ISO-BMFF box {fcc!r} invalid size {sz} at {pos}..{end}"
            )
        yield fcc, pos + hdr, pos + sz
        pos += sz


def _parse_bmff_chunk_offsets(stbl_kids: dict[bytes, tuple[int, int]], data: bytes) -> list[int]:
    offsets: list[int] = []
    if b"stco" in stbl_kids:
        cs, ce = stbl_kids[b"stco"]
        if cs + 8 <= ce:
            cnt = struct.unpack(">I", data[cs + 4 : cs + 8])[0]
            for i in range(min(cnt, (ce - cs - 8) // 4)):
                offsets.append(struct.unpack(">I", data[cs + 8 + i * 4 : cs + 12 + i * 4])[0])
    elif b"co64" in stbl_kids:
        cs, ce = stbl_kids[b"co64"]
        if cs + 8 <= ce:
            cnt = struct.unpack(">I", data[cs + 4 : cs + 8])[0]
            for i in range(min(cnt, (ce - cs - 8) // 8)):
                offsets.append(struct.unpack(">Q", data[cs + 8 + i * 8 : cs + 16 + i * 8])[0])
    return offsets


def validate_iso_bmff(
    path: Path,
    expected_w: Optional[int] = None,
    expected_h: Optional[int] = None,
    expected_video_fourccs: Optional[set[bytes]] = None,
    require_qt_brand: bool = False,
    expect_audio: Optional[bool] = None,
    expected_audio_fourccs: Optional[set[bytes]] = None,
) -> dict:
    """Pure-Python QuickTime / ISO-BMFF (.mov/.mp4) box & sample-table validator."""
    if not path.is_file():
        raise AssertionError(f"MOV/MP4 file missing: {path}")
    data = path.read_bytes()
    top = list(_iter_bmff_boxes(data, 0, len(data)))
    by_type = {fcc: (s, e) for fcc, s, e in top}
    for req in (b"ftyp", b"moov", b"mdat"):
        if req not in by_type:
            raise AssertionError(
                f"ISO-BMFF {path} missing top-level box {req!r} (found {[x[0] for x in top]})"
            )
    ftyp_s, ftyp_e = by_type[b"ftyp"]
    major_brand = data[ftyp_s : ftyp_s + 4]
    compat_brands = [
        data[i : i + 4] for i in range(ftyp_s + 8, ftyp_e - 3, 4)
    ]
    if require_qt_brand and major_brand != b"qt  " and b"qt  " not in compat_brands:
        raise AssertionError(
            f"QuickTime MOV {path} lacks 'qt  ' brand in ftyp (major={major_brand!r}, compat={compat_brands!r})"
        )

    mdat_spans = [(s, e) for fcc, s, e in top if fcc == b"mdat" and e > s]
    if not mdat_spans:
        raise AssertionError(f"ISO-BMFF {path} has empty or missing mdat payload")

    moov_s, moov_e = by_type[b"moov"]
    moov_children = list(_iter_bmff_boxes(data, moov_s, moov_e))
    traks = [(s, e) for fcc, s, e in moov_children if fcc == b"trak"]
    if not traks:
        raise AssertionError(f"ISO-BMFF {path} moov contains 0 trak boxes")

    video_info = None
    audio_info = None
    for ts, te in traks:
        trak_kids = {fcc: (s, e) for fcc, s, e in _iter_bmff_boxes(data, ts, te)}
        if b"mdia" not in trak_kids:
            continue
        ms, me = trak_kids[b"mdia"]
        mdia_kids = {fcc: (s, e) for fcc, s, e in _iter_bmff_boxes(data, ms, me)}
        if b"hdlr" not in mdia_kids or b"minf" not in mdia_kids:
            continue
        hs, he = mdia_kids[b"hdlr"]
        if hs + 12 > he:
            continue
        hdlr_type = data[hs + 8 : hs + 12]
        minf_s, minf_e = mdia_kids[b"minf"]
        minf_kids = {fcc: (s, e) for fcc, s, e in _iter_bmff_boxes(data, minf_s, minf_e)}
        if b"stbl" not in minf_kids:
            continue
        stbl_s, stbl_e = minf_kids[b"stbl"]
        stbl_kids = {fcc: (s, e) for fcc, s, e in _iter_bmff_boxes(data, stbl_s, stbl_e)}
        if b"stsd" not in stbl_kids:
            continue
        stsd_s, stsd_e = stbl_kids[b"stsd"]
        if stsd_s + 16 > stsd_e:
            continue
        entry_sz = struct.unpack(">I", data[stsd_s + 8 : stsd_s + 12])[0]
        entry_fcc = data[stsd_s + 12 : stsd_s + 16]
        entry_s = stsd_s + 8
        if hdlr_type == b"vide" and entry_s + 36 <= stsd_e:
            vw, vh = struct.unpack(">HH", data[entry_s + 32 : entry_s + 36])
            sample_count = 0
            if b"stsz" in stbl_kids:
                ss, se = stbl_kids[b"stsz"]
                if ss + 12 <= se:
                    sample_count = struct.unpack(">I", data[ss + 8 : ss + 12])[0]
            if sample_count == 0:
                raise AssertionError(f"ISO-BMFF {path} 'vide' track has sample_count=0")
            offsets = _parse_bmff_chunk_offsets(stbl_kids, data)
            if not offsets:
                raise AssertionError(f"ISO-BMFF {path} 'vide' track has empty stco/co64 chunk offsets")
            for off in offsets:
                if not any(ms <= off < me for ms, me in mdat_spans):
                    raise AssertionError(
                        f"ISO-BMFF {path} 'vide' chunk offset {off} outside mdat spans {mdat_spans}"
                    )
            video_info = {
                "fourcc": entry_fcc,
                "width": vw,
                "height": vh,
                "sample_count": sample_count,
                "entry_size": entry_sz,
            }
        elif hdlr_type == b"soun" and entry_s + 36 <= stsd_e:
            ch = struct.unpack(">H", data[entry_s + 24 : entry_s + 26])[0]
            sr_fixed = struct.unpack(">I", data[entry_s + 32 : entry_s + 36])[0]
            sr = sr_fixed >> 16
            offsets = _parse_bmff_chunk_offsets(stbl_kids, data)
            if not offsets:
                raise AssertionError(f"ISO-BMFF {path} 'soun' track has empty stco/co64 chunk offsets")
            for off in offsets:
                if not any(ms <= off < me for ms, me in mdat_spans):
                    raise AssertionError(
                        f"ISO-BMFF {path} 'soun' chunk offset {off} outside mdat spans {mdat_spans}"
                    )
            audio_info = {"fourcc": entry_fcc, "channels": ch, "sample_rate": sr}

    if video_info is None:
        raise AssertionError(f"ISO-BMFF {path} missing 'vide' track")
    if expected_w is not None and video_info["width"] != expected_w:
        raise AssertionError(
            f"ISO-BMFF {path} width={video_info['width']}, expected {expected_w}"
        )
    if expected_h is not None and video_info["height"] != expected_h:
        raise AssertionError(
            f"ISO-BMFF {path} height={video_info['height']}, expected {expected_h}"
        )
    if expected_video_fourccs is not None and video_info["fourcc"] not in expected_video_fourccs:
        raise AssertionError(
            f"ISO-BMFF {path} video fourcc={video_info['fourcc']!r}, expected one of {expected_video_fourccs!r}"
        )
    if expect_audio is True and audio_info is None:
        raise AssertionError(f"ISO-BMFF {path} expected an audio 'soun' track, found none")
    if expect_audio is False and audio_info is not None:
        raise AssertionError(f"ISO-BMFF {path} unexpected audio track present")
    if (
        expected_audio_fourccs is not None
        and audio_info is not None
        and audio_info["fourcc"] not in expected_audio_fourccs
    ):
        raise AssertionError(
            f"ISO-BMFF {path} audio fourcc={audio_info['fourcc']!r}, expected one of {expected_audio_fourccs!r}"
        )

    return {
        "major_brand": major_brand,
        "compat_brands": compat_brands,
        "video": video_info,
        "audio": audio_info,
    }


def _read_ebml_vint(buf: bytes, pos: int, is_id: bool) -> tuple[int, int, bool]:
    if pos >= len(buf):
        raise AssertionError(f"EBML VINT out of bounds at {pos}")
    b0 = buf[pos]
    if b0 == 0:
        raise AssertionError(f"EBML VINT leading byte is 0x00 at {pos}")
    width = 8 - b0.bit_length() + 1
    if pos + width > len(buf):
        raise AssertionError(f"EBML VINT truncated at {pos} (width={width})")
    raw = int.from_bytes(buf[pos : pos + width], "big")
    if is_id:
        return raw, pos + width, False
    mask = (1 << (7 * width)) - 1
    val = raw & mask
    unknown = val == mask
    return val, pos + width, unknown


def _iter_ebml_elements(buf: bytes, start: int, end: int):
    pos = start
    while pos < end:
        eid, pos, _ = _read_ebml_vint(buf, pos, is_id=True)
        sz, pos, unknown = _read_ebml_vint(buf, pos, is_id=False)
        elem_end = end if unknown else min(end, pos + sz)
        if not unknown and pos + sz > end:
            raise AssertionError(
                f"EBML element 0x{eid:X} size {sz} at {pos} exceeds parent end {end}"
            )
        yield eid, pos, elem_end
        pos = elem_end


def validate_mkv(
    path: Path,
    expected_w: Optional[int] = None,
    expected_h: Optional[int] = None,
    expected_video_codec_ids: Optional[set[str]] = None,
    expect_audio: Optional[bool] = None,
    expected_audio_codec_ids: Optional[set[str]] = None,
    expected_sr: Optional[int] = None,
    expected_ch: Optional[int] = None,
    allowed_doc_types: Optional[set[str]] = None,
) -> dict:
    """Pure-Python EBML/Matroska (.mkv/.webm) header, Tracks, and Cluster/SimpleBlock validator."""
    if not path.is_file():
        raise AssertionError(f"MKV file missing: {path}")
    data = path.read_bytes()
    top = list(_iter_ebml_elements(data, 0, len(data)))
    if not top or top[0][0] != 0x1A45DFA3:
        raise AssertionError(f"MKV {path} does not start with EBML header (0x1A45DFA3)")

    doc_type = None
    for eid, s, e in _iter_ebml_elements(data, top[0][1], top[0][2]):
        if eid == 0x4282:
            doc_type = data[s:e].rstrip(b"\x00").decode("ascii", errors="replace")
    valid_docs = allowed_doc_types if allowed_doc_types is not None else {"matroska"}
    if doc_type not in valid_docs:
        raise AssertionError(f"MKV {path} DocType={doc_type!r}, expected one of {sorted(valid_docs)}")

    segments = [(s, e) for eid, s, e in top if eid == 0x18538067]
    if not segments:
        raise AssertionError(f"MKV {path} missing Segment (0x18538067)")
    seg_s, seg_e = segments[0]

    video_tracks: list[dict] = []
    audio_tracks: list[dict] = []
    cluster_count = 0
    block_count = 0

    for eid, s, e in _iter_ebml_elements(data, seg_s, seg_e):
        if eid == 0x1654AE6B:  # Tracks
            for tid, ts, te in _iter_ebml_elements(data, s, e):
                if tid != 0xAE:  # TrackEntry
                    continue
                tinfo: dict = {}
                for teid, ks, ke in _iter_ebml_elements(data, ts, te):
                    if teid == 0xD7:
                        tinfo["track_number"] = int.from_bytes(data[ks:ke], "big")
                    elif teid == 0x83:
                        tinfo["track_type"] = int.from_bytes(data[ks:ke], "big")
                    elif teid == 0x86:
                        tinfo["codec_id"] = data[ks:ke].rstrip(b"\x00").decode("ascii", errors="replace")
                    elif teid == 0x63A2:
                        tinfo["codec_private"] = data[ks:ke]
                    elif teid == 0xE0:  # Video
                        for veid, vs, ve in _iter_ebml_elements(data, ks, ke):
                            if veid == 0xB0:
                                tinfo["width"] = int.from_bytes(data[vs:ve], "big")
                            elif veid == 0xBA:
                                tinfo["height"] = int.from_bytes(data[vs:ve], "big")
                    elif teid == 0xE1:  # Audio
                        for aeid, a_s, a_e in _iter_ebml_elements(data, ks, ke):
                            if aeid == 0xB5:
                                if a_e - a_s == 4:
                                    tinfo["sample_rate"] = struct.unpack(">f", data[a_s:a_e])[0]
                                elif a_e - a_s == 8:
                                    tinfo["sample_rate"] = struct.unpack(">d", data[a_s:a_e])[0]
                            elif aeid == 0x9F:
                                tinfo["channels"] = int.from_bytes(data[a_s:a_e], "big")
                if tinfo.get("track_type") == 1:
                    video_tracks.append(tinfo)
                elif tinfo.get("track_type") == 2:
                    audio_tracks.append(tinfo)
        elif eid == 0x1F43B675:  # Cluster
            cluster_count += 1
            for ceid, cs, ce in _iter_ebml_elements(data, s, e):
                if ceid == 0xA3:  # SimpleBlock (>=4B header + >=1B frame payload)
                    if ce - cs > 4:
                        block_count += 1
                elif ceid == 0xA0:  # BlockGroup -> Block (0xA1)
                    for geid, gs, ge in _iter_ebml_elements(data, cs, ce):
                        if geid == 0xA1 and ge - gs > 4:
                            block_count += 1

    if not video_tracks:
        raise AssertionError(f"MKV {path} has no Video TrackEntry")
    if cluster_count == 0 or block_count == 0:
        raise AssertionError(
            f"MKV {path} missing Cluster/SimpleBlock data (clusters={cluster_count}, blocks={block_count})"
        )

    vt = video_tracks[0]
    if vt.get("width", 0) <= 0 or vt.get("height", 0) <= 0:
        raise AssertionError(
            f"MKV {path} Video TrackEntry missing positive PixelWidth/PixelHeight (got {vt.get('width')}x{vt.get('height')})"
        )
    if expected_w is not None and vt.get("width") != expected_w:
        raise AssertionError(f"MKV {path} PixelWidth={vt.get('width')}, expected {expected_w}")
    if expected_h is not None and vt.get("height") != expected_h:
        raise AssertionError(f"MKV {path} PixelHeight={vt.get('height')}, expected {expected_h}")
    if (
        expected_video_codec_ids is not None
        and vt.get("codec_id") not in expected_video_codec_ids
    ):
        raise AssertionError(
            f"MKV {path} video CodecID={vt.get('codec_id')!r}, expected {expected_video_codec_ids!r}"
        )

    if expect_audio is True and not audio_tracks:
        raise AssertionError(f"MKV {path} expected audio TrackEntry, found none")
    if expect_audio is False and audio_tracks:
        raise AssertionError(f"MKV {path} unexpected audio TrackEntry present")
    if audio_tracks:
        at = audio_tracks[0]
        if (
            expected_audio_codec_ids is not None
            and at.get("codec_id") not in expected_audio_codec_ids
        ):
            raise AssertionError(
                f"MKV {path} audio CodecID={at.get('codec_id')!r}, expected {expected_audio_codec_ids!r}"
            )
        if expected_sr is not None and int(round(at.get("sample_rate", 0))) != expected_sr:
            raise AssertionError(
                f"MKV {path} audio SamplingFrequency={at.get('sample_rate')}, expected {expected_sr}"
            )
        if expected_ch is not None and at.get("channels") != expected_ch:
            raise AssertionError(
                f"MKV {path} audio Channels={at.get('channels')}, expected {expected_ch}"
            )

    return {
        "doc_type": doc_type,
        "video": vt,
        "audio": audio_tracks[0] if audio_tracks else None,
        "clusters": cluster_count,
        "blocks": block_count,
    }


def validate_avi(
    path: Path,
    expected_w: Optional[int] = None,
    expected_h: Optional[int] = None,
    expected_compressions: Optional[set[bytes]] = None,
    expected_bit_count: Optional[int] = None,
    expect_audio: Optional[bool] = None,
    expected_sr: Optional[int] = None,
    expected_ch: Optional[int] = None,
) -> dict:
    """Pure-Python RIFF/AVI structural validator (`avih`, `strh`, `strf`, `movi`, `idx1`)."""
    if not path.is_file():
        raise AssertionError(f"AVI file missing: {path}")
    data = path.read_bytes()
    if len(data) < 64 or data[0:4] != b"RIFF" or data[8:12] != b"AVI ":
        raise AssertionError(f"Invalid RIFF AVI header in {path}")
    riff_sz = struct.unpack("<I", data[4:8])[0]
    if 8 + riff_sz > len(data):
        raise AssertionError(f"AVI {path} truncated RIFF size {riff_sz}")

    video_strf = None
    audio_strf = None
    idx1_size = 0
    stream_idx = 0
    video_stream_indices: list[int] = []
    audio_stream_indices: list[int] = []
    movi_chunks: list[tuple[bytes, int]] = []

    def walk_chunks(start: int, end: int, cur_stream_type: Optional[bytes] = None, in_movi: bool = False):
        nonlocal video_strf, audio_strf, idx1_size, stream_idx
        pos = start
        stream_type = cur_stream_type
        while pos + 8 <= end:
            cid = data[pos : pos + 4]
            csz = struct.unpack("<I", data[pos + 4 : pos + 8])[0]
            body_s = pos + 8
            body_e = min(end, body_s + csz)
            if cid == b"LIST" and body_s + 4 <= body_e:
                ltype = data[body_s : body_s + 4]
                walk_chunks(
                    body_s + 4,
                    body_e,
                    stream_type if ltype != b"strl" else None,
                    in_movi=(in_movi or ltype == b"movi"),
                )
            elif cid == b"strh" and body_s + 4 <= body_e:
                stream_type = data[body_s : body_s + 4]
                if stream_type == b"vids":
                    video_stream_indices.append(stream_idx)
                elif stream_type == b"auds":
                    audio_stream_indices.append(stream_idx)
                stream_idx += 1
            elif cid == b"strf":
                if stream_type == b"vids" and body_s + 40 <= body_e:
                    bw, bh, _pl, bcnt = struct.unpack("<iiHH", data[body_s + 4 : body_s + 16])
                    comp = data[body_s + 16 : body_s + 20]
                    video_strf = {
                        "width": bw,
                        "height": abs(bh),
                        "bit_count": bcnt,
                        "compression": comp,
                    }
                elif stream_type == b"auds" and body_s + 16 <= body_e:
                    wtag, ch, sr, _br, _ba, bps = struct.unpack(
                        "<HHIIHH", data[body_s : body_s + 16]
                    )
                    audio_strf = {
                        "format_tag": wtag,
                        "channels": ch,
                        "sample_rate": sr,
                        "bits": bps,
                    }
            elif cid == b"idx1":
                if body_s + csz <= end:
                    idx1_size = csz
            elif in_movi and len(cid) == 4 and csz > 0 and body_s + csz <= end:
                movi_chunks.append((cid, csz))
            pos = body_s + csz + (csz & 1)

    walk_chunks(12, min(len(data), 8 + riff_sz))
    video_cids = {
        f"{i:02d}{suf}".encode("ascii") for i in video_stream_indices for suf in ("dc", "db")
    } or {b"00dc", b"00db"}
    audio_cids = {f"{i:02d}wb".encode("ascii") for i in audio_stream_indices}
    video_chunks = sum(1 for cid, csz in movi_chunks if cid in video_cids and csz > 0)
    audio_chunks = sum(1 for cid, csz in movi_chunks if cid in audio_cids and csz > 0)

    if video_strf is None:
        raise AssertionError(f"AVI {path} missing video strf")
    if video_chunks == 0:
        raise AssertionError(f"AVI {path} has 0 non-empty video chunks in movi (expected {sorted(video_cids)})")
    if idx1_size < 16:
        raise AssertionError(f"AVI {path} missing or empty idx1 index (size={idx1_size})")
    if expected_w is not None and video_strf["width"] != expected_w:
        raise AssertionError(
            f"AVI {path} width={video_strf['width']}, expected {expected_w}"
        )
    if expected_h is not None and video_strf["height"] != expected_h:
        raise AssertionError(
            f"AVI {path} height={video_strf['height']}, expected {expected_h}"
        )
    if (
        expected_compressions is not None
        and video_strf["compression"] not in expected_compressions
    ):
        raise AssertionError(
            f"AVI {path} compression={video_strf['compression']!r}, expected {expected_compressions!r}"
        )
    if expected_bit_count is not None and video_strf["bit_count"] != expected_bit_count:
        raise AssertionError(
            f"AVI {path} bit_count={video_strf['bit_count']}, expected {expected_bit_count}"
        )
    if expect_audio is True and (audio_strf is None or audio_chunks == 0):
        raise AssertionError(
            f"AVI {path} expected audio stream & 01wb chunks, got strf={audio_strf}, chunks={audio_chunks}"
        )
    if expect_audio is False and audio_strf is not None:
        raise AssertionError(f"AVI {path} unexpected audio stream present")
    if expected_sr is not None and audio_strf is not None and audio_strf["sample_rate"] != expected_sr:
        raise AssertionError(
            f"AVI {path} audio sample_rate={audio_strf['sample_rate']}, expected {expected_sr}"
        )
    if expected_ch is not None and audio_strf is not None and audio_strf["channels"] != expected_ch:
        raise AssertionError(
            f"AVI {path} audio channels={audio_strf['channels']}, expected {expected_ch}"
        )

    return {
        "video": video_strf,
        "audio": audio_strf,
        "video_chunks": video_chunks,
        "audio_chunks": audio_chunks,
    }


_DECODERS_CACHE: Optional[set[str]] = None


def ffmpeg_has_decoder(decoder_name: str) -> bool:
    """Return True iff ffmpeg/ffprobe on PATH lists `decoder_name` in `-decoders`."""
    global _DECODERS_CACHE
    if _DECODERS_CACHE is None:
        tool = shutil.which("ffmpeg") or shutil.which("ffprobe")
        if not tool:
            _DECODERS_CACHE = set()
        else:
            res = subprocess.run(
                [tool, "-hide_banner", "-decoders"],
                capture_output=True,
                text=True,
                encoding="utf-8",
                errors="replace",
                timeout=30,
            )
            names: set[str] = set()
            if res.returncode == 0:
                for line in res.stdout.splitlines():
                    parts = line.strip().split()
                    if len(parts) >= 2 and len(parts[0]) == 6:
                        names.add(parts[1].lower())
            _DECODERS_CACHE = names
    return decoder_name.lower() in _DECODERS_CACHE


def run_ffprobe_oracle(path: Path) -> Optional[dict]:
    """Run ffprobe + ffmpeg decode check when available on PATH; return parsed JSON or None."""
    if path.suffix.lower() == ".cin" and not ffmpeg_has_decoder("cineon"):
        return None
    ffprobe = shutil.which("ffprobe")
    if not ffprobe:
        return None
    res = subprocess.run(
        [
            ffprobe,
            "-v",
            "error",
            "-show_format",
            "-show_streams",
            "-of",
            "json",
            str(path),
        ],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=60,
    )
    if res.returncode != 0:
        raise AssertionError(
            f"ffprobe rejected {path} (rc={res.returncode}):\n{res.stderr}"
        )
    info = json.loads(res.stdout)
    ffmpeg = shutil.which("ffmpeg")
    if ffmpeg:
        dec = subprocess.run(
            [ffmpeg, "-v", "error", "-i", str(path), "-f", "null", "-"],
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=60,
        )
        if dec.returncode != 0 or (dec.stderr or "").strip():
            raise AssertionError(
                f"ffmpeg decode check failed for {path} (rc={dec.returncode}):\n{dec.stderr}"
            )
    return info


# =============================================================================
# Isolated Workspace Helper for `build_portable.py` (Concurrent-Safe)
# =============================================================================


def isolated_build_env(extra: Optional[dict[str, str]] = None) -> dict[str, str]:
    """Copy os.environ and strip CARGO_TARGET_DIR so isolated build_portable.py uses ws/target."""
    env = os.environ.copy()
    env.pop("CARGO_TARGET_DIR", None)
    if extra:
        env.update(extra)
    return env


def stage_isolated_build_workspace(
    tmp_dir: Path,
    with_release_bin: bool = True,
    copy_repo_assets: bool = True,
) -> Path:
    """Create an isolated temp repo copy for testing build_portable.py safely.

    This avoids touching `<repo_root>/target/` while concurrent cargo builds run.
    """
    ws = tmp_dir / "ws"
    ws.mkdir(parents=True, exist_ok=True)
    shutil.copy2(REPO_ROOT / "build_portable.py", ws / "build_portable.py")
    for doc in ("README.md", "ROADMAP.md", "ATTRIBUTION.md", "LICENSE-MIT", "LICENSE-APACHE"):
        src = REPO_ROOT / doc
        if src.is_file():
            shutil.copy2(src, ws / doc)

    # Minimal single-crate Cargo manifest so `cargo metadata --format-version 1 --no-deps`
    # resolves `target_directory` to `ws/target` cleanly and offline.
    (ws / "Cargo.toml").write_text(
        '[package]\nname = "neoveg"\nversion = "0.1.0"\nedition = "2021"\n',
        encoding="utf-8",
    )
    (ws / "src").mkdir(exist_ok=True)
    (ws / "src" / "main.rs").write_text("fn main() {}\n", encoding="utf-8")

    if copy_repo_assets and (REPO_ROOT / "assets").is_dir():
        shutil.copytree(REPO_ROOT / "assets", ws / "assets")

    if with_release_bin:
        rel_dir = ws / "target" / "release"
        rel_dir.mkdir(parents=True, exist_ok=True)
        src_bin = REPO_ROOT / "target" / "release" / exe_name()
        if src_bin.is_file():
            shutil.copy2(src_bin, rel_dir / exe_name())
        else:
            # Create a dummy marker executable only if no real release binary exists yet
            # (tests that execute the packaged binary use find_neoveg_bin()).
            (rel_dir / exe_name()).write_bytes(b"MZ" + b"\x00" * 1024)
    return ws
