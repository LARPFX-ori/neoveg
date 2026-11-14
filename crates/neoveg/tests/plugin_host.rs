//! Testes de integração do host isolado de plug-ins (`--plugin-host`).

use neoveg_engine::plugin_host::probe_ofx_isolated;
use neoveg_engine::plugins::{scan_default, ExternalKind};
use std::path::Path;
use std::time::Duration;

fn host() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_neoveg"))
}

#[test]
fn host_rejects_non_ofx_library_without_crashing() {
    let r = probe_ofx_isolated(
        host(),
        Path::new("arquivo-que-nao-existe.ofx"),
        Duration::from_secs(20),
    );
    assert!(r.is_err());
    #[cfg(windows)]
    {
        let e = probe_ofx_isolated(host(), Path::new("kernel32.dll"), Duration::from_secs(20))
            .expect_err("kernel32 não é OFX");
        assert!(e.contains("OFX"), "{e}");
    }
}

/// Sonda os pacotes OFX instalados nas pastas padrão (código de terceiros,
/// sempre no subprocesso). Pula quando não há nenhum instalado.
#[test]
fn host_lists_installed_ofx_plugins() {
    let bundles: Vec<_> = scan_default()
        .into_iter()
        .filter(|p| p.kind == ExternalKind::Ofx)
        .filter_map(|p| p.binary.map(|b| (p.name, b)))
        .collect();
    if bundles.is_empty() {
        eprintln!("nenhum plug-in OFX instalado; teste pulado");
        return;
    }
    let mut any = false;
    for (name, bin) in bundles {
        match probe_ofx_isolated(host(), &bin, Duration::from_secs(60)) {
            Ok(list) => {
                eprintln!("{name}: {} plug-in(s)", list.len());
                for p in list.iter().take(3) {
                    eprintln!("  {} ({} v{})", p.identifier, p.api, p.api_version);
                    assert!(!p.identifier.is_empty());
                    assert_eq!(p.api, "OfxImageEffectPluginAPI");
                }
                any |= !list.is_empty();
            }
            Err(e) => eprintln!("{name}: erro isolado: {e}"),
        }
    }
    assert!(any, "nenhum pacote OFX instalado respondeu com plug-ins");
}
