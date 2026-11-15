//! Testes de integração do host isolado de plug-ins (`--plugin-host`).

use neoveg_engine::model::OfxFxRef;
use neoveg_engine::plugin_host::{ofx_params, ofx_render, probe_ofx_isolated, shutdown_workers};
use neoveg_engine::plugins::{scan_default, ExternalKind};
use std::path::Path;
use std::time::Duration;

/// Caminho completo: evento de cor sólida com o FX OFX `S_Invert` renderizado
/// por `render_frame` (prévia e render usam o mesmo caminho).
#[test]
fn project_render_applies_ofx_event_fx() {
    use neoveg_engine::model::{Event, FxInstance, GenSpec, Project, Ticks, TrackKind};
    let Some(bin) = scan_default()
        .into_iter()
        .find(|p| p.kind == ExternalKind::Ofx && p.name == "Sapphire")
        .and_then(|p| p.binary)
    else {
        eprintln!("Sapphire não instalado; teste pulado");
        return;
    };
    // o render chama o host pelo executável indicado nesta variável
    std::env::set_var("NEOVEG_PLUGIN_HOST", host());
    let imp = neoveg_engine::media::import_generated(GenSpec::SolidColor {
        color: [0.2, 0.4, 0.6, 1.0],
    });
    let cache = neoveg_engine::media::MediaCache::default();
    cache.insert(imp.item.id, imp.entry);
    let mut p = Project::new_default();
    let mut ev = Event::new(&imp.item, Ticks::ZERO, Ticks::from_secs_i64(1));
    ev.fx.push(FxInstance::new_ofx(
        &bin.display().to_string(),
        "com.genarts.sapphire.Adjust.S_Invert",
        "S_Invert",
    ));
    p.media.push(imp.item);
    let v = p
        .tracks
        .iter_mut()
        .find(|t| t.kind == TrackKind::Video)
        .expect("trilha de vídeo");
    v.events.push(ev);
    let f = neoveg_engine::engine::video::render_frame(&p, &cache, Ticks::ZERO, 32, 18);
    shutdown_workers();
    let c = f.px[9 * 32 + 16];
    eprintln!("pixel central após S_Invert: {c:?}");
    assert!(
        (c[0] - 0.8).abs() < 0.06 && (c[2] - 0.4).abs() < 0.06,
        "{c:?}"
    );
}

/// Aplica um efeito de inversão instalado (Sapphire ou BCC) a um quadro sólido
/// pelo host isolado e confere a inversão. Pula quando nenhum está instalado.
#[test]
fn host_renders_installed_ofx_invert() {
    let candidates = [
        ("Sapphire", "com.genarts.sapphire.Adjust.S_Invert"),
        ("BCC13_OFX", "com.borisfx.BCC3Invert_Solarize"),
    ];
    let installed = scan_default();
    let mut tried = 0;
    let mut passed = vec![];
    for (bundle, id) in candidates {
        let Some(bin) = installed
            .iter()
            .find(|p| p.kind == ExternalKind::Ofx && p.name == bundle)
            .and_then(|p| p.binary.clone())
        else {
            continue;
        };
        tried += 1;
        let r = OfxFxRef {
            binary: bin.display().to_string(),
            plugin_id: id.into(),
            label: id.into(),
            params: vec![],
        };
        match ofx_params(host(), &r) {
            Ok(p) => eprintln!("{id}: {} parâmetro(s)", p.len()),
            Err(e) => {
                eprintln!("{id}: não abriu: {e}");
                continue;
            }
        }
        let (w, h) = (64u32, 36u32);
        let src = [0.2f32, 0.4, 0.6, 1.0];
        let mut px: Vec<f32> = (0..w * h).flat_map(|_| src).collect();
        match ofx_render(host(), &r, w, h, 0.0, 30.0, &mut px) {
            Ok(()) => {
                let inverted = px
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .filter(|p| (p[0] - 0.8).abs() < 0.06 && (p[2] - 0.4).abs() < 0.06)
                    .count();
                eprintln!(
                    "{id}: pixel central {:?}; {inverted}/{} invertidos",
                    &px[((h / 2 * w + w / 2) * 4) as usize..][..4],
                    w * h
                );
                if inverted * 2 > (w * h) as usize {
                    passed.push(id);
                }
            }
            Err(e) => eprintln!("{id}: render falhou: {e}"),
        }
    }
    shutdown_workers();
    if tried == 0 {
        eprintln!("nenhum efeito de inversão OFX instalado; teste pulado");
        return;
    }
    assert!(
        !passed.is_empty(),
        "nenhum efeito OFX instalado inverteu o quadro"
    );
}

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
