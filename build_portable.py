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
    python build_portable.py --no-gh-fallback

Se a compilação local falhar fora do GitHub Actions, o script tenta
automaticamente disparar `gh workflow run build-packages.yml -R LARPFX-ori/neoveg`
e baixar os pacotes prontos (use `--no-gh-fallback` para desativar).
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
import tarfile
import tempfile
import time
import zipfile
from pathlib import Path

DEFAULT_GH_REPO = "LARPFX-ori/neoveg"
DEFAULT_GH_WORKFLOW = "build-packages.yml"
GH_DISCOVERY_TIMEOUT_SECS = 90
GH_POLL_INTERVAL_SECS = 3

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


def should_use_gh_fallback(no_gh_fallback: bool, no_build: bool) -> bool:
    """Retorna True quando o fallback pelo GitHub Actions pode ser acionado."""
    if no_gh_fallback or no_build:
        return False
    if os.environ.get("GITHUB_ACTIONS", "").strip().lower() == "true":
        return False
    return True


def build_gh_workflow_cmd(
    repo: str = DEFAULT_GH_REPO,
    system: str | None = None,
) -> list[str]:
    """Monta o comando `gh workflow run` correspondente ao sistema atual."""
    sys_name = system if system is not None else platform.system()
    if sys_name == "Linux":
        win_flag, lin_flag = "false", "true"
    else:
        win_flag, lin_flag = "true", "false"
    return [
        "gh",
        "workflow",
        "run",
        DEFAULT_GH_WORKFLOW,
        "-R",
        repo,
        "-f",
        f"windows={win_flag}",
        "-f",
        f"linux={lin_flag}",
    ]


def _list_workflow_run_ids(repo: str) -> list[dict[str, object]]:
    res = subprocess.run(
        [
            "gh",
            "run",
            "list",
            "--workflow",
            DEFAULT_GH_WORKFLOW,
            "-R",
            repo,
            "--limit",
            "10",
            "--json",
            "databaseId,status,conclusion,url",
        ],
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=True,
    )
    data = json.loads(res.stdout or "[]")
    return data if isinstance(data, list) else [] #type: ignore


def _wait_for_new_run(
    repo: str,
    known_ids: set[int],
    timeout_secs: int = GH_DISCOVERY_TIMEOUT_SECS,
) -> tuple[int, str]:
    deadline = time.monotonic() + timeout_secs
    while time.monotonic() < deadline:
        for run in _list_workflow_run_ids(repo):
            rid = run.get("databaseId")
            if isinstance(rid, int) and rid not in known_ids:
                url = str(run.get("url") or "")
                return rid, url
        time.sleep(GH_POLL_INTERVAL_SECS)
    raise RuntimeError(
        f"Nenhuma nova execução de `{DEFAULT_GH_WORKFLOW}` apareceu em {timeout_secs}s."
    )


def _wait_until_run_visible(
    repo: str,
    run_id: int,
    timeout_secs: int = GH_DISCOVERY_TIMEOUT_SECS,
) -> None:
    """Espera `gh run view` enxergar a execução antes do `gh run watch`.

    Logo depois do `workflow_dispatch`, a execução já aparece em `gh run list`,
    mas o endpoint `actions/runs/<id>` ainda pode responder HTTP 404 por alguns
    segundos; o `gh run watch` desiste no primeiro 404.
    """
    deadline = time.monotonic() + timeout_secs
    while True:
        res = subprocess.run(
            ["gh", "run", "view", str(run_id), "-R", repo, "--json", "status"],
            capture_output=True,
            text=True,
            encoding="utf-8",
        )
        if res.returncode == 0:
            return
        if time.monotonic() >= deadline:
            detail = (res.stderr or res.stdout or "").strip()
            raise RuntimeError(
                f"A execução #{run_id} não ficou acessível em {timeout_secs}s: {detail}"
            )
        time.sleep(GH_POLL_INTERVAL_SECS)


def verify_sha256sums(sums_file: Path) -> list[Path]:
    """Verifica todos os arquivos listados em `SHA256SUMS.txt` no mesmo diretório."""
    base_dir = sums_file.parent.resolve()
    verified: list[Path] = []
    lines = [
        ln.strip()
        for ln in sums_file.read_text(encoding="utf-8").splitlines()
        if ln.strip()
    ]
    if not lines:
        raise RuntimeError(f"Arquivo de somas vazio: {sums_file}")
    for line in lines:
        parts = line.split(None, 1)
        if len(parts) != 2:
            raise RuntimeError(f"Linha inválida em {sums_file}: {line!r}")
        expected_hex, raw_name = parts[0].lower(), parts[1].strip()
        if raw_name.startswith("*"):
            raw_name = raw_name[1:]
        candidate = (base_dir / raw_name).resolve()
        if not _is_within(candidate, base_dir) or candidate.parent != base_dir:
            raise RuntimeError(f"Nome de arquivo inválido em {sums_file}: {raw_name!r}")
        if not candidate.is_file():
            raise RuntimeError(f"Arquivo ausente na verificação SHA-256: {candidate}")
        actual_hex = _sha256_file(candidate).lower()
        if actual_hex != expected_hex:
            raise RuntimeError(
                f"SHA-256 divergente para {raw_name}: esperado {expected_hex}, obtido {actual_hex}"
            )
        verified.append(candidate)
    return verified


def _safe_rel_member(raw_name: str, strip_top: str | None) -> Path | None:
    norm = raw_name.replace("\\", "/")
    raw_rel = Path(norm)
    if norm.startswith("/") or raw_rel.is_absolute() or ".." in raw_rel.parts:
        raise RuntimeError(f"Caminho inseguro no arquivo compactado: {raw_name!r}")
    if strip_top and norm.startswith(strip_top):
        norm = norm[len(strip_top) :]
    norm = norm.lstrip("/")
    if not norm:
        return None
    rel = Path(norm)
    if rel.is_absolute() or ".." in rel.parts:
        raise RuntimeError(f"Caminho inseguro no arquivo compactado: {raw_name!r}")
    return rel


def _common_safe_top_prefix(names: list[str]) -> str | None:
    if not names:
        return None
    first_seg = names[0].replace("\\", "/").split("/", 1)[0]
    if not first_seg or first_seg in (".", "..") or ":" in first_seg:
        return None
    prefix = first_seg + "/"
    if all(
        n.replace("\\", "/") == first_seg or n.replace("\\", "/").startswith(prefix)
        for n in names
    ):
        return prefix
    return None


def extract_portable_zip(zip_path: Path, dest_dir: Path) -> int:
    """Extrai o ZIP portátil em `dest_dir` com proteção contra Zip Slip,
    preservando arquivos de execução existentes (ex.: preferencias.json, logs/)."""
    dest_dir.mkdir(parents=True, exist_ok=True)
    dest_resolved = dest_dir.resolve()
    count = 0
    with zipfile.ZipFile(zip_path, "r") as zf:
        bad = zf.testzip()
        if bad is not None:
            raise RuntimeError(f"Entrada corrompida no ZIP {zip_path}: {bad}")
        names = [n for n in zf.namelist() if n.strip()]
        top_prefix = _common_safe_top_prefix(names)
        for info in zf.infolist():
            rel = _safe_rel_member(info.filename, top_prefix)
            if rel is None:
                continue
            target = (dest_dir / rel).resolve()
            if not _is_within(target, dest_resolved):
                raise RuntimeError(f"Caminho fora do destino no ZIP: {info.filename!r}")
            if info.is_dir():
                target.mkdir(parents=True, exist_ok=True)
            else:
                target.parent.mkdir(parents=True, exist_ok=True)
                with zf.open(info, "r") as src, target.open("wb") as dst:
                    shutil.copyfileobj(src, dst)
                count += 1
    return count


def extract_portable_tar_gz(tar_path: Path, dest_dir: Path) -> int:
    """Extrai o tar.gz portátil do Linux em `dest_dir` validando cada entrada."""
    dest_dir.mkdir(parents=True, exist_ok=True)
    dest_resolved = dest_dir.resolve()
    count = 0
    with tarfile.open(tar_path, "r:gz") as tf:
        members = tf.getmembers()
        names = [m.name.lstrip("./") for m in members if m.name.lstrip("./")]
        top_prefix = _common_safe_top_prefix(names)
        for m in members:
            if m.issym() or m.islnk():
                raise RuntimeError(f"Link simbólico não permitido no pacote portátil: {m.name!r}")
            cleaned = m.name.lstrip("./")
            if top_prefix and cleaned == top_prefix.rstrip("/"):
                continue
            rel = _safe_rel_member(cleaned, top_prefix)
            if rel is None:
                continue
            target = (dest_dir / rel).resolve()
            if not _is_within(target, dest_resolved):
                raise RuntimeError(f"Caminho fora do destino no tar.gz: {m.name!r}")
            if m.isdir():
                target.mkdir(parents=True, exist_ok=True)
            elif m.isfile():
                src = tf.extractfile(m)
                if src is None:
                    continue
                target.parent.mkdir(parents=True, exist_ok=True)
                with src, target.open("wb") as dst:
                    shutil.copyfileobj(src, dst)
                os.chmod(target, m.mode & 0o755)
                count += 1
    return count


def extract_audited_portable(archive: Path, dest_dir: Path, staging_root: Path) -> int:
    """Extrai o pacote portátil baixado (ZIP ou tar.gz) em `dest_dir` só depois
    de auditar o conteúdo dele (§1.3, §1.4, §2.3).

    A auditoria roda numa cópia extraída em `staging_root`, e não em `dest_dir`.
    Por quê: `dest_dir` (ex.: `dist/`) guarda arquivos que o usuário gerou ao
    usar o programa (pré-renders `.avi`, renders, projetos), que a extração
    preserva e que não fazem parte do pacote. Auditar `dest_dir` inteiro os
    acusaria como "mídia proibida no pacote" (issue #175). Um pacote reprovado
    não chega a tocar `dest_dir`."""
    extract = extract_portable_zip if archive.name.endswith(".zip") else extract_portable_tar_gz
    stage = staging_root / "auditoria-pacote"
    if stage.exists():
        shutil.rmtree(stage)
    extract(archive, stage)
    ensure_no_forbidden_files(stage)
    return extract(archive, dest_dir)


def build_via_github_actions(
    repo_root: Path,
    portable_out: Path | None,
    installer_out: Path,
    repo: str = DEFAULT_GH_REPO,
) -> int:
    """Dispara `build-packages.yml` via GitHub CLI (`gh`), aguarda concluir,
    baixa os artefatos, verifica SHA-256 e atualiza `dist-installer/` e `dist/`."""
    if shutil.which("gh") is None:
        print(
            "ERRO: GitHub CLI (`gh`) não encontrado no PATH; não foi possível acionar o fallback.",
            file=sys.stderr,
        )
        return 1

    cmd = build_gh_workflow_cmd(repo=repo)
    print(f"Acionando fallback no GitHub Actions: {' '.join(cmd)}")
    try:
        before_runs = _list_workflow_run_ids(repo)
        known_ids = {
            r["databaseId"] for r in before_runs if isinstance(r.get("databaseId"), int)
        }
        subprocess.run(cmd, cwd=repo_root, check=True)
        print("Aguardando início da execução no GitHub Actions...")
        run_id, run_url = _wait_for_new_run(repo, known_ids) #type: ignore
        print(f"Execução #{run_id} iniciada{f': {run_url}' if run_url else ''}")
        _wait_until_run_visible(repo, run_id)
        subprocess.run(
            ["gh", "run", "watch", str(run_id), "-R", repo, "--exit-status"],
            cwd=repo_root,
            check=True,
        )
        with tempfile.TemporaryDirectory(prefix="neoveg_gh_dl_") as tmp:
            tmp_dir = Path(tmp)
            print(f"Baixando artefatos da execução #{run_id}...")
            subprocess.run(
                ["gh", "run", "download", str(run_id), "-R", repo, "--dir", str(tmp_dir)],
                cwd=repo_root,
                check=True,
            )
            sums_files = sorted(tmp_dir.rglob("SHA256SUMS.txt"))
            if not sums_files:
                raise RuntimeError("Nenhum arquivo SHA256SUMS.txt encontrado nos artefatos baixados.")
            installer_out.mkdir(parents=True, exist_ok=True)
            copied_artifacts: list[Path] = []
            for sf in sums_files:
                verified = verify_sha256sums(sf)
                for f in verified + [sf]:
                    dst = installer_out / f.name
                    shutil.copy2(f, dst)
                    copied_artifacts.append(dst)
                    print(f"Baixado e verificado (SHA-256 OK): {dst}")

            if portable_out is not None:
                zips = [p for p in copied_artifacts if p.name.endswith("-portable.zip")]
                tars = [p for p in copied_artifacts if p.name.endswith(".tar.gz")]
                archive = zips[0] if zips else (tars[0] if tars else None)
                if archive is not None:
                    n = extract_audited_portable(archive, portable_out, tmp_dir)
                    print(f"Pacote portátil atualizado em: {portable_out} ({n} arquivos)")
    except (subprocess.CalledProcessError, OSError, RuntimeError, ValueError) as e:
        print(f"ERRO no fallback do GitHub Actions: {e}", file=sys.stderr)
        return 1
    return 0


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
    parser.add_argument(
        "--no-gh-fallback",
        action="store_true",
        help="Não dispara o workflow do GitHub Actions caso a compilação local falhe",
    )
    parser.add_argument(
        "--gh-repo",
        default=DEFAULT_GH_REPO,
        help=f"Repositório GitHub usado no fallback remoto (padrão: {DEFAULT_GH_REPO})",
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
    except (subprocess.CalledProcessError, OSError) as e:
        print(
            f"ERRO: a compilação ou cópia local falhou ({e}); a pasta de build foi mantida.",
            file=sys.stderr,
        )
        if should_use_gh_fallback(args.no_gh_fallback, args.no_build):
            return build_via_github_actions(
                repo_root,
                portable_out=out_path,
                installer_out=repo_root / "dist-installer",
                repo=args.gh_repo,
            )
        return 1
    if not args.keep_target:
        remove_target_dir(target_dir, repo_root, out_path)
    return 0


if __name__ == "__main__":
    sys.exit(main())
