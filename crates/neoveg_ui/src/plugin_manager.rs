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
use neoveg_engine::plugin_host::{
    probe_ofx_isolated, probe_vst3_isolated, OfxParamInfo, OfxPluginInfo, Vst3ClassInfo,
    Vst3ParamInfo,
};
use neoveg_engine::plugins::{self, ExternalKind, ExternalPlugin};
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
type ProbeFn<T> = fn(&Path, &Path, Duration) -> Result<Vec<T>, String>;

type Vst3ParamsState = Option<Result<Arc<Vec<Vst3ParamInfo>>, String>>;
type Vst3ParamsMap = Arc<Mutex<HashMap<(String, String), Vst3ParamsState>>>;

/// Editor dos parâmetros de um efeito VST3 (no diálogo de FX), em valores
/// normalizados. Os descritores vêm do host isolado numa thread de fundo e
/// ficam em cache por plug-in. Devolve verdadeiro quando algum valor mudou.
pub(crate) fn vst3_params_editor(
    ui: &mut egui::Ui,
    fx: &mut crate::model::FxInstance,
    theme: &crate::theme::ThemeColors,
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
            let _ = std::thread::Builder::new()
                .name("vst3-params".into())
                .spawn(move || {
                    let res = neoveg_engine::plugin_host::default_host_exe()
                        .ok_or_else(|| "executável do Neoveg não encontrado".to_string())
                        .and_then(|exe| neoveg_engine::plugin_host::vst3_params(&exe, &r2))
                        .map(Arc::new);
                    map2.lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .insert((r2.binary, r2.class_id), Some(res));
                    ctx.request_repaint();
                });
            ui.label(muted("Abrindo o plug-in em processo isolado…".into()));
            return false;
        }
        Some(None) => {
            ui.label(muted("Abrindo o plug-in em processo isolado…".into()));
            return false;
        }
        Some(Some(Err(e))) => {
            ui.label(muted(format!("Plug-in indisponível: {e}")));
            return false;
        }
        Some(Some(Ok(infos))) => infos,
    };
    if infos.is_empty() {
        ui.label(muted("O plug-in não expõe parâmetros.".into()));
        return false;
    }
    let filter_key = egui::Id::new(("vst3_param_filter", fx_id));
    let mut filter: String = ui.data(|d| d.get_temp(filter_key)).unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label(muted(format!("{} parâmetro(s)", infos.len())));
        ui.add(
            egui::TextEdit::singleline(&mut filter)
                .hint_text("Filtrar")
                .desired_width(120.0),
        );
    });
    let f = filter.trim().to_lowercase();
    ui.data_mut(|d| d.insert_temp(filter_key, filter));
    let mut changed = false;
    egui::ScrollArea::vertical()
        .id_salt(("vst3_params_scroll", fx_id))
        .max_height(260.0)
        .show(ui, |ui| {
            egui::Grid::new(("vst3_params", fx_id))
                .num_columns(2)
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
                            .map(|p| p.value)
                            .unwrap_or(info.current);
                        let mut v = cur;
                        ui.label(&info.title);
                        let edited = ui
                            .horizontal(|ui| {
                                if info.bypass || info.step_count == 1 {
                                    let mut b = v > 0.5;
                                    let e = ui.checkbox(&mut b, "").changed();
                                    v = if b { 1.0 } else { 0.0 };
                                    e
                                } else if info.step_count > 1 {
                                    let steps = f64::from(info.step_count);
                                    let mut k = (v * steps).round().clamp(0.0, steps) as i32;
                                    let e = ui
                                        .add(egui::Slider::new(&mut k, 0..=info.step_count))
                                        .changed();
                                    v = f64::from(k) / steps;
                                    e
                                } else {
                                    ui.add(egui::Slider::new(&mut v, 0.0..=1.0).fixed_decimals(3))
                                        .changed()
                                }
                            })
                            .inner;
                        if !info.units.is_empty() {
                            ui.label(muted(info.units.clone()));
                        }
                        ui.end_row();
                        if edited && v != cur {
                            r.set_param(info.id, v);
                            changed = true;
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
            let _ = std::thread::Builder::new()
                .name("ofx-params".into())
                .spawn(move || {
                    let res = neoveg_engine::plugin_host::default_host_exe()
                        .ok_or_else(|| "executável do Neoveg não encontrado".to_string())
                        .and_then(|exe| neoveg_engine::plugin_host::ofx_params(&exe, &r2))
                        .map(Arc::new);
                    map2.lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .insert((r2.binary, r2.plugin_id), Some(res));
                    ctx.request_repaint();
                });
            ui.label(muted("Abrindo o plug-in em processo isolado…".into()));
            return false;
        }
        Some(None) => {
            ui.label(muted("Abrindo o plug-in em processo isolado…".into()));
            return false;
        }
        Some(Some(Err(e))) => {
            ui.label(muted(format!("Plug-in indisponível: {e}")));
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
                    .on_hover_text("Criar keyframe neste tempo")
                    .clicked()
                    {
                        key_op = Some(true);
                    }
                    if has_keys > 0 {
                        ui.label(egui::RichText::new(format!("{has_keys}")).small());
                        if icons::small_icon_button(ui, Icon::Backspace, None, false, theme)
                            .on_hover_text("Remover keyframes")
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
    probe: ProbeFn<T>,
) {
    probes
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(bin.clone(), ProbeState::Running);
    let worker = probes.clone();
    let spawned = std::thread::Builder::new()
        .name("plugin-probe".into())
        .spawn(move || {
            let result = std::env::current_exe()
                .map_err(|e| format!("executável do Neoveg não encontrado: {e}"))
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

    pub(crate) fn name(self) -> &'static str {
        match self {
            PluginCategory::VideoFx => "Efeitos de vídeo",
            PluginCategory::AudioFx => "Efeitos de áudio",
            PluginCategory::Transition => "Transições",
            PluginCategory::Compositor => "Compositores",
        }
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
    pub(crate) fn name(self) -> &'static str {
        match self {
            PluginEntry::Fx(k) => k.name(),
            PluginEntry::Transition(k) => k.name(),
            PluginEntry::Compositor(b) => b.name(),
        }
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
pub(crate) fn plugin_catalog(cat: PluginCategory, filter: &str) -> Vec<PluginEntry> {
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
        .filter(|e| e.name().to_lowercase().contains(&f))
        .collect()
}

impl App {
    pub(crate) fn plugin_manager_tab(&mut self, ui: &mut egui::Ui) {
        let filter_key = egui::Id::new("plugin_manager_filter");
        let sel_key = egui::Id::new("plugin_manager_selected");
        let mut filter: String = ui.data(|d| d.get_temp(filter_key)).unwrap_or_default();
        let mut selected: Option<PluginEntry> = ui.data(|d| d.get_temp(sel_key));
        ui.horizontal(|ui| {
            ui.label("Pesquisar:");
            ui.add(egui::TextEdit::singleline(&mut filter).desired_width(160.0));
            if !filter.is_empty() && ui.small_button("×").clicked() {
                filter.clear();
            }
        });
        ui.separator();
        ui.columns(2, |cols| {
            egui::ScrollArea::vertical()
                .id_salt("plugin_tree")
                .show(&mut cols[0], |ui| {
                    for cat in PluginCategory::ALL {
                        let items = plugin_catalog(cat, &filter);
                        let header = format!("{} ({})", cat.name(), items.len());
                        egui::CollapsingHeader::new(header)
                            .id_salt(("plugin_cat", cat))
                            .default_open(!filter.is_empty())
                            .show(ui, |ui| {
                                for e in items {
                                    let id = egui::Id::new(("plugin_item", cat, e.name()));
                                    let payload = match e {
                                        PluginEntry::Fx(k) => Some(DndPayload::Fx(k)),
                                        PluginEntry::Transition(k) => {
                                            Some(DndPayload::Transition(k))
                                        }
                                        PluginEntry::Compositor(_) => None,
                                    };
                                    let is_sel = selected == Some(e);
                                    let resp = match payload {
                                        Some(p) => {
                                            ui.dnd_drag_source(id, p, |ui| {
                                                ui.selectable_label(is_sel, e.name())
                                            })
                                            .inner
                                        }
                                        None => ui.selectable_label(is_sel, e.name()),
                                    };
                                    if resp.clicked() {
                                        selected = Some(e);
                                    }
                                }
                            });
                    }
                    egui::CollapsingHeader::new("Plug-ins externos (VST3 / OFX)")
                        .id_salt("plugin_cat_external")
                        .show(ui, |ui| self.external_plugins_list(ui, &filter));
                });
            let ui = &mut cols[1];
            match selected {
                Some(e) => self.plugin_details(ui, e),
                None => {
                    ui.label(
                        egui::RichText::new(
                            "Selecione um plug-in para ver os detalhes. Arraste efeitos e \
                             transições até um evento ou trilha.",
                        )
                        .small()
                        .color(self.theme.text_muted),
                    );
                }
            }
        });
        ui.data_mut(|d| {
            d.insert_temp(filter_key, filter);
            match selected {
                Some(s) => d.insert_temp(sel_key, s),
                None => d.remove::<PluginEntry>(sel_key),
            }
        });
    }

    /// Plug-ins VST3/OFX encontrados nas pastas padrão. Binários OFX só são
    /// carregados sob pedido e sempre no subprocesso isolado.
    fn external_plugins_list(&mut self, ui: &mut egui::Ui, filter: &str) {
        let key = egui::Id::new("plugin_manager_external");
        let cached: Option<Arc<Vec<ExternalPlugin>>> = ui.data(|d| d.get_temp(key));
        let refresh = ui.small_button("Atualizar").clicked();
        let list = match (cached, refresh) {
            (Some(l), false) => l,
            _ => {
                let l = Arc::new(plugins::scan_default());
                ui.data_mut(|d| d.insert_temp(key, l.clone()));
                l
            }
        };
        let f = filter.trim().to_lowercase();
        let shown: Vec<&ExternalPlugin> = list
            .iter()
            .filter(|p| f.is_empty() || p.name.to_lowercase().contains(&f))
            .collect();
        if shown.is_empty() {
            ui.label(
                egui::RichText::new("Nenhum plug-in externo encontrado nas pastas padrão.")
                    .small()
                    .color(self.theme.text_muted),
            );
            return;
        }
        for p in shown {
            let mut tip = format!("{}\n{}", p.kind.name(), p.path.display());
            if let Some(v) = &p.vendor {
                tip.push_str(&format!("\nFabricante: {v}"));
            }
            if let Some(v) = &p.version {
                tip.push_str(&format!("\nVersão: {v}"));
            }
            for c in &p.classes {
                tip.push_str(&format!("\n• {c}"));
            }
            if p.binary.is_none() {
                tip.push_str("\nSem binário para esta arquitetura.");
            }
            let title = format!("{} [{}]", p.name, p.kind.name());
            match (&p.binary, p.kind) {
                (Some(bin), ExternalKind::Ofx) => {
                    egui::CollapsingHeader::new(title)
                        .id_salt(("plugin_ext", p.path.display().to_string()))
                        .show(ui, |ui| self.ofx_probe_view(ui, bin));
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
    }

    /// Lista os plug-ins de um binário OFX, sondado no subprocesso isolado
    /// `neoveg --plugin-host` numa thread de fundo (a UI nunca bloqueia).
    fn ofx_probe_view(&mut self, ui: &mut egui::Ui, bin: &Path) {
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
        match state {
            None => {
                if ui
                    .small_button("Listar plug-ins")
                    .on_hover_text("Carrega o binário num processo isolado e lista os efeitos dele")
                    .clicked()
                {
                    start_probe(
                        &probes,
                        bin.to_path_buf(),
                        ui.ctx().clone(),
                        probe_ofx_isolated,
                    );
                }
            }
            Some(ProbeState::Running) => {
                ui.label(
                    egui::RichText::new("Consultando o plug-in em processo isolado…")
                        .small()
                        .color(self.theme.text_muted),
                );
            }
            Some(ProbeState::Done(Err(e))) => {
                ui.label(
                    egui::RichText::new(format!("Erro: {e}"))
                        .small()
                        .color(self.theme.text_muted),
                );
            }
            Some(ProbeState::Done(Ok(list))) => {
                ui.label(
                    egui::RichText::new(format!("{} plug-in(s)", list.len()))
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
                                    .add_enabled(can_apply, egui::Button::new("Aplicar").small())
                                    .on_hover_text("Aplicar aos eventos de vídeo selecionados")
                                    .on_disabled_hover_text(
                                        "Selecione eventos de vídeo na timeline",
                                    )
                                    .clicked()
                                {
                                    self.apply_ofx_to_selection(bin, &info.identifier);
                                }
                                ui.label(egui::RichText::new(&info.identifier).small().monospace())
                                    .on_hover_text(format!(
                                        "{} v{} — versão {}.{}",
                                        info.api,
                                        info.api_version,
                                        info.version_major,
                                        info.version_minor
                                    ));
                            });
                        }
                    });
            }
        }
    }

    /// Lista as classes de efeito de um módulo VST3, sondado no subprocesso
    /// isolado numa thread de fundo, com *Aplicar* para a seleção de áudio.
    fn vst3_probe_view(&mut self, ui: &mut egui::Ui, bin: &Path) {
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
                    .small_button("Listar plug-ins")
                    .on_hover_text("Carrega o módulo num processo isolado e lista os efeitos dele")
                    .clicked()
                {
                    start_probe(
                        &probes,
                        bin.to_path_buf(),
                        ui.ctx().clone(),
                        probe_vst3_isolated,
                    );
                }
            }
            Some(ProbeState::Running) => {
                ui.label(muted("Consultando o plug-in em processo isolado…".into()));
            }
            Some(ProbeState::Done(Err(e))) => {
                ui.label(muted(format!("Erro: {e}")));
            }
            Some(ProbeState::Done(Ok(list))) => {
                if list.is_empty() {
                    ui.label(muted("Nenhum efeito de áudio neste módulo.".into()));
                }
                let can_apply = self.audio_fx_target().is_some();
                for info in list.iter() {
                    ui.horizontal(|ui| {
                        let instrument = info.is_instrument();
                        if ui
                            .add_enabled(
                                can_apply && !instrument,
                                egui::Button::new("Aplicar").small(),
                            )
                            .on_hover_text(
                                "Aplicar aos eventos de áudio selecionados (ou à trilha de \
                                 áudio selecionada)",
                            )
                            .on_disabled_hover_text(if instrument {
                                "Instrumentos precisam de MIDI, que ainda não é suportado"
                            } else {
                                "Selecione eventos ou uma trilha de áudio na timeline"
                            })
                            .clicked()
                        {
                            self.apply_vst3_to_selection(bin, info);
                        }
                        ui.label(egui::RichText::new(&info.name).small())
                            .on_hover_text(format!(
                                "{}\nVersão {}\nClasse {}",
                                info.sub_categories, info.version, info.class_id
                            ));
                    });
                }
            }
        }
    }

    fn plugin_details(&mut self, ui: &mut egui::Ui, e: PluginEntry) {
        icons::icon_label(ui, e.category().icon(), 16.0, e.name(), &self.theme);
        ui.label(
            egui::RichText::new(format!("Categoria: {}", e.category().name()))
                .small()
                .color(self.theme.text_muted),
        );
        ui.label(
            egui::RichText::new("Origem: nativo do Neoveg")
                .small()
                .color(self.theme.text_muted),
        );
        ui.separator();
        match e {
            PluginEntry::Fx(k) => {
                let params = k.params();
                if params.is_empty() {
                    ui.label("Sem parâmetros.");
                } else {
                    egui::Grid::new(("plugin_params", k as u32))
                        .num_columns(2)
                        .striped(true)
                        .show(ui, |ui| {
                            for p in params {
                                ui.label(p.name);
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{} … {} (padrão {})",
                                        p.min, p.max, p.default
                                    ))
                                    .small()
                                    .monospace(),
                                );
                                ui.end_row();
                            }
                        });
                }
            }
            PluginEntry::Transition(_) => {
                ui.label("Arraste até um evento de vídeo para usar no crossfade ou fade.");
            }
            PluginEntry::Compositor(b) => {
                let target = self.selected_video_track();
                if ui
                    .add_enabled(
                        target.is_some(),
                        egui::Button::new("Aplicar à trilha selecionada"),
                    )
                    .clicked()
                {
                    self.apply_compositor(b);
                }
            }
        }
    }

    pub(crate) fn compositors_tab(&mut self, ui: &mut egui::Ui) {
        let Some(ti) = self.selected_video_track() else {
            ui.label(
                egui::RichText::new(
                    "Selecione uma trilha de vídeo para escolher o modo de composição \
                     (Compositing Mode).",
                )
                .small()
                .color(self.theme.text_muted),
            );
            return;
        };
        let (name, current) = match self.project.tracks.get(ti) {
            Some(t) => (t.name.clone(), t.blend),
            None => return,
        };
        ui.label(egui::RichText::new(format!("Trilha: {name}")).strong());
        ui.separator();
        egui::Grid::new("compositors_grid")
            .num_columns(1)
            .striped(true)
            .show(ui, |ui| {
                for b in BlendMode::ALL {
                    if ui.selectable_label(current == b, b.name()).clicked() && current != b {
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
        self.set_status(format!(
            "Efeito OFX \"{label}\" aplicado a {} evento(s)",
            ids.len()
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
        let Some(target) = self.audio_fx_target() else {
            return;
        };
        let bin = bin.display().to_string();
        let make = || crate::model::FxInstance::new_vst3(&bin, &info.class_id, &info.name);
        self.checkpoint();
        let what = match target {
            Ok(ids) => {
                for id in &ids {
                    if let Some(ev) = self.project.event_mut(*id) {
                        ev.fx.push(make());
                    }
                }
                if let Some(first) = ids.first() {
                    self.dlg.fx_target = Some(super::FxTarget::Event(*first));
                }
                format!("{} evento(s)", ids.len())
            }
            Err(ti) => {
                let Some(t) = self.project.tracks.get_mut(ti) else {
                    return;
                };
                t.fx.push(make());
                let (id, name) = (t.id, t.name.clone());
                self.dlg.fx_target = Some(super::FxTarget::Track(id));
                format!("trilha \"{name}\"")
            }
        };
        self.changed();
        self.set_status(format!("Efeito VST3 \"{}\" aplicado a {what}", info.name));
    }

    /// Índice da trilha de vídeo selecionada, se houver.
    pub(crate) fn selected_video_track(&self) -> Option<usize> {
        self.sel_track
            .filter(|&t| self.project.tracks.get(t).map(|x| x.kind) == Some(TrackKind::Video))
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
