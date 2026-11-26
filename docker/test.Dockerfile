# Imagem para rodar os portões de qualidade (AGENTS.md §3.4) em Linux, num
# contêiner. É o mesmo ambiente do job "Portões (Linux)" da CI
# (.github/workflows/ci.yml): Ubuntu 24.04, Rust stable, ALSA/Wayland/xkbcommon,
# Xvfb para os testes de janela, ffmpeg e pyaaf2 só como geradores de fixtures
# (§2.3: oráculos externos, nunca linkados nem empacotados).
#
# Por quê: deixa rodar clippy e cargo test numa máquina onde o build local não
# funciona (ex.: antivírus que bloqueia os build scripts do cargo) ou que não é
# Linux. O código-fonte não entra na imagem: ele é montado na hora por
# tools/docker_test.py. Se mudar as bibliotecas do job Linux da CI, mude aqui
# também.
#
# Construir (o contexto é só esta pasta):
#   docker build -f docker/test.Dockerfile -t neoveg-test docker

FROM ubuntu:24.04

ARG DEBIAN_FRONTEND=noninteractive

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        build-essential ca-certificates curl git pkg-config \
        libasound2-dev libwayland-dev libxkbcommon-dev xvfb xauth \
        ffmpeg python3 python3-venv \
    && rm -rf /var/lib/apt/lists/*

# Certificados raiz extras da rede (antivírus/proxy que inspeciona HTTPS); ver
# docker/certs/README.md. Sem nenhum .crt na pasta, não muda nada.
COPY certs/ /usr/local/share/ca-certificates/extra/
RUN find /usr/local/share/ca-certificates/extra -type f ! -name '*.crt' -delete \
    && update-ca-certificates
ENV SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt \
    PIP_CERT=/etc/ssl/certs/ca-certificates.crt \
    REQUESTS_CA_BUNDLE=/etc/ssl/certs/ca-certificates.crt \
    CARGO_HTTP_CAINFO=/etc/ssl/certs/ca-certificates.crt

ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:/opt/venv/bin:$PATH \
    CARGO_TERM_COLOR=always \
    CARGO_INCREMENTAL=0

# Rust stable, como a CI (dtolnay/rust-toolchain@stable). Reconstrua a imagem
# com --no-cache para pegar uma versão nova do stable.
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
        | sh -s -- -y --no-modify-path --profile minimal \
            --default-toolchain stable --component rustfmt,clippy \
    && rustc --version && cargo --version

# pyaaf2 fixado na mesma versão da CI (tools/gen_aaf_fixtures.py).
RUN python3 -m venv /opt/venv \
    && /opt/venv/bin/pip install --no-cache-dir pyaaf2==1.7.1

WORKDIR /src
