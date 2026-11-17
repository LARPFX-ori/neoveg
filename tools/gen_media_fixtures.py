"""Gera as fixtures de mídia dos testes do motor (AGENTS.md §2.3 e §3.4).

Uso: python tools/gen_media_fixtures.py [pasta_de_saida]

A pasta padrão é `target/fixtures/`. O `ffmpeg` é usado só como gerador
externo de fixtures (fontes sintéticas `lavfi`): ele não é linkado nem
empacotado, e as fixtures não são commitadas. Rode de novo depois de um
`cargo clean`.

Arquivos gerados (todos sintéticos):
  oracle_wav.wav, oracle_flac.flac, oracle_ogg.ogg, oracle_mp3.mp3,
  oracle_aac.m4a, oracle_aac.aac
      tom de 440 Hz, 2 s, 48 kHz, estéreo
      (usados por `media::audio::tests::fixtures_e_oraculo_externo_ffprobe`)
  mf_smoke.mp4
      H.264 320x180, 25 fps, 2 s, com áudio AAC
      (usado por `fuzz_tests::fuzz_mp4_e_media_foundation_hostil`)
"""

import os
import shutil
import subprocess
import sys

TONE = ["-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000:duration=2"]
STEREO = ["-ac", "2"]

AUDIO = [
    ("oracle_wav.wav", ["-c:a", "pcm_s16le"]),
    ("oracle_flac.flac", ["-c:a", "flac"]),
    ("oracle_ogg.ogg", ["-c:a", "libvorbis", "-q:a", "4"]),
    ("oracle_mp3.mp3", ["-c:a", "libmp3lame", "-b:a", "128k"]),
    ("oracle_aac.m4a", ["-c:a", "aac", "-b:a", "128k"]),
    ("oracle_aac.aac", ["-c:a", "aac", "-b:a", "128k", "-f", "adts"]),
    ("oracle_au.au", ["-c:a", "pcm_mulaw"]),
    ("oracle_caf.caf", ["-c:a", "pcm_s16be"]),
    ("oracle_w64.w64", ["-c:a", "pcm_s16le"]),
    ("oracle_adpcm.wav", ["-c:a", "adpcm_ima_wav"]),
    ("oracle_alaw.wav", ["-c:a", "pcm_alaw"]),
]

VIDEO = [
    "-f", "lavfi", "-i", "testsrc2=size=320x180:rate=25:duration=2",
    "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000:duration=2",
    "-c:v", "libx264", "-pix_fmt", "yuv420p", "-profile:v", "main",
    "-c:a", "aac", "-b:a", "96k", "-shortest", "-movflags", "+faststart",
]


def run(ffmpeg, args, out):
    cmd = [ffmpeg, "-hide_banner", "-loglevel", "error", "-y", *args, out]
    r = subprocess.run(cmd, check=False)
    return r.returncode == 0


def main() -> int:
    out_dir = sys.argv[1] if len(sys.argv) > 1 else os.path.join("target", "fixtures")
    ffmpeg = shutil.which("ffmpeg")
    if ffmpeg is None:
        print("ffmpeg não encontrado no PATH", file=sys.stderr)
        return 3
    os.makedirs(out_dir, exist_ok=True)

    status = 0
    jobs = [(name, [*TONE, *STEREO, *codec]) for name, codec in AUDIO]
    jobs.append(("mf_smoke.mp4", VIDEO))
    for name, args in jobs:
        path = os.path.join(out_dir, name)
        if run(ffmpeg, args, path):
            print("ok", path)
        else:
            print("falhou", name, file=sys.stderr)
            status = 1
    return status


if __name__ == "__main__":
    sys.exit(main())
