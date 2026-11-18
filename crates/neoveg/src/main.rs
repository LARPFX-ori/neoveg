//! Neoveg - editor de vídeo não-linear em Rust puro com semântica VEGAS Pro.
//!
//! Uso na linha de comando:
//!   neoveg [--open] [projeto.neoveg | mídia]  abre a interface gráfica
//!   neoveg --render projeto.neoveg saida.ext  renderiza sem interface (MP4, MOV, 3GP, WMV, AVI, GIF, Y4M, sequências PNG/JPG/TIF/TGA/BMP/WebP/DPX/EXR/PNM/QOI ou áudio WAV/BWF/RF64/W64/AIFF/AU/CAF/QOA/M4A/WMA/MP3)
//!   neoveg --run-script script.cs             tenta executar um script C# (.NET Host)
//!   neoveg --command <id> [projeto [saida]]   executa comando da Tabela Única headless
//!   neoveg --list-commands                    lista todos os comandos e atalhos

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use neoveg_common::{set_active_language, tr, tr_args, Language};
use neoveg_engine::{media, model, prefs, recovery, render, util};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, PartialEq, Eq)]
enum Cli {
    Help,
    ListCommands,
    Gui(Option<PathBuf>),
    Render {
        proj: PathBuf,
        out: PathBuf,
    },
    Encode {
        src: PathBuf,
        out: PathBuf,
        preset_id: Option<String>,
    },
    RunScript(PathBuf),
    RunCommand {
        id: String,
        proj: Option<PathBuf>,
        out: Option<PathBuf>,
    },
    ProbeIntegrations,
}

fn help_text(lang: Language) -> &'static str {
    tr(lang, HELP)
}

#[allow(dead_code)]
fn parse_args(args: &[String]) -> Result<Cli, String> {
    parse_args_in(args, Language::PtBr).map(|(_, cli)| cli)
}

fn parse_args_in(args: &[String], fallback: Language) -> Result<(Option<Language>, Cli), String> {
    let raw: Vec<&str> = args.iter().skip(1).map(String::as_str).collect();
    if raw.len() > 10 {
        return Err(tr(fallback, "argumentos demais na linha de comando").into());
    }
    let mut lang_override: Option<Language> = None;
    let mut filtered: Vec<&str> = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        let arg = raw[i];
        if arg == "--lang" {
            let Some(&code) = raw.get(i + 1) else {
                let l = lang_override.unwrap_or(fallback);
                return Err(tr_args(
                    l,
                    "uso inválido: {msg}\n{help}",
                    &[("msg", &"--lang"), ("help", &help_text(l))],
                ));
            };
            let Some(parsed) = Language::from_code(code) else {
                let l = lang_override.unwrap_or(fallback);
                return Err(tr_args(
                    l,
                    "idioma desconhecido '{code}'; use: pt-BR, en, es, fr, de, ja",
                    &[("code", &code)],
                ));
            };
            lang_override = Some(parsed);
            i += 2;
            continue;
        }
        if let Some(code) = arg.strip_prefix("--lang=") {
            let Some(parsed) = Language::from_code(code) else {
                let l = lang_override.unwrap_or(fallback);
                return Err(tr_args(
                    l,
                    "idioma desconhecido '{code}'; use: pt-BR, en, es, fr, de, ja",
                    &[("code", &code)],
                ));
            };
            lang_override = Some(parsed);
            i += 1;
            continue;
        }
        filtered.push(arg);
        i += 1;
    }

    let err_lang = lang_override.unwrap_or(fallback);
    if filtered.len() > 8 {
        return Err(tr(err_lang, "argumentos demais na linha de comando").into());
    }
    let cmd = match filtered.as_slice() {
        [] => Cli::Gui(None),
        ["-h"] | ["--help"] | ["/?"] => Cli::Help,
        ["--list-commands"] => Cli::ListCommands,
        ["--probe-integrations"] => Cli::ProbeIntegrations,
        ["--open", p] | [p] if !p.starts_with('-') => Cli::Gui(Some(PathBuf::from(p))),
        ["--render", proj, out] => Cli::Render {
            proj: PathBuf::from(proj),
            out: PathBuf::from(out),
        },
        ["--encode", src, out] => Cli::Encode {
            src: PathBuf::from(src),
            out: PathBuf::from(out),
            preset_id: None,
        },
        ["--encode", src, out, preset] => Cli::Encode {
            src: PathBuf::from(src),
            out: PathBuf::from(out),
            preset_id: Some((*preset).to_string()),
        },
        ["--run-script", s] => Cli::RunScript(PathBuf::from(s)),
        ["--command", id] => Cli::RunCommand {
            id: (*id).to_string(),
            proj: None,
            out: None,
        },
        ["--command", id, proj] => Cli::RunCommand {
            id: (*id).to_string(),
            proj: Some(PathBuf::from(proj)),
            out: None,
        },
        ["--command", id, proj, out] => Cli::RunCommand {
            id: (*id).to_string(),
            proj: Some(PathBuf::from(proj)),
            out: Some(PathBuf::from(out)),
        },
        _ => {
            let joined = filtered.join(" ");
            return Err(tr_args(
                err_lang,
                "uso inválido: {msg}\n{help}",
                &[("msg", &joined), ("help", &help_text(err_lang))],
            ));
        }
    };
    Ok((lang_override, cmd))
}

fn render_cli(
    proj_path: &std::path::Path,
    out_path: &std::path::Path,
    lang: Language,
) -> anyhow::Result<()> {
    let (p, fixes) = model::Project::load_file(proj_path).map_err(anyhow::Error::msg)?;
    for f in &fixes {
        util::log_warn(&format!("projeto corrigido ao renderizar: {f}"));
    }
    let media = Arc::new(media::MediaCache::default());
    for m in &p.media {
        let mut m = m.clone();
        if let Ok(imp) = media::load_entry(&mut m) {
            media.insert(m.id, imp.entry);
        }
    }
    let fmt = render::RenderFormat::from_path(out_path);
    let s = render::RenderSettings {
        format: fmt,
        path: out_path.to_path_buf(),
        width: p.settings.width,
        height: p.settings.height,
        start: model::Ticks::ZERO,
        end: p.duration(),
        ..render::RenderSettings::default()
    };
    let prog = render::RenderProgress::default();
    render::run(&p, &media, &s, &prog)?;
    let out_disp = out_path.display().to_string();
    eprintln!(
        "{}",
        tr_args(lang, "Renderizado: {path}", &[("path", &out_disp)])
    );
    Ok(())
}

fn encode_cli(
    src_path: &std::path::Path,
    out_path: &std::path::Path,
    preset_id: Option<&str>,
    lang: Language,
) -> anyhow::Result<()> {
    use neoveg_engine::encoder::presets::PresetCatalog;
    use neoveg_engine::encoder::worker::encode_item_sync;
    use neoveg_engine::encoder::{EncoderState, JobStatus};

    let cat = PresetCatalog::default();
    let mut state = EncoderState::default();
    let id = if src_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("neoveg") || e.eq_ignore_ascii_case("nvp"))
        .unwrap_or(false)
    {
        let (p, _) = model::Project::load_file(src_path).map_err(anyhow::Error::msg)?;
        state.add_project_snapshot(
            src_path.display().to_string(),
            Some(src_path.to_path_buf()),
            p,
            None,
            &cat,
        )
    } else {
        state.add_media_file(src_path.to_path_buf(), &cat)?
    };
    if let Some(pid) = preset_id {
        if state.apply_preset_or_group(id, &cat, pid, false) == 0 {
            anyhow::bail!(
                "{}",
                tr_args(
                    lang,
                    "predefinição ou grupo desconhecido: {pid}",
                    &[("pid", &pid)],
                )
            );
        }
    }
    if let Some(item) = state.items.iter_mut().find(|i| i.id == id) {
        for (idx, out) in item.outputs.iter_mut().enumerate() {
            if idx == 0 && preset_id.is_none() {
                let fmt = render::RenderFormat::from_path(out_path);
                out.format = fmt;
                out.output_path = out_path.to_path_buf();
            } else if idx == 0 {
                out.output_path = out_path.to_path_buf();
            } else {
                let dir = out_path
                    .parent()
                    .filter(|d| !d.as_os_str().is_empty())
                    .unwrap_or_else(|| std::path::Path::new("."));
                let stem = out_path
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| "saida".into());
                out.output_path = dir.join(format!("{stem}_{idx}.{}", out.format.ext()));
            }
        }
        let results = encode_item_sync(item, &state.prefs, None);
        for r in &results {
            if !matches!(r.status, JobStatus::Done | JobStatus::DoneWithWarnings) {
                let st = r.status.name_in(lang);
                let msg = r
                    .message
                    .as_deref()
                    .unwrap_or_else(|| tr(lang, "erro desconhecido"));
                anyhow::bail!(
                    "{}",
                    tr_args(
                        lang,
                        "falha no Neoveg Encoder ({status}): {msg}",
                        &[("status", &st), ("msg", &msg)],
                    )
                );
            }
            if let Some(p) = &r.final_output_path {
                let path_s = p.display().to_string();
                eprintln!(
                    "{}",
                    tr_args(lang, "Codificado: {path}", &[("path", &path_s)])
                );
            }
        }
    }
    Ok(())
}

const HELP: &str = "Uso:\n  neoveg [--lang <código>] [--open] [projeto.neoveg | mídia]  abre a interface gráfica\n  neoveg [--lang <código>] --render projeto.neoveg saida.ext  renderiza sem interface\n  neoveg [--lang <código>] --encode entrada saida [predefinicao]  transcodifica via Neoveg Encoder\n  neoveg [--lang <código>] --run-script script.<cs|dll>       executa script C#/.NET (ScriptPortal.Vegas) via netcorehost\n  neoveg [--lang <código>] --command <id> [projeto [saida]]   executa comando da Tabela Única sem interface\n  neoveg [--lang <código>] --list-commands                    lista os comandos e atalhos disponíveis\n  neoveg [--lang <código>] --probe-integrations               lista aplicativos ArtCraft/Blender e portas RPC em JSON";

/// Imprime o resultado de uma sondagem em JSON e devolve o código de saída.
fn print_probe<T: serde::Serialize>(res: Result<T, String>) -> i32 {
    match res.and_then(|list| serde_json::to_string(&list).map_err(|e| e.to_string())) {
        Ok(json) => {
            println!("{json}");
            0
        }
        Err(e) => {
            eprintln!("{e}");
            3
        }
    }
}

/// Modo host de plug-ins (`neoveg --plugin-host probe-ofx <binário>`): roda
/// num subprocesso descartável, carrega o binário de terceiros e responde em
/// JSON no stdout. Fica antes de preferências e logs para não tocar no estado
/// do usuário.
fn plugin_host_mode(args: &[String]) -> Option<i32> {
    use neoveg_engine::plugin_host as ph;
    let rest: Vec<&str> = args.iter().skip(1).map(String::as_str).collect();
    match rest.as_slice() {
        ["--plugin-host", "probe-ofx", bin] => Some(print_probe(ph::probe_ofx_in_process(
            std::path::Path::new(bin),
        ))),
        ["--plugin-host", "serve-ofx", bin, id] => {
            Some(ph::serve_ofx(std::path::Path::new(bin), id))
        }
        ["--plugin-host", "probe-vst3", bin] => Some(print_probe(ph::probe_vst3_in_process(
            std::path::Path::new(bin),
        ))),
        ["--plugin-host", "serve-vst3", bin, class_id, sr] => {
            Some(ph::serve_vst3(std::path::Path::new(bin), class_id, sr))
        }
        ["--plugin-host", ..] => {
            eprintln!("modo de host de plug-ins desconhecido");
            Some(2)
        }
        _ => None,
    }
}

fn main() -> anyhow::Result<()> {
    let raw_args: Vec<String> = std::env::args().collect();
    if let Some(code) = plugin_host_mode(&raw_args) {
        std::process::exit(code);
    }
    recovery::install_panic_hook();
    let boot_prefs = prefs::Preferences::load_or_default();
    let _ = rayon::ThreadPoolBuilder::new()
        .num_threads(boot_prefs.effective_worker_threads())
        .build_global();
    util::log_info(&format!(
        "Neoveg iniciado (workers={})",
        boot_prefs.effective_worker_threads()
    ));

    let args: Vec<String> = std::env::args().collect();
    let env_lang = std::env::var("NEOVEG_LANG")
        .ok()
        .and_then(|v| Language::from_code(&v));
    let fallback_lang = env_lang.unwrap_or_else(|| boot_prefs.lang());
    let (cli_lang, cli_cmd) = match parse_args_in(&args, fallback_lang) {
        Ok(parsed) => parsed,
        Err(msg) => {
            eprintln!("{msg}");
            std::process::exit(1);
        }
    };
    let explicit_lang = cli_lang.or(env_lang);
    let effective_lang = explicit_lang.unwrap_or_else(|| boot_prefs.lang());
    set_active_language(effective_lang);

    let open = match cli_cmd {
        Cli::Help => {
            println!("{}", help_text(effective_lang));
            return Ok(());
        }
        Cli::ListCommands => {
            for (id, menu, label, sc) in
                neoveg_scripting::list_commands_in(&boot_prefs.shortcuts, effective_lang)
            {
                println!(
                    "{:<28} {:<36} {} ({})",
                    id,
                    menu,
                    label,
                    sc.unwrap_or_else(|| "-".into())
                );
            }
            return Ok(());
        }
        Cli::Render { proj, out } => {
            if let Err(e) = render_cli(&proj, &out, effective_lang) {
                let err_s = format!("{e:#}");
                eprintln!(
                    "{}",
                    tr_args(
                        effective_lang,
                        "erro ao renderizar: {err}",
                        &[("err", &err_s)]
                    )
                );
                std::process::exit(1);
            }
            return Ok(());
        }
        Cli::Encode {
            src,
            out,
            preset_id,
        } => {
            if let Err(e) = encode_cli(&src, &out, preset_id.as_deref(), effective_lang) {
                let err_s = format!("{e:#}");
                eprintln!(
                    "{}",
                    tr_args(
                        effective_lang,
                        "erro no Neoveg Encoder: {err}",
                        &[("err", &err_s)],
                    )
                );
                std::process::exit(1);
            }
            return Ok(());
        }
        Cli::RunScript(s) => {
            let mut ctx = neoveg_scripting::ScriptContext::new_in(
                model::Project::new_default(),
                effective_lang,
            );
            if let Err(e) = neoveg_scripting::run_cs_script(&s, &mut ctx) {
                eprintln!("Neoveg: {e}");
                std::process::exit(2);
            }
            for msg in &ctx.messages {
                println!("{msg}");
            }
            let path_s = s.display().to_string();
            let trk_n = ctx.session.project.tracks.len().to_string();
            let mrk_n = ctx.session.project.markers.len().to_string();
            let bus_n = ctx.session.project.audio_buses.len().to_string();
            println!(
                "{}",
                tr_args(
                    effective_lang,
                    "Script executado ({path}): trilhas={tracks}, marcadores={markers}, barramentos={buses}",
                    &[
                        ("path", &path_s),
                        ("tracks", &trk_n),
                        ("markers", &mrk_n),
                        ("buses", &bus_n),
                    ]
                )
            );
            return Ok(());
        }
        Cli::RunCommand { id, proj, out } => {
            match neoveg_scripting::run_command_headless_in(
                &id,
                proj.as_deref(),
                out.as_deref(),
                effective_lang,
            ) {
                Ok(msg) => {
                    println!("{msg}");
                    return Ok(());
                }
                Err(e) => {
                    eprintln!("Neoveg: {e}");
                    std::process::exit(1);
                }
            }
        }
        Cli::ProbeIntegrations => {
            let p = prefs::Preferences::load_or_default();
            let rep = neoveg_engine::integrations::discovery::discover_all(&p.external_apps, true);
            std::process::exit(print_probe(Ok(rep)));
        }
        Cli::Gui(p) => p,
    };

    // marcador de sessão: só no modo de interface (CLI, scripts e hosts de
    // plug-in não criam); detecta sessões anteriores que não fecharam
    // normalmente e mostra o relatório na janela
    let crash_reports = neoveg_engine::session_marker::begin(env!("CARGO_PKG_VERSION"));
    let result = neoveg_ui::run_gui_in(open, crash_reports, explicit_lang)
        .map_err(|e| anyhow::anyhow!("{e}"));
    // saída normal da janela; um pânico na thread principal não chega aqui
    neoveg_engine::session_marker::end();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| (*x).to_string()).collect()
    }

    #[test]
    fn argumentos_hostis_nao_entram_em_panico() {
        assert_eq!(parse_args(&a(&["neoveg"])), Ok(Cli::Gui(None)));
        assert_eq!(
            parse_args(&a(&["neoveg", "--open", "x.neoveg"])),
            Ok(Cli::Gui(Some(PathBuf::from("x.neoveg"))))
        );
        assert_eq!(
            parse_args(&a(&["neoveg", "--render", "a", "b"])),
            Ok(Cli::Render {
                proj: PathBuf::from("a"),
                out: PathBuf::from("b"),
            })
        );
        assert_eq!(
            parse_args(&a(&["neoveg", "--encode", "in.mp4", "out.avi"])),
            Ok(Cli::Encode {
                src: PathBuf::from("in.mp4"),
                out: PathBuf::from("out.avi"),
                preset_id: None,
            })
        );
        assert_eq!(
            parse_args(&a(&[
                "neoveg",
                "--encode",
                "a.neoveg",
                "b.mp4",
                "sys.web_1080p_hq"
            ])),
            Ok(Cli::Encode {
                src: PathBuf::from("a.neoveg"),
                out: PathBuf::from("b.mp4"),
                preset_id: Some("sys.web_1080p_hq".into()),
            })
        );
        assert_eq!(
            parse_args(&a(&["neoveg", "--run-script", "s.cs"])),
            Ok(Cli::RunScript(PathBuf::from("s.cs")))
        );
        assert_eq!(
            parse_args(&a(&["neoveg", "--command", "insert.marker"])),
            Ok(Cli::RunCommand {
                id: "insert.marker".into(),
                proj: None,
                out: None,
            })
        );
        assert_eq!(
            parse_args(&a(&["neoveg", "--list-commands"])),
            Ok(Cli::ListCommands)
        );
        assert_eq!(
            parse_args(&a(&["neoveg", "--probe-integrations"])),
            Ok(Cli::ProbeIntegrations)
        );
        assert!(parse_args(&a(&["neoveg", "--render"])).is_err());
        assert!(parse_args(&a(&["neoveg", "--xyz", "1", "2", "3", "4"])).is_err());
    }

    #[test]
    fn cli_lang_flag_parses_all_supported_languages_and_rejects_invalid() {
        assert_eq!(
            parse_args_in(&a(&["neoveg", "--lang", "en"]), Language::PtBr),
            Ok((Some(Language::En), Cli::Gui(None)))
        );
        assert_eq!(
            parse_args_in(
                &a(&["neoveg", "--lang=ja", "--list-commands"]),
                Language::PtBr
            ),
            Ok((Some(Language::Ja), Cli::ListCommands))
        );
        assert_eq!(
            parse_args_in(
                &a(&["neoveg", "--lang", "de", "--render", "a.neoveg", "out.avi"]),
                Language::PtBr
            ),
            Ok((
                Some(Language::De),
                Cli::Render {
                    proj: PathBuf::from("a.neoveg"),
                    out: PathBuf::from("out.avi"),
                }
            ))
        );
        let bad = parse_args_in(&a(&["neoveg", "--lang", "xx"]), Language::PtBr);
        assert!(bad.is_err());
        let missing = parse_args_in(&a(&["neoveg", "--lang"]), Language::En);
        assert!(missing.is_err());

        for (lang, expected) in [
            (Language::PtBr, "Uso:"),
            (Language::En, "Usage:"),
            (Language::Es, "renderiza sin interfaz"),
            (Language::Fr, "Utilisation :"),
            (Language::De, "Verwendung:"),
            (Language::Ja, "使い方:"),
        ] {
            let h = help_text(lang);
            assert!(
                h.contains(expected),
                "HELP de {lang:?} deveria conter '{expected}', obteve: {h}"
            );
            let cmds = neoveg_scripting::list_commands_in(&Default::default(), lang);
            assert!(!cmds.is_empty());
        }
    }
}
