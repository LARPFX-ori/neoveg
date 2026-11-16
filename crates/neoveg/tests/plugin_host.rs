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

// ---------------------------------------------------------------------------
// VST3
// ---------------------------------------------------------------------------

/// Módulo VST3 instalado com este nome, se houver (os testes com plug-in real
/// pulam sem ele).
fn installed_vst3(name: &str) -> Option<std::path::PathBuf> {
    scan_default()
        .into_iter()
        .find(|p| p.kind == ExternalKind::Vst3 && p.name.eq_ignore_ascii_case(name))
        .and_then(|p| p.binary)
}

fn vst3_ref(bin: &Path, class_id: &str, label: &str) -> neoveg_engine::model::Vst3FxRef {
    neoveg_engine::model::Vst3FxRef {
        binary: bin.display().to_string(),
        class_id: class_id.into(),
        label: label.into(),
        params: vec![],
    }
}

#[test]
fn vst3_host_rejects_bad_modules_and_keeps_the_block_dry() {
    use neoveg_engine::plugin_host::{new_vst3_owner, probe_vst3_isolated, vst3_process};
    let r = probe_vst3_isolated(
        host(),
        Path::new("nao-existe.vst3"),
        Duration::from_secs(20),
    );
    assert!(r.is_err());
    #[cfg(windows)]
    {
        let e = probe_vst3_isolated(host(), Path::new("kernel32.dll"), Duration::from_secs(20))
            .expect_err("kernel32 não é VST3");
        assert!(e.contains("VST3"), "{e}");
    }
    // efeito apontando para um módulo inexistente: erro, bloco intacto
    let fx = vst3_ref(Path::new("nao-existe.vst3"), &"0".repeat(32), "fantasma");
    let mut buf = vec![0.25f32; 512];
    let res = vst3_process(host(), new_vst3_owner(), 1, &fx, 48_000, &mut buf, true);
    assert!(res.is_err(), "{res:?}");
    assert!(buf.iter().all(|s| *s == 0.25));
    // classe com id hostil
    let fx = vst3_ref(Path::new("x.vst3"), "não-hex", "ruim");
    let res = vst3_process(host(), new_vst3_owner(), 2, &fx, 48_000, &mut buf, true);
    assert!(res.is_err(), "{res:?}");
    assert!(buf.iter().all(|s| *s == 0.25));
}

/// Caminho completo com um plug-in real: sondagem, parâmetros, texto de
/// valor, processamento síncrono e assíncrono (a primeira chamada sem
/// espera só dispara a abertura) e reset.
#[test]
fn vst3_generator_processes_audio_in_isolated_host() {
    use neoveg_engine::plugin_host::{
        new_vst3_owner, probe_vst3_isolated, vst3_param_text, vst3_params, vst3_process,
        vst3_release_owner, vst3_reset_owner, Vst3Skip,
    };
    let Some(bin) = installed_vst3("MOscillator") else {
        eprintln!("MOscillator (MeldaProduction) não instalado; teste pulado");
        return;
    };
    let classes =
        probe_vst3_isolated(host(), &bin, Duration::from_secs(60)).expect("sondagem VST3");
    let class = classes.first().expect("ao menos uma classe de efeito");
    assert!(!class.is_instrument());
    let fx = vst3_ref(&bin, &class.class_id, &class.name);

    let params = vst3_params(host(), &fx).expect("parâmetros");
    eprintln!("{}: {} parâmetro(s)", class.name, params.len());
    assert!(!params.is_empty());
    assert!(params.iter().all(|p| (0.0..=1.0).contains(&p.default)));
    let p0 = &params[0];
    let txt = vst3_param_text(host(), &fx, p0.id, p0.current).expect("texto do valor");
    eprintln!("  {} = {txt:?} {}", p0.title, p0.units);

    // síncrono: o oscilador gera sinal a partir de silêncio
    let owner = new_vst3_owner();
    let mut buf = vec![0f32; 2048 * 2];
    vst3_process(host(), owner, 7, &fx, 48_000, &mut buf, true)
        .expect("processa")
        .expect("sem pulo no modo síncrono");
    let peak = buf.iter().fold(0f32, |m, s| m.max(s.abs()));
    eprintln!("  pico do oscilador: {peak}");
    assert!(buf.iter().all(|s| s.is_finite()));
    assert!(peak > 1e-4, "o oscilador não gerou sinal");
    vst3_reset_owner(owner);
    vst3_release_owner(owner);

    // assíncrono: primeiro bloco seco (abrindo), depois processado
    let owner = new_vst3_owner();
    let mut dry = vec![0f32; 256 * 2];
    let first = vst3_process(host(), owner, 7, &fx, 48_000, &mut dry, false).expect("dispara");
    assert_eq!(first, Err(Vst3Skip::Loading));
    assert!(dry.iter().all(|s| *s == 0.0));
    let start = std::time::Instant::now();
    loop {
        let mut b = vec![0f32; 256 * 2];
        match vst3_process(host(), owner, 7, &fx, 48_000, &mut b, false).expect("processa") {
            Ok(()) => {
                assert!(b.iter().any(|s| *s != 0.0));
                break;
            }
            Err(Vst3Skip::Loading) if start.elapsed() < Duration::from_secs(60) => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => panic!("plug-in não abriu: {e:?}"),
        }
    }
    vst3_release_owner(owner);
    shutdown_workers();
}

/// O FX VST3 no Master entra na mixagem offline (render) de um projeto.
#[test]
fn vst3_master_fx_reaches_offline_mix() {
    use neoveg_engine::engine::audio::Mixer;
    use neoveg_engine::model::{FxInstance, Project};
    let Some(bin) = installed_vst3("MOscillator") else {
        eprintln!("MOscillator (MeldaProduction) não instalado; teste pulado");
        return;
    };
    let classes =
        neoveg_engine::plugin_host::probe_vst3_isolated(host(), &bin, Duration::from_secs(60))
            .expect("sondagem VST3");
    let class = classes.first().expect("classe");
    // o mixer chama o host pelo executável indicado nesta variável
    std::env::set_var("NEOVEG_PLUGIN_HOST", host());
    let mut p = Project::new_default();
    p.master.fx.push(FxInstance::new_vst3(
        &bin.display().to_string(),
        &class.class_id,
        &class.name,
    ));
    let audio = std::collections::HashMap::new();
    let mut out = vec![0f32; 1024 * 2];
    {
        let mut mixer = Mixer::new_offline(48_000);
        mixer.process(&p, &audio, 0, &mut out);
    } // o drop do mixer encerra o host dele
    assert!(out.iter().all(|s| s.is_finite()));
    assert!(
        out.iter().any(|s| s.abs() > 1e-4),
        "o FX VST3 do Master não entrou na mixagem"
    );
    shutdown_workers();
}

/// Um efeito de verdade recebe a entrada: senoide entra, sinal sai, e um
/// parâmetro alterado chega ao plug-in sem derrubar o host.
#[test]
fn vst3_effect_processes_its_input() {
    use neoveg_engine::plugin_host::{
        new_vst3_owner, probe_vst3_isolated, vst3_params, vst3_process, vst3_release_owner,
    };
    let Some(bin) = installed_vst3("MCompressor") else {
        eprintln!("MCompressor (MeldaProduction) não instalado; teste pulado");
        return;
    };
    let classes =
        probe_vst3_isolated(host(), &bin, Duration::from_secs(60)).expect("sondagem VST3");
    let class = classes.first().expect("classe");
    let mut fx = vst3_ref(&bin, &class.class_id, &class.name);
    let params = vst3_params(host(), &fx).expect("parâmetros");
    let owner = new_vst3_owner();
    let sine = |n: usize| -> Vec<f32> {
        (0..n)
            .flat_map(|i| {
                let s = 0.5 * (i as f32 * 2.0 * std::f32::consts::PI * 440.0 / 48_000.0).sin();
                [s, s]
            })
            .collect()
    };
    // alguns blocos para passar pela latência do plug-in
    let mut energy = 0f32;
    for _ in 0..8 {
        let mut buf = sine(1024);
        vst3_process(host(), owner, 9, &fx, 48_000, &mut buf, true)
            .expect("processa")
            .expect("sem pulo");
        assert!(buf.iter().all(|s| s.is_finite()));
        energy += buf.iter().map(|s| s * s).sum::<f32>();
    }
    assert!(
        energy > 1.0,
        "o efeito não devolveu a entrada processada ({energy})"
    );
    // muda o primeiro parâmetro contínuo para o extremo e segue processando
    if let Some(p) = params.iter().find(|p| p.step_count == 0 && !p.bypass) {
        fx.params.push(neoveg_engine::model::Vst3ParamValue {
            id: p.id,
            value: 1.0,
        });
        let mut buf = sine(1024);
        vst3_process(host(), owner, 9, &fx, 48_000, &mut buf, true)
            .expect("processa com parâmetro")
            .expect("sem pulo");
        assert!(buf.iter().all(|s| s.is_finite()));
    }
    vst3_release_owner(owner);
    shutdown_workers();
}
