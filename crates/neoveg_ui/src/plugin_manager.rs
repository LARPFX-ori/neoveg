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
use neoveg_engine::plugins::{self, ExternalPlugin};
use std::sync::Arc;

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

    /// Plug-ins VST3/OFX encontrados nas pastas padrão. Só lista: nenhum
    /// binário é carregado (a hospedagem isolada ainda não existe).
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
            tip.push_str("\nHospedagem de plug-ins externos ainda não disponível.");
            ui.add_enabled(
                false,
                egui::Label::new(format!("{} [{}]", p.name, p.kind.name())),
            )
            .on_disabled_hover_text(tip);
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

    /// Índice da trilha de vídeo selecionada, se houver.
    fn selected_video_track(&self) -> Option<usize> {
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
