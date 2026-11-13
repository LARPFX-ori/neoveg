"""Verifica as licenças das dependências (AGENTS.md, portão de qualidade 4).

Uso: python tools/check_licenses.py

Lê `cargo metadata` e falha se algum pacote usado pelo projeto tiver licença
fora da lista SPDX permitida, exceto as exceções autorizadas e documentadas em
docs/licencas-de-dependencias.md.
"""

import json
import re
import subprocess
import sys

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
    "MPL-2.0": {
        "symphonia",
        "symphonia-bundle-flac",
        "symphonia-bundle-mp3",
        "symphonia-codec-aac",
        "symphonia-codec-adpcm",
        "symphonia-codec-alac",
        "symphonia-codec-pcm",
        "symphonia-codec-vorbis",
        "symphonia-common",
        "symphonia-core",
        "symphonia-format-caf",
        "symphonia-format-isomp4",
        "symphonia-format-mkv",
        "symphonia-format-ogg",
        "symphonia-format-riff",
        "symphonia-metadata",
    },
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


def used_packages():
    """Pacotes realmente presentes no grafo de dependências normais da plataforma atual."""
    out = subprocess.run(
        ["cargo", "tree", "--workspace", "-e", "normal", "--prefix", "none", "--format", "{p}"],
        capture_output=True, text=True, check=True,
    ).stdout
    names = set()
    for line in out.splitlines():
        parts = line.split()
        if parts:
            names.add(parts[0])
    return names


def main():
    meta = json.loads(subprocess.run(
        ["cargo", "metadata", "--format-version", "1"],
        capture_output=True, text=True, check=True,
    ).stdout)
    used = used_packages()
    root = {p["name"] for p in meta["packages"] if p.get("source") is None}
    bad = []
    for p in meta["packages"]:
        if p["name"] not in used or p["name"] in root:
            continue
        lic = p.get("license")
        if not lic:
            bad.append((p["name"], "(sem identificador SPDX: verifique license-file)"))
        elif not is_ok(p["name"], lic):
            bad.append((p["name"], lic))
    if bad:
        print("Licenças fora da lista permitida:")
        for name, lic in sorted(set(bad)):
            print(f"  {name}: {lic}")
        return 1
    print(f"OK: {len(used) - len(root)} dependências com licenças permitidas ou autorizadas.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
