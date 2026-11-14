#!/usr/bin/env python3
"""Empacotador portátil do Neoveg (AGENTS.md §3.4 / §3.6).

Compila o binário principal `neoveg` em modo `--release` (sem `-C target-cpu=native`
para preservar compatibilidade com CPUs x86-64 sem AVX2) e gera uma pasta
portátil autocontida em `<target-dir>/portable/neoveg-portable/`, onde `<target-dir>`
é a pasta de build do cargo (respeita `CARGO_TARGET_DIR` e `.cargo/config.toml`).

Uso:
    python build_portable.py
    python build_portable.py --no-build
    python build_portable.py --out ../pacotes/neoveg-portable
"""

from __future__ import annotations

import argparse
import json
import os
import platform
import shutil
import subprocess
import sys
from pathlib import Path

FORBIDDEN_EXTS = {
    ".ttf",
    ".otf",
    ".mp4",
    ".mov",
    ".mkv",
    ".avi",
    ".wav",
    ".mp3",
    ".ogg",
    ".vectorcraft",
}
FORBIDDEN_NAMES = {"ffmpeg", "ffprobe", "ffmpeg.exe", "ffprobe.exe", "AGENTS.md"}
FORBIDDEN_DIRS = {"manuais", "plan", "test_media", "design de layouts"}


def cargo_target_dir(repo_root: Path) -> Path:
    res = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--no-deps"],
        cwd=repo_root,
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=True,
    )
    data = json.loads(res.stdout)
    return Path(data["target_directory"])


def sanitized_env() -> dict[str, str]:
    env = os.environ.copy()
    flags = env.get("RUSTFLAGS", "")
    if "target-cpu=native" in flags:
        raise SystemExit(
            "ERRO (§3.6): RUSTFLAGS contém `target-cpu=native`, proibido em pacotes portáteis."
        )
    return env


def ensure_no_forbidden_files(package_dir: Path) -> None:
    for root, dirs, files in os.walk(package_dir):
        rel_parts = set(Path(root).relative_to(package_dir).parts)
        if rel_parts & FORBIDDEN_DIRS:
            raise SystemExit(f"ERRO (§1.4): pasta proibida incluída no pacote: {root}")
        for d in dirs:
            if d in FORBIDDEN_DIRS:
                raise SystemExit(f"ERRO (§1.4): subpasta proibida encontrada: {Path(root) / d}")
        for name in files:
            p = Path(root) / name
            if name in FORBIDDEN_NAMES:
                raise SystemExit(f"ERRO (§2.3): arquivo proibido incluído no pacote: {p}")
            if p.suffix.lower() in FORBIDDEN_EXTS:
                raise SystemExit(f"ERRO (§1.3 / §1.5): asset/mídia/fonte não permitida: {p}")


def build_and_package(repo_root: Path, out_dir: Path, skip_build: bool) -> None:
    env = sanitized_env()
    if not skip_build:
        print("Compilando `neoveg` em --release (portável, sem target-cpu=native)...")
        subprocess.run(
            ["cargo", "build", "--release", "-p", "neoveg"],
            cwd=repo_root,
            env=env,
            check=True,
        )

    target_dir = cargo_target_dir(repo_root)
    exe_name = "neoveg.exe" if platform.system() == "Windows" else "neoveg"
    bin_path = target_dir / "release" / exe_name
    if not bin_path.is_file():
        raise SystemExit(
            f"Binário de release não encontrado em {bin_path}. Execute sem `--no-build`."
        )

    if out_dir.exists():
        shutil.rmtree(out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)

    shutil.copy2(bin_path, out_dir / exe_name)
    for doc in ("README.md", "ROADMAP.md", "ATTRIBUTION.md", "LICENSE-MIT", "LICENSE-APACHE"):
        src = repo_root / doc
        if src.is_file():
            shutil.copy2(src, out_dir / doc)

    # Pastas criadas em tempo de execução ao lado do executável portátil.
    for runtime_dir in ("logs", "autosave", "layouts"):
        (out_dir / runtime_dir).mkdir(exist_ok=True)

    ensure_no_forbidden_files(out_dir)
    size_mb = (out_dir / exe_name).stat().st_size / (1024 * 1024)
    print(f"Pacote portátil gerado em: {out_dir}")
    print(f"Executável: {exe_name} ({size_mb:.2f} MiB)")


def main() -> int:
    parser = argparse.ArgumentParser(description="Empacotador portátil do Neoveg")
    parser.add_argument(
        "--no-build",
        action="store_true",
        help="Reutiliza o binário release já compilado sem rodar `cargo build`",
    )
    parser.add_argument(
        "--out",
        default=None,
        help=(
            "Pasta de saída do pacote portátil (padrão: <target-dir>/portable/neoveg-portable; "
            "caminhos relativos partem da raiz do repositório)"
        ),
    )
    args = parser.parse_args()

    repo_root = Path(__file__).resolve().parent
    if args.out is None:
        # Segue o target-dir do cargo para que o pacote fique fora de pastas
        # sincronizadas (ex.: Google Drive) quando CARGO_TARGET_DIR apontar para fora.
        out_path = cargo_target_dir(repo_root) / "portable" / "neoveg-portable"
    else:
        out_path = Path(args.out)
        if not out_path.is_absolute():
            out_path = repo_root / out_path

    build_and_package(repo_root, out_path, args.no_build)
    return 0


if __name__ == "__main__":
    sys.exit(main())
