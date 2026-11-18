#!/usr/bin/env python3
"""Empacotador portátil do Neoveg (AGENTS.md §3.4 / §3.6).

Compila o binário principal `neoveg` em modo `--release` (sem `-C target-cpu=native`
para preservar compatibilidade com CPUs x86-64 sem AVX2), copia o executável e os
arquivos de que ele precisa para `dist/` na raiz do repositório e, depois de
confirmar que a cópia deu certo, apaga a pasta de build do cargo (`<target-dir>`,
que respeita `CARGO_TARGET_DIR` e `.cargo/config.toml`). Se a compilação ou a cópia
falhar, a pasta de build é mantida e o erro é informado (AGENTS.md §3.4, item 5).

Uso:
    python build_portable.py
    python build_portable.py --no-build
    python build_portable.py --keep-target
    python build_portable.py --out ../pacotes/neoveg-portable
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import shutil
import subprocess
import sys
from pathlib import Path

FORBIDDEN_MEDIA_EXTS = {
    ".mp4",
    ".mov",
    ".mkv",
    ".avi",
    ".wav",
    ".mp3",
    ".ogg",
    ".vectorcraft",
}
FONT_EXTS = {".ttf", ".otf", ".ttc", ".woff", ".woff2"}
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
}
FORBIDDEN_NAMES = {"ffmpeg", "ffprobe", "ffmpeg.exe", "ffprobe.exe", "agents.md"}
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


def _parse_sidecar_license(sidecar: Path) -> dict[str, str]:
    fields: dict[str, str] = {}
    text = sidecar.read_text(encoding="utf-8", errors="replace")
    for line in text.splitlines():
        if ":" in line:
            key, val = line.split(":", 1)
            fields[key.strip().lower()] = val.strip()
    return fields


def _validate_font_file(package_dir: Path, font_path: Path) -> None:
    rel = font_path.relative_to(package_dir)
    if len(rel.parts) < 3 or rel.parts[0] != "assets" or rel.parts[1] != "fonts":
        raise SystemExit(
            f"ERRO (§1.5): fonte fora de assets/fonts/ não permitida no pacote: {font_path}"
        )
    stem = font_path.stem.lower()
    for bad in PROPRIETARY_FONT_STEMS:
        if bad in stem:
            raise SystemExit(
                f"ERRO (§1.5): fonte proprietária do sistema operacional proibida no pacote: {font_path}"
            )
    sidecar = font_path.parent / f"{font_path.name}.attribution"
    if not sidecar.is_file():
        raise SystemExit(
            f"ERRO (§1.2): fonte sem arquivo de atribuição `.attribution`: {font_path}"
        )
    fields = _parse_sidecar_license(sidecar)
    lic = fields.get("license", "")
    if lic not in ALLOWED_SPDX_LICENSES:
        raise SystemExit(
            f"ERRO (SPDX): licença '{lic}' de {font_path} fora da lista permitida."
        )


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
            if name.lower() in FORBIDDEN_NAMES:
                raise SystemExit(f"ERRO (§2.3): arquivo proibido incluído no pacote: {p}")
            ext = p.suffix.lower()
            if ext in FORBIDDEN_MEDIA_EXTS:
                raise SystemExit(f"ERRO (§1.3): asset/mídia não permitida: {p}")
            if ext in FONT_EXTS:
                _validate_font_file(package_dir, p)


def _sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as fh:
        while True:
            chunk = fh.read(65536)
            if not chunk:
                break
            h.update(chunk)
    return h.hexdigest()


def verify_copied_files(copied_pairs: list[tuple[Path, Path]]) -> None:
    """Confere existência, tamanho > 0 e SHA-256 idêntico de cada arquivo copiado."""
    if not copied_pairs:
        raise SystemExit("ERRO: nenhum arquivo copiado para o pacote portátil.")
    for src, dst in copied_pairs:
        if not dst.is_file():
            raise SystemExit(f"ERRO: arquivo esperado não foi copiado para {dst}.")
        src_size = src.stat().st_size
        dst_size = dst.stat().st_size
        if dst_size == 0 or dst_size != src_size:
            raise SystemExit(
                f"ERRO: a cópia de {src.name} em {dst} tem tamanho inválido ({dst_size} != {src_size})."
            )
        if _sha256_file(dst) != _sha256_file(src):
            raise SystemExit(f"ERRO: SHA-256 divergente ao verificar {dst}.")


def build_and_package(repo_root: Path, out_dir: Path, skip_build: bool) -> Path:
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

    copied_pairs: list[tuple[Path, Path]] = []

    exe_dst = out_dir / exe_name
    shutil.copy2(bin_path, exe_dst)
    copied_pairs.append((bin_path, exe_dst))

    for doc in ("README.md", "ROADMAP.md", "ATTRIBUTION.md", "LICENSE-MIT", "LICENSE-APACHE"):
        src = repo_root / doc
        if src.is_file():
            dst = out_dir / doc
            shutil.copy2(src, dst)
            copied_pairs.append((src, dst))

    # Catálogos de idiomas em `.json` (pt-BR, en, es, fr, de, ja).
    i18n_src = repo_root / "i18n"
    if i18n_src.is_dir():
        i18n_dst = out_dir / "i18n"
        i18n_dst.mkdir(exist_ok=True)
        for lang_file in sorted(i18n_src.glob("*.json")):
            dst = i18n_dst / lang_file.name
            shutil.copy2(lang_file, dst)
            copied_pairs.append((lang_file, dst))

    # Fontes tipográficas abertas e suas atribuições/licenças (assets/fonts/).
    fonts_src = repo_root / "assets" / "fonts"
    if fonts_src.is_dir():
        fonts_dst = out_dir / "assets" / "fonts"
        fonts_dst.mkdir(parents=True, exist_ok=True)
        for src in sorted(fonts_src.rglob("*")):
            if src.is_file():
                rel = src.relative_to(fonts_src)
                dst = fonts_dst / rel
                dst.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(src, dst)
                copied_pairs.append((src, dst))

    # Pastas criadas em tempo de execução ao lado do executável portátil.
    for runtime_dir in ("logs", "autosave", "layouts"):
        (out_dir / runtime_dir).mkdir(exist_ok=True)

    ensure_no_forbidden_files(out_dir)
    verify_copied_files(copied_pairs)
    size_mb = exe_dst.stat().st_size / (1024 * 1024)
    print(f"Pacote portátil gerado em: {out_dir}")
    print(f"Executável: {exe_name} ({size_mb:.2f} MiB)")
    return target_dir


def _is_within(child: Path, parent: Path) -> bool:
    try:
        child.resolve().relative_to(parent.resolve())
    except ValueError:
        return False
    return True


def _looks_like_cargo_target(target_dir: Path) -> bool:
    if (target_dir / "CACHEDIR.TAG").is_file():
        return True
    if target_dir.name == "target" and (
        (target_dir / "release").is_dir() or (target_dir / "debug").is_dir()
    ):
        return True
    return False


def remove_target_dir(target_dir: Path, repo_root: Path, out_dir: Path) -> None:
    """Apaga a pasta de build do cargo depois do pacote pronto (§3.4, item 5).

    Recusa apagar quando a pasta não parece uma pasta de build do cargo, quando
    contém a raiz do repositório, ou quando o pacote portátil está dentro dela.
    """
    candidates: list[Path] = []
    seen: set[Path] = set()
    for p in (target_dir, repo_root / "target"):
        resolved = p.resolve()
        if resolved not in seen:
            seen.add(resolved)
            candidates.append(p)

    for candidate in candidates:
        if not candidate.is_dir():
            continue
        if not _looks_like_cargo_target(candidate):
            print(f"Aviso: {candidate} não parece uma pasta de build do cargo; mantida.")
            continue
        if _is_within(repo_root, candidate):
            print(f"Aviso: {candidate} contém o repositório; mantida.")
            continue
        if _is_within(out_dir, candidate):
            print(f"Aviso: o pacote está dentro de {candidate}; a pasta de build foi mantida.")
            continue
        print(f"Apagando a pasta de build {candidate}...")
        shutil.rmtree(candidate)


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
            "Pasta de saída do pacote portátil (padrão: dist/ na raiz do repositório; "
            "caminhos relativos partem da raiz do repositório)"
        ),
    )
    parser.add_argument(
        "--keep-target",
        action="store_true",
        help="Não apaga a pasta de build do cargo depois de gerar o pacote",
    )
    args = parser.parse_args()

    repo_root = Path(__file__).resolve().parent
    if args.out is None:
        out_path = repo_root / "dist"
    else:
        out_path = Path(args.out)
        if not out_path.is_absolute():
            out_path = repo_root / out_path

    try:
        target_dir = build_and_package(repo_root, out_path, args.no_build)
    except subprocess.CalledProcessError as e:
        print(f"ERRO: a compilação falhou ({e}); a pasta de build foi mantida.", file=sys.stderr)
        return 1
    except OSError as e:
        print(f"ERRO: a cópia do pacote falhou ({e}); a pasta de build foi mantida.", file=sys.stderr)
        return 1
    if not args.keep_target:
        remove_target_dir(target_dir, repo_root, out_path)
    return 0


if __name__ == "__main__":
    sys.exit(main())
