#!/usr/bin/env python3
"""Monta o instalador do Neoveg para Windows ou Linux, no sistema em que roda.

- Windows: `Neoveg-Setup-<versão>.exe`.
- Linux: `Neoveg-Setup-<versão>-linux-<arquitetura>` (executável) e o pacote
  portátil `Neoveg-<versão>-linux-<arquitetura>.tar.gz`.

1. Gera o pacote portátil com `build_portable.py` (pasta `dist/`).
2. Compacta `dist/` em ZIP.
3. Anexa o ZIP ao fim do `neoveg-setup` (o instalador próprio, em Rust),
   seguido de um trailer de 16 bytes: tamanho do ZIP (u64 little-endian) e a
   assinatura `NVSETUP1` (ver `crates/neoveg/src/setup/payload.rs`).

O binário do Linux usa a glibc do sistema em que foi compilado: compile na
distribuição mais antiga que deve rodá-lo.

O resultado vai para `dist-installer/` (ignorada pelo git). Como o
`build_portable.py`, a pasta de build do cargo é apagada no fim, a não ser
com `--keep-target`.

Uso:
    python build_installer.py
    python build_installer.py --no-build
    python build_installer.py --keep-target
"""

from __future__ import annotations

import argparse
import platform
import shutil
import stat
import struct
import subprocess
import sys
import tarfile
import tempfile
import tomllib
import zipfile
from pathlib import Path

import build_portable

MAGIC = b"NVSETUP1"
# pastas criadas pelo pacote portátil e que o programa instalado não usa
# (instalado, os dados ficam em %APPDATA%\Neoveg ou ~/.local/share/Neoveg)
SKIP_EMPTY_DIRS = {"logs", "autosave", "layouts"}


def workspace_version(repo_root: Path) -> str:
    with open(repo_root / "Cargo.toml", "rb") as f:
        data = tomllib.load(f)
    return str(data.get("workspace", {}).get("package", {}).get("version", "0.0.0"))


def zip_dist(dist: Path, out_zip: Path) -> int:
    count = 0
    with zipfile.ZipFile(out_zip, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=6) as z:
        for p in sorted(dist.rglob("*")):
            if not p.is_file():
                continue
            rel = p.relative_to(dist)
            if rel.parts and rel.parts[0] in SKIP_EMPTY_DIRS:
                continue
            z.write(p, rel.as_posix())
            count += 1
    return count


def linux_arch() -> str:
    m = platform.machine().lower()
    return {"amd64": "x86_64", "arm64": "aarch64"}.get(m, m or "x86_64")


def tar_portable(dist: Path, out_tar: Path, top: str) -> int:
    """Pacote portátil do Linux: `dist/` dentro da pasta `top`, com as permissões."""
    count = 0
    with tarfile.open(out_tar, "w:gz", compresslevel=6) as t:
        for p in sorted(dist.rglob("*")):
            rel = p.relative_to(dist)
            t.add(p, arcname=f"{top}/{rel.as_posix()}", recursive=False)
            if p.is_file():
                count += 1
    return count


def assemble(setup_exe: Path, payload_zip: Path, out_exe: Path) -> None:
    out_exe.parent.mkdir(parents=True, exist_ok=True)
    size = payload_zip.stat().st_size
    with open(out_exe, "wb") as out:
        with open(setup_exe, "rb") as f:
            shutil.copyfileobj(f, out)
        with open(payload_zip, "rb") as f:
            shutil.copyfileobj(f, out)
        out.write(struct.pack("<Q", size))
        out.write(MAGIC)


def main() -> int:
    parser = argparse.ArgumentParser(description="Monta o instalador do Neoveg")
    parser.add_argument("--no-build", action="store_true", help="Reutiliza o build release")
    parser.add_argument(
        "--keep-target", action="store_true", help="Não apaga a pasta de build do cargo"
    )
    args = parser.parse_args()
    system = platform.system()
    if system not in ("Windows", "Linux"):
        print(f"ERRO: instalador não disponível para {system}.", file=sys.stderr)
        return 1
    windows = system == "Windows"

    repo_root = Path(__file__).resolve().parent
    dist = repo_root / "dist"
    out_dir = repo_root / "dist-installer"
    try:
        target_dir = build_portable.build_and_package(repo_root, dist, args.no_build)
    except subprocess.CalledProcessError as e:
        print(f"ERRO: a compilação falhou ({e}); a pasta de build foi mantida.", file=sys.stderr)
        return 1
    setup_exe = target_dir / "release" / ("neoveg-setup.exe" if windows else "neoveg-setup")
    if not setup_exe.is_file():
        print(f"ERRO: {setup_exe} não encontrado; rode sem --no-build.", file=sys.stderr)
        return 1

    version = workspace_version(repo_root)
    if windows:
        out_exe = out_dir / f"Neoveg-Setup-{version}.exe"
    else:
        out_exe = out_dir / f"Neoveg-Setup-{version}-linux-{linux_arch()}"
    with tempfile.TemporaryDirectory() as tmp:
        payload = Path(tmp) / "payload.zip"
        n = zip_dist(dist, payload)
        try:
            assemble(setup_exe, payload, out_exe)
        except OSError as e:
            print(f"ERRO: não foi possível gravar {out_exe} ({e}).", file=sys.stderr)
            return 1
    if not windows:
        mode = out_exe.stat().st_mode
        out_exe.chmod(mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
    size_mb = out_exe.stat().st_size / (1024 * 1024)
    print(f"Instalador gerado: {out_exe} ({n} arquivos, {size_mb:.1f} MiB)")
    if not windows:
        top = f"Neoveg-{version}-linux-{linux_arch()}"
        out_tar = out_dir / f"{top}.tar.gz"
        try:
            nt = tar_portable(dist, out_tar, top)
        except OSError as e:
            print(f"ERRO: não foi possível gravar {out_tar} ({e}).", file=sys.stderr)
            return 1
        tar_mb = out_tar.stat().st_size / (1024 * 1024)
        print(f"Pacote portátil gerado: {out_tar} ({nt} arquivos, {tar_mb:.1f} MiB)")
    if not args.keep_target:
        build_portable.remove_target_dir(target_dir, repo_root, out_dir)
    return 0


if __name__ == "__main__":
    sys.exit(main())
