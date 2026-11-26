# escape=`
#
# Imagem Windows para rodar os portões de qualidade (AGENTS.md §3.4) e gerar o
# pacote portátil Windows num contêiner. É o mesmo ambiente do job
# "Portões (Windows)" da CI (.github/workflows/ci.yml): MSVC (Build Tools),
# Rust stable, Python 3.12, ffmpeg e pyaaf2 só como geradores de fixtures
# (§2.3: oráculos externos, nunca linkados nem empacotados).
#
# Por quê: deixa compilar, testar e empacotar a versão Windows numa máquina
# onde o build local não funciona (ex.: antivírus que bloqueia os build
# scripts do cargo). O código-fonte não entra na imagem: ele é montado na hora
# por tools/docker_test.py --windows.
#
# Requisitos: Docker Desktop no modo "Windows containers" e os recursos
# Containers e Hyper-V do Windows ativos. ltsc2019 porque um host Windows 10
# só roda, com isolamento Hyper-V, imagens de versão igual ou anterior à dele.
#
# Construir (o contexto é só esta pasta):
#   docker build -f docker/test-windows.Dockerfile -t neoveg-test-windows docker

FROM mcr.microsoft.com/windows/servercore:ltsc2019

SHELL ["powershell", "-NoLogo", "-NoProfile", "-Command", "$ErrorActionPreference = 'Stop'; $ProgressPreference = 'SilentlyContinue';"]

# Certificados raiz extras da rede (antivírus/proxy que inspeciona HTTPS); ver
# docker/certs/README.md. Sem nenhum .crt na pasta, não muda nada.
COPY certs/ C:/certs/
RUN Get-ChildItem C:\certs -Filter *.crt | ForEach-Object { `
        Import-Certificate -FilePath $_.FullName -CertStoreLocation Cert:\LocalMachine\Root | Out-Null }; `
    Remove-Item -Recurse C:\certs

# MSVC: só a carga de trabalho de ferramentas C++ (linker e SDK do Windows).
# 3010 = instalado, pede reinício (não se aplica a contêiner).
RUN Invoke-WebRequest https://aka.ms/vs/17/release/vs_buildtools.exe -OutFile C:\vs_buildtools.exe; `
    $p = Start-Process C:\vs_buildtools.exe -Wait -PassThru -ArgumentList `
        '--quiet', '--wait', '--norestart', '--nocache', `
        '--add', 'Microsoft.VisualStudio.Workload.VCTools', '--includeRecommended'; `
    if ($p.ExitCode -ne 0 -and $p.ExitCode -ne 3010) { throw \"vs_buildtools: $($p.ExitCode)\" }; `
    Remove-Item C:\vs_buildtools.exe

# Rust stable, como a CI (dtolnay/rust-toolchain@stable). Reconstrua a imagem
# com --no-cache para pegar uma versão nova do stable.
ENV RUSTUP_HOME=C:\rustup `
    CARGO_HOME=C:\cargo `
    CARGO_TERM_COLOR=always `
    CARGO_INCREMENTAL=0
RUN Invoke-WebRequest https://win.rustup.rs/x86_64 -OutFile C:\rustup-init.exe; `
    & C:\rustup-init.exe -y --no-modify-path --profile minimal `
        --default-toolchain stable --component rustfmt,clippy; `
    if ($LASTEXITCODE -ne 0) { throw \"rustup-init: $LASTEXITCODE\" }; `
    Remove-Item C:\rustup-init.exe

# Python 3.12 (mesma versão da CI) e pyaaf2 fixado (tools/gen_aaf_fixtures.py).
RUN Invoke-WebRequest https://www.python.org/ftp/python/3.12.8/python-3.12.8-amd64.exe -OutFile C:\python.exe; `
    $p = Start-Process C:\python.exe -Wait -PassThru -ArgumentList `
        '/quiet', 'InstallAllUsers=1', 'PrependPath=0', 'Include_test=0', 'TargetDir=C:\Python312'; `
    if ($p.ExitCode -ne 0) { throw \"python: $($p.ExitCode)\" }; `
    Remove-Item C:\python.exe; `
    & C:\Python312\python.exe -m pip install --no-cache-dir pyaaf2==1.7.1

# ffmpeg só como gerador de fixtures, pelo Chocolatey, como a CI.
RUN Set-ExecutionPolicy Bypass -Scope Process -Force; `
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor 3072; `
    Invoke-Expression ((New-Object Net.WebClient).DownloadString('https://community.chocolatey.org/install.ps1')); `
    & C:\ProgramData\chocolatey\bin\choco.exe install ffmpeg -y --no-progress; `
    if ($LASTEXITCODE -ne 0) { throw \"choco ffmpeg: $LASTEXITCODE\" }

RUN setx /M PATH $('C:\cargo\bin;C:\Python312;C:\Python312\Scripts;C:\ProgramData\chocolatey\bin;' + $env:PATH)

WORKDIR C:\src
