//! Janela Efeitos de Vídeo no formato do manual do VEGAS Pro 21 (p. 54):
//! categorias no alto (com Todos, Terceiros e Favoritos), busca, a lista de
//! efeitos da categoria à esquerda e as miniaturas dos presets do efeito
//! escolhido à direita. Arrastar o efeito ou a miniatura para um evento ou
//! trilha aplica o efeito; os botões aplicam à seleção, à trilha ou ao Master.

use crate::app::{App, DndPayload};
use eframe::egui;
use neoveg_common::{tr, tr_args, Language};
use neoveg_engine::engine::fx_preview::{
    frame_from_rgba, render_preview, render_preview_at, render_preview_on_frame, FX_PRESET_THUMB_H,
    FX_PRESET_THUMB_W, THUMB_H, THUMB_W,
};
use neoveg_engine::model::{FrameRate, FxCategory, FxInstance, FxKind, Id, Ticks, TrackKind};
use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

type EventPreviewKey = (Id, Id, i64);
const MAX_EVENT_FX_THUMBS: usize = 64;

/// Categoria escolhida no alto da janela (Efeitos de Vídeo e Geradores
/// de Mídia): as três especiais do manual e as categorias de cada janela.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum BrowserCat<C> {
    #[default]
    All,
    ThirdParty,
    Favorites,
    Cat(C),
}

/// Linha de categorias no alto da janela.
pub(crate) fn category_row<C: Copy + PartialEq>(
    ui: &mut egui::Ui,
    lang: Language,
    cat: &mut BrowserCat<C>,
    cats: &[(C, &str)],
) {
    ui.horizontal_wrapped(|ui| {
        ui.selectable_value(cat, BrowserCat::All, tr(lang, "Todos"));
        ui.selectable_value(cat, BrowserCat::ThirdParty, tr(lang, "Terceiros"));
        ui.selectable_value(cat, BrowserCat::Favorites, tr(lang, "Favoritos"));
        ui.separator();
        for (c, name) in cats {
            ui.selectable_value(cat, BrowserCat::Cat(*c), *name);
        }
    });
}

struct FxHoverAnim {
    kind: FxKind,
    preset_idx: usize,
    tex: egui::TextureHandle,
    started: std::time::Instant,
    bucket: u64,
}

/// Estado da janela Efeitos de Vídeo.
#[derive(Default)]
pub(crate) struct FxBrowser {
    pub(crate) cat: BrowserCat<FxCategory>,
    pub(crate) search: String,
    pub(crate) selected: Option<FxKind>,
    pub(crate) selected_preset: Option<usize>,
    /// Miniaturas já carregadas (uma textura por efeito, criada uma vez).
    thumbs: HashMap<FxKind, egui::TextureHandle>,
    user_thumbs: HashMap<(String, String), egui::TextureHandle>,
    /// Quadro do evento de vídeo selecionado (ou sob o cursor) usado como base
    /// das prévias de presets em *Efeitos de Vídeo*.
    event_base_key: Option<EventPreviewKey>,
    event_base: Option<Arc<neoveg_engine::engine::Frame>>,
    event_thumbs: HashMap<(FxKind, String), egui::TextureHandle>,
    hover_anim: Option<FxHoverAnim>,
    rx: Option<Receiver<(FxKind, egui::ColorImage)>>,
    started: bool,
    /// Miniaturas da janela Transições.
    pub(crate) transitions: TransitionThumbs,
    /// Miniaturas dos efeitos OFX de terceiros.
    pub(crate) ofx: OfxThumbs,
}

/// Miniaturas da janela Transições (índice em `TransitionKind::ALL`), geradas
/// uma vez fora da thread da interface; passar o mouse anima a transição.
#[derive(Default)]
pub(crate) struct TransitionThumbs {
    thumbs: HashMap<usize, egui::TextureHandle>,
    rx: Option<Receiver<(usize, egui::ColorImage)>>,
    started: bool,
    hover: Option<(usize, egui::TextureHandle, std::time::Instant, u64)>,
}

impl TransitionThumbs {
    fn start(&mut self) {
        if self.started {
            return;
        }
        self.started = true;
        let (tx, rx) = channel();
        self.rx = Some(rx);
        let spawned = crate::util::spawn("miniaturas-transicoes", move || {
            for (i, k) in neoveg_engine::model::TransitionKind::ALL
                .into_iter()
                .enumerate()
            {
                let img = crate::util::guard("miniatura de transição", || {
                    let f = neoveg_engine::engine::fx_preview::render_transition_preview(
                        k, 0.5, THUMB_W, THUMB_H,
                    );
                    egui::ColorImage::from_rgba_unmultiplied([f.w, f.h], &f.to_rgba8())
                });
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

    /// Miniatura da transição `i`: animada (progresso 0..1 em 1,5 s) quando
    /// `hovered`, senão a fixa no meio do progresso.
    pub(crate) fn texture(
        &mut self,
        ctx: &egui::Context,
        i: usize,
        hovered: bool,
    ) -> Option<egui::TextureHandle> {
        self.start();
        if let Some(rx) = &self.rx {
            while let Ok((k, img)) = rx.try_recv() {
                let tex =
                    ctx.load_texture(format!("tr_thumb_{k}"), img, egui::TextureOptions::LINEAR);
                self.thumbs.insert(k, tex);
            }
        }
        if self.thumbs.len() < neoveg_engine::model::TransitionKind::ALL.len() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        if !hovered {
            if self.hover.as_ref().is_some_and(|h| h.0 == i) {
                self.hover = None;
            }
            return self.thumbs.get(&i).cloned();
        }
        let kind = neoveg_engine::model::TransitionKind::ALL.get(i).copied()?;
        let now = std::time::Instant::now();
        let started = match &self.hover {
            Some((hi, _, at, _)) if *hi == i => *at,
            _ => now,
        };
        let secs = now.saturating_duration_since(started).as_secs_f32();
        let p = (secs / 1.5).fract();
        // ~15 quadros por segundo bastam para a miniatura
        let bucket = (secs * 15.0) as u64;
        let need = !matches!(&self.hover, Some((hi, _, _, b)) if *hi == i && *b == bucket);
        if need {
            let f = neoveg_engine::engine::fx_preview::render_transition_preview(
                kind, p, THUMB_W, THUMB_H,
            );
            let img = egui::ColorImage::from_rgba_unmultiplied([f.w, f.h], &f.to_rgba8());
            match &mut self.hover {
                Some((hi, tex, _, b)) if *hi == i => {
                    tex.set(img, egui::TextureOptions::LINEAR);
                    *b = bucket;
                }
                _ => {
                    let tex = ctx.load_texture("tr_thumb_hover", img, egui::TextureOptions::LINEAR);
                    self.hover = Some((i, tex, started, bucket));
                }
            }
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(66));
        self.hover.as_ref().map(|h| h.1.clone())
    }
}

/// Estado da miniatura de um efeito OFX de terceiros.
pub(crate) enum OfxThumb {
    Pending,
    Ready(egui::TextureHandle),
    Failed(String),
}

/// Miniaturas dos efeitos OFX de terceiros: renderizadas uma de cada vez no
/// host isolado de plug-ins, numa thread só, quando a categoria Terceiros é
/// aberta (carregamento sob demanda, §3.6). Um plug-in que trava ou falha
/// fica "sem prévia" e não derruba a janela (§0.6).
#[derive(Default)]
pub(crate) struct OfxThumbs {
    state: HashMap<(String, String), OfxThumb>,
    jobs: Option<std::sync::mpsc::Sender<(String, String)>>,
    rx: Option<Receiver<OfxThumbResult>>,
}

/// Resultado de uma miniatura OFX: (binário, id) e a imagem ou o erro.
type OfxThumbResult = ((String, String), Result<egui::ColorImage, String>);

/// Limite de miniaturas OFX guardadas (uma por plug-in consultado).
const MAX_OFX_THUMBS: usize = 512;

impl OfxThumbs {
    fn ensure_worker(&mut self) {
        if self.jobs.is_some() {
            return;
        }
        let (job_tx, job_rx) = channel::<(String, String)>();
        let (res_tx, res_rx) = channel();
        let spawned = crate::util::spawn("miniaturas-ofx", move || {
            while let Ok((bin, id)) = job_rx.recv() {
                let r = neoveg_engine::model::OfxFxRef {
                    binary: bin.clone(),
                    plugin_id: id.clone(),
                    label: String::new(),
                    params: Vec::new(),
                };
                let res = crate::util::guard("miniatura OFX", || {
                    neoveg_engine::engine::fx_preview::render_ofx_preview(&r, THUMB_W, THUMB_H).map(
                        |f| egui::ColorImage::from_rgba_unmultiplied([f.w, f.h], &f.to_rgba8()),
                    )
                })
                .unwrap_or_else(|_| Err("o plug-in travou ao gerar a prévia".into()));
                if res_tx.send(((bin, id), res)).is_err() {
                    return;
                }
            }
        });
        if spawned.is_ok() {
            self.jobs = Some(job_tx);
            self.rx = Some(res_rx);
        }
    }

    /// Estado da miniatura do plug-in (`binário`, `id`); pede a geração na
    /// primeira vez.
    pub(crate) fn get(&mut self, ctx: &egui::Context, bin: &str, id: &str) -> &OfxThumb {
        if let Some(rx) = &self.rx {
            while let Ok((key, res)) = rx.try_recv() {
                let st = match res {
                    Ok(img) => OfxThumb::Ready(ctx.load_texture(
                        format!("ofx_thumb_{}_{}", key.0, key.1),
                        img,
                        egui::TextureOptions::LINEAR,
                    )),
                    Err(e) => OfxThumb::Failed(e),
                };
                self.state.insert(key, st);
            }
        }
        let key = (bin.to_string(), id.to_string());
        if !self.state.contains_key(&key) {
            if self.state.len() >= MAX_OFX_THUMBS {
                self.state
                    .insert(key.clone(), OfxThumb::Failed("prévias demais".into()));
            } else {
                self.ensure_worker();
                let sent = self
                    .jobs
                    .as_ref()
                    .is_some_and(|tx| tx.send(key.clone()).is_ok());
                self.state.insert(
                    key.clone(),
                    if sent {
                        OfxThumb::Pending
                    } else {
                        OfxThumb::Failed("não foi possível gerar a prévia".into())
                    },
                );
            }
        }
        if self.state.values().any(|s| matches!(s, OfxThumb::Pending)) {
            ctx.request_repaint_after(std::time::Duration::from_millis(150));
        }
        match self.state.get(&key) {
            Some(s) => s,
            None => &OfxThumb::Pending,
        }
    }
}

/// Presets de um efeito (padrão + presets salvos pelo usuário nas Preferências).
pub(crate) fn presets(k: FxKind) -> Vec<(&'static str, FxInstance)> {
    vec![("Padrão", FxInstance::new(k))]
}

fn combined_presets(
    k: FxKind,
    user: &[neoveg_common::prefs::UserFxPresetPref],
) -> Vec<(String, FxInstance, bool)> {
    let mut out: Vec<(String, FxInstance, bool)> = presets(k)
        .into_iter()
        .map(|(n, fx)| (n.to_string(), fx, false))
        .collect();
    let kind_id = k.id();
    for p in user {
        if p.fx_kind == kind_id {
            let inst =
                neoveg_engine::session::EditorSession::instantiate_fx_with_preset(k, &p.values);
            out.push((p.name.clone(), inst, true));
        }
    }
    out
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

    /// Miniatura de prévia de um efeito de vídeo nativo (estática ou animada ao passar o mouse).
    /// Reutilizada no navegador *Efeitos de Vídeo*, no *Plug-In Manager* e no *Plug-In Chooser*.
    pub(crate) fn preview_texture(
        &mut self,
        ctx: &egui::Context,
        k: FxKind,
        hovered: bool,
    ) -> Option<egui::TextureHandle> {
        if k.is_audio() {
            return None;
        }
        self.start_thumbs();
        self.poll(ctx);
        if !hovered {
            return self.thumbs.get(&k).cloned();
        }
        let fx = FxInstance::new(k);
        let needs_new = self
            .hover_anim
            .as_ref()
            .is_none_or(|h| h.kind != k || h.preset_idx != usize::MAX);
        if needs_new {
            let f = render_preview_at(&fx, Ticks::ZERO, FrameRate::FPS_30, THUMB_W, THUMB_H);
            let img = egui::ColorImage::from_rgba_unmultiplied([f.w, f.h], &f.to_rgba8());
            let tex = ctx.load_texture(
                format!("fx_preview_hover_{}", k.id()),
                img,
                egui::TextureOptions::LINEAR,
            );
            self.hover_anim = Some(FxHoverAnim {
                kind: k,
                preset_idx: usize::MAX,
                tex,
                started: std::time::Instant::now(),
                bucket: 0,
            });
        }
        if let Some(h) = self.hover_anim.as_mut() {
            let bucket = (h.started.elapsed().as_millis() / 80) as u64;
            if bucket != h.bucket {
                h.bucket = bucket;
                let ms = ((bucket % 50) * 80) as i64;
                let f = render_preview_at(
                    &fx,
                    Ticks::from_millis_i64(ms),
                    FrameRate::FPS_30,
                    THUMB_W,
                    THUMB_H,
                );
                let img = egui::ColorImage::from_rgba_unmultiplied([f.w, f.h], &f.to_rgba8());
                h.tex.set(img, egui::TextureOptions::LINEAR);
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(80));
            return Some(h.tex.clone());
        }
        self.thumbs.get(&k).cloned()
    }

    fn matches(&self, k: FxKind, favs: &[String], q: &str, lang: Language) -> bool {
        let in_cat = match self.cat {
            BrowserCat::All => true,
            BrowserCat::ThirdParty => false,
            BrowserCat::Favorites => favs.contains(&fav_id(k)),
            BrowserCat::Cat(c) => k.category() == Some(c),
        };
        // a busca olha o nome e o grupo (manual, p. 55)
        in_cat
            && (q.is_empty()
                || k.name_in(lang).to_lowercase().contains(q)
                || k.name().to_lowercase().contains(q)
                || k.category().is_some_and(|c| {
                    c.name_in(lang).to_lowercase().contains(q)
                        || c.name().to_lowercase().contains(q)
                }))
    }
}

impl App {
    /// Atualiza o quadro-base das prévias de presets usando o evento de vídeo
    /// selecionado (ou o evento de vídeo sob o cursor). Quando não há evento de
    /// vídeo disponível, limpa a base para cair de volta na amostra sintética.
    fn sync_fx_event_base(&mut self) {
        let mut target: Option<(Id, Id, Ticks, Ticks, neoveg_engine::model::Event)> = None;
        for tr in &self.project.tracks {
            if tr.kind != TrackKind::Video {
                continue;
            }
            for e in &tr.events {
                if self.sel_events.contains(&e.id)
                    && self.project.media(e.media_id).is_some_and(|m| m.has_video)
                {
                    let better = target
                        .as_ref()
                        .is_none_or(|(_, _, prev_start, _, _)| e.start < *prev_start);
                    if better {
                        target = Some((e.id, e.media_id, e.start, e.length, e.clone()));
                    }
                }
            }
        }
        if target.is_none() {
            let c = self.cursor;
            for tr in &self.project.tracks {
                if tr.kind != TrackKind::Video || tr.mute {
                    continue;
                }
                if let Some(e) = tr.events.iter().find(|e| {
                    !e.mute
                        && e.start <= c
                        && c < e.end()
                        && self.project.media(e.media_id).is_some_and(|m| m.has_video)
                }) {
                    target = Some((e.id, e.media_id, e.start, e.length, e.clone()));
                    break;
                }
            }
        }
        let Some((ev_id, media_id, ev_start, ev_len, ev)) = target else {
            if self.fx_browser.event_base.is_some() || self.fx_browser.event_base_key.is_some() {
                self.fx_browser.event_base = None;
                self.fx_browser.event_base_key = None;
                self.fx_browser.event_thumbs.clear();
                self.fx_browser.hover_anim = None;
            }
            return;
        };
        let Some(m) = self.project.media(media_id).cloned() else {
            return;
        };
        let tl_t = if self.cursor >= ev_start && self.cursor < ev_start + ev_len {
            self.cursor
        } else {
            ev_start + ev_len.div_i64(2)
        };
        let media_t = ev.media_time(tl_t, m.duration).unwrap_or(Ticks::ZERO);
        let fps = m.fps.unwrap_or(self.project.settings.fps);
        let frame_idx = fps.ticks_to_frame_floor(media_t);
        let key: EventPreviewKey = (ev_id, media_id, frame_idx);
        if self.fx_browser.event_base_key == Some(key) && self.fx_browser.event_base.is_some() {
            return;
        }
        if let Some(img) = self.media.video_frame_for_quality(
            &m,
            media_t,
            FX_PRESET_THUMB_W as u32,
            FX_PRESET_THUMB_H as u32,
            true,
        ) {
            let base = frame_from_rgba(&img, FX_PRESET_THUMB_W, FX_PRESET_THUMB_H);
            self.fx_browser.event_base = Some(Arc::new(base));
            self.fx_browser.event_base_key = Some(key);
            self.fx_browser.event_thumbs.clear();
            self.fx_browser.hover_anim = None;
        } else if self
            .fx_browser
            .event_base_key
            .is_some_and(|(_, prev_mid, _)| prev_mid != media_id)
        {
            self.fx_browser.event_base = None;
            self.fx_browser.event_base_key = None;
            self.fx_browser.event_thumbs.clear();
            self.fx_browser.hover_anim = None;
        }
    }

    pub(crate) fn video_fx_browser(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang();
        self.fx_browser.start_thumbs();
        self.fx_browser.poll(ui.ctx());
        self.sync_fx_event_base();

        let cats: Vec<(FxCategory, &str)> = FxCategory::ALL
            .iter()
            .map(|c| (*c, c.name_in(lang)))
            .collect();
        category_row(ui, lang, &mut self.fx_browser.cat, &cats);
        ui.add(
            egui::TextEdit::singleline(&mut self.fx_browser.search)
                .hint_text(tr(lang, "Buscar plug-ins"))
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
            .filter(|k| {
                self.fx_browser
                    .matches(*k, &self.prefs.fx_favorites, &q, lang)
            })
            .collect();
        if !self.fx_browser.selected.is_some_and(|k| list.contains(&k)) {
            self.fx_browser.selected = list.first().copied();
        }
        if list.is_empty() {
            ui.label(
                egui::RichText::new(if self.fx_browser.cat == BrowserCat::Favorites {
                    tr(
                        lang,
                        "Nenhum favorito. Clique na estrela de um efeito para marcá-lo.",
                    )
                } else {
                    tr(lang, "Nenhum efeito encontrado.")
                })
                .small()
                .weak(),
            );
            return;
        }

        let left_w = (ui.available_width() * 0.38).clamp(140.0, 240.0);
        // o resto da largura visível, para a fileira de miniaturas quebrar
        // de linha (a aba fica numa área com rolagem horizontal, onde a
        // largura disponível não tem limite)
        let visible_w = (ui.clip_rect().right() - ui.cursor().left()).min(ui.available_width());
        let right_w = (visible_w - left_w - 24.0).max(FX_PRESET_THUMB_W as f32 + 8.0);
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
                                        tr(lang, "Tirar dos favoritos")
                                    } else {
                                        tr(lang, "Marcar como favorito")
                                    })
                                    .clicked()
                                {
                                    toggle_fav = Some(*k);
                                }
                                let sel = self.fx_browser.selected == Some(*k);
                                if ui.selectable_label(sel, k.name_in(lang)).clicked() {
                                    select = Some(*k);
                                }
                            });
                        }
                    });
            });
            ui.separator();
            // direita: as miniaturas dos presets do efeito escolhido (mesmo fluxo de Geradores de Mídia)
            ui.vertical(|ui| {
                ui.set_max_width(right_w);
                let Some(k) = self.fx_browser.selected else {
                    return;
                };
                let all_presets = combined_presets(k, &self.prefs.user_fx_presets);
                let chosen_idx = self
                    .fx_browser
                    .selected_preset
                    .filter(|&i| i < all_presets.len())
                    .unwrap_or(0);
                let (widget_bg, accent) = (self.theme.widget_bg, self.theme.accent);
                let mut pick_preset: Option<usize> = None;
                let mut del_user_preset: Option<String> = None;
                let mut any_hovered = false;
                ui.strong(tr_args(
                    lang,
                    "Presets: {name}",
                    &[("name", &k.name_in(lang))],
                ));
                let event_base = self.fx_browser.event_base.clone();
                let fps = self.project.settings.fps;
                let per_row = ((right_w + 8.0) / (FX_PRESET_THUMB_W as f32 + 24.0))
                    .floor()
                    .max(1.0) as usize;
                let idxs: Vec<usize> = (0..all_presets.len()).collect();
                for row in idxs.chunks(per_row) {
                    ui.horizontal(|ui| {
                        for &i in row {
                            let Some((name, fx, is_user)) = all_presets.get(i) else {
                                continue;
                            };
                            let cache_key = (k.id().to_string(), name.clone());
                            let ev_cache_key = (k, name.clone());
                            if let Some(base) = event_base.as_ref() {
                                if !self.fx_browser.event_thumbs.contains_key(&ev_cache_key) {
                                    if self.fx_browser.event_thumbs.len() >= MAX_EVENT_FX_THUMBS {
                                        self.fx_browser.event_thumbs.clear();
                                    }
                                    if let Ok(img) =
                                        crate::util::guard("miniatura de efeito no evento", || {
                                            let f =
                                                render_preview_on_frame(base, fx, Ticks::ZERO, fps);
                                            egui::ColorImage::from_rgba_unmultiplied(
                                                [f.w, f.h],
                                                &f.to_rgba8(),
                                            )
                                        })
                                    {
                                        let tex = ui.ctx().load_texture(
                                            format!("fx_ev_thumb_{}_{}", k.id(), name),
                                            img,
                                            egui::TextureOptions::LINEAR,
                                        );
                                        self.fx_browser
                                            .event_thumbs
                                            .insert(ev_cache_key.clone(), tex);
                                    }
                                }
                            } else if *is_user
                                && !self.fx_browser.user_thumbs.contains_key(&cache_key)
                            {
                                let f = render_preview(fx, FX_PRESET_THUMB_W, FX_PRESET_THUMB_H);
                                let img = egui::ColorImage::from_rgba_unmultiplied(
                                    [f.w, f.h],
                                    &f.to_rgba8(),
                                );
                                let tex = ui.ctx().load_texture(
                                    format!("fx_user_thumb_{}_{}", k.id(), name),
                                    img,
                                    egui::TextureOptions::LINEAR,
                                );
                                self.fx_browser.user_thumbs.insert(cache_key.clone(), tex);
                            }
                            let chosen = chosen_idx == i;
                            let r = ui.dnd_drag_source(
                                egui::Id::new(("fxb_preset", k as u32, i)),
                                DndPayload::FxPreset(fx.clone()),
                                |ui| {
                                    ui.vertical(|ui| {
                                        let size = egui::vec2(
                                            FX_PRESET_THUMB_W as f32,
                                            FX_PRESET_THUMB_H as f32,
                                        );
                                        let (rect, _) =
                                            ui.allocate_exact_size(size, egui::Sense::hover());
                                        let hovered = ui.rect_contains_pointer(rect);
                                        if hovered && ui.input(|inp| inp.pointer.primary_clicked())
                                        {
                                            pick_preset = Some(i);
                                        }
                                        let tex = if hovered {
                                            any_hovered = true;
                                            let needs_new =
                                                self.fx_browser.hover_anim.as_ref().is_none_or(
                                                    |h| h.kind != k || h.preset_idx != i,
                                                );
                                            if needs_new {
                                                let f = match event_base.as_ref() {
                                                    Some(base) => render_preview_on_frame(
                                                        base,
                                                        fx,
                                                        Ticks::ZERO,
                                                        fps,
                                                    ),
                                                    None => render_preview_at(
                                                        fx,
                                                        Ticks::ZERO,
                                                        FrameRate::FPS_30,
                                                        FX_PRESET_THUMB_W,
                                                        FX_PRESET_THUMB_H,
                                                    ),
                                                };
                                                let img = egui::ColorImage::from_rgba_unmultiplied(
                                                    [f.w, f.h],
                                                    &f.to_rgba8(),
                                                );
                                                let tex = ui.ctx().load_texture(
                                                    format!("fx_hover_anim_{}_{i}", k.id()),
                                                    img,
                                                    egui::TextureOptions::LINEAR,
                                                );
                                                self.fx_browser.hover_anim = Some(FxHoverAnim {
                                                    kind: k,
                                                    preset_idx: i,
                                                    tex,
                                                    started: std::time::Instant::now(),
                                                    bucket: 0,
                                                });
                                            }
                                            if let Some(h) = self.fx_browser.hover_anim.as_mut() {
                                                let bucket =
                                                    (h.started.elapsed().as_millis() / 80) as u64;
                                                if bucket != h.bucket {
                                                    h.bucket = bucket;
                                                    let ms = ((bucket % 50) * 80) as i64;
                                                    let f = match event_base.as_ref() {
                                                        Some(base) => render_preview_on_frame(
                                                            base,
                                                            fx,
                                                            Ticks::from_millis_i64(ms),
                                                            fps,
                                                        ),
                                                        None => render_preview_at(
                                                            fx,
                                                            Ticks::from_millis_i64(ms),
                                                            FrameRate::FPS_30,
                                                            FX_PRESET_THUMB_W,
                                                            FX_PRESET_THUMB_H,
                                                        ),
                                                    };
                                                    let img =
                                                        egui::ColorImage::from_rgba_unmultiplied(
                                                            [f.w, f.h],
                                                            &f.to_rgba8(),
                                                        );
                                                    h.tex.set(img, egui::TextureOptions::LINEAR);
                                                }
                                                ui.ctx().request_repaint_after(
                                                    std::time::Duration::from_millis(80),
                                                );
                                                Some(h.tex.clone())
                                            } else {
                                                None
                                            }
                                        } else if event_base.is_some() {
                                            self.fx_browser.event_thumbs.get(&ev_cache_key).cloned()
                                        } else if *is_user {
                                            self.fx_browser.user_thumbs.get(&cache_key).cloned()
                                        } else {
                                            self.fx_browser.thumbs.get(&k).cloned()
                                        };
                                        match &tex {
                                            Some(t) => {
                                                egui::Image::new((t.id(), size)).paint_at(ui, rect);
                                            }
                                            None => {
                                                ui.painter().rect_filled(rect, 2.0, widget_bg);
                                            }
                                        }
                                        if chosen {
                                            ui.painter().rect_stroke(
                                                rect,
                                                2.0,
                                                egui::Stroke::new(2.0_f32, accent),
                                                egui::StrokeKind::Outside,
                                            );
                                        }
                                        ui.horizontal(|ui| {
                                            let lbl_r = ui.selectable_label(
                                                chosen,
                                                egui::RichText::new(tr(lang, name)).small(),
                                            );
                                            if *is_user
                                                && ui
                                                    .small_button("×")
                                                    .on_hover_text(tr(lang, "Excluir preset salvo"))
                                                    .clicked()
                                            {
                                                del_user_preset = Some(name.clone());
                                            }
                                            lbl_r
                                        })
                                        .inner
                                    })
                                    .inner
                                },
                            );
                            if r.inner.clicked() {
                                pick_preset = Some(i);
                            }
                            r.response.on_hover_text(tr(
                                lang,
                                "Arraste para um evento ou trilha de vídeo",
                            ));
                        }
                    });
                }
                if !any_hovered {
                    self.fx_browser.hover_anim = None;
                }
                if let Some(i) = pick_preset {
                    self.fx_browser.selected_preset = Some(i);
                }
                if let Some(del_name) = del_user_preset {
                    let kind_id = k.id();
                    self.prefs
                        .user_fx_presets
                        .retain(|p| !(p.fx_kind == kind_id && p.name == del_name));
                    self.fx_browser
                        .user_thumbs
                        .remove(&(kind_id.to_string(), del_name.clone()));
                    self.fx_browser.event_thumbs.remove(&(k, del_name));
                    self.fx_browser.selected_preset = Some(0);
                    let _ = self.prefs.save();
                }
                ui.separator();
                ui.label(egui::RichText::new(tr(lang, "Aplicar a:")).small().weak());
                let active_fx = all_presets
                    .get(chosen_idx)
                    .map(|(_, fx, _)| fx.clone())
                    .unwrap_or_else(|| FxInstance::new(k));
                ui.horizontal(|ui| self.fx_apply_instance_buttons(ui, active_fx, false));
            });
        });
        if let Some(k) = select {
            self.fx_browser.selected = Some(k);
            self.fx_browser.selected_preset = Some(0);
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
        let lang = self.lang();
        let q = self.fx_browser.search.trim().to_lowercase();
        let list: Vec<_> = crate::plugin_manager::probed_ofx_plugins(ui.ctx())
            .into_iter()
            .filter(|(_, info)| q.is_empty() || info.identifier.to_lowercase().contains(&q))
            .collect();
        if list.is_empty() {
            ui.label(
                egui::RichText::new(tr(
                    lang,
                    "Nenhum plug-in OFX consultado. Os plug-ins instalados aparecem no \
                     Plug-In Manager; consulte-os lá para usá-los aqui.",
                ))
                .small()
                .weak(),
            );
            if ui.button(tr(lang, "Abrir o Plug-In Manager")).clicked() {
                let ctx = ui.ctx().clone();
                self.exec_cmd(Some(&ctx), "view.tab.plugin_manager");
            }
            return;
        }
        let can_apply = !self.selected_video_events().is_empty();
        let thumb_sz = egui::vec2(THUMB_W as f32, THUMB_H as f32);
        egui::ScrollArea::vertical()
            .id_salt("fxb_terceiros")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (bin, info) in &list {
                    ui.horizontal(|ui| {
                        // prévia no quadro de amostra, gerada no host isolado
                        let bin_s = bin.to_string_lossy().to_string();
                        let ctx = ui.ctx().clone();
                        match self.fx_browser.ofx.get(&ctx, &bin_s, &info.identifier) {
                            OfxThumb::Ready(tex) => {
                                ui.image((tex.id(), thumb_sz));
                            }
                            OfxThumb::Pending => {
                                let (r, _) = ui.allocate_exact_size(thumb_sz, egui::Sense::hover());
                                ui.painter().rect_filled(r, 2.0, self.theme.extreme_bg);
                                ui.painter().text(
                                    r.center(),
                                    egui::Align2::CENTER_CENTER,
                                    tr(lang, "Gerando prévia..."),
                                    egui::FontId::proportional(9.0),
                                    self.theme.text_muted,
                                );
                            }
                            OfxThumb::Failed(e) => {
                                let e = e.clone();
                                let (r, resp) =
                                    ui.allocate_exact_size(thumb_sz, egui::Sense::hover());
                                ui.painter().rect_filled(r, 2.0, self.theme.extreme_bg);
                                ui.painter().text(
                                    r.center(),
                                    egui::Align2::CENTER_CENTER,
                                    tr(lang, "sem prévia"),
                                    egui::FontId::proportional(9.0),
                                    self.theme.text_muted,
                                );
                                resp.on_hover_text(e);
                            }
                        }
                        ui.vertical(|ui| {
                            ui.label(egui::RichText::new(&info.identifier).small());
                            if ui
                                .add_enabled(
                                    can_apply,
                                    egui::Button::new(tr(lang, "Aplicar")).small(),
                                )
                                .on_disabled_hover_text(tr(
                                    lang,
                                    "Selecione eventos de vídeo na timeline",
                                ))
                                .clicked()
                            {
                                self.apply_ofx_to_selection(bin, &info.identifier);
                            }
                        });
                    });
                }
            });
    }
}

#[cfg(test)]
mod fx_event_preview_tests {
    use super::*;
    use crate::model::GenSpec;

    #[test]
    fn previa_de_efeitos_usa_quadro_do_evento_selecionado_e_limpa_sem_evento() {
        let _lock = crate::kittest_tests::lock_test_prefs();
        let ctx = egui::Context::default();
        let mut app = {
            let _r = crate::tests::PREFS_LOCK
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            App::with_context(&ctx, None)
        };
        app.sync_fx_event_base();
        assert!(app.fx_browser.event_base.is_none());

        app.insert_generated(GenSpec::SolidColor {
            color: [0.85, 0.20, 0.15, 1.0],
        });
        app.sync_fx_event_base();
        let base = app
            .fx_browser
            .event_base
            .as_ref()
            .expect("evento de vídeo selecionado deve preencher a base da prévia");
        assert_eq!((base.w, base.h), (FX_PRESET_THUMB_W, FX_PRESET_THUMB_H));
        // O pixel central reflete o vermelho sólido do evento, não o céu azul da amostra
        let mid = base.px[(FX_PRESET_THUMB_H / 2) * FX_PRESET_THUMB_W + (FX_PRESET_THUMB_W / 2)];
        assert!(mid[0] > 0.7 && mid[1] < 0.35 && mid[2] < 0.35, "{mid:?}");

        // Remover o evento limpa a base e volta à amostra padrão
        app.project.tracks[0].events.clear();
        app.sel_events.clear();
        app.sync_fx_event_base();
        assert!(app.fx_browser.event_base.is_none());
    }
}
