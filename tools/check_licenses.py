"""Verifica as licenças das dependências (AGENTS.md, portão de qualidade 4).

Uso:
    python tools/check_licenses.py            # audita os 4 alvos desktop suportados
    python tools/check_licenses.py --current  # audita apenas o alvo nativo da máquina
    python tools/check_licenses.py --target <triple>

Lê `cargo metadata` e o grafo de `cargo tree --workspace -e normal` por alvo
desktop suportado (Windows x64, Linux x64, macOS ARM64 e macOS x64 — evitando
`--target all`, que inclui alvos `wasm32` inativos no editor desktop) e falha se
algum pacote tiver licença fora da lista SPDX permitida, exceto as exceções
autorizadas e documentadas em docs/licencas-de-dependencias.md.
"""

import argparse
import json
import re
import subprocess
import sys

DESKTOP_TARGETS = [
    "x86_64-pc-windows-msvc",
    "x86_64-unknown-linux-gnu",
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
]

ALLOWED = {
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
}

# Exceção técnica do SPDX usada junto com Apache-2.0 (não é uma licença).
EXCEPTIONS_SPDX = {"LLVM-exception"}

# Exceções autorizadas: licença -> pacotes (docs/licencas-de-dependencias.md).
AUTHORIZED = {
    "BSL-1.0": {"clipboard-win", "error-code"},
    "Unicode-3.0": {
        "icu_collections", "icu_locale_core", "icu_normalizer", "icu_normalizer_data",
        "icu_properties", "icu_properties_data", "icu_provider", "litemap", "potential_utf",
        "tinystr", "writeable", "yoke", "yoke-derive", "zerofrom", "zerofrom-derive",
        "zerotrie", "zerovec", "zerovec-derive",
    },
    "Zlib": {"foldhash", "const_panic", "konst", "typewit"},
}


def alternatives(expr):
    """Divide uma expressão SPDX em alternativas (OR), cada uma com seus termos (AND/WITH)."""
    expr = expr.replace("/", " OR ")
    alts = []
    for alt in re.split(r"\s+OR\s+", expr):
        terms = [t.strip(" ()") for t in re.split(r"\s+AND\s+|\s+WITH\s+", alt)]
        alts.append([t for t in terms if t])
    return alts


def is_ok(name, expr):
    for terms in alternatives(expr):
        if all(t in ALLOWED or t in EXCEPTIONS_SPDX or name in AUTHORIZED.get(t, set()) for t in terms):
            return True
    return False


def used_packages(target=None):
    """Pacotes presentes no grafo de dependências normais do workspace no alvo."""
    cmd = ["cargo", "tree", "--workspace", "-e", "normal", "--prefix", "none", "--format", "{p}"]
    if target:
        cmd.extend(["--target", target])
    out = subprocess.run(
        cmd, capture_output=True, text=True, encoding="utf-8", check=True,
    ).stdout
    names = set()
    for line in out.splitlines():
        parts = line.split()
        if parts:
            names.add(parts[0])
    return names


def main():
    parser = argparse.ArgumentParser(description="Auditoria SPDX das dependências do Neoveg")
    parser.add_argument(
        "--current",
        action="store_true",
        help="Audita apenas o grafo da plataforma em execução",
    )
    parser.add_argument(
        "--target",
        action="append",
        default=[],
        help="Audita um triple específico (pode ser repetido)",
    )
    args = parser.parse_args()

    meta = json.loads(subprocess.run(
        ["cargo", "metadata", "--format-version", "1"],
        capture_output=True, text=True, encoding="utf-8", check=True,
    ).stdout)
    root = {p["name"] for p in meta["packages"] if p.get("source") is None}
    by_name = {}
    for p in meta["packages"]:
        by_name.setdefault(p["name"], []).append(p)

    if args.target:
        targets = args.target
    elif args.current:
        targets = [None]
    else:
        targets = DESKTOP_TARGETS

    used_union = set()
    bad = {}
    for tgt in targets:
        used = used_packages(tgt)
        used_union.update(used)
        label = tgt or "host"
        for pkg_name in used:
            if pkg_name in root:
                continue
            for p in by_name.get(pkg_name, []):
                lic = p.get("license")
                if not lic:
                    bad.setdefault((pkg_name, "(sem identificador SPDX: verifique license-file)"), []).append(label)
                elif not is_ok(pkg_name, lic):
                    bad.setdefault((pkg_name, lic), []).append(label)

    if bad:
        print("Licenças fora da lista permitida:")
        for (name, lic), tgts in sorted(bad.items()):
            tgt_list = ", ".join(sorted(set(tgts)))
            print(f"  {name}: {lic} [{tgt_list}]")
        return 1
    scope = f"{len(targets)} alvos ({', '.join(t or 'host' for t in targets)})"
    print(f"OK: {len(used_union - root)} dependências únicas em {scope} com licenças permitidas ou autorizadas.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
