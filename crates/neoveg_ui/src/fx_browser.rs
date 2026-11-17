//! Janela Efeitos de Vídeo no formato do manual do VEGAS Pro 21 (p. 54):
//! categorias no alto (com Todos, Terceiros e Favoritos), busca, a lista de
//! efeitos da categoria à esquerda e as miniaturas dos presets do efeito
//! escolhido à direita. Arrastar o efeito ou a miniatura para um evento ou
//! trilha aplica o efeito; os botões aplicam à seleção, à trilha ou ao Master.

use crate::app::{App, DndPayload};
use eframe::egui;
use neoveg_engine::engine::fx_preview::{render_preview, THUMB_H, THUMB_W};
use neoveg_engine::model::{FxCategory, FxInstance, FxKind};
use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver};

/// Categoria escolhida no alto da janela.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum BrowserCat {
    #[default]
    All,
    ThirdParty,
    Favorites,
    Cat(FxCategory),
}

/// Estado da janela Efeitos de Vídeo.
#[derive(Default)]
pub(crate) struct FxBrowser {
    pub(crate) cat: BrowserCat,
    pub(crate) search: String,
    pub(crate) selected: Option<FxKind>,
    /// Miniaturas já carregadas (uma textura por efeito, criada uma vez).
    thumbs: HashMap<FxKind, egui::TextureHandle>,
    rx: Option<Receiver<(FxKind, egui::ColorImage)>>,
    started: bool,
}

/// Presets de um efeito. Por enquanto um por efeito, com os valores padrão;
/// a lista deixa espaço para presets próprios do Neoveg depois.
pub(crate) fn presets(k: FxKind) -> Vec<(&'static str, FxInstance)> {
    vec![("Padrão", FxInstance::new(k))]
}

/// Id estável do efeito na lista de favoritos das Preferências.
pub(crate) fn fav_id(k: FxKind) -> String {
    format!("{k:?}")
}

impl FxBrowser {
    /// Gera as miniaturas uma vez, fora da thread da interface (§3.6); um
    /// pânico de um efeito fica isolado e só aquela miniatura falta (§0.6).
    fn start_thumbs(&mut self) {
        if self.started {
            return;
        }
        self.started = true;
        let (tx, rx) = channel();
        self.rx = Some(rx);
        let spawned = crate::util::spawn("miniaturas-efeitos", move || {
            for k in FxKind::VIDEO {
                let img = crate::util::guard("miniatura de efeito", || {
                    let f = render_preview(&FxInstance::new(k), THUMB_W, THUMB_H);
                    egui::ColorImage::from_rgba_unmultiplied([f.w, f.h], &f.to_rgba8())
                });
                if let Ok(img) = img {
                    if tx.send((k, img)).is_err() {
                        return;
                    }
                }
            }
        });
        if spawned.is_err() {
            self.rx = None;
        }
    }

    /// Carrega como textura as miniaturas que chegaram.
    fn poll(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.rx else {
            return;
        };
        let mut got = Vec::new();
        while let Ok(x) = rx.try_recv() {
            got.push(x);
        }
        for (k, img) in got {
            let tex =
                ctx.load_texture(format!("fx_thumb_{k:?}"), img, egui::TextureOptions::LINEAR);
            self.thumbs.insert(k, tex);
        }
        if self.thumbs.len() < FxKind::VIDEO.len() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    fn matches(&self, k: FxKind, favs: &[String], q: &str) -> bool {
        let in_cat = match self.cat {
            BrowserCat::All => true,
            BrowserCat::ThirdParty => false,
            BrowserCat::Favorites => favs.contains(&fav_id(k)),
            BrowserCat::Cat(c) => k.category() == Some(c),
        };
        // a busca olha o nome e o grupo (manual, p. 55)
        in_cat
            && (q.is_empty()
                || k.name().to_lowercase().contains(q)
                || k.category()
                    .is_some_and(|c| c.name().to_lowercase().contains(q)))
    }
}

impl App {
    pub(crate) fn video_fx_browser(&mut self, ui: &mut egui::Ui) {
        self.fx_browser.start_thumbs();
        self.fx_browser.poll(ui.ctx());

        ui.horizontal_wrapped(|ui| {
            let mut cat = self.fx_browser.cat;
            ui.selectable_value(&mut cat, BrowserCat::All, "Todos");
            ui.selectable_value(&mut cat, BrowserCat::ThirdParty, "Terceiros");
            ui.selectable_value(&mut cat, BrowserCat::Favorites, "Favoritos");
            ui.separator();
            for c in FxCategory::ALL {
                ui.selectable_value(&mut cat, BrowserCat::Cat(c), c.name());
            }
            self.fx_browser.cat = cat;
        });
        ui.add(
            egui::TextEdit::singleline(&mut self.fx_browser.search)
                .hint_text("Buscar plug-ins")
                .desired_width(220.0),
        );
        ui.separator();

        if self.fx_browser.cat == BrowserCat::ThirdParty {
            self.third_party_fx(ui);
            return;
        }

        let q = self.fx_browser.search.trim().to_lowercase();
        let list: Vec<FxKind> = FxKind::VIDEO
            .into_iter()
            .filter(|k| self.fx_browser.matches(*k, &self.prefs.fx_favorites, &q))
            .collect();
        if !self.fx_browser.selected.is_some_and(|k| list.contains(&k)) {
            self.fx_browser.selected = list.first().copied();
        }
        if list.is_empty() {
            ui.label(
                egui::RichText::new(if self.fx_browser.cat == BrowserCat::Favorites {
                    "Nenhum favorito. Clique na estrela de um efeito para marcá-lo."
                } else {
                    "Nenhum efeito encontrado."
                })
                .small()
                .weak(),
            );
            return;
        }

        let left_w = (ui.available_width() * 0.38).clamp(140.0, 240.0);
        let mut toggle_fav: Option<FxKind> = None;
        let mut select: Option<FxKind> = None;
        ui.horizontal_top(|ui| {
            // esquerda: os efeitos da categoria
            ui.vertical(|ui| {
                ui.set_width(left_w);
                egui::ScrollArea::vertical()
                    .id_salt("fxb_lista")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for k in &list {
                            let fav = self.prefs.fx_favorites.contains(&fav_id(*k));
                            ui.horizontal(|ui| {
                                if ui
                                    .selectable_label(fav, if fav { "★" } else { "☆" })
                                    .on_hover_text(if fav {
                                        "Tirar dos favoritos"
                                    } else {
                                        "Marcar como favorito"
                                    })
                                    .clicked()
                                {
                                    toggle_fav = Some(*k);
                                }
                                let sel = self.fx_browser.selected == Some(*k);
                                let r = ui.dnd_drag_source(
                                    egui::Id::new(("fxb_item", *k as u32)),
                                    DndPayload::Fx(*k),
                                    |ui| ui.selectable_label(sel, k.name()),
                                );
                                if r.inner.clicked() {
                                    select = Some(*k);
                                }
                            });
                        }
                    });
            });
            ui.separator();
            // direita: as miniaturas dos presets do efeito escolhido
            ui.vertical(|ui| {
                let Some(k) = self.fx_browser.selected else {
                    return;
                };
                ui.strong(format!("Presets: {}", k.name()));
                ui.horizontal_wrapped(|ui| {
                    for (i, (name, _fx)) in presets(k).into_iter().enumerate() {
                        let thumb = self.fx_browser.thumbs.get(&k).cloned();
                        ui.dnd_drag_source(
                            egui::Id::new(("fxb_preset", k as u32, i)),
                            DndPayload::Fx(k),
                            |ui| {
                                ui.vertical(|ui| {
                                    let size = egui::vec2(THUMB_W as f32, THUMB_H as f32);
                                    match &thumb {
                                        Some(t) => {
                                            ui.add(egui::Image::new((t.id(), size)));
                                        }
                                        None => {
                                            let (r, _) =
                                                ui.allocate_exact_size(size, egui::Sense::hover());
                                            ui.painter().rect_filled(r, 2.0, self.theme.widget_bg);
                                        }
                                    }
                                    ui.label(egui::RichText::new(name).small());
                                });
                            },
                        )
                        .response
                        .on_hover_text("Arraste para um evento ou trilha de vídeo");
                    }
                });
                ui.separator();
                ui.label(egui::RichText::new("Aplicar a:").small().weak());
                ui.horizontal(|ui| self.fx_apply_buttons(ui, k, false));
            });
        });
        if let Some(k) = select {
            self.fx_browser.selected = Some(k);
        }
        if let Some(k) = toggle_fav {
            let id = fav_id(k);
            if let Some(pos) = self.prefs.fx_favorites.iter().position(|x| *x == id) {
                self.prefs.fx_favorites.remove(pos);
            } else {
                self.prefs.fx_favorites.push(id);
            }
            let _ = self.prefs.save();
        }
    }

    /// Categoria Terceiros: os plug-ins OFX já consultados no Plug-In
    /// Manager. A consulta roda num processo isolado e não é refeita aqui.
    fn third_party_fx(&mut self, ui: &mut egui::Ui) {
        let q = self.fx_browser.search.trim().to_lowercase();
        let list: Vec<_> = crate::plugin_manager::probed_ofx_plugins(ui.ctx())
            .into_iter()
            .filter(|(_, info)| q.is_empty() || info.identifier.to_lowercase().contains(&q))
            .collect();
        if list.is_empty() {
            ui.label(
                egui::RichText::new(
                    "Nenhum plug-in OFX consultado. Os plug-ins instalados aparecem no \
                     Plug-In Manager; consulte-os lá para usá-los aqui.",
                )
                .small()
                .weak(),
            );
            if ui.button("Abrir o Plug-In Manager").clicked() {
                let ctx = ui.ctx().clone();
                self.exec_cmd(Some(&ctx), "view.tab.plugin_manager");
            }
            return;
        }
        let can_apply = !self.selected_video_events().is_empty();
        egui::ScrollArea::vertical()
            .id_salt("fxb_terceiros")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (bin, info) in &list {
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(can_apply, egui::Button::new("Aplicar").small())
                            .on_disabled_hover_text("Selecione eventos de vídeo na timeline")
                            .clicked()
                        {
                            self.apply_ofx_to_selection(bin, &info.identifier);
                        }
                        ui.label(egui::RichText::new(&info.identifier).small());
                    });
                }
            });
    }
}
