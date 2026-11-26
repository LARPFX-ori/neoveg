#!/usr/bin/env python3
"""Roda os portões de qualidade (AGENTS.md §3.4) num contêiner Linux.

Mesmos passos do job "Portões (Linux)" da CI: cargo fmt, auditoria de
licenças, clippy, fixtures e cargo test (um teste por vez, com Xvfb).

O repositório é montado em /src. A pasta target/ do contêiner é um volume do
Docker (neoveg-target), e o cache do cargo é outro (neoveg-cargo-registry): o
build não toca a target/ do host e fica entre execuções. As fixtures vão para
/src/target/fixtures, que é onde os testes as procuram.

Uso:
    python tools/docker_test.py                 # tudo, no workspace inteiro
    python tools/docker_test.py -p neoveg_ui    # clippy e testes de um crate
    python tools/docker_test.py --steps clippy test
    python tools/docker_test.py --build         # reconstrói a imagem antes
    python tools/docker_test.py --shell         # abre um bash no contêiner

Precisa do Docker (Docker Desktop no Windows/macOS). O padrão é -j 2, para
caber em máquinas com pouca memória (§3.6); mude com --jobs.
"""

from __future__ import annotations

import argparse
import shlex
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
IMAGE = "neoveg-test"
DOCKERFILE = REPO / "docker" / "test.Dockerfile"
STEPS = ("fmt", "licenses", "clippy", "fixtures", "test")


def run(cmd: list[str]) -> int:
    print("+", " ".join(shlex.quote(c) for c in cmd), flush=True)
    return subprocess.call(cmd)


def image_exists() -> bool:
    return (
        subprocess.call(
            ["docker", "image", "inspect", IMAGE],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        == 0
    )


def build_image(no_cache: bool) -> int:
    cmd = ["docker", "build", "-f", str(DOCKERFILE), "-t", IMAGE]
    if no_cache:
        cmd.append("--no-cache")
    cmd.append(str(DOCKERFILE.parent))
    return run(cmd)


def inner_script(steps: list[str], packages: list[str], jobs: int) -> str:
    scope = " ".join(f"-p {shlex.quote(p)}" for p in packages) or "--workspace"
    j = f"-j {jobs}"
    parts = ["set -eu", "cd /src"]
    if "fmt" in steps:
        parts.append("echo '== cargo fmt' && cargo fmt --all -- --check")
    if "licenses" in steps:
        parts.append("echo '== licencas' && python3 tools/check_licenses.py")
    if "clippy" in steps:
        parts.append(
            f"echo '== cargo clippy' && cargo clippy {scope} --all-targets {j} -- -D warnings"
        )
    if "fixtures" in steps:
        parts.append(
            "echo '== fixtures' && mkdir -p target/fixtures"
            " && python3 tools/gen_media_fixtures.py target/fixtures"
            " && python3 tools/gen_aaf_fixtures.py target/fixtures"
        )
    if "test" in steps:
        parts.append(
            f"echo '== cargo test' && xvfb-run -a cargo test {scope} {j} -- --test-threads=1"
        )
    return " && ".join(parts)


def main() -> int:
    ap = argparse.ArgumentParser(
        description="Roda os portões de qualidade (AGENTS.md §3.4) num contêiner Linux."
    )
    ap.add_argument("-p", "--package", action="append", default=[], help="crate (repetível)")
    ap.add_argument("--steps", nargs="+", choices=STEPS, default=list(STEPS))
    ap.add_argument("--jobs", type=int, default=2, help="jobs do cargo (padrão: 2)")
    ap.add_argument("--build", action="store_true", help="reconstrói a imagem")
    ap.add_argument("--no-cache", action="store_true", help="com --build: sem cache")
    ap.add_argument("--shell", action="store_true", help="abre um bash no contêiner")
    args = ap.parse_args()

    if args.jobs < 1:
        ap.error("--jobs precisa ser >= 1")

    if args.build or not image_exists():
        rc = build_image(args.no_cache)
        if rc != 0:
            print("falha ao construir a imagem", file=sys.stderr)
            return rc

    cmd = [
        "docker", "run", "--rm", "--init",
        "-v", f"{REPO}:/src",
        "-v", "neoveg-target:/src/target",
        "-v", "neoveg-cargo-registry:/usr/local/cargo/registry",
        "-v", "neoveg-cargo-git:/usr/local/cargo/git",
    ]
    if args.shell:
        cmd += ["-it", IMAGE, "bash"]
    else:
        cmd += [IMAGE, "bash", "-c", inner_script(args.steps, args.package, args.jobs)]
    return run(cmd)


if __name__ == "__main__":
    sys.exit(main())
