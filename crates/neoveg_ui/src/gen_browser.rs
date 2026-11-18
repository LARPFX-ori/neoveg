//! Janela Geradores de Mídia no formato do manual do VEGAS Pro 21 (p. 56):
//! categorias no alto (com Todos, Terceiros e Favoritos), busca, os tipos de
//! gerador à esquerda e as miniaturas dos presets do tipo escolhido à
//! direita. Arrastar uma miniatura para a timeline cria o evento; o botão
//! insere no cursor.

use crate::app::{App, DndPayload};
use crate::fx_browser::{category_row, BrowserCat};
use eframe::egui;
use neoveg_engine::engine::fx_preview::{THUMB_H, THUMB_W};
use neoveg_engine::media::generate;
use neoveg_engine::model::{GenSpec, TextAlign, TextAnim, TextSpec};
use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver};

/// Categoria de um tipo de gerador (nomes do Neoveg).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum GenCategory {
    Text,
    Backgrounds,
    Utility,
}

impl GenCategory {
    pub(crate) const ALL: [GenCategory; 3] = [
        GenCategory::Text,
        GenCategory::Backgrounds,
        GenCategory::Utility,
    ];

    pub(crate) fn name(self) -> &'static str {
        match self {
            GenCategory::Text => "Texto e títulos",
            GenCategory::Backgrounds => "Fundos e cores",
            GenCategory::Utility => "Utilitários",
        }
    }
}

/// Tipo de gerador: a lista da esquerda.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum GenKind {
    Text,
    SolidColor,
    Gradient,
    Checkerboard,
    ColorBars,
    Noise,
}

impl GenKind {
    pub(crate) const ALL: [GenKind; 6] = [
        GenKind::Text,
        GenKind::SolidColor,
        GenKind::Gradient,
        GenKind::Checkerboard,
        GenKind::ColorBars,
        GenKind::Noise,
    ];

    /// O tipo de um gerador (exaustivo: um tipo novo de `GenSpec` obriga a
    /// escolher o lugar dele aqui).
    pub(crate) fn of(spec: &GenSpec) -> Self {
        match spec {
            GenSpec::Text(_) => GenKind::Text,
            GenSpec::SolidColor { .. } => GenKind::SolidColor,
            GenSpec::Gradient { .. } => GenKind::Gradient,
            GenSpec::Checkerboard { .. } => GenKind::Checkerboard,
            GenSpec::ColorBars => GenKind::ColorBars,
            GenSpec::Noise { .. } => GenKind::Noise,
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            GenKind::Text => "Texto",
            GenKind::SolidColor => "Cor sólida",
            GenKind::Gradient => "Gradiente",
            GenKind::Checkerboard => "Quadriculado",
            GenKind::ColorBars => "Barras de teste",
            GenKind::Noise => "Ruído",
        }
    }

    pub(crate) fn category(self) -> GenCategory {
        match self {
            GenKind::Text => GenCategory::Text,
            GenKind::SolidColor | GenKind::Gradient => GenCategory::Backgrounds,
            GenKind::Checkerboard | GenKind::ColorBars | GenKind::Noise => GenCategory::Utility,
        }
    }

    fn fav_id(self) -> String {
        format!("{self:?}")
    }
}

/// Presets dos geradores (os mesmos que a aba já oferecia, agora agrupados
/// pelo tipo na lista da esquerda).
pub(crate) fn gen_presets() -> Vec<(&'static str, GenSpec)> {
    vec![
        ("Texto simples", GenSpec::Text(TextSpec::default())),
        (
            "Título com contorno",
            GenSpec::Text(TextSpec {
                text: "TÍTULO".into(),
                size: 16.0,
                outline: 0.6,
                outline_color: [0.0, 0.0, 0.0, 1.0],
                color: [1.0, 0.85, 0.2, 1.0],
                ..TextSpec::default()
            }),
        ),
        (
            "Terço inferior (lower third)",
            GenSpec::Text(TextSpec {
                text: "Nome Sobrenome\nCargo ou descrição".into(),
                size: 5.5,
                align: TextAlign::Left,
                x: 0.3,
                y: 0.82,
                background: [0.0, 0.0, 0.0, 0.0],
                ..TextSpec::default()
            }),
        ),
        (
            "Créditos rolando",
            GenSpec::Text(TextSpec {
                text: "Direção\nFulano de Tal\n\nEdição\nNeoveg\n\nObrigado por assistir".into(),
                size: 6.0,
                anim: TextAnim::ScrollUp,
                anim_duration: 10.0,
                ..TextSpec::default()
            }),
        ),
        (
            "Letreiro (crawl)",
            GenSpec::Text(TextSpec {
                text: "Notícia urgente: texto passando na parte inferior da tela".into(),
                size: 5.0,
                y: 0.9,
                anim: TextAnim::CrawlLeft,
                anim_duration: 8.0,
                background: [0.0, 0.0, 0.0, 0.0],
                ..TextSpec::default()
            }),
        ),
        (
            "Máquina de escrever",
            GenSpec::Text(TextSpec {
                text: "Era uma vez...".into(),
                anim: TextAnim::Typewriter,
                anim_duration: 3.0,
                ..TextSpec::default()
            }),
        ),
        (
            "Cor sólida (preto)",
            GenSpec::SolidColor {
                color: [0.0, 0.0, 0.0, 1.0],
            },
        ),
        (
            "Cor sólida (branco)",
            GenSpec::SolidColor {
                color: [1.0, 1.0, 1.0, 1.0],
            },
        ),
        (
            "Gradiente linear",
            GenSpec::Gradient {
                a: [0.05, 0.1, 0.35, 1.0],
                b: [0.6, 0.2, 0.5, 1.0],
                angle: 90.0,
                radial: false,
            },
        ),
        (
            "Gradiente radial",
            GenSpec::Gradient {
                a: [1.0, 0.9, 0.6, 1.0],
                b: [0.3, 0.05, 0.0, 1.0],
                angle: 0.0,
                radial: true,
            },
        ),
        (
            "Xadrez",
            GenSpec::Checkerboard {
                a: [0.85, 0.85, 0.85, 1.0],
                b: [0.35, 0.35, 0.35, 1.0],
                cells: 9,
            },
        ),
        ("Barras de cor", GenSpec::ColorBars),
        (
            "Ruído (estático)",
            GenSpec::Noise {
                mono: true,
                animated: true,
            },
        ),
    ]
}

/// Instante mostrado na miniatura: no meio da animação, para o texto
/// animado aparecer (no início, os créditos e o letreiro ainda estão fora).
fn thumb_time(spec: &GenSpec) -> f64 {
    match spec {
        GenSpec::Text(t) if t.anim != TextAnim::None => f64::from(t.anim_duration.max(0.0)) * 0.5,
        _ => 0.0,
    }
}

/// Miniatura de um preset (RGBA 8 bits), usada pela thread e pelos testes.
pub(crate) fn render_thumb(spec: &GenSpec, fonts: &generate::Fonts) -> egui::ColorImage {
    let img = generate::render(
        spec,
        thumb_time(spec),
        THUMB_W as u32,
        THUMB_H as u32,
        fonts,
    );
    let size = [img.width() as usize, img.height() as usize];
    egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw())
}

/// Estado da janela Geradores de Mídia.
#[derive(Default)]
pub(crate) struct GenBrowser {
    pub(crate) cat: BrowserCat<GenCategory>,
    pub(crate) search: String,
    pub(crate) selected: Option<GenKind>,
    /// Preset escolhido (índice em `gen_presets`).
    pub(crate) preset: Option<usize>,
    thumbs: HashMap<usize, egui::TextureHandle>,
    hover_fonts: Option<generate::Fonts>,
    hover_anim: Option<(usize, egui::TextureHandle, std::time::Instant, u64)>,
    rx: Option<Receiver<(usize, egui::ColorImage)>>,
    started: bool,
}

impl GenBrowser {
    /// Gera as miniaturas uma vez, fora da thread da interface (§3.6); um
    /// pânico num gerador fica isolado (§0.6).
    fn start_thumbs(&mut self) {
        if self.started {
            return;
        }
        self.started = true;
        let (tx, rx) = channel();
        self.rx = Some(rx);
        let spawned = crate::util::spawn("miniaturas-geradores", move || {
            let fonts = generate::Fonts::default();
            for (i, (_, spec)) in gen_presets().into_iter().enumerate() {
                let img =
                    crate::util::guard("miniatura de gerador", || render_thumb(&spec, &fonts));
                if let Ok(img) = img {
                    if tx.send((i, img)).is_err() {
                        return;
                    }
                }
            }
        });
        if spawned.is_err() {
            self.rx = None;
        }
    }

    fn poll(&mut self, ctx: &egui::Context, total: usize) {
        let Some(rx) = &self.rx else {
            return;
        };
        let mut got = Vec::new();
        while let Ok(x) = rx.try_recv() {
            got.push(x);
        }
        for (i, img) in got {
            let tex = ctx.load_texture(format!("gen_thumb_{i}"), img, egui::TextureOptions::LINEAR);
            self.thumbs.insert(i, tex);
        }
        if self.thumbs.len() < total {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }
}

impl App {
    pub(crate) fn generators_browser(&mut self, ui: &mut egui::Ui) {
        let presets = gen_presets();
        self.gen_browser.start_thumbs();
        self.gen_browser.poll(ui.ctx(), presets.len());

        let cats: Vec<(GenCategory, &str)> =
            GenCategory::ALL.iter().map(|c| (*c, c.name())).collect();
        category_row(ui, &mut self.gen_browser.cat, &cats);
        ui.add(
            egui::TextEdit::singleline(&mut self.gen_browser.search)
                .hint_text("Buscar plug-ins")
                .desired_width(220.0),
        );
        ui.separator();

        if self.gen_browser.cat == BrowserCat::ThirdParty {
            ui.label(
                egui::RichText::new(
                    "Nenhum gerador de terceiros: geradores de mídia OFX ainda não são suportados.",
                )
                .small()
                .weak(),
            );
            return;
        }

        let q = self.gen_browser.search.trim().to_lowercase();
        let favs = &self.prefs.gen_favorites;
        let cat = self.gen_browser.cat;
        // a busca olha o tipo, o grupo e os nomes dos presets (manual, p. 56)
        let list: Vec<GenKind> = GenKind::ALL
            .into_iter()
            .filter(|k| match cat {
                BrowserCat::All => true,
                BrowserCat::ThirdParty => false,
                BrowserCat::Favorites => favs.contains(&k.fav_id()),
                BrowserCat::Cat(c) => k.category() == c,
            })
            .filter(|k| {
                q.is_empty()
                    || k.name().to_lowercase().contains(&q)
                    || k.category().name().to_lowercase().contains(&q)
                    || presets
                        .iter()
                        .any(|(n, s)| GenKind::of(s) == *k && n.to_lowercase().contains(&q))
            })
            .collect();
        if !self.gen_browser.selected.is_some_and(|k| list.contains(&k)) {
            self.gen_browser.selected = list.first().copied();
        }
        if list.is_empty() {
            ui.label(
                egui::RichText::new(if cat == BrowserCat::Favorites {
                    "Nenhum favorito. Clique na estrela de um gerador para marcá-lo."
                } else {
                    "Nenhum gerador encontrado."
                })
                .small()
                .weak(),
            );
            return;
        }
        let Some(kind) = self.gen_browser.selected else {
            return;
        };
        // o preset escolhido precisa ser do tipo escolhido
        if !self
            .gen_browser
            .preset
            .and_then(|i| presets.get(i))
            .is_some_and(|(_, s)| GenKind::of(s) == kind)
        {
            self.gen_browser.preset = presets.iter().position(|(_, s)| GenKind::of(s) == kind);
        }

        let left_w = (ui.available_width() * 0.32).clamp(130.0, 200.0);
        // o resto da largura visível, para a fileira de miniaturas quebrar
        // de linha (a aba fica numa área com rolagem horizontal, onde a
        // largura disponível não tem limite)
        let visible_w = (ui.clip_rect().right() - ui.cursor().left()).min(ui.available_width());
        let right_w = (visible_w - left_w - 24.0).max(THUMB_W as f32 + 8.0);
        let mut toggle_fav: Option<GenKind> = None;
        let mut select: Option<GenKind> = None;
        let mut pick: Option<usize> = None;
        let mut insert: Option<GenSpec> = None;
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(left_w);
                egui::ScrollArea::vertical()
                    .id_salt("genb_lista")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for k in &list {
                            let fav = self.prefs.gen_favorites.contains(&k.fav_id());
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
                                if ui.selectable_label(kind == *k, k.name()).clicked() {
                                    select = Some(*k);
                                }
                            });
                        }
                    });
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.set_max_width(right_w);
                ui.strong(format!("Presets: {}", kind.name()));
                // linhas montadas aqui: o `horizontal_wrapped` do egui não
                // quebra antes de grupos (miniatura + nome) de tamanho
                // desconhecido
                let idxs: Vec<usize> = presets
                    .iter()
                    .enumerate()
                    .filter(|(_, (_, s))| GenKind::of(s) == kind)
                    .map(|(i, _)| i)
                    .collect();
                let per_row = ((right_w + 8.0) / (THUMB_W as f32 + 24.0)).floor().max(1.0) as usize;
                let (widget_bg, accent) = (self.theme.widget_bg, self.theme.accent);
                let mut any_hovered = false;
                for row in idxs.chunks(per_row) {
                    ui.horizontal(|ui| {
                        for &i in row {
                            let Some((name, spec)) = presets.get(i) else {
                                continue;
                            };
                            let chosen = self.gen_browser.preset == Some(i);
                            let static_thumb = self.gen_browser.thumbs.get(&i).cloned();
                            let r = ui.dnd_drag_source(
                                egui::Id::new(("genb_preset", i)),
                                DndPayload::Generator(spec.clone()),
                                |ui| {
                                    ui.vertical(|ui| {
                                        let size = egui::vec2(THUMB_W as f32, THUMB_H as f32);
                                        let (r, _) =
                                            ui.allocate_exact_size(size, egui::Sense::hover());
                                        // fundo escuro: texto e áreas transparentes
                                        // somem no fundo do painel sem ele
                                        ui.painter().rect_filled(
                                            r,
                                            2.0,
                                            egui::Color32::from_gray(16),
                                        );
                                        let thumb = if ui.rect_contains_pointer(r) {
                                            any_hovered = true;
                                            let fonts = self
                                                .gen_browser
                                                .hover_fonts
                                                .get_or_insert_with(generate::Fonts::default);
                                            let needs_new = self
                                                .gen_browser
                                                .hover_anim
                                                .as_ref()
                                                .is_none_or(|(pi, _, _, _)| *pi != i);
                                            if needs_new {
                                                let img_raw = generate::render(
                                                    spec,
                                                    0.0,
                                                    THUMB_W as u32,
                                                    THUMB_H as u32,
                                                    fonts,
                                                );
                                                let img = egui::ColorImage::from_rgba_unmultiplied(
                                                    [
                                                        img_raw.width() as usize,
                                                        img_raw.height() as usize,
                                                    ],
                                                    img_raw.as_raw(),
                                                );
                                                let tex = ui.ctx().load_texture(
                                                    format!("gen_hover_anim_{i}"),
                                                    img,
                                                    egui::TextureOptions::LINEAR,
                                                );
                                                self.gen_browser.hover_anim =
                                                    Some((i, tex, std::time::Instant::now(), 0));
                                            }
                                            if let Some((_, tex, started, prev_bucket)) =
                                                self.gen_browser.hover_anim.as_mut()
                                            {
                                                let bucket =
                                                    (started.elapsed().as_millis() / 80) as u64;
                                                if bucket != *prev_bucket {
                                                    *prev_bucket = bucket;
                                                    let t = ((bucket % 60) as f64) * 0.1;
                                                    let img_raw = generate::render(
                                                        spec,
                                                        t,
                                                        THUMB_W as u32,
                                                        THUMB_H as u32,
                                                        fonts,
                                                    );
                                                    let img =
                                                        egui::ColorImage::from_rgba_unmultiplied(
                                                            [
                                                                img_raw.width() as usize,
                                                                img_raw.height() as usize,
                                                            ],
                                                            img_raw.as_raw(),
                                                        );
                                                    tex.set(img, egui::TextureOptions::LINEAR);
                                                }
                                                ui.ctx().request_repaint_after(
                                                    std::time::Duration::from_millis(80),
                                                );
                                                Some(tex.clone())
                                            } else {
                                                static_thumb.clone()
                                            }
                                        } else {
                                            static_thumb.clone()
                                        };
                                        match &thumb {
                                            Some(t) => {
                                                egui::Image::new((t.id(), size)).paint_at(ui, r);
                                            }
                                            None => {
                                                ui.painter().rect_filled(r, 2.0, widget_bg);
                                            }
                                        }
                                        if chosen {
                                            ui.painter().rect_stroke(
                                                r,
                                                2.0,
                                                egui::Stroke::new(2.0_f32, accent),
                                                egui::StrokeKind::Outside,
                                            );
                                        }
                                        ui.selectable_label(
                                            chosen,
                                            egui::RichText::new(*name).small(),
                                        )
                                    })
                                    .inner
                                },
                            );
                            if r.inner.clicked() {
                                pick = Some(i);
                            }
                            r.response.on_hover_text("Arraste para a timeline");
                        }
                    });
                }
                if !any_hovered {
                    self.gen_browser.hover_anim = None;
                }
                ui.separator();
                let cur = self.gen_browser.preset.and_then(|i| presets.get(i));
                if ui
                    .add_enabled(cur.is_some(), egui::Button::new("Inserir no cursor"))
                    .clicked()
                {
                    insert = cur.map(|(_, s)| s.clone());
                }
            });
        });
        if let Some(k) = select {
            self.gen_browser.selected = Some(k);
        }
        if let Some(i) = pick {
            self.gen_browser.preset = Some(i);
        }
        if let Some(k) = toggle_fav {
            let id = k.fav_id();
            if let Some(pos) = self.prefs.gen_favorites.iter().position(|x| *x == id) {
                self.prefs.gen_favorites.remove(pos);
            } else {
                self.prefs.gen_favorites.push(id);
            }
            let _ = self.prefs.save();
        }
        if let Some(spec) = insert {
            self.insert_generated(spec);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn todo_preset_de_gerador_tem_tipo_e_miniatura_do_tamanho_certo() {
        let fonts = generate::Fonts::default();
        for (name, spec) in gen_presets() {
            let img = render_thumb(&spec, &fonts);
            assert_eq!(img.size, [THUMB_W, THUMB_H], "{name}");
            assert!(GenKind::ALL.contains(&GenKind::of(&spec)), "{name}");
        }
        // todo tipo tem ao menos um preset e uma categoria com tipos
        for k in GenKind::ALL {
            assert!(
                gen_presets().iter().any(|(_, s)| GenKind::of(s) == k),
                "{} sem preset",
                k.name()
            );
        }
        for c in GenCategory::ALL {
            assert!(
                GenKind::ALL.iter().any(|k| k.category() == c),
                "{}",
                c.name()
            );
        }
    }

    #[test]
    fn texto_animado_aparece_no_meio_da_animacao() {
        let spec = GenSpec::Text(TextSpec {
            anim: TextAnim::ScrollUp,
            anim_duration: 10.0,
            ..TextSpec::default()
        });
        assert!((thumb_time(&spec) - 5.0).abs() < 1e-9);
        assert_eq!(thumb_time(&GenSpec::ColorBars), 0.0);
    }
}
