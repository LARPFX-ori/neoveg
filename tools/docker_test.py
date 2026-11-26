#!/usr/bin/env python3
"""Roda os portões de qualidade (AGENTS.md §3.4) num contêiner Linux ou Windows.

Mesmos passos dos jobs "Portões (Linux)" e "Portões (Windows)" da CI: cargo
fmt, auditoria de licenças, clippy, fixtures e cargo test (um teste por vez;
no Linux, com Xvfb). Só testes: os pacotes vêm do workflow "Pacotes" da CI
(build-packages.yml; `python build_portable.py` o aciona quando o build local falha).

O repositório é montado no contêiner (/src no Linux, C:\\src no Windows). O
build fica num volume do Docker e o cache do cargo em outro: nada é compilado
nem executado na target/ do host, e a compilação fica entre execuções. As
fixtures vão para <repo>/target/fixtures, que é onde os testes as procuram.

Uso:
    python tools/docker_test.py                 # tudo, no workspace inteiro
    python tools/docker_test.py -p neoveg_ui    # clippy e testes de um crate
    python tools/docker_test.py --steps clippy test
    python tools/docker_test.py --windows       # o mesmo num contêiner Windows
    python tools/docker_test.py --build         # reconstrói a imagem antes
    python tools/docker_test.py --shell         # abre um shell no contêiner

Precisa do Docker (Docker Desktop no Windows/macOS). --windows precisa do
Docker Desktop no modo "Windows containers" (o Linux, no modo padrão); os dois
modos não rodam ao mesmo tempo. O padrão é -j 2, para caber em máquinas com
pouca memória (§3.6); mude com --jobs e, no Windows, --memory.
"""

from __future__ import annotations

import argparse
import shlex
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
STEPS = ("fmt", "licenses", "clippy", "fixtures", "test")


@dataclass(frozen=True)
class Platform:
    name: str  # "linux" ou "windows", como o `docker version` informa
    image: str
    dockerfile: Path


LINUX = Platform("linux", "neoveg-test", REPO / "docker" / "test.Dockerfile")
WINDOWS = Platform("windows", "neoveg-test-windows", REPO / "docker" / "test-windows.Dockerfile")


def run(cmd: list[str]) -> int:
    print("+", " ".join(shlex.quote(c) for c in cmd), flush=True)
    return subprocess.call(cmd)


def daemon_os() -> str | None:
    try:
        out = subprocess.run(
            ["docker", "version", "--format", "{{.Server.Os}}"],
            capture_output=True,
            text=True,
            check=False,
        )
    except OSError:
        return None
    return out.stdout.strip() or None if out.returncode == 0 else None


def image_exists(image: str) -> bool:
    return (
        subprocess.call(
            ["docker", "image", "inspect", image],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        == 0
    )


def build_image(p: Platform, no_cache: bool, memory: str) -> int:
    cmd = ["docker", "build", "-f", str(p.dockerfile), "-t", p.image]
    if p is WINDOWS:
        cmd += ["--memory", memory]
    if no_cache:
        cmd.append("--no-cache")
    cmd.append(str(p.dockerfile.parent))
    return run(cmd)


def linux_script(steps: list[str], packages: list[str], jobs: int) -> str:
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


def windows_script(steps: list[str], packages: list[str], jobs: int) -> str:
    scope = " ".join(f"-p '{p}'" for p in packages) or "--workspace"
    j = f"-j {jobs}"
    cmds: list[tuple[str, str]] = []
    if "fmt" in steps:
        cmds.append(("cargo fmt", "cargo fmt --all -- --check"))
    if "licenses" in steps:
        cmds.append(("licencas", "python tools/check_licenses.py"))
    if "clippy" in steps:
        cmds.append(("cargo clippy", f"cargo clippy {scope} --all-targets {j} -- -D warnings"))
    if "fixtures" in steps:
        cmds.append(("fixtures", "python tools/gen_media_fixtures.py target/fixtures"))
        cmds.append(("fixtures AAF", "python tools/gen_aaf_fixtures.py target/fixtures"))
    if "test" in steps:
        cmds.append(("cargo test", f"cargo test {scope} {j} -- --test-threads=1"))
    parts = ["$ErrorActionPreference = 'Stop'", "Set-Location C:\\src"]
    for label, c in cmds:
        parts.append(f"Write-Host '== {label}'")
        parts.append(c)
        parts.append(f"if ($LASTEXITCODE -ne 0) {{ Write-Host 'FALHOU: {label}'; exit $LASTEXITCODE }}")
    return "; ".join(parts)


def main() -> int:
    ap = argparse.ArgumentParser(
        description="Roda os portões de qualidade (AGENTS.md §3.4) num contêiner."
    )
    ap.add_argument("-p", "--package", action="append", default=[], help="crate (repetível)")
    ap.add_argument("--steps", nargs="+", choices=STEPS, default=list(STEPS))
    ap.add_argument("--jobs", type=int, default=2, help="jobs do cargo (padrão: 2)")
    ap.add_argument("--windows", action="store_true", help="contêiner Windows (MSVC)")
    ap.add_argument(
        "--memory",
        default="4g",
        help="memória do contêiner Windows (padrão: 4g; o Hyper-V dá só 1 GB sem isso)",
    )
    ap.add_argument("--build", action="store_true", help="reconstrói a imagem")
    ap.add_argument("--no-cache", action="store_true", help="com --build: sem cache")
    ap.add_argument("--shell", action="store_true", help="abre um shell no contêiner")
    args = ap.parse_args()

    if args.jobs < 1:
        ap.error("--jobs precisa ser >= 1")

    plat = WINDOWS if args.windows else LINUX
    current = daemon_os()
    if current is None:
        print("Docker não está rodando (inicie o Docker Desktop).", file=sys.stderr)
        return 1
    if current != plat.name:
        modo = "Windows containers" if plat is WINDOWS else "Linux containers"
        print(
            f"O Docker está no modo {current}; troque para \"{modo}\" no menu do "
            "Docker Desktop (ou com DockerCli.exe -SwitchDaemon) e rode de novo.",
            file=sys.stderr,
        )
        return 1

    if args.build or not image_exists(plat.image):
        rc = build_image(plat, args.no_cache, args.memory)
        if rc != 0:
            print("falha ao construir a imagem", file=sys.stderr)
            return rc

    if plat is LINUX:
        cmd = [
            "docker", "run", "--rm", "--init",
            "-v", f"{REPO}:/src",
            "-v", "neoveg-target:/src/target",
            "-v", "neoveg-cargo-registry:/usr/local/cargo/registry",
            "-v", "neoveg-cargo-git:/usr/local/cargo/git",
            "-e", f"CARGO_BUILD_JOBS={args.jobs}",
        ]
        shell = ["bash"]
        script = ["bash", "-c", linux_script(args.steps, args.package, args.jobs)]
    else:
        # A target/ do cargo fica no volume (C:\target); só as fixtures ficam
        # em C:\src\target\fixtures, onde os testes as procuram.
        cmd = [
            "docker", "run", "--rm", "--memory", args.memory,
            "-v", f"{REPO}:C:\\src",
            "-v", "neoveg-target-windows:C:\\target",
            "-v", "neoveg-cargo-registry-windows:C:\\cargo\\registry",
            "-e", "CARGO_TARGET_DIR=C:\\target",
            "-e", f"CARGO_BUILD_JOBS={args.jobs}",
        ]
        shell = ["powershell", "-NoLogo"]
        script = [
            "powershell", "-NoLogo", "-NoProfile", "-Command",
            windows_script(args.steps, args.package, args.jobs),
        ]
    if args.shell:
        cmd += ["-it", plat.image, *shell]
    else:
        cmd += [plat.image, *script]
    return run(cmd)


if __name__ == "__main__":
    sys.exit(main())
