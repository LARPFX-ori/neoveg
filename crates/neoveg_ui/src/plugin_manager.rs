//! Painéis acopláveis *Plug-In Manager* e *Compositors*.
//!
//! O *Plug-In Manager* lista, numa árvore pesquisável, todos os plug-ins que o
//! Neoveg oferece (efeitos de vídeo e áudio, transições e modos de composição),
//! com detalhes dos parâmetros e arrastar-e-soltar para eventos e trilhas. O
//! *Compositors* aplica modos de composição à trilha de vídeo selecionada.

use super::{App, DndPayload};
use crate::icons::{self, Icon};
use crate::model::{BlendMode, FxKind, TrackKind, TransitionKind};
use eframe::egui;
use neoveg_common::{tr, tr_args, Language};
use neoveg_engine::plugin_host::{
    probe_ofx_isolated, probe_vst3_isolated, OfxParamInfo, OfxPluginInfo, Vst3ClassInfo,
    Vst3ParamInfo,
};
use neoveg_engine::plugins::{self, ExternalKind, ExternalPlugin, FolderScan, FolderStatus};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

/// Tempo máximo de uma sondagem de plug-in no subprocesso.
const PROBE_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone)]
enum ProbeState<T> {
    Running,
    Done(Result<Arc<Vec<T>>, String>),
}

type ProbeMap<T> = Arc<Mutex<HashMap<PathBuf, ProbeState<T>>>>;

/// Plug-ins encontrados e o estado de cada pasta mapeada.
type ScanResult = (Vec<ExternalPlugin>, Vec<FolderScan>);

/// Varredura das pastas de plug-ins, feita numa thread de fundo: pastas do
/// Google Drive ou de rede podem levar segundos para listar (`AGENTS.md` §3.6).
#[derive(Clone, Default)]
struct ScanState {
    /// Pastas mapeadas usadas na última varredura (ou na que está rodando).
    folders: Vec<PathBuf>,
    running: bool,
    result: Option<Result<Arc<ScanResult>, String>>,
}

type ScanSlot = Arc<Mutex<ScanState>>;

fn start_scan(slot: &ScanSlot, folders: Vec<PathBuf>, ctx: egui::Context, lang: Language) {
    {
        let mut st = slot.lock().unwrap_or_else(PoisonError::into_inner);
        st.folders = folders.clone();
        st.running = true;
    }
    let worker = slot.clone();
    let failed = tr(lang, "a varredura de plug-ins falhou").to_string();
    let spawned = std::thread::Builder::new()
        .name("plugin-scan".into())
        .spawn(move || {
            // §0.6: uma pasta estranha não pode derrubar a interface
            let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                plugins::scan_with_folders(&folders)
            }))
            .map(Arc::new)
            .map_err(|_| failed);
            let mut st = worker.lock().unwrap_or_else(PoisonError::into_inner);
            st.running = false;
            st.result = Some(res);
            drop(st);
            ctx.request_repaint();
        });
    if spawned.is_err() {
        let mut st = slot.lock().unwrap_or_else(PoisonError::into_inner);
        st.running = false;
        st.result = Some(Err(tr(
            lang,
            "não foi possível iniciar a varredura de plug-ins",
        )
        .into()));
    }
}

/// Plug-ins OFX já consultados no Plug-In Manager (binário e plug-in), para
/// a categoria Terceiros da janela Efeitos de Vídeo. Não consulta nada novo.
pub(crate) fn probed_ofx_plugins(ctx: &egui::Context) -> Vec<(PathBuf, OfxPluginInfo)> {
    let key = egui::Id::new("plugin_manager_ofx_probes");
    let Some(map) = ctx.data(|d| d.get_temp::<ProbeMap<OfxPluginInfo>>(key)) else {
        return Vec::new();
    };
    let map = map.lock().unwrap_or_else(PoisonError::into_inner);
    let mut out: Vec<(PathBuf, OfxPluginInfo)> = map
        .iter()
        .filter_map(|(bin, st)| match st {
            ProbeState::Done(Ok(list)) => Some(
                list.iter()
                    .map(|i| (bin.clone(), i.clone()))
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        })
        .flatten()
        .collect();
    out.sort_by(|a, b| a.1.identifier.cmp(&b.1.identifier));
    out
}

/// Plug-ins VST3 já consultados no Plug-In Manager (binário e classe), para o Plug-In Chooser.
pub(crate) fn probed_vst3_plugins(ctx: &egui::Context) -> Vec<(PathBuf, Vst3ClassInfo)> {
    let key = egui::Id::new("plugin_manager_vst3_probes");
    let Some(map) = ctx.data(|d| d.get_temp::<ProbeMap<Vst3ClassInfo>>(key)) else {
        return Vec::new();
    };
    let map = map.lock().unwrap_or_else(PoisonError::into_inner);
    let mut out: Vec<(PathBuf, Vst3ClassInfo)> = map
        .iter()
        .filter_map(|(bin, st)| match st {
            ProbeState::Done(Ok(list)) => Some(
                list.iter()
                    .map(|i| (bin.clone(), i.clone()))
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        })
        .flatten()
        .collect();
    out.sort_by(|a, b| a.1.name.cmp(&b.1.name));
    out
}
type ProbeFn<T> = fn(&Path, &Path, Duration) -> Result<Vec<T>, String>;

type Vst3ParamsState = Option<Result<Arc<Vec<Vst3ParamInfo>>, String>>;
type Vst3ParamsMap = Arc<Mutex<HashMap<(String, String), Vst3ParamsState>>>;

/// Editor dos parâmetros de um efeito/instrumento VST3 (no diálogo de FX), em
/// valores normalizados, com automação por keyframes em tempo real e botão para
/// abrir a interface gráfica nativa (`IPlugView`) do plug-in.
pub(crate) fn vst3_params_editor(
    ui: &mut egui::Ui,
    lang: Language,
    fx: &mut crate::model::FxInstance,
    theme: &crate::theme::ThemeColors,
    local_t: crate::model::Ticks,
    tol: crate::model::Ticks,
) -> bool {
    let fx_id = fx.id;
    let Some(r) = fx.vst3.as_mut() else {
        return false;
    };
    let cache_key = egui::Id::new("vst3_param_descriptors");
    let map: Vst3ParamsMap = ui.data_mut(|d| {
        d.get_temp_mut_or_default::<Vst3ParamsMap>(cache_key)
            .clone()
    });
    let k = (r.binary.clone(), r.class_id.clone());
    let state = map
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&k)
        .cloned();
    let muted = |t: String| egui::RichText::new(t).small().color(theme.text_muted);
    let infos = match state {
        None => {
            map.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(k.clone(), None);
            let (map2, r2, ctx) = (map.clone(), r.clone(), ui.ctx().clone());
            let missing_exe = tr(lang, "executável do Neoveg não encontrado").to_string();
            let _ = std::thread::Builder::new()
                .name("vst3-params".into())
                .spawn(move || {
                    let res = neoveg_engine::plugin_host::default_host_exe()
                        .ok_or(missing_exe)
                        .and_then(|exe| neoveg_engine::plugin_host::vst3_params(&exe, &r2))
                        .map(Arc::new);
                    map2.lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .insert((r2.binary, r2.class_id), Some(res));
                    ctx.request_repaint();
                });
            ui.label(muted(
                tr(lang, "Abrindo o plug-in em processo isolado…").into(),
            ));
            return false;
        }
        Some(None) => {
            ui.label(muted(
                tr(lang, "Abrindo o plug-in em processo isolado…").into(),
            ));
            return false;
        }
        Some(Some(Err(e))) => {
            ui.label(muted(tr_args(
                lang,
                "Plug-in indisponível: {err}",
                &[("err", &e)],
            )));
            return false;
        }
        Some(Some(Ok(infos))) => infos,
    };
    let filter_key = egui::Id::new(("vst3_param_filter", fx_id));
    let mut filter: String = ui.data(|d| d.get_temp(filter_key)).unwrap_or_default();
    let state_op_key = egui::Id::new(("vst3_state_capture", fx_id));
    type CaptureSlot = Arc<Mutex<Option<Result<String, String>>>>;
    let capture_slot: CaptureSlot = ui.data_mut(|d| {
        d.get_temp_mut_or_default::<CaptureSlot>(state_op_key)
            .clone()
    });
    let gui_op_key = egui::Id::new(("vst3_gui_editor", fx_id));
    type GuiSlot = Arc<Mutex<Option<Result<neoveg_engine::plugin_host::Vst3EditorResult, String>>>>;
    let gui_slot: GuiSlot =
        ui.data_mut(|d| d.get_temp_mut_or_default::<GuiSlot>(gui_op_key).clone());
    let gui_err_key = egui::Id::new(("vst3_gui_err", fx_id));
    let mut gui_err: Option<String> = ui.data(|d| d.get_temp(gui_err_key));
    let mut changed = false;
    {
        let mut g = capture_slot.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(Ok(hex)) = g.take() {
            r.state = Some(hex);
            changed = true;
        }
    }
    {
        let mut g = gui_slot.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(res) = g.take() {
            match res {
                Ok(ed) => {
                    gui_err = None;
                    for p in ed.params {
                        r.set_param(p.id, p.value);
                    }
                    if let Some(st) = ed.state {
                        r.state = Some(st);
                    }
                    changed = true;
                }
                Err(e) => {
                    gui_err = Some(e);
                }
            }
        }
    }
    let count_s = infos.len().to_string();
    ui.horizontal_wrapped(|ui| {
        if r.is_instrument {
            ui.label(
                egui::RichText::new(tr(lang, "Instrumento MIDI"))
                    .small()
                    .strong()
                    .color(theme.accent),
            );
        }
        ui.label(muted(tr_args(lang, "{n} parâmetro(s)", &[("n", &count_s)])));
        ui.add(
            egui::TextEdit::singleline(&mut filter)
                .hint_text(tr(lang, "Filtrar"))
                .desired_width(110.0),
        );
        if ui
            .small_button(tr(lang, "Interface do plug-in..."))
            .on_hover_text(tr(
                lang,
                "Abrir a interface gráfica nativa (IPlugView) do plug-in VST3 em processo isolado",
            ))
            .clicked()
        {
            gui_err = None;
            let r2 = r.clone();
            let title = if r.label.is_empty() {
                "VST3".to_string()
            } else {
                r.label.clone()
            };
            let slot2 = gui_slot.clone();
            let ctx = ui.ctx().clone();
            let missing_exe = tr(lang, "executável do Neoveg não encontrado").to_string();
            let _ = std::thread::Builder::new()
                .name("vst3-gui".into())
                .spawn(move || {
                    let res = neoveg_engine::plugin_host::default_host_exe()
                        .ok_or(missing_exe)
                        .and_then(|exe| {
                            neoveg_engine::plugin_host::vst3_open_editor_isolated(&exe, &r2, &title)
                        });
                    *slot2.lock().unwrap_or_else(PoisonError::into_inner) = Some(res);
                    ctx.request_repaint();
                });
        }
        if ui
            .small_button(tr(lang, "Capturar estado"))
            .on_hover_text(tr(
                lang,
                "Salva o estado interno completo do plug-in VST3 no projeto",
            ))
            .clicked()
        {
            let r2 = r.clone();
            let slot2 = capture_slot.clone();
            let ctx = ui.ctx().clone();
            let _ = std::thread::Builder::new()
                .name("vst3-get-state".into())
                .spawn(move || {
                    let res = neoveg_engine::plugin_host::default_host_exe()
                        .ok_or_else(|| "host de plug-ins não encontrado".to_string())
                        .and_then(|exe| neoveg_engine::plugin_host::vst3_get_state(&exe, &r2));
                    *slot2.lock().unwrap_or_else(PoisonError::into_inner) = Some(res);
                    ctx.request_repaint();
                });
        }
        if let Some(hex) = &r.state {
            ui.label(muted(format!("Estado: {} B", hex.len() / 2)));
            if ui.small_button(tr(lang, "Limpar estado")).clicked() {
                r.state = None;
                changed = true;
            }
        }
    });
    if let Some(err) = &gui_err {
        ui.label(muted(tr_args(
            lang,
            "Interface indisponível: {err}",
            &[("err", err)],
        )));
    }
    ui.data_mut(|d| {
        if let Some(e) = gui_err {
            d.insert_temp(gui_err_key, e);
        } else {
            d.remove_temp::<String>(gui_err_key);
        }
    });
    if infos.is_empty() {
        ui.label(muted(tr(lang, "O plug-in não expõe parâmetros.").into()));
        return changed;
    }
    let f = filter.trim().to_lowercase();
    ui.data_mut(|d| d.insert_temp(filter_key, filter));
    egui::ScrollArea::vertical()
        .id_salt(("vst3_params_scroll", fx_id))
        .max_height(260.0)
        .show(ui, |ui| {
            egui::Grid::new(("vst3_params", fx_id))
                .num_columns(3)
                .striped(true)
                .show(ui, |ui| {
                    for info in infos.iter() {
                        if !f.is_empty() && !info.title.to_lowercase().contains(&f) {
                            continue;
                        }
                        let cur = r
                            .params
                            .iter()
                            .find(|p| p.id == info.id)
                            .map(|p| p.value_at(local_t))
                            .unwrap_or(info.current);
                        let mut v = cur;
                        ui.label(&info.title);
                        let edited = ui
                            .horizontal(|ui| {
                                let e = if info.bypass || info.step_count == 1 {
                                    let mut b = v > 0.5;
                                    let ch = ui.checkbox(&mut b, "").changed();
                                    v = if b { 1.0 } else { 0.0 };
                                    ch
                                } else if info.step_count > 1 {
                                    let steps = f64::from(info.step_count);
                                    let mut k = (v * steps).round().clamp(0.0, steps) as i32;
                                    let ch = ui
                                        .add(egui::Slider::new(&mut k, 0..=info.step_count))
                                        .changed();
                                    v = f64::from(k) / steps;
                                    ch
                                } else {
                                    ui.add(egui::Slider::new(&mut v, 0.0..=1.0).fixed_decimals(3))
                                        .changed()
                                };
                                if !info.units.is_empty() {
                                    ui.label(muted(info.units.clone()));
                                }
                                e
                            })
                            .inner;
                        let mut key_op: Option<bool> = None;
                        let (has_keys, key_here) = r
                            .params
                            .iter()
                            .find(|p| p.id == info.id)
                            .map(|p| {
                                (
                                    p.keys.len(),
                                    p.keys.iter().any(|k| k.0.abs_diff(local_t) <= tol),
                                )
                            })
                            .unwrap_or((0, false));
                        ui.horizontal(|ui| {
                            if icons::small_icon_button(
                                ui,
                                if key_here {
                                    Icon::KeyframeFilled
                                } else {
                                    Icon::KeyframeEmpty
                                },
                                None,
                                key_here,
                                theme,
                            )
                            .on_hover_text(tr(lang, "Criar keyframe neste tempo"))
                            .clicked()
                            {
                                key_op = Some(true);
                            }
                            if has_keys > 0 {
                                ui.label(egui::RichText::new(format!("{has_keys}")).small());
                                if icons::small_icon_button(ui, Icon::Backspace, None, false, theme)
                                    .on_hover_text(tr(lang, "Remover keyframes"))
                                    .clicked()
                                {
                                    key_op = Some(false);
                                }
                            }
                        });
                        ui.end_row();
                        if (edited && v != cur) || key_op.is_some() {
                            changed = true;
                            let idx = match r.params.iter().position(|p| p.id == info.id) {
                                Some(i) => i,
                                None => {
                                    r.params.push(crate::model::Vst3ParamValue {
                                        id: info.id,
                                        value: info.current,
                                        keys: Vec::new(),
                                    });
                                    r.params.len() - 1
                                }
                            };
                            if let Some(p) = r.params.get_mut(idx) {
                                match key_op {
                                    Some(true) => {
                                        if p.keys.is_empty() && local_t > tol {
                                            p.keys.push((crate::model::Ticks::ZERO, p.value));
                                        }
                                        p.set_key(local_t, v, tol);
                                    }
                                    Some(false) => {
                                        p.value = v;
                                        p.keys.clear();
                                    }
                                    None if p.keys.is_empty() => p.value = v,
                                    None => p.set_key(local_t, v, tol),
                                }
                            }
                        }
                    }
                });
        });
    changed
}

type ParamsState = Option<Result<Arc<Vec<OfxParamInfo>>, String>>;
type ParamsMap = Arc<Mutex<HashMap<(String, String), ParamsState>>>;

/// Editor dos parâmetros de um efeito OFX (no diálogo de FX). Os descritores
/// vêm do host isolado numa thread de fundo e ficam em cache por plug-in.
/// Devolve verdadeiro quando algum valor mudou.
pub(crate) fn ofx_params_editor(
    ui: &mut egui::Ui,
    lang: Language,
    fx: &mut crate::model::FxInstance,
    theme: &crate::theme::ThemeColors,
    local_t: crate::model::Ticks,
    tol: crate::model::Ticks,
) -> bool {
    let Some(r) = fx.ofx.as_mut() else {
        return false;
    };
    let cache_key = egui::Id::new("ofx_param_descriptors");
    let map: ParamsMap = ui.data_mut(|d| d.get_temp_mut_or_default::<ParamsMap>(cache_key).clone());
    let k = (r.binary.clone(), r.plugin_id.clone());
    let state = map
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&k)
        .cloned();
    let muted = |t: String| egui::RichText::new(t).small().color(theme.text_muted);
    let infos = match state {
        None => {
            map.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(k.clone(), None);
            let (map2, r2, ctx) = (map.clone(), r.clone(), ui.ctx().clone());
            let missing_exe = tr(lang, "executável do Neoveg não encontrado").to_string();
            let _ = std::thread::Builder::new()
                .name("ofx-params".into())
                .spawn(move || {
                    let res = neoveg_engine::plugin_host::default_host_exe()
                        .ok_or(missing_exe)
                        .and_then(|exe| neoveg_engine::plugin_host::ofx_params(&exe, &r2))
                        .map(Arc::new);
                    map2.lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .insert((r2.binary, r2.plugin_id), Some(res));
                    ctx.request_repaint();
                });
            ui.label(muted(
                tr(lang, "Abrindo o plug-in em processo isolado…").into(),
            ));
            return false;
        }
        Some(None) => {
            ui.label(muted(
                tr(lang, "Abrindo o plug-in em processo isolado…").into(),
            ));
            return false;
        }
        Some(Some(Err(e))) => {
            ui.label(muted(tr_args(
                lang,
                "Plug-in indisponível: {err}",
                &[("err", &e)],
            )));
            return false;
        }
        Some(Some(Ok(infos))) => infos,
    };
    let mut changed = false;
    egui::Grid::new(("ofx_params", fx.id))
        .num_columns(3)
        .striped(true)
        .show(ui, |ui| {
            for info in infos.iter() {
                let n = info.default.len();
                if n == 0 || n > 4 {
                    continue;
                }
                let mut vals: Vec<f64> = r
                    .params
                    .iter()
                    .find(|p| p.name == info.name)
                    .map(|p| p.values_at(local_t))
                    .unwrap_or_else(|| info.default.clone());
                vals.resize(n, 0.0);
                ui.label(&info.label);
                let mut edited = false;
                ui.horizontal(|ui| match info.kind.as_str() {
                    "Boolean" => {
                        let mut b = vals[0] > 0.5;
                        edited |= ui.checkbox(&mut b, "").changed();
                        vals[0] = if b { 1.0 } else { 0.0 };
                    }
                    "Choice" if !info.choices.is_empty() => {
                        let cur =
                            vals[0].round().clamp(0.0, (info.choices.len() - 1) as f64) as usize;
                        let mut sel = cur;
                        egui::ComboBox::from_id_salt(("ofx_choice", fx.id, &info.name))
                            .selected_text(info.choices.get(cur).cloned().unwrap_or_default())
                            .show_ui(ui, |ui| {
                                for (ci, c) in info.choices.iter().enumerate() {
                                    ui.selectable_value(&mut sel, ci, c);
                                }
                            });
                        if sel != cur {
                            vals[0] = sel as f64;
                            edited = true;
                        }
                    }
                    kind => {
                        let int = kind.starts_with("Integer");
                        for v in vals.iter_mut() {
                            let mut dv =
                                egui::DragValue::new(v).speed(if int { 1.0 } else { 0.01 });
                            if let (Some(lo), Some(hi)) = (info.min, info.max) {
                                if lo.is_finite() && hi.is_finite() && lo <= hi {
                                    dv = dv.range(lo..=hi);
                                }
                            }
                            if int {
                                dv = dv.fixed_decimals(0);
                            }
                            edited |= ui.add(dv).changed();
                        }
                    }
                });
                // keyframes: mesmo comportamento dos parâmetros nativos
                let mut key_op: Option<bool> = None; // Some(true) cria, Some(false) remove
                let (has_keys, key_here) = r
                    .params
                    .iter()
                    .find(|p| p.name == info.name)
                    .map(|p| {
                        (
                            p.keys.len(),
                            p.keys.iter().any(|k| k.0.abs_diff(local_t) <= tol),
                        )
                    })
                    .unwrap_or((0, false));
                ui.horizontal(|ui| {
                    if icons::small_icon_button(
                        ui,
                        if key_here {
                            Icon::KeyframeFilled
                        } else {
                            Icon::KeyframeEmpty
                        },
                        None,
                        key_here,
                        theme,
                    )
                    .on_hover_text(tr(lang, "Criar keyframe neste tempo"))
                    .clicked()
                    {
                        key_op = Some(true);
                    }
                    if has_keys > 0 {
                        ui.label(egui::RichText::new(format!("{has_keys}")).small());
                        if icons::small_icon_button(ui, Icon::Backspace, None, false, theme)
                            .on_hover_text(tr(lang, "Remover keyframes"))
                            .clicked()
                        {
                            key_op = Some(false);
                        }
                    }
                });
                ui.end_row();
                if edited || key_op.is_some() {
                    changed = true;
                    let idx = match r.params.iter().position(|p| p.name == info.name) {
                        Some(i) => i,
                        None => {
                            r.params.push(crate::model::OfxParamValue {
                                name: info.name.clone(),
                                values: info.default.clone(),
                                ..Default::default()
                            });
                            r.params.len() - 1
                        }
                    };
                    let Some(p) = r.params.get_mut(idx) else {
                        continue;
                    };
                    // como interpolar entre keyframes depende do tipo
                    p.hold = matches!(info.kind.as_str(), "Boolean" | "Choice" | "String");
                    p.integer = info.kind.starts_with("Integer");
                    match key_op {
                        Some(true) => {
                            if p.keys.is_empty() && local_t > tol {
                                p.keys.push((crate::model::Ticks::ZERO, p.values.clone()));
                            }
                            p.set_key(local_t, vals, tol);
                        }
                        Some(false) => {
                            p.values = vals;
                            p.keys.clear();
                        }
                        None if p.keys.is_empty() => p.values = vals,
                        None => p.set_key(local_t, vals, tol),
                    }
                }
            }
        });
    changed
}

/// Dispara a sondagem isolada numa thread; o resultado volta pelo mapa.
fn start_probe<T: Send + Sync + 'static>(
    probes: &ProbeMap<T>,
    bin: PathBuf,
    ctx: egui::Context,
    lang: Language,
    probe: ProbeFn<T>,
) {
    probes
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(bin.clone(), ProbeState::Running);
    let worker = probes.clone();
    let prefix = tr(lang, "executável do Neoveg não encontrado").to_string();
    let spawned = std::thread::Builder::new()
        .name("plugin-probe".into())
        .spawn(move || {
            let result = std::env::current_exe()
                .map_err(|e| format!("{prefix}: {e}"))
                .and_then(|exe| probe(&exe, &bin, PROBE_TIMEOUT))
                .map(Arc::new);
            worker
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(bin, ProbeState::Done(result));
            ctx.request_repaint();
        });
    if spawned.is_err() {
        // sem thread: devolve o estado para o usuário tentar de novo
        probes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }
}

/// Categoria da árvore do *Plug-In Manager*.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub(crate) enum PluginCategory {
    VideoFx,
    AudioFx,
    Transition,
    Compositor,
}

impl PluginCategory {
    pub(crate) const ALL: [PluginCategory; 4] = [
        PluginCategory::VideoFx,
        PluginCategory::AudioFx,
        PluginCategory::Transition,
        PluginCategory::Compositor,
    ];

    pub(crate) fn name_in(self, lang: Language) -> &'static str {
        let key = match self {
            PluginCategory::VideoFx => "Efeitos de vídeo",
            PluginCategory::AudioFx => "Efeitos de áudio",
            PluginCategory::Transition => "Transições",
            PluginCategory::Compositor => "Compositores",
        };
        tr(lang, key)
    }

    fn icon(self) -> Icon {
        match self {
            PluginCategory::VideoFx => Icon::FxSparkle,
            PluginCategory::AudioFx => Icon::FxAudio,
            PluginCategory::Transition => Icon::TransitionSplit,
            PluginCategory::Compositor => Icon::Compositors,
        }
    }
}

/// Um plug-in do catálogo.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum PluginEntry {
    Fx(FxKind),
    Transition(TransitionKind),
    Compositor(BlendMode),
}

impl PluginEntry {
    pub(crate) fn name_in(self, lang: Language) -> &'static str {
        match self {
            PluginEntry::Fx(k) => k.name_in(lang),
            PluginEntry::Transition(k) => k.name_in(lang),
            PluginEntry::Compositor(b) => b.name_in(lang),
        }
    }

    pub(crate) fn name(self) -> &'static str {
        self.name_in(Language::PtBr)
    }

    pub(crate) fn category(self) -> PluginCategory {
        match self {
            PluginEntry::Fx(k) if k.is_audio() => PluginCategory::AudioFx,
            PluginEntry::Fx(_) => PluginCategory::VideoFx,
            PluginEntry::Transition(_) => PluginCategory::Transition,
            PluginEntry::Compositor(_) => PluginCategory::Compositor,
        }
    }
}

/// Catálogo de uma categoria, filtrado por texto (sem diferenciar maiúsculas).
#[cfg(test)]
pub(crate) fn plugin_catalog(cat: PluginCategory, filter: &str) -> Vec<PluginEntry> {
    plugin_catalog_in(cat, filter, Language::PtBr)
}

/// Versão localizada do catálogo por idioma (casa nome localizado ou canônico).
pub(crate) fn plugin_catalog_in(
    cat: PluginCategory,
    filter: &str,
    lang: Language,
) -> Vec<PluginEntry> {
    let all: Vec<PluginEntry> = match cat {
        PluginCategory::VideoFx => FxKind::VIDEO.iter().map(|k| PluginEntry::Fx(*k)).collect(),
        PluginCategory::AudioFx => FxKind::AUDIO.iter().map(|k| PluginEntry::Fx(*k)).collect(),
        PluginCategory::Transition => TransitionKind::ALL
            .iter()
            .map(|k| PluginEntry::Transition(*k))
            .collect(),
        PluginCategory::Compositor => BlendMode::ALL
            .iter()
            .map(|b| PluginEntry::Compositor(*b))
            .collect(),
    };
    let f = filter.trim().to_lowercase();
    if f.is_empty() {
        return all;
    }
    all.into_iter()
        .filter(|e| {
            e.name_in(lang).to_lowercase().contains(&f) || e.name().to_lowercase().contains(&f)
        })
        .collect()
}

impl App {
    pub(crate) fn plugin_manager_tab(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang();
        let filter_key = egui::Id::new("plugin_manager_filter");
        let sel_key = egui::Id::new("plugin_manager_selected");
        let sel_ofx_key = egui::Id::new("plugin_manager_selected_ofx");
        let mut filter: String = ui.data(|d| d.get_temp(filter_key)).unwrap_or_default();
        let mut selected: Option<PluginEntry> = ui.data(|d| d.get_temp(sel_key));
        let mut selected_ofx: Option<(PathBuf, OfxPluginInfo)> =
            ui.data(|d| d.get_temp(sel_ofx_key));
        ui.horizontal(|ui| {
            ui.label(tr(lang, "Pesquisar:"));
            ui.add(egui::TextEdit::singleline(&mut filter).desired_width(160.0));
            if !filter.is_empty() && ui.small_button("×").clicked() {
                filter.clear();
            }
        });
        ui.separator();
        ui.columns(2, |cols| {
            egui::ScrollArea::vertical()
                .id_salt("plugin_tree")
                .auto_shrink([false, false])
                .show(&mut cols[0], |ui| {
                    for cat in PluginCategory::ALL {
                        let items = plugin_catalog_in(cat, &filter, lang);
                        let header = format!("{} ({})", cat.name_in(lang), items.len());
                        egui::CollapsingHeader::new(header)
                            .id_salt(("plugin_cat", cat))
                            .default_open(!filter.is_empty() || cat == PluginCategory::VideoFx)
                            .show(ui, |ui| {
                                for e in items {
                                    let is_sel = selected == Some(e);
                                    let label = e.name_in(lang);
                                    if ui.selectable_label(is_sel, label).clicked() {
                                        selected = Some(e);
                                        selected_ofx = None;
                                    }
                                }
                            });
                    }
                    egui::CollapsingHeader::new(tr(lang, "Plug-ins externos (VST3 / OFX)"))
                        .id_salt("plugin_cat_external")
                        .show(ui, |ui| {
                            if let Some(ofx_pick) =
                                self.external_plugins_list(ui, &filter, &selected_ofx)
                            {
                                selected_ofx = Some(ofx_pick);
                                selected = None;
                            }
                        });
                });
            egui::ScrollArea::vertical()
                .id_salt("plugin_details_scroll")
                .auto_shrink([false, false])
                .show(&mut cols[1], |ui| {
                    if let Some((ref bin, ref info)) = selected_ofx {
                        self.external_ofx_details(ui, bin, info);
                    } else if let Some(e) = selected {
                        self.plugin_details(ui, e);
                    } else {
                        ui.label(
                            egui::RichText::new(tr(
                                lang,
                                "Selecione um plug-in para ver os detalhes. Arraste efeitos e \
                                 transições até um evento ou trilha.",
                            ))
                            .small()
                            .color(self.theme.text_muted),
                        );
                    }
                });
        });
        ui.data_mut(|d| {
            d.insert_temp(filter_key, filter);
            match selected {
                Some(s) => d.insert_temp(sel_key, s),
                None => d.remove::<PluginEntry>(sel_key),
            }
            match selected_ofx {
                Some(s) => d.insert_temp(sel_ofx_key, s),
                None => d.remove::<(PathBuf, OfxPluginInfo)>(sel_ofx_key),
            }
        });
    }

    /// Plug-ins VST3/OFX encontrados nas pastas padrão. Binários OFX só são
    /// carregados sob pedido e sempre no subprocesso isolado.
    fn external_plugins_list(
        &mut self,
        ui: &mut egui::Ui,
        filter: &str,
        selected_ofx: &Option<(PathBuf, OfxPluginInfo)>,
    ) -> Option<(PathBuf, OfxPluginInfo)> {
        let lang = self.lang();
        let key = egui::Id::new("plugin_manager_external_scan");
        let slot: ScanSlot = ui.data_mut(|d| d.get_temp_mut_or_default::<ScanSlot>(key).clone());
        let folders: Vec<PathBuf> = self
            .prefs
            .plugin_folders
            .iter()
            .map(PathBuf::from)
            .collect();
        let state = slot.lock().unwrap_or_else(PoisonError::into_inner).clone();
        let refresh = ui
            .small_button(tr(lang, "Atualizar"))
            .on_hover_text(tr(
                lang,
                "Varre de novo as pastas padrão e as pastas mapeadas",
            ))
            .clicked();
        // varre ao abrir o painel, ao pedir e quando as pastas mapeadas mudam
        if !state.running && (refresh || state.result.is_none() || state.folders != folders) {
            start_scan(&slot, folders.clone(), ui.ctx().clone(), lang);
        }
        let statuses: Vec<FolderScan> = match &state.result {
            Some(Ok(r)) if state.folders == folders => r.1.clone(),
            _ => Vec::new(),
        };
        self.plugin_folders_ui(ui, &statuses, state.running);
        ui.separator();
        let list: Arc<ScanResult> = match &state.result {
            Some(Ok(r)) => r.clone(),
            Some(Err(e)) => {
                ui.colored_label(self.theme.offline_fg, e.as_str());
                return None;
            }
            None => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(tr(lang, "Procurando plug-ins..."));
                });
                return None;
            }
        };
        let f = filter.trim().to_lowercase();
        let shown: Vec<&ExternalPlugin> = list
            .0
            .iter()
            .filter(|p| f.is_empty() || p.name.to_lowercase().contains(&f))
            .collect();
        if shown.is_empty() {
            ui.label(
                egui::RichText::new(tr(
                    lang,
                    "Nenhum plug-in externo encontrado nas pastas padrão nem nas pastas mapeadas.",
                ))
                .small()
                .color(self.theme.text_muted),
            );
            return None;
        }
        let mut picked_ofx: Option<(PathBuf, OfxPluginInfo)> = None;
        for p in shown {
            let mut tip = format!("{}\n{}", p.kind.name(), p.path.display());
            if let Some(v) = &p.vendor {
                tip.push_str(&tr_args(lang, "\nFabricante: {v}", &[("v", v)]));
            }
            if let Some(v) = &p.version {
                tip.push_str(&tr_args(lang, "\nVersão: {v}", &[("v", v)]));
            }
            for c in &p.classes {
                tip.push_str(&format!("\n• {c}"));
            }
            if p.binary.is_none() {
                tip.push_str(tr(lang, "\nSem binário para esta arquitetura."));
            }
            let title = format!("{} [{}]", p.name, p.kind.name());
            match (&p.binary, p.kind) {
                (Some(bin), ExternalKind::Ofx) => {
                    egui::CollapsingHeader::new(title)
                        .id_salt(("plugin_ext", p.path.display().to_string()))
                        .show(ui, |ui| {
                            if let Some(sel) = self.ofx_probe_view(ui, bin, selected_ofx) {
                                picked_ofx = Some(sel);
                            }
                        });
                }
                (Some(bin), ExternalKind::Vst3) => {
                    egui::CollapsingHeader::new(title)
                        .id_salt(("plugin_ext", p.path.display().to_string()))
                        .show(ui, |ui| self.vst3_probe_view(ui, bin));
                }
                _ => {
                    ui.add_enabled(false, egui::Label::new(title))
                        .on_disabled_hover_text(tip);
                }
            }
        }
        picked_ofx
    }

    /// Pastas de plug-ins mapeadas pelo usuário (ex.: uma pasta do Google
    /// Drive), salvas nas Preferências: ao reabrir o Neoveg elas são varridas
    /// de novo e os plug-ins delas voltam a aparecer aqui.
    fn plugin_folders_ui(&mut self, ui: &mut egui::Ui, statuses: &[FolderScan], running: bool) {
        let lang = self.lang();
        ui.label(egui::RichText::new(tr(lang, "Pastas de plug-ins")).strong());
        let mut remove: Option<usize> = None;
        let mut relocate: Option<usize> = None;
        for (i, folder) in self.prefs.plugin_folders.iter().enumerate() {
            let status = statuses
                .iter()
                .find(|s| s.path == Path::new(folder))
                .map(|s| s.status.clone());
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new(folder).monospace().small());
                match status {
                    Some(FolderStatus::Found(n)) => {
                        let n_s = n.to_string();
                        ui.label(
                            egui::RichText::new(tr_args(lang, "{n} plug-in(s)", &[("n", &n_s)]))
                                .small()
                                .color(self.theme.text_muted),
                        );
                    }
                    Some(FolderStatus::Missing) | Some(FolderStatus::NotDir) => {
                        let why = if matches!(status, Some(FolderStatus::Missing)) {
                            tr(
                                lang,
                                "pasta não encontrada (unidade ou Google Drive desconectado?)",
                            )
                        } else {
                            tr(lang, "o caminho não é uma pasta")
                        };
                        ui.colored_label(self.theme.offline_fg, egui::RichText::new(why).small());
                        if ui
                            .small_button(tr(lang, "Localizar..."))
                            .on_hover_text(tr(lang, "Escolher o novo lugar desta pasta"))
                            .clicked()
                        {
                            relocate = Some(i);
                        }
                    }
                    None if running => {
                        ui.spinner();
                    }
                    None => {}
                }
                if ui
                    .small_button(tr(lang, "Remover"))
                    .on_hover_text(tr(lang, "Deixa de procurar plug-ins nesta pasta"))
                    .clicked()
                {
                    remove = Some(i);
                }
            });
        }
        if self.prefs.plugin_folders.is_empty() {
            ui.label(
                egui::RichText::new(tr(
                    lang,
                    "Nenhuma pasta mapeada. As pastas padrão do sistema são sempre varridas.",
                ))
                .small()
                .color(self.theme.text_muted),
            );
        }
        let mut changed = false;
        if ui
            .small_button(tr(lang, "Adicionar pasta..."))
            .on_hover_text(tr(
                lang,
                "Procura plug-ins VST3 (áudio) e OFX (vídeo) nesta pasta e nas subpastas dela",
            ))
            .clicked()
        {
            if let Some(dir) = rfd::FileDialog::new()
                .set_title(tr(lang, "Pasta de plug-ins VST3/OFX"))
                .pick_folder()
            {
                if self.prefs.add_plugin_folder(&dir) {
                    changed = true;
                } else {
                    self.set_status(tr(
                        lang,
                        "Essa pasta já está mapeada ou a lista está cheia.",
                    ));
                }
            }
        }
        if let Some(i) = relocate {
            if let Some(dir) = rfd::FileDialog::new()
                .set_title(tr(lang, "Pasta de plug-ins VST3/OFX"))
                .pick_folder()
            {
                changed |= self.prefs.replace_plugin_folder(i, &dir);
            }
        }
        if let Some(i) = remove {
            if i < self.prefs.plugin_folders.len() {
                self.prefs.plugin_folders.remove(i);
                changed = true;
            }
        }
        if changed {
            if let Err(e) = self.prefs.save() {
                self.set_status(e);
            }
        }
        ui.label(
            egui::RichText::new(tr(
                lang,
                "No Google Drive em modo streaming, marque a pasta como \"Disponível off-line\": \
                 assim o plug-in abre sem esperar o download.",
            ))
            .small()
            .color(self.theme.text_muted),
        );
    }

    /// Lista os plug-ins de um binário OFX, sondado no subprocesso isolado
    /// `neoveg --plugin-host` numa thread de fundo (a UI nunca bloqueia).
    fn ofx_probe_view(
        &mut self,
        ui: &mut egui::Ui,
        bin: &Path,
        selected_ofx: &Option<(PathBuf, OfxPluginInfo)>,
    ) -> Option<(PathBuf, OfxPluginInfo)> {
        let lang = self.lang();
        let key = egui::Id::new("plugin_manager_ofx_probes");
        let probes: ProbeMap<OfxPluginInfo> = ui.data_mut(|d| {
            d.get_temp_mut_or_default::<ProbeMap<OfxPluginInfo>>(key)
                .clone()
        });
        let state = probes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(bin)
            .cloned();
        let mut picked: Option<(PathBuf, OfxPluginInfo)> = None;
        match state {
            None => {
                if ui
                    .small_button(tr(lang, "Listar plug-ins"))
                    .on_hover_text(tr(
                        lang,
                        "Carrega o binário num processo isolado e lista os efeitos dele",
                    ))
                    .clicked()
                {
                    start_probe(
                        &probes,
                        bin.to_path_buf(),
                        ui.ctx().clone(),
                        lang,
                        probe_ofx_isolated,
                    );
                }
            }
            Some(ProbeState::Running) => {
                ui.label(
                    egui::RichText::new(tr(lang, "Consultando o plug-in em processo isolado…"))
                        .small()
                        .color(self.theme.text_muted),
                );
            }
            Some(ProbeState::Done(Err(e))) => {
                ui.label(
                    egui::RichText::new(tr_args(lang, "Erro: {err}", &[("err", &e)]))
                        .small()
                        .color(self.theme.text_muted),
                );
            }
            Some(ProbeState::Done(Ok(list))) => {
                let n_s = list.len().to_string();
                ui.label(
                    egui::RichText::new(tr_args(lang, "{n} plug-in(s)", &[("n", &n_s)]))
                        .small()
                        .color(self.theme.text_muted),
                );
                egui::ScrollArea::vertical()
                    .id_salt(("ofx_probe_scroll", bin.display().to_string()))
                    .max_height(160.0)
                    .show(ui, |ui| {
                        let can_apply = !self.selected_video_events().is_empty();
                        for info in list.iter() {
                            ui.horizontal(|ui| {
                                if ui
                                    .add_enabled(
                                        can_apply,
                                        egui::Button::new(tr(lang, "Aplicar")).small(),
                                    )
                                    .on_hover_text(tr(
                                        lang,
                                        "Aplicar aos eventos de vídeo selecionados",
                                    ))
                                    .on_disabled_hover_text(tr(
                                        lang,
                                        "Selecione eventos de vídeo na timeline",
                                    ))
                                    .clicked()
                                {
                                    self.apply_ofx_to_selection(bin, &info.identifier);
                                }
                                let apiv = info.api_version.to_string();
                                let maj = info.version_major.to_string();
                                let min = info.version_minor.to_string();
                                let is_sel = selected_ofx.as_ref().is_some_and(|(b, i)| {
                                    b == bin && i.identifier == info.identifier
                                });
                                if ui
                                    .selectable_label(
                                        is_sel,
                                        egui::RichText::new(&info.identifier).small().monospace(),
                                    )
                                    .on_hover_text(tr_args(
                                        lang,
                                        "{api} v{apiv} — versão {maj}.{min}",
                                        &[
                                            ("api", &info.api),
                                            ("apiv", &apiv),
                                            ("maj", &maj),
                                            ("min", &min),
                                        ],
                                    ))
                                    .clicked()
                                {
                                    picked = Some((bin.to_path_buf(), info.clone()));
                                }
                            });
                        }
                    });
            }
        }
        picked
    }

    /// Lista as classes de efeito de um módulo VST3, sondado no subprocesso
    /// isolado numa thread de fundo, com *Aplicar* para a seleção de áudio.
    fn vst3_probe_view(&mut self, ui: &mut egui::Ui, bin: &Path) {
        let lang = self.lang();
        let key = egui::Id::new("plugin_manager_vst3_probes");
        let probes: ProbeMap<Vst3ClassInfo> = ui.data_mut(|d| {
            d.get_temp_mut_or_default::<ProbeMap<Vst3ClassInfo>>(key)
                .clone()
        });
        let state = probes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(bin)
            .cloned();
        let muted = |t: String| egui::RichText::new(t).small().color(self.theme.text_muted);
        match state {
            None => {
                if ui
                    .small_button(tr(lang, "Listar plug-ins"))
                    .on_hover_text(tr(
                        lang,
                        "Carrega o módulo num processo isolado e lista os efeitos dele",
                    ))
                    .clicked()
                {
                    start_probe(
                        &probes,
                        bin.to_path_buf(),
                        ui.ctx().clone(),
                        lang,
                        probe_vst3_isolated,
                    );
                }
            }
            Some(ProbeState::Running) => {
                ui.label(muted(
                    tr(lang, "Consultando o plug-in em processo isolado…").into(),
                ));
            }
            Some(ProbeState::Done(Err(e))) => {
                ui.label(muted(tr_args(lang, "Erro: {err}", &[("err", &e)])));
            }
            Some(ProbeState::Done(Ok(list))) => {
                if list.is_empty() {
                    ui.label(muted(
                        tr(lang, "Nenhum efeito de áudio neste módulo.").into(),
                    ));
                }
                let can_apply = self.audio_fx_target().is_some();
                for info in list.iter() {
                    ui.horizontal(|ui| {
                        let instrument = info.is_instrument();
                        if ui
                            .add_enabled(
                                can_apply,
                                egui::Button::new(tr(lang, "Aplicar")).small(),
                            )
                            .on_hover_text(if instrument {
                                tr(
                                    lang,
                                    "Aplicar instrumento VST3 aos eventos MIDI/áudio ou à trilha de áudio selecionada",
                                )
                            } else {
                                tr(
                                    lang,
                                    "Aplicar aos eventos de áudio selecionados (ou à trilha de \
                                     áudio selecionada)",
                                )
                            })
                            .on_disabled_hover_text(tr(
                                lang,
                                "Selecione eventos ou uma trilha de áudio na timeline",
                            ))
                            .clicked()
                        {
                            self.apply_vst3_to_selection(bin, info);
                        }
                        if instrument {
                            ui.label(
                                egui::RichText::new("[MIDI]")
                                    .small()
                                    .color(self.theme.accent),
                            );
                        }
                        ui.label(egui::RichText::new(&info.name).small())
                            .on_hover_text(tr_args(
                                lang,
                                "{cats}\nVersão {ver}\nClasse {cls}",
                                &[
                                    ("cats", &info.sub_categories),
                                    ("ver", &info.version),
                                    ("cls", &info.class_id),
                                ],
                            ));
                    });
                }
            }
        }
    }

    fn external_ofx_details(&mut self, ui: &mut egui::Ui, bin: &Path, info: &OfxPluginInfo) {
        let lang = self.lang();
        icons::icon_label(ui, Icon::FxSparkle, 16.0, &info.identifier, &self.theme);
        let apiv = info.api_version.to_string();
        let maj = info.version_major.to_string();
        let min = info.version_minor.to_string();
        ui.label(
            egui::RichText::new(tr_args(
                lang,
                "{api} v{apiv} — versão {maj}.{min}",
                &[
                    ("api", &info.api),
                    ("apiv", &apiv),
                    ("maj", &maj),
                    ("min", &min),
                ],
            ))
            .small()
            .color(self.theme.text_muted),
        );
        ui.separator();
        let preview_sz = egui::vec2(192.0, 108.0);
        let bin_s = bin.to_string_lossy().to_string();
        let ctx = ui.ctx().clone();
        match self.fx_browser.ofx.get(&ctx, &bin_s, &info.identifier) {
            crate::fx_browser::OfxThumb::Ready(tex) => {
                ui.image((tex.id(), preview_sz));
            }
            crate::fx_browser::OfxThumb::Pending => {
                let (r, _) = ui.allocate_exact_size(preview_sz, egui::Sense::hover());
                ui.painter().rect_filled(r, 2.0, self.theme.extreme_bg);
                ui.painter().text(
                    r.center(),
                    egui::Align2::CENTER_CENTER,
                    tr(lang, "Gerando prévia..."),
                    egui::FontId::proportional(11.0),
                    self.theme.text_muted,
                );
            }
            crate::fx_browser::OfxThumb::Failed(e) => {
                let e = e.clone();
                let (r, resp) = ui.allocate_exact_size(preview_sz, egui::Sense::hover());
                ui.painter().rect_filled(r, 2.0, self.theme.extreme_bg);
                ui.painter().text(
                    r.center(),
                    egui::Align2::CENTER_CENTER,
                    tr(lang, "sem prévia"),
                    egui::FontId::proportional(11.0),
                    self.theme.text_muted,
                );
                resp.on_hover_text(e);
            }
        }
        ui.add_space(6.0);
        let can_apply = !self.selected_video_events().is_empty();
        if ui
            .add_enabled(can_apply, egui::Button::new(tr(lang, "Aplicar")))
            .on_disabled_hover_text(tr(lang, "Selecione eventos de vídeo na timeline"))
            .clicked()
        {
            self.apply_ofx_to_selection(bin, &info.identifier);
        }
    }

    fn plugin_details(&mut self, ui: &mut egui::Ui, e: PluginEntry) {
        let lang = self.lang();
        icons::icon_label(ui, e.category().icon(), 16.0, e.name_in(lang), &self.theme);
        ui.label(
            egui::RichText::new(tr_args(
                lang,
                "Categoria: {cat}",
                &[("cat", &e.category().name_in(lang))],
            ))
            .small()
            .color(self.theme.text_muted),
        );
        ui.label(
            egui::RichText::new(tr(lang, "Origem: nativo do Neoveg"))
                .small()
                .color(self.theme.text_muted),
        );
        ui.separator();
        match e {
            PluginEntry::Fx(k) => {
                if !k.is_audio() {
                    let preview_sz = egui::vec2(192.0, 108.0);
                    let drag = ui.dnd_drag_source(
                        egui::Id::new(("pm_fx_preview_drag", k as u32)),
                        DndPayload::Fx(k),
                        |ui| {
                            ui.vertical(|ui| {
                                let (rect, _) =
                                    ui.allocate_exact_size(preview_sz, egui::Sense::hover());
                                let hovered = ui.rect_contains_pointer(rect);
                                let ctx = ui.ctx().clone();
                                if let Some(tex) = self.fx_browser.preview_texture(&ctx, k, hovered)
                                {
                                    egui::Image::new((tex.id(), preview_sz)).paint_at(ui, rect);
                                } else {
                                    ui.painter().rect_filled(rect, 2.0, self.theme.widget_bg);
                                }
                                ui.painter().rect_stroke(
                                    rect,
                                    2.0,
                                    egui::Stroke::new(1.5_f32, self.theme.accent),
                                    egui::StrokeKind::Inside,
                                );
                                ui.label(egui::RichText::new(k.name_in(lang)).small().strong());
                            });
                        },
                    );
                    drag.response
                        .on_hover_text(tr(lang, "Arraste para um evento ou trilha de vídeo"));
                    ui.add_space(4.0);
                } else {
                    let drag = ui.dnd_drag_source(
                        egui::Id::new(("pm_afx_preview_drag", k as u32)),
                        DndPayload::Fx(k),
                        |ui| {
                            egui::Frame::new()
                                .fill(self.theme.widget_bg)
                                .stroke(egui::Stroke::new(1.0_f32, self.theme.accent))
                                .corner_radius(3.0)
                                .inner_margin(egui::Margin::symmetric(8, 5))
                                .show(ui, |ui| {
                                    icons::icon_label(
                                        ui,
                                        Icon::FxAudio,
                                        14.0,
                                        k.name_in(lang),
                                        &self.theme,
                                    );
                                });
                        },
                    );
                    drag.response
                        .on_hover_text(tr(lang, "Arraste para um evento ou trilha de áudio"));
                    ui.add_space(4.0);
                }
                ui.horizontal_wrapped(|ui| {
                    self.fx_apply_instance_buttons(
                        ui,
                        crate::model::FxInstance::new(k),
                        k.is_audio(),
                    );
                });
                ui.separator();
                let params = k.params();
                if params.is_empty() {
                    ui.label(tr(lang, "Sem parâmetros."));
                } else {
                    egui::Grid::new(("plugin_params", k as u32))
                        .num_columns(2)
                        .striped(true)
                        .show(ui, |ui| {
                            for p in params {
                                let min_s = p.min.to_string();
                                let max_s = p.max.to_string();
                                let def_s = p.default.to_string();
                                ui.label(p.name_in(lang));
                                ui.label(
                                    egui::RichText::new(tr_args(
                                        lang,
                                        "{min} … {max} (padrão {def})",
                                        &[("min", &min_s), ("max", &max_s), ("def", &def_s)],
                                    ))
                                    .small()
                                    .monospace(),
                                );
                                ui.end_row();
                            }
                        });
                }
            }
            PluginEntry::Transition(tk) => {
                if let Some(idx) = TransitionKind::ALL.iter().position(|x| *x == tk) {
                    let preview_sz = egui::vec2(192.0, 108.0);
                    let drag = ui.dnd_drag_source(
                        egui::Id::new(("pm_tr_preview_drag", tk as u32)),
                        DndPayload::Transition(tk),
                        |ui| {
                            ui.vertical(|ui| {
                                let (rect, _) =
                                    ui.allocate_exact_size(preview_sz, egui::Sense::hover());
                                let hovered = ui.rect_contains_pointer(rect);
                                let ctx = ui.ctx().clone();
                                if let Some(tex) =
                                    self.fx_browser.transitions.texture(&ctx, idx, hovered)
                                {
                                    egui::Image::new((tex.id(), preview_sz)).paint_at(ui, rect);
                                } else {
                                    ui.painter().rect_filled(rect, 2.0, self.theme.widget_bg);
                                }
                                ui.painter().rect_stroke(
                                    rect,
                                    2.0,
                                    egui::Stroke::new(1.5_f32, self.theme.accent),
                                    egui::StrokeKind::Inside,
                                );
                                ui.label(egui::RichText::new(tk.name_in(lang)).small().strong());
                            });
                        },
                    );
                    drag.response.on_hover_text(tr(
                        lang,
                        "Arraste até um evento de vídeo para usar no crossfade ou fade.",
                    ));
                    ui.add_space(4.0);
                }
                ui.label(tr(
                    lang,
                    "Arraste até um evento de vídeo para usar no crossfade ou fade.",
                ));
            }
            PluginEntry::Compositor(b) => {
                let target = self.selected_video_track();
                if ui
                    .add_enabled(
                        target.is_some(),
                        egui::Button::new(tr(lang, "Aplicar à trilha selecionada")),
                    )
                    .clicked()
                {
                    self.apply_compositor(b);
                }
            }
        }
    }

    pub(crate) fn compositors_tab(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang();
        // eventos de vídeo selecionados: o compositor vale só para eles
        let evs = self.selected_video_events();
        if !evs.is_empty() {
            let modes: Vec<Option<BlendMode>> = evs
                .iter()
                .filter_map(|id| self.project.event(*id).map(|e| e.blend))
                .collect();
            let common = modes
                .first()
                .copied()
                .filter(|m| modes.iter().all(|x| x == m));
            let n = evs.len().to_string();
            ui.label(
                egui::RichText::new(tr_args(lang, "Eventos selecionados: {n}", &[("n", &n)]))
                    .strong(),
            )
            .on_hover_text(tr(
                lang,
                "O modo vale só para estes eventos; \"Usar o da trilha\" volta ao modo da trilha",
            ));
            ui.separator();
            egui::Grid::new("compositors_events_grid")
                .num_columns(1)
                .striped(true)
                .show(ui, |ui| {
                    if ui
                        .selectable_label(common == Some(None), tr(lang, "Usar o da trilha"))
                        .clicked()
                        && common != Some(None)
                    {
                        self.apply_event_compositor(&evs, None);
                    }
                    ui.end_row();
                    for b in BlendMode::ALL {
                        let on = common == Some(Some(b));
                        if ui.selectable_label(on, b.name_in(lang)).clicked() && !on {
                            self.apply_event_compositor(&evs, Some(b));
                        }
                        ui.end_row();
                    }
                });
            return;
        }
        let Some(ti) = self.selected_video_track() else {
            ui.label(
                egui::RichText::new(tr(
                    lang,
                    "Selecione uma trilha de vídeo, ou eventos de vídeo, para escolher o \
                     modo de composição (Compositing Mode).",
                ))
                .small()
                .color(self.theme.text_muted),
            );
            return;
        };
        let (name, current) = match self.project.tracks.get(ti) {
            Some(t) => (t.name.clone(), t.blend),
            None => return,
        };
        ui.label(egui::RichText::new(tr_args(lang, "Trilha: {name}", &[("name", &name)])).strong());
        ui.separator();
        egui::Grid::new("compositors_grid")
            .num_columns(1)
            .striped(true)
            .show(ui, |ui| {
                for b in BlendMode::ALL {
                    if ui.selectable_label(current == b, b.name_in(lang)).clicked() && current != b
                    {
                        self.apply_compositor(b);
                    }
                    ui.end_row();
                }
            });
    }

    /// Eventos selecionados que estão em trilhas de vídeo.
    pub(crate) fn selected_video_events(&self) -> Vec<crate::model::Id> {
        self.sel_events
            .iter()
            .filter(|id| {
                self.project
                    .find_event(**id)
                    .and_then(|(t, _)| self.project.tracks.get(t))
                    .is_some_and(|t| t.kind == TrackKind::Video)
            })
            .copied()
            .collect()
    }

    /// Aplica o efeito OFX aos eventos de vídeo selecionados (um passo de Desfazer).
    pub(crate) fn apply_ofx_to_selection(&mut self, bin: &Path, plugin_id: &str) {
        let lang = self.lang();
        let ids = self.selected_video_events();
        let Some(first) = ids.first().copied() else {
            return;
        };
        let label = plugin_id
            .rsplit('.')
            .next()
            .filter(|s| !s.is_empty())
            .unwrap_or(plugin_id)
            .to_string();
        let bin = bin.display().to_string();
        self.checkpoint();
        for id in &ids {
            if let Some(ev) = self.project.event_mut(*id) {
                ev.fx
                    .push(crate::model::FxInstance::new_ofx(&bin, plugin_id, &label));
            }
        }
        self.dlg.fx_target = Some(super::FxTarget::Event(first));
        self.changed();
        let count_s = ids.len().to_string();
        self.set_status(tr_args(
            lang,
            "Efeito OFX \"{label}\" aplicado a {n} evento(s)",
            &[("label", &label), ("n", &count_s)],
        ));
    }

    /// Onde um efeito de áudio externo entra: nos eventos de áudio
    /// selecionados ou, sem eles, na trilha de áudio selecionada.
    fn audio_fx_target(&self) -> Option<Result<Vec<crate::model::Id>, usize>> {
        let ids: Vec<crate::model::Id> = self
            .sel_events
            .iter()
            .filter(|id| {
                self.project
                    .find_event(**id)
                    .and_then(|(t, _)| self.project.tracks.get(t))
                    .is_some_and(|t| t.kind == TrackKind::Audio)
            })
            .copied()
            .collect();
        if !ids.is_empty() {
            return Some(Ok(ids));
        }
        self.sel_track
            .filter(|&t| self.project.tracks.get(t).map(|x| x.kind) == Some(TrackKind::Audio))
            .map(Err)
    }

    /// Aplica o efeito VST3 à seleção de áudio (um passo de Desfazer).
    pub(crate) fn apply_vst3_to_selection(&mut self, bin: &Path, info: &Vst3ClassInfo) {
        let lang = self.lang();
        let Some(target) = self.audio_fx_target() else {
            return;
        };
        let bin = bin.display().to_string();
        let make = || {
            crate::model::FxInstance::new_vst3_with_kind(
                &bin,
                &info.class_id,
                &info.name,
                info.is_instrument(),
            )
        };
        self.checkpoint();
        let status = match target {
            Ok(ids) => {
                for id in &ids {
                    if let Some(ev) = self.project.event_mut(*id) {
                        ev.fx.push(make());
                    }
                }
                if let Some(first) = ids.first() {
                    self.dlg.fx_target = Some(super::FxTarget::Event(*first));
                }
                let count_s = ids.len().to_string();
                let what = tr_args(lang, "{n} evento(s)", &[("n", &count_s)]);
                tr_args(
                    lang,
                    "Efeito VST3 \"{label}\" aplicado a {what}",
                    &[("label", &info.name), ("what", &what)],
                )
            }
            Err(ti) => {
                let Some(t) = self.project.tracks.get_mut(ti) else {
                    return;
                };
                t.fx.push(make());
                let (id, name) = (t.id, t.name.clone());
                self.dlg.fx_target = Some(super::FxTarget::Track(id));
                let what = tr_args(lang, "trilha \"{name}\"", &[("name", &name)]);
                tr_args(
                    lang,
                    "Efeito VST3 \"{label}\" aplicado a {what}",
                    &[("label", &info.name), ("what", &what)],
                )
            }
        };
        self.changed();
        self.set_status(status);
    }

    /// Índice da trilha de vídeo selecionada, se houver.
    pub(crate) fn selected_video_track(&self) -> Option<usize> {
        self.sel_track
            .filter(|&t| self.project.tracks.get(t).map(|x| x.kind) == Some(TrackKind::Video))
    }

    /// Modo de composição só dos eventos `ids` (`None` volta ao da trilha),
    /// num passo de Desfazer.
    pub(crate) fn apply_event_compositor(
        &mut self,
        ids: &[crate::model::Id],
        b: Option<BlendMode>,
    ) {
        if ids.is_empty() {
            return;
        }
        self.checkpoint();
        for id in ids {
            if let Some(e) = self.project.event_mut(*id) {
                e.blend = b;
            }
        }
        self.changed();
    }

    fn apply_compositor(&mut self, b: BlendMode) {
        let Some(ti) = self.selected_video_track() else {
            return;
        };
        self.checkpoint();
        if let Some(t) = self.project.tracks.get_mut(ti) {
            t.blend = b;
        }
        self.changed();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_lists_every_native_plugin() {
        assert_eq!(
            plugin_catalog(PluginCategory::VideoFx, "").len(),
            FxKind::VIDEO.len()
        );
        assert_eq!(
            plugin_catalog(PluginCategory::AudioFx, "").len(),
            FxKind::AUDIO.len()
        );
        assert_eq!(
            plugin_catalog(PluginCategory::Transition, "").len(),
            TransitionKind::ALL.len()
        );
        assert_eq!(
            plugin_catalog(PluginCategory::Compositor, "").len(),
            BlendMode::ALL.len()
        );
    }

    #[test]
    fn catalog_entries_report_their_category() {
        for cat in PluginCategory::ALL {
            for e in plugin_catalog(cat, "") {
                assert_eq!(e.category(), cat, "{e:?}");
            }
        }
    }

    #[test]
    fn catalog_filter_is_case_insensitive() {
        let name = FxKind::Invert.name();
        let hits = plugin_catalog(PluginCategory::VideoFx, &name.to_uppercase());
        assert!(hits.contains(&PluginEntry::Fx(FxKind::Invert)));
        assert!(plugin_catalog(PluginCategory::VideoFx, "zzz-nada").is_empty());
        // só espaços = sem filtro
        assert_eq!(
            plugin_catalog(PluginCategory::AudioFx, "   ").len(),
            FxKind::AUDIO.len()
        );
    }
}
