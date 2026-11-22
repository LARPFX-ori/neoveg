#!/usr/bin/env python3
"""Monta o instalador do Neoveg para Windows, Linux ou macOS, no sistema em que roda.

- Windows: `Neoveg-Setup-<versão>.exe`.
- Linux: `Neoveg-Setup-<versão>-linux-<arquitetura>` (executável) e o pacote
  portátil `Neoveg-<versão>-linux-<arquitetura>.tar.gz`.
- macOS: `Neoveg.app` em dois pacotes, `Neoveg-<versão>-macos-universal.zip`
  (Intel e Apple Silicon) e `Neoveg-<versão>-macos-x86_64.zip` (só Intel).
  Veja `build_macos()`; os passos 1 a 3 abaixo valem para Windows e Linux.

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
import os
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


MACOS_TARGETS = ("x86_64-apple-darwin", "aarch64-apple-darwin")
# macOS mínimo declarado no Info.plist: o backend Metal do wgpu exige 10.13.
MACOS_MIN_VERSION = "10.13"
MACOS_BUNDLE_ID = "io.github.larpfx-ori.neoveg"


def info_plist(version: str) -> str:
    """Info.plist do `Neoveg.app`.

    `NSMicrophoneUsageDescription` é obrigatório: sem ele, o macOS encerra o
    app quando a gravação de áudio (`playback.rs`) abre o microfone.
    """
    return f"""<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key><string>pt-BR</string>
  <key>CFBundleDisplayName</key><string>Neoveg</string>
  <key>CFBundleExecutable</key><string>neoveg</string>
  <key>CFBundleIconFile</key><string>Neoveg</string>
  <key>CFBundleIdentifier</key><string>{MACOS_BUNDLE_ID}</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleName</key><string>Neoveg</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>{version}</string>
  <key>CFBundleVersion</key><string>{version}</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.video</string>
  <key>LSMinimumSystemVersion</key><string>{MACOS_MIN_VERSION}</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSMicrophoneUsageDescription</key><string>O Neoveg usa o microfone para gravar áudio na timeline.</string>
</dict>
</plist>
"""


def macos_host_target() -> str:
    m = platform.machine().lower()
    return "aarch64-apple-darwin" if m in ("arm64", "aarch64") else "x86_64-apple-darwin"


def build_macos(repo_root: Path, out_dir: Path, skip_build: bool) -> Path:
    """Gera os dois pacotes `.app` do macOS e devolve a pasta de build do cargo.

    1. Compila `neoveg` e `neoveg-setup` para Intel e Apple Silicon
       (`rustup target add aarch64-apple-darwin`).
    2. Gera `Neoveg.icns` com `neoveg-setup --write-iconset` e `iconutil`.
    3. Monta `Neoveg.app` (binário universal feito com `lipo`, e outro só
       Intel), com assinatura ad hoc (`codesign -s -`): o Apple Silicon não
       executa binário sem assinatura. Sem Developer ID, o Gatekeeper pede
       para liberar o app na primeira abertura.
    4. Compacta cada `.app` com `ditto`, que preserva a assinatura.
    """
    env = build_portable.sanitized_env()
    target_dir = build_portable.cargo_target_dir(repo_root)
    if not skip_build:
        for target in MACOS_TARGETS:
            print(f"Compilando `neoveg` em --release para {target}...")
            subprocess.run(
                ["cargo", "build", "--release", "-p", "neoveg", "--target", target],
                cwd=repo_root,
                env=env,
                check=True,
            )
    bins = {t: target_dir / t / "release" / "neoveg" for t in MACOS_TARGETS}
    for b in bins.values():
        if not b.is_file():
            raise SystemExit(f"Binário não encontrado em {b}. Execute sem `--no-build`.")
    setup = target_dir / macos_host_target() / "release" / "neoveg-setup"
    if not setup.is_file():
        raise SystemExit(f"{setup} não encontrado. Execute sem `--no-build`.")

    version = workspace_version(repo_root)
    out_dir.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as tmp_s:
        tmp = Path(tmp_s)
        iconset = tmp / "Neoveg.iconset"
        subprocess.run([str(setup), "--write-iconset", str(iconset)], check=True)
        icns = tmp / "Neoveg.icns"
        subprocess.run(["iconutil", "-c", "icns", "-o", str(icns), str(iconset)], check=True)

        universal = tmp / "neoveg-universal"
        subprocess.run(
            ["lipo", "-create", "-output", str(universal), *map(str, bins.values())],
            check=True,
        )
        variants = (
            ("universal", universal),
            ("x86_64", bins["x86_64-apple-darwin"]),
        )
        for name, exe in variants:
            stage = tmp / name
            app = stage / "Neoveg.app"
            make_app_bundle(repo_root, app, exe, icns, version)
            out_zip = out_dir / f"Neoveg-{version}-macos-{name}.zip"
            if out_zip.exists():
                out_zip.unlink()
            subprocess.run(
                ["ditto", "-c", "-k", "--sequesterRsrc", "--keepParent", str(app), str(out_zip)],
                check=True,
            )
            archs = subprocess.run(
                ["lipo", "-archs", str(app / "Contents" / "MacOS" / "neoveg")],
                check=True,
                capture_output=True,
                text=True,
            ).stdout.strip()
            size_mb = out_zip.stat().st_size / (1024 * 1024)
            print(f"Pacote gerado: {out_zip} ({archs}, {size_mb:.1f} MiB)")
    return target_dir


def make_app_bundle(repo_root: Path, app: Path, exe: Path, icns: Path, version: str) -> None:
    """Monta e assina `Neoveg.app`.

    As traduções e as fontes vão embutidas no binário. `Resources/` leva os
    textos de licença e atribuição, como o pacote portátil. Dentro do `.app`,
    o Neoveg grava os dados em `~/Library/Application Support/Neoveg`
    (`neoveg_common::util::is_macos_app_bundle_dir`), nunca no pacote.
    """
    contents = app / "Contents"
    macos = contents / "MacOS"
    res = contents / "Resources"
    macos.mkdir(parents=True)
    res.mkdir()
    copied: list[tuple[Path, Path]] = []

    exe_dst = macos / "neoveg"
    shutil.copy2(exe, exe_dst)
    os.chmod(exe_dst, 0o755)
    copied.append((exe, exe_dst))
    (contents / "Info.plist").write_text(info_plist(version), encoding="utf-8")
    shutil.copy2(icns, res / "Neoveg.icns")

    for doc in ("README.md", "ATTRIBUTION.md", "LICENSE-MIT", "LICENSE-APACHE"):
        src = repo_root / doc
        if src.is_file():
            shutil.copy2(src, res / doc)
            copied.append((src, res / doc))
    fonts_src = repo_root / "assets" / "fonts"
    if fonts_src.is_dir():
        fonts_dst = res / "assets" / "fonts"
        fonts_dst.mkdir(parents=True)
        for src in sorted(fonts_src.iterdir()):
            if src.is_file():
                shutil.copy2(src, fonts_dst / src.name)
                copied.append((src, fonts_dst / src.name))

    # `Resources/` é a raiz das fontes (`assets/fonts/`), como `dist/` no
    # pacote portátil; `MacOS/` só pode ter o executável.
    build_portable.ensure_no_forbidden_files(res)
    build_portable.ensure_no_forbidden_files(macos)
    build_portable.verify_copied_files(copied)
    subprocess.run(
        ["codesign", "--force", "--sign", "-", "--timestamp=none", str(app)], check=True
    )
    subprocess.run(["codesign", "--verify", "--strict", "--verbose=1", str(app)], check=True)


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
    if system not in ("Windows", "Linux", "Darwin"):
        print(f"ERRO: instalador não disponível para {system}.", file=sys.stderr)
        return 1
    windows = system == "Windows"

    repo_root = Path(__file__).resolve().parent
    dist = repo_root / "dist"
    out_dir = repo_root / "dist-installer"
    if system == "Darwin":
        try:
            target_dir = build_macos(repo_root, out_dir, args.no_build)
        except subprocess.CalledProcessError as e:
            print(f"ERRO: um passo falhou ({e}); a pasta de build foi mantida.", file=sys.stderr)
            return 1
        except OSError as e:
            print(f"ERRO: a montagem do pacote falhou ({e}); a pasta de build foi mantida.", file=sys.stderr)
            return 1
        if not args.keep_target:
            build_portable.remove_target_dir(target_dir, repo_root, out_dir)
        return 0
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
