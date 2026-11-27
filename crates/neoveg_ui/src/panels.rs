//! Área de janelas acopláveis (Window Docking Area) e janela de prévia de vídeo.

use super::timeline::draw_meter;
use super::{App, DndPayload, DockTab, FxTarget};
use crate::app::{LoadMsg, LoudnessJob};
use crate::icons::{self, Icon};
use crate::model::*;
use eframe::egui;
use egui::{pos2, vec2, Color32, Rect, Sense, Stroke, StrokeKind, Vec2};
use neoveg_common::{tr, tr_args, Language};
use neoveg_engine::engine::loudness::{
    analyze_project_loudness, format_db, format_lufs, LoudnessSnapshot, LoudnessTarget,
};
use neoveg_engine::util;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

/// Filtro de categoria de canais na barra lateral do Console de Mixagem (manual VEGAS Pro 21, p. 63).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum MixerChannelFilter {
    #[default]
    All,
    AudioTracks,
    AudioBuses,
    MasterOnly,
}

impl MixerChannelFilter {
    pub const ALL: [Self; 4] = [
        Self::All,
        Self::AudioTracks,
        Self::AudioBuses,
        Self::MasterOnly,
    ];

    pub fn label_in(self, lang: Language) -> &'static str {
        let key = match self {
            Self::All => "Todas",
            Self::AudioTracks => "Trilhas",
            Self::AudioBuses => "Barramentos",
            Self::MasterOnly => "Master",
        };
        tr(lang, key)
    }
}

/// Proporção de tela da prévia: largura/altura do projeto e, com *Simular
/// proporção do dispositivo*, corrigida pela proporção do pixel (PAR).
/// Valores inválidos caem para pixels quadrados (§0.4).
pub(crate) fn preview_display_aspect(w: u32, h: u32, par: f32, simulate: bool) -> f32 {
    let base = w.max(1) as f32 / h.max(1) as f32;
    if simulate && par.is_finite() && par > 0.0 {
        base * par.clamp(0.1, 4.0)
    } else {
        base
    }
}

/// Estado persistente da interface do Console de Mixagem (barra lateral, visibilidade
/// individual de faixas/barramentos, seções de tira e largura dos canais, p. 63).
pub struct MixerConsoleState {
    pub sidebar_open: bool,
    pub channel_filter: MixerChannelFilter,
    pub hidden_tracks: HashSet<Id>,
    pub hidden_buses: HashSet<Id>,
    pub hide_master: bool,
    pub show_io: bool,
    pub show_sends: bool,
    pub show_automation: bool,
    pub show_pan: bool,
    pub show_meters: bool,
    pub show_faders: bool,
    pub strip_width: f32,
    /// Área do painel e o quadro (`cumulative_pass_nr`) em que foi
    /// desenhado na janela principal: com o mouse sobre ela, as teclas do
    /// Mixer não chegam à timeline. Vale só a do quadro anterior (a aba pode
    /// ficar escondida atrás de outra).
    pub kb_rect: Option<(Rect, u64)>,
}

impl Default for MixerConsoleState {
    fn default() -> Self {
        Self {
            sidebar_open: true,
            channel_filter: MixerChannelFilter::All,
            hidden_tracks: HashSet::new(),
            hidden_buses: HashSet::new(),
            hide_master: false,
            show_io: true,
            show_sends: true,
            show_automation: true,
            show_pan: true,
            show_meters: true,
            show_faders: true,
            strip_width: 112.0,
            kb_rect: None,
        }
    }
}

/// Ação de uma tecla do Console de Mixagem.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MixerKey {
    ChannelList,
    AllStrips,
    BusStrips,
    TrackStrips,
    MasterStrip,
    Faders,
    Io,
    Meters,
    Sends,
    WidthDefault,
    WidthNarrow,
    WidthWide,
}

/// Larguras das tiras para D (padrão), N (estreita) e W (larga).
const STRIP_DEFAULT: f32 = 112.0;
const STRIP_NARROW: f32 = 84.0;
const STRIP_WIDE: f32 = 156.0;

/// Tecla → ação, como na lista de atalhos do VEGAS Pro 21 ("Mixing
/// Console"): Shift+C, Q, U, A, T, F, H, M, S e D/N/W. Com Ctrl ou Alt não
/// é tecla do Mixer. As que o Neoveg ainda não tem (E, P, I, Shift+T,
/// Shift+L, Shift+H) não são tratadas.
pub(crate) fn mixer_key(ctrl: bool, shift: bool, alt: bool, key: egui::Key) -> Option<MixerKey> {
    use egui::Key as K;
    if ctrl || alt {
        return None;
    }
    Some(match (shift, key) {
        (true, K::C) => MixerKey::ChannelList,
        (false, K::Q) => MixerKey::AllStrips,
        (false, K::U) => MixerKey::BusStrips,
        (false, K::A) => MixerKey::TrackStrips,
        (false, K::T) => MixerKey::MasterStrip,
        (false, K::F) => MixerKey::Faders,
        (false, K::H) => MixerKey::Io,
        (false, K::M) => MixerKey::Meters,
        (false, K::S) => MixerKey::Sends,
        (false, K::D) => MixerKey::WidthDefault,
        (false, K::N) => MixerKey::WidthNarrow,
        (false, K::W) => MixerKey::WidthWide,
        _ => return None,
    })
}

/// Com o mouse sobre o Mixer (`rect`), alguma tecla deste quadro é dele.
pub(crate) fn mixer_owns_keys(ctx: &egui::Context, rect: Option<(Rect, u64)>) -> bool {
    let Some((rect, pass)) = rect else {
        return false;
    };
    if pass.saturating_add(1) < ctx.cumulative_pass_nr() {
        return false;
    }
    ctx.input(|i| {
        i.pointer.hover_pos().is_some_and(|p| rect.contains(p))
            && i.events.iter().any(|e| {
                matches!(e, egui::Event::Key { key, pressed: true, modifiers, .. }
                    if mixer_key(modifiers.command || modifiers.ctrl, modifiers.shift, modifiers.alt, *key).is_some())
            })
    })
}

/// Alterna a visibilidade de um grupo de tiras: mostra todas se todas
/// estão ocultas, senão oculta todas.
fn toggle_group(hidden: &mut HashSet<Id>, ids: &[Id]) {
    if !ids.is_empty() && ids.iter().all(|id| hidden.contains(id)) {
        for id in ids {
            hidden.remove(id);
        }
    } else {
        hidden.extend(ids.iter().copied());
    }
}

impl MixerConsoleState {
    /// Aplica uma tecla do Mixer. `tracks` e `buses` são os ids das faixas de
    /// áudio e dos barramentos do projeto.
    pub(crate) fn apply_key(&mut self, k: MixerKey, tracks: &[Id], buses: &[Id]) {
        match k {
            MixerKey::ChannelList => self.sidebar_open = !self.sidebar_open,
            MixerKey::AllStrips => {
                let all_hidden = self.hide_master
                    && tracks.iter().all(|id| self.hidden_tracks.contains(id))
                    && buses.iter().all(|id| self.hidden_buses.contains(id));
                if all_hidden {
                    self.show_all_channels();
                } else {
                    self.hidden_tracks.extend(tracks.iter().copied());
                    self.hidden_buses.extend(buses.iter().copied());
                    self.hide_master = true;
                }
            }
            MixerKey::BusStrips => toggle_group(&mut self.hidden_buses, buses),
            MixerKey::TrackStrips => toggle_group(&mut self.hidden_tracks, tracks),
            MixerKey::MasterStrip => self.hide_master = !self.hide_master,
            MixerKey::Faders => self.show_faders = !self.show_faders,
            MixerKey::Io => self.show_io = !self.show_io,
            MixerKey::Meters => self.show_meters = !self.show_meters,
            MixerKey::Sends => self.show_sends = !self.show_sends,
            MixerKey::WidthDefault => self.strip_width = STRIP_DEFAULT,
            MixerKey::WidthNarrow => self.strip_width = STRIP_NARROW,
            MixerKey::WidthWide => self.strip_width = STRIP_WIDE,
        }
    }

    pub fn show_all_channels(&mut self) {
        self.hidden_tracks.clear();
        self.hidden_buses.clear();
        self.hide_master = false;
        self.channel_filter = MixerChannelFilter::All;
    }

    pub fn clamped_strip_width(&self) -> f32 {
        if self.strip_width.is_finite() {
            self.strip_width.clamp(84.0, 156.0)
        } else {
            112.0
        }
    }
}

const MAX_EXPLORER_HISTORY: usize = 64;
const MAX_EXPLORER_FAVORITES: usize = 64;

/// Coluna de ordenação no painel Explorer (manual VEGAS Pro 21, p. 45).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ExplorerSortCol {
    #[default]
    Name,
    Kind,
    Size,
}

/// Estado de navegação do Explorer: histórico voltar/avançar, favoritos, modo
/// Lista/Detalhes e painel lateral de pastas (manual VEGAS Pro 21, p. 45).
pub struct ExplorerState {
    pub back_stack: Vec<PathBuf>,
    pub forward_stack: Vec<PathBuf>,
    pub favorites: Vec<PathBuf>,
    pub show_tree: bool,
    pub details_view: bool,
    pub editing_path: bool,
    pub path_buf: String,
    pub sort_col: ExplorerSortCol,
    pub sort_asc: bool,
    pub selected: Option<PathBuf>,
}

impl Default for ExplorerState {
    fn default() -> Self {
        Self {
            back_stack: Vec::new(),
            forward_stack: Vec::new(),
            favorites: Vec::new(),
            show_tree: true,
            details_view: true,
            editing_path: false,
            path_buf: String::new(),
            sort_col: ExplorerSortCol::Name,
            sort_asc: true,
            selected: None,
        }
    }
}

impl ExplorerState {
    pub fn navigate(&mut self, current: &mut PathBuf, next: PathBuf) {
        if *current == next {
            return;
        }
        if self.back_stack.len() >= MAX_EXPLORER_HISTORY {
            self.back_stack.remove(0);
        }
        self.back_stack.push(current.clone());
        self.forward_stack.clear();
        *current = next;
        self.editing_path = false;
    }

    pub fn go_back(&mut self, current: &mut PathBuf) -> bool {
        let Some(prev) = self.back_stack.pop() else {
            return false;
        };
        if self.forward_stack.len() >= MAX_EXPLORER_HISTORY {
            self.forward_stack.remove(0);
        }
        self.forward_stack.push(current.clone());
        *current = prev;
        self.editing_path = false;
        true
    }

    pub fn go_forward(&mut self, current: &mut PathBuf) -> bool {
        let Some(next) = self.forward_stack.pop() else {
            return false;
        };
        if self.back_stack.len() >= MAX_EXPLORER_HISTORY {
            self.back_stack.remove(0);
        }
        self.back_stack.push(current.clone());
        *current = next;
        self.editing_path = false;
        true
    }

    pub fn toggle_favorite(&mut self, dir: &std::path::Path) -> bool {
        if let Some(pos) = self.favorites.iter().position(|p| p == dir) {
            self.favorites.remove(pos);
            false
        } else if self.favorites.len() < MAX_EXPLORER_FAVORITES {
            self.favorites.push(dir.to_path_buf());
            true
        } else {
            false
        }
    }
}

/// Modo de disposição dos Escopos de Vídeo (manual VEGAS Pro 21, p. 61).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ScopeViewLayout {
    #[default]
    All,
    Vectorscope,
    Waveform,
    Histogram,
    RgbParade,
}

impl ScopeViewLayout {
    pub const ALL: [Self; 5] = [
        Self::All,
        Self::Vectorscope,
        Self::Waveform,
        Self::Histogram,
        Self::RgbParade,
    ];

    pub fn label_in(self, lang: Language) -> &'static str {
        let key = match self {
            Self::All => "Todos (2×2)",
            Self::Vectorscope => "Vectorscope",
            Self::Waveform => "Forma de onda",
            Self::Histogram => "Histograma",
            Self::RgbParade => "RGB Parade",
        };
        tr(lang, key)
    }
}

/// Formata um tamanho em bytes de forma legível para o Explorer.
pub(crate) fn format_file_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * 1024;
    const GB: u64 = 1024 * 1024 * 1024;
    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{} KB", bytes / KB)
    } else {
        format!("{bytes} B")
    }
}

struct FloatingViewportSpec<'a> {
    title: &'a str,
    inner_size: [f32; 2],
    min_size: [f32; 2],
    icon: std::sync::Arc<egui::IconData>,
    theme: &'a crate::theme::ThemeColors,
    lang: Language,
    /// Mostra o botão Acoplar (falso nos painéis só de janela).
    can_dock: bool,
    /// Posição e tamanho com que a janela abre (fixos enquanto ela está
    /// aberta: o egui só reenvia o que muda no `ViewportBuilder`, então a
    /// janela não briga com o usuário quando ele a move).
    geom: Option<crate::layout::FloatingGeom>,
}

struct FloatingOutcome {
    open: bool,
    reattach: bool,
    /// Posição e tamanho reais da janela neste quadro (só janela nativa).
    geom: Option<crate::layout::FloatingGeom>,
    /// `Ctrl` apertado na janela (para acoplar ou não ao soltar).
    ctrl: bool,
}

enum FloatDockAction {
    None,
    DockToMain,
    MergeInto(String),
}

fn show_floating_viewport(
    ctx: &egui::Context,
    id_salt: impl std::hash::Hash,
    win_id_salt: impl std::hash::Hash,
    spec: &FloatingViewportSpec<'_>,
    mut body: impl FnMut(&mut egui::Ui),
) -> FloatingOutcome {
    let mut out = FloatingOutcome {
        open: true,
        reattach: false,
        geom: None,
        ctrl: false,
    };
    let vid = egui::ViewportId::from_hash_of(&id_salt);
    let builder = egui::ViewportBuilder::default()
        .with_title(format!("{} - Neoveg", spec.title))
        .with_min_inner_size(spec.min_size)
        .with_resizable(true)
        .with_decorations(true)
        .with_icon(spec.icon.clone());
    let builder = match spec.geom {
        Some([x, y, w, h]) => builder.with_position([x, y]).with_inner_size([w, h]),
        None => builder.with_inner_size(spec.inner_size),
    };
    ctx.show_viewport_immediate(vid, builder, |v_ctx, class| {
        if class == egui::ViewportClass::Embedded {
            egui::Window::new(spec.title)
                .id(egui::Id::new(&win_id_salt))
                .open(&mut out.open)
                .default_size([spec.inner_size[0] - 40.0, spec.inner_size[1] - 40.0])
                .min_size([spec.min_size[0] - 20.0, spec.min_size[1] - 20.0])
                .show(v_ctx, |ui| {
                    if spec.can_dock {
                        ui.horizontal(|ui| {
                            if icons::small_icon_button(
                                ui,
                                Icon::DockIn,
                                Some(tr(spec.lang, "Acoplar na área de janelas")),
                                false,
                                spec.theme,
                            )
                            .clicked()
                            {
                                out.reattach = true;
                            }
                        });
                        ui.separator();
                    }
                    body(ui);
                });
        } else {
            if v_ctx.input(|i| i.viewport().close_requested()) {
                out.open = false;
            }
            let (outer, inner) = v_ctx.input(|i| (i.viewport().outer_rect, i.viewport().inner_rect));
            out.ctrl = v_ctx.input(|i| i.modifiers.ctrl);
            if let (Some(o), Some(r)) = (outer, inner) {
                out.geom = crate::layout::sanitize_floating_geom([
                    o.min.x,
                    o.min.y,
                    r.width(),
                    r.height(),
                ]);
            }
            egui::CentralPanel::default().show(v_ctx, |ui| {
                if spec.can_dock {
                    ui.horizontal(|ui| {
                        if icons::small_icon_button(
                            ui,
                            Icon::DockIn,
                            Some(tr(spec.lang, "Acoplar na área de janelas")),
                            false,
                            spec.theme,
                        )
                        .clicked()
                        {
                            out.reattach = true;
                        }
                        ui.label(
                            egui::RichText::new(tr(
                                spec.lang,
                                "Janela independente (arraste pela barra de título para mover entre monitores)",
                            ))
                            .small()
                            .weak(),
                        );
                    });
                    ui.separator();
                }
                body(ui);
            });
        }
    });
    out
}

impl App {
    /// Abre um painel só de janela (ver `layout::window_only`) na janela
    /// própria e a traz para a frente.
    pub fn open_window_panel(&mut self, ctx: Option<&egui::Context>, tab: DockTab) {
        self.closed_tabs.remove(&tab);
        self.detached_tabs.insert(tab);
        if let Some(ctx) = ctx {
            let key = crate::layout::panel_key(tab);
            let vid = egui::ViewportId::from_hash_of(("neoveg_floating_dock_viewport", key));
            ctx.send_viewport_cmd_to(vid, egui::ViewportCommand::Focus);
        }
    }

    pub fn detach_dock_tab(&mut self, tab: DockTab) {
        self.detached_tabs.insert(tab);
        if self.dock_tab == tab {
            if let Some(next) = DockTab::ALL
                .into_iter()
                .find(|t| !self.detached_tabs.contains(t))
            {
                self.dock_tab = next;
            }
        }
    }

    /// Menu das Sobreposições (seta ⏷ ao lado e botão direito do botão Sobreposições,
    /// manual do VEGAS Pro 21, p. 197): o que aparece quando elas estão
    /// ligadas. As guias ficam salvas nas Preferências; o isolamento de canal
    /// vale só para a sessão.
    pub(crate) fn safe_guides_menu(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang();
        let mut changed = false;
        let mut show = false;
        {
            let g = &mut self.prefs.safe_guides;
            changed |= ui
                .checkbox(&mut g.action_title, tr(lang, "Áreas de ação e de título"))
                .changed();
            changed |= ui.checkbox(&mut g.grid, tr(lang, "Grade")).changed();
            changed |= ui
                .checkbox(&mut g.rulers, tr(lang, "Réguas (pixels do projeto)"))
                .changed();
        }
        ui.menu_button(tr(lang, "Isolamento de canal"), |ui| {
            for m in crate::preview_overlays::ChannelIsolation::ALL {
                if ui
                    .radio(self.preview_channel == m, tr(lang, m.label()))
                    .clicked()
                {
                    self.preview_channel = m;
                    show = m != crate::preview_overlays::ChannelIsolation::Off;
                    ui.close_menu();
                }
            }
            ui.separator();
            if ui
                .checkbox(
                    &mut self.preview_channel_gray,
                    tr(lang, "Mostrar em escala de cinza"),
                )
                .changed()
            {
                show = true;
            }
        });
        if show {
            self.opts.safe_areas = true;
        }
        ui.menu_button(tr(lang, "Legendas fechadas (CC1–CC4)"), |ui| {
            for ch in crate::preview_overlays::ClosedCaptionChannel::ALL {
                if ui
                    .radio(self.preview_cc_channel == ch, tr(lang, ch.label()))
                    .clicked()
                {
                    self.preview_cc_channel = ch;
                    ui.close_menu();
                }
            }
        });
        if ui
            .checkbox(
                &mut self.preview_360.enabled,
                tr(lang, "Prévia 360° (equiretangular)"),
            )
            .changed()
            && !self.preview_360.enabled
        {
            self.preview_360.reset_view();
        }
        if self.preview_360.enabled && ui.button(tr(lang, "Restaurar visão 360°")).clicked() {
            self.preview_360.reset_view();
            ui.close_menu();
        }
        ui.separator();
        let g = &mut self.prefs.safe_guides;
        changed |= ui
            .checkbox(&mut g.center_cross, tr(lang, "Cruz central"))
            .changed();
        changed |= ui
            .checkbox(&mut g.thirds, tr(lang, "Grade dos terços"))
            .changed();
        changed |= ui
            .checkbox(&mut g.cut_4x3, tr(lang, "Recorte central 4:3"))
            .changed();
        changed |= ui
            .checkbox(&mut g.cut_9x16, tr(lang, "Recorte central 9:16 (vertical)"))
            .changed();
        changed |= ui
            .checkbox(&mut g.cut_1x1, tr(lang, "Recorte central 1:1 (quadrado)"))
            .changed();
        if changed {
            // ligar uma guia mostra as sobreposições
            if g.center_cross
                || g.thirds
                || g.cut_4x3
                || g.cut_9x16
                || g.cut_1x1
                || g.action_title
                || g.grid
            {
                self.opts.safe_areas = true;
            }
            if let Err(e) = self.prefs.save() {
                self.set_status(e);
            }
        }
    }

    /// Fecha um painel: sai da área de janelas (e da janela própria, se
    /// estava destacado) e volta pelo menu Exibir.
    pub fn close_dock_tab(&mut self, tab: DockTab) {
        self.detached_tabs.remove(&tab);
        self.closed_tabs.insert(tab);
        if self.dock_maximized == Some(crate::dock::DockPane::Tab(tab)) {
            self.dock_maximized = None;
        }
        if self.dock_tab == tab {
            if let Some(next) = DockTab::ALL
                .into_iter()
                .find(|t| !self.detached_tabs.contains(t) && !self.closed_tabs.contains(t))
            {
                self.dock_tab = next;
            }
        }
    }

    pub fn dock_area(&mut self, ui: &mut egui::Ui) {
        crate::dock::draw_dock_tree(self, ui);
    }

    /// Geometria com que a janela destacada `key` abre: a última conhecida,
    /// congelada enquanto a janela estiver aberta (ver `FloatingViewportSpec`).
    pub(crate) fn floating_open_geom(&mut self, key: &str) -> Option<crate::layout::FloatingGeom> {
        self.floating_shown.insert(key.to_string());
        if let Some(g) = self.floating_open.get(key) {
            return *g;
        }
        let g = self.floating_geom.get(key).copied();
        self.floating_open.insert(key.to_string(), g);
        g
    }

    /// Guarda onde a janela `key` está agora (vai para o `.neoveglayout`).
    pub(crate) fn remember_floating_geom(
        &mut self,
        key: &str,
        geom: Option<crate::layout::FloatingGeom>,
    ) {
        if let Some(g) = geom {
            if self.floating_geom.len() < 64 || self.floating_geom.contains_key(key) {
                self.floating_geom.insert(key.to_string(), g);
            }
        }
    }

    pub fn floating_panel_windows(&mut self, ctx: &egui::Context) {
        self.floating_shown.clear();
        self.float_dock_hint = false;
        self.floating_panel_windows_inner(ctx);
        // janela fechada: a próxima abertura parte da última posição
        let shown = std::mem::take(&mut self.floating_shown);
        self.floating_open.retain(|k, _| shown.contains(k));
        self.float_dock.retain(&shown);
        self.floating_shown = shown;
        // contorno da área de janelas enquanto uma janela vai acoplar
        if let (true, Some(r)) = (self.float_dock_hint, self.dock_rect) {
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("neoveg_float_dock_hint"),
            ));
            painter.rect_filled(r, 2.0, self.theme.accent.gamma_multiply(0.12));
            painter.rect_stroke(
                r,
                2.0,
                Stroke::new(2.0_f32, self.theme.accent),
                StrokeKind::Inside,
            );
        }
    }

    /// Acoplar arrastando (ver `float_dock`): `true` quando a janela `key`
    /// Ação resultante de soltar ou mover uma janela destacada.
    fn float_dock_target(
        &mut self,
        ctx: &egui::Context,
        key: &str,
        geom: Option<crate::layout::FloatingGeom>,
        ctrl: bool,
    ) -> FloatDockAction {
        use crate::float_dock::{grab_point, wants_dock, MoveState};
        let Some([x, y, w, h]) = geom else {
            return FloatDockAction::None;
        };
        let state = self
            .float_dock
            .update(key, pos2(x, y), std::time::Instant::now());
        if state == MoveState::Idle {
            return FloatDockAction::None;
        }
        // ainda vai decidir: precisa de quadros depois que a janela para
        ctx.request_repaint_after(std::time::Duration::from_millis(100));

        let pt = grab_point(Rect::from_min_size(pos2(x, y), vec2(w, h)));
        let over_dock = if let (Some(dock), Some(main)) =
            (self.dock_rect, ctx.input(|i| i.viewport().inner_rect))
        {
            let screen_dock = dock.translate(main.min.to_vec2());
            screen_dock.contains(pt)
        } else {
            false
        };

        let ctrl = ctrl || ctx.input(|i| i.modifiers.ctrl);
        let want_dock = over_dock && wants_dock(self.prefs.allow_floating_dock, ctrl);

        match state {
            MoveState::Moving => {
                self.float_dock_hint |= want_dock;
                FloatDockAction::None
            }
            MoveState::Settled => {
                if want_dock {
                    self.float_dock.forget(key);
                    FloatDockAction::DockToMain
                } else {
                    for other in &self.floating_docks {
                        if other.id != key {
                            if let Some(og) = self.floating_geom.get(&other.id) {
                                let other_rect =
                                    Rect::from_min_size(pos2(og[0], og[1]), vec2(og[2], og[3]));
                                if other_rect.contains(pt) {
                                    self.float_dock.forget(key);
                                    return FloatDockAction::MergeInto(other.id.clone());
                                }
                            }
                        }
                    }
                    FloatDockAction::None
                }
            }
            MoveState::Idle => FloatDockAction::None,
        }
    }

    fn floating_panel_windows_inner(&mut self, ctx: &egui::Context) {
        self.sync_floating_docks();
        let lang = self.lang();
        let icon = self.window_icon.clone();
        let theme = self.theme.clone();

        let groups = self.floating_docks.clone();
        let mut to_dock_groups: Vec<String> = Vec::new();
        let mut to_close_groups: Vec<String> = Vec::new();
        let mut to_dock_panes: Vec<crate::dock::DockPane> = Vec::new();
        let mut to_pop_out_panes: Vec<crate::dock::DockPane> = Vec::new();
        let mut to_add_panes: Vec<(crate::dock::DockPane, crate::dock::DockPane)> = Vec::new();
        let mut to_close_panes: Vec<crate::dock::DockPane> = Vec::new();
        let mut to_set_active: Vec<(String, usize)> = Vec::new();
        let mut to_merge: Vec<(String, String)> = Vec::new();

        for grp in groups {
            let Some(active_pane) = grp.active_pane() else {
                continue;
            };
            let key = grp.id.as_str();

            let has_encoder = grp
                .panes
                .iter()
                .any(|p| matches!(p, crate::dock::DockPane::Tab(DockTab::Encoder)));
            let has_capture = grp
                .panes
                .iter()
                .any(|p| matches!(p, crate::dock::DockPane::Tab(DockTab::Capture)));
            let has_preview = grp
                .panes
                .iter()
                .any(|p| matches!(p, crate::dock::DockPane::Preview));
            let has_master = grp
                .panes
                .iter()
                .any(|p| matches!(p, crate::dock::DockPane::MasterBus));
            let is_single_master = has_master && grp.panes.len() == 1;

            let (inner_size, min_size) = if has_encoder {
                ([1120.0, 680.0], [740.0, 440.0])
            } else if has_capture {
                ([980.0, 640.0], [620.0, 420.0])
            } else if has_preview {
                ([560.0, 400.0], [280.0, 220.0])
            } else if is_single_master {
                ([220.0, 420.0], [140.0, 260.0])
            } else {
                ([540.0, 380.0], [280.0, 200.0])
            };

            let can_dock = grp.panes.iter().any(
                |p| !matches!(p, crate::dock::DockPane::Tab(t) if crate::layout::window_only(*t)),
            );
            let title = if grp.panes.len() > 1 {
                format!("{} (+{})", active_pane.title_in(lang), grp.panes.len() - 1)
            } else {
                active_pane.title_in(lang).to_string()
            };

            let spec = FloatingViewportSpec {
                title: &title,
                inner_size,
                min_size,
                icon: icon.clone(),
                theme: &theme,
                lang,
                can_dock,
                geom: self.floating_open_geom(key),
            };

            let mut tab_action_set_active = None;
            let mut tab_action_dock_pane = None;
            let mut tab_action_pop_out = None;
            let mut tab_action_close_pane = None;
            let mut tab_action_add_pane = None;

            let out = show_floating_viewport(
                ctx,
                ("neoveg_floating_dock_viewport", key),
                ("floating_dock_window", key),
                &spec,
                |ui| {
                    if grp.panes.len() > 1 {
                        ui.horizontal_wrapped(|ui| {
                            let active_idx = grp.active.min(grp.panes.len().saturating_sub(1));
                            for (i, &pane) in grp.panes.iter().enumerate() {
                                let is_active = i == active_idx;
                                let label = pane.title_in(lang);
                                let resp = icons::small_icon_drag_button(
                                    ui,
                                    pane.icon(),
                                    Some(label),
                                    is_active,
                                    &theme,
                                )
                                .on_hover_text(tr(
                                    lang,
                                    "Clique para selecionar; clique com botão direito para opções",
                                ));

                                if resp.clicked() {
                                    tab_action_set_active = Some(i);
                                }

                                if is_active
                                    && icons::small_icon_button(
                                        ui,
                                        Icon::CloseX,
                                        None,
                                        false,
                                        &theme,
                                    )
                                    .on_hover_text(tr(lang, "Fechar aba"))
                                    .clicked()
                                {
                                    tab_action_close_pane = Some(pane);
                                }

                                resp.context_menu(|ui| {
                                    if ui
                                        .button(tr(lang, "Destacar aba em janela própria"))
                                        .clicked()
                                    {
                                        tab_action_pop_out = Some(pane);
                                        ui.close_menu();
                                    }
                                    if ui.button(tr(lang, "Acoplar esta aba")).clicked() {
                                        tab_action_dock_pane = Some(pane);
                                        ui.close_menu();
                                    }
                                    if ui.button(tr(lang, "Acoplar todas as abas")).clicked() {
                                        tab_action_dock_pane = None;
                                        ui.close_menu();
                                    }
                                    ui.separator();
                                    if ui.button(tr(lang, "Fechar aba")).clicked() {
                                        tab_action_close_pane = Some(pane);
                                        ui.close_menu();
                                    }
                                });
                            }

                            ui.menu_button(tr(lang, "Adicionar aba a esta janela"), |ui| {
                                for p in crate::dock::DockPane::all_canonical() {
                                    if !grp.panes.contains(&p) {
                                        ui.horizontal(|ui| {
                                            let (r, _) = ui.allocate_exact_size(
                                                vec2(14.0, 14.0),
                                                Sense::hover(),
                                            );
                                            icons::paint(ui.painter(), r, p.icon(), &theme, true);
                                            if ui.button(p.title_in(lang)).clicked() {
                                                tab_action_add_pane = Some(p);
                                                ui.close_menu();
                                            }
                                        });
                                    }
                                }
                            });
                        });
                        ui.separator();
                    } else {
                        ui.horizontal(|ui| {
                            ui.menu_button(tr(lang, "Adicionar aba a esta janela"), |ui| {
                                for p in crate::dock::DockPane::all_canonical() {
                                    if !grp.panes.contains(&p) {
                                        ui.horizontal(|ui| {
                                            let (r, _) = ui.allocate_exact_size(
                                                vec2(14.0, 14.0),
                                                Sense::hover(),
                                            );
                                            icons::paint(ui.painter(), r, p.icon(), &theme, true);
                                            if ui.button(p.title_in(lang)).clicked() {
                                                tab_action_add_pane = Some(p);
                                                ui.close_menu();
                                            }
                                        });
                                    }
                                }
                            });
                        });
                        ui.separator();
                    }

                    match active_pane {
                        crate::dock::DockPane::Preview => {
                            self.preview_panel(ui, true);
                        }
                        crate::dock::DockPane::MasterBus => {
                            self.master_bus_panel(ui, true);
                        }
                        crate::dock::DockPane::Tab(tab) => {
                            egui::ScrollArea::both()
                                .id_salt(("floating_os_scroll", tab.command_id(), key))
                                .auto_shrink([false, false])
                                .show(ui, |ui| self.show_dock_tab_contents(ui, tab));
                        }
                    }
                },
            );

            self.remember_floating_geom(key, out.geom);

            let dock_action = if can_dock {
                self.float_dock_target(ctx, key, out.geom, out.ctrl)
            } else {
                FloatDockAction::None
            };

            if out.reattach || matches!(dock_action, FloatDockAction::DockToMain) {
                to_dock_groups.push(grp.id.clone());
            } else if let FloatDockAction::MergeInto(other_id) = dock_action {
                to_merge.push((grp.id.clone(), other_id));
            } else if !out.open {
                to_close_groups.push(grp.id.clone());
            }

            if let Some(idx) = tab_action_set_active {
                to_set_active.push((grp.id.clone(), idx));
            }
            if let Some(p) = tab_action_dock_pane {
                to_dock_panes.push(p);
            }
            if let Some(p) = tab_action_pop_out {
                to_pop_out_panes.push(p);
            }
            if let Some(p) = tab_action_close_pane {
                to_close_panes.push(p);
            }
            if let Some(p) = tab_action_add_pane {
                to_add_panes.push((active_pane, p));
            }
        }

        for (src_id, dst_id) in to_merge {
            if let (Some(src_grp), Some(dst_grp)) = (
                self.floating_docks.iter().find(|g| g.id == src_id).cloned(),
                self.floating_docks.iter().find(|g| g.id == dst_id).cloned(),
            ) {
                if let Some(dst_pane) = dst_grp.active_pane() {
                    for pane in src_grp.panes {
                        self.group_floating_panes(dst_pane, pane);
                    }
                }
            }
        }

        for (grp_id, idx) in to_set_active {
            if let Some(grp) = self.floating_docks.iter_mut().find(|g| g.id == grp_id) {
                if idx < grp.panes.len() {
                    grp.active = idx;
                }
            }
        }

        for p in to_dock_panes {
            self.dock_pane(p);
        }

        for p in to_pop_out_panes {
            self.detach_pane_to_own_window(p);
        }

        for (target, to_add) in to_add_panes {
            self.group_floating_panes(target, to_add);
        }

        for p in to_close_panes {
            match p {
                crate::dock::DockPane::Tab(t) => self.close_dock_tab(t),
                crate::dock::DockPane::Preview => {
                    self.preview_detached = false;
                    self.preview_visible = false;
                }
                crate::dock::DockPane::MasterBus => {
                    self.master_bus_detached = false;
                    self.master_bus_visible = false;
                }
            }
            self.sync_floating_docks();
        }

        for grp_id in to_dock_groups {
            self.dock_floating_group(&grp_id);
        }

        for grp_id in to_close_groups {
            if let Some(grp) = self.floating_docks.iter().find(|g| g.id == grp_id).cloned() {
                for pane in grp.panes {
                    match pane {
                        crate::dock::DockPane::Tab(t) => self.close_dock_tab(t),
                        crate::dock::DockPane::Preview => {
                            self.preview_detached = false;
                            self.preview_visible = false;
                        }
                        crate::dock::DockPane::MasterBus => {
                            self.master_bus_detached = false;
                            self.master_bus_visible = false;
                        }
                    }
                }
                self.sync_floating_docks();
            }
        }

        if self.external_monitor {
            let vid = egui::ViewportId::from_hash_of("neoveg_external_monitor_viewport");
            let builder = egui::ViewportBuilder::default()
                .with_title("Monitor Externo (Prévia) - Neoveg")
                .with_inner_size([960.0, 560.0])
                .with_min_inner_size([320.0, 200.0])
                .with_resizable(true)
                .with_icon(icon);
            let mut close_ext = false;
            ctx.show_viewport_immediate(vid, builder, |v_ctx, class| {
                let draw_body = |ui: &mut egui::Ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Monitor Externo").strong().small());
                        if ui
                            .selectable_label(self.external_monitor_fullscreen, "Tela cheia")
                            .clicked()
                        {
                            self.external_monitor_fullscreen = !self.external_monitor_fullscreen;
                            v_ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(
                                self.external_monitor_fullscreen,
                            ));
                        }
                        if ui.small_button("Fechar").clicked() {
                            close_ext = true;
                        }
                    });
                    let avail_ext = ui.available_rect_before_wrap();
                    let ext_sz = vec2(
                        avail_ext.width().clamp(64.0, 4096.0),
                        avail_ext.height().clamp(48.0, 2160.0),
                    );
                    let (rect, _) = ui.allocate_exact_size(ext_sz, Sense::hover());
                    let _ = self.draw_preview_canvas(ui, rect, false);
                };
                if class == egui::ViewportClass::Embedded {
                    let mut open_win = true;
                    egui::Window::new("Monitor Externo")
                        .id(egui::Id::new("external_monitor_embedded_window"))
                        .open(&mut open_win)
                        .default_size([640.0, 380.0])
                        .show(v_ctx, draw_body);
                    if !open_win {
                        close_ext = true;
                    }
                } else {
                    egui::CentralPanel::default().show(v_ctx, draw_body);
                    if v_ctx.input(|i| i.viewport().close_requested()) {
                        close_ext = true;
                    }
                }
            });
            if close_ext {
                self.external_monitor = false;
                self.external_monitor_fullscreen = false;
            }
        }
    }

    pub(crate) fn show_dock_tab_contents(&mut self, ui: &mut egui::Ui, tab: DockTab) {
        match tab {
            DockTab::ProjectMedia => self.media_tab(ui),
            DockTab::Explorer => self.explorer_tab(ui),
            DockTab::Transitions => self.transitions_tab(ui),
            DockTab::VideoFx => self.video_fx_browser(ui),
            DockTab::ColorGrading => self.color_grading_tab(ui),
            DockTab::AudioFx => self.fx_tab(ui, true),
            DockTab::Generators => self.generators_browser(ui),
            DockTab::Mixer => self.mixer_tab(ui),
            DockTab::Loudness => self.loudness_tab(ui),
            DockTab::SurroundPanner => self.surround_panner_tab(ui),
            DockTab::PluginManager => self.plugin_manager_tab(ui),
            DockTab::Compositors => self.compositors_tab(ui),
            DockTab::Trimmer => self.trimmer_tab(ui),
            DockTab::EditDetails => self.edit_details_tab(ui),
            DockTab::Scopes => self.scopes_tab(ui),
            DockTab::Notes => self.project_notes_tab(ui),
            DockTab::Encoder => self.encoder_tab(ui),
            DockTab::Capture => self.capture_tab(ui),
            DockTab::Transcript => self.transcript_tab(ui),
            DockTab::Store => self.store_tab(ui),
        }
    }

    pub(crate) fn preview_panel(&mut self, ui: &mut egui::Ui, is_floating: bool) {
        let lang = self.lang();
        self.preview_toolbar(ui, is_floating);
        if let Some(exp) = self.session.expanded_edit.clone() {
            use neoveg_engine::session::ExpandedEdgeFocus;
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new("Expandido:").small().strong());
                for f in [
                    ExpandedEdgeFocus::Both,
                    ExpandedEdgeFocus::LeftOut,
                    ExpandedEdgeFocus::RightIn,
                ] {
                    if ui.selectable_label(exp.focus == f, f.label()).clicked() {
                        self.session.set_expanded_focus(f);
                    }
                }
                if ui.small_button("< 1q").clicked() {
                    self.exec_cmd(Some(ui.ctx()), "edit.expanded_nudge_left");
                }
                if ui.small_button("1q >").clicked() {
                    self.exec_cmd(Some(ui.ctx()), "edit.expanded_nudge_right");
                }
            });
        }
        let avail = ui.available_rect_before_wrap();
        let font = egui::FontId::proportional(10.5);
        let gap_y = ui.spacing().item_spacing.y;
        let line_h = ui.fonts(|f| f.row_height(&font)).ceil().max(13.0);
        let show_transport = avail.height() >= 132.0;
        let transport_h = if show_transport {
            ui.spacing().interact_size.y.max(22.0)
        } else {
            0.0
        };
        let two_line_info = avail.height() >= 96.0;
        let info_h = if two_line_info {
            line_h * 2.0 + 6.0
        } else {
            line_h + 4.0
        };
        let reserved_h = info_h
            + gap_y
            + if show_transport {
                transport_h + gap_y
            } else {
                0.0
            };
        let canvas_h = (avail.height() - reserved_h).max(24.0);
        let drop_out = ui.dnd_drop_zone::<DndPayload, Rect>(egui::Frame::NONE, |ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(avail.width(), canvas_h), Sense::hover());
            self.draw_preview_canvas(ui, rect, true)
        });
        if let Some(payload) = drop_out.1 {
            match (*payload).clone() {
                DndPayload::Fx(kind) => {
                    if self.session.apply_video_fx_to_output(kind) {
                        self.dlg.fx_target = Some(FxTarget::MasterVideo);
                        self.sync_after_session();
                    } else {
                        self.sync_status_from_session();
                    }
                }
                DndPayload::FxPreset(mut fx) => {
                    if fx.kind.is_audio() {
                        self.set_status(
                            "Efeitos de áudio não podem ser aplicados na saída de vídeo",
                        );
                    } else {
                        self.checkpoint();
                        fx.id = new_id();
                        self.project.master.video_fx.push(fx);
                        self.dlg.fx_target = Some(FxTarget::MasterVideo);
                        self.changed();
                    }
                }
                _ => {}
            }
        }
        let img_rect = drop_out.0.inner;

        if show_transport {
            let playing = self.is_playing();
            ui.allocate_ui_with_layout(
                vec2(avail.width(), transport_h),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    // barra de transporte da prévia (manual do VEGAS Pro 21,
                    // p. 201-202), na ordem da barra principal: Gravar, Loop,
                    // Reproduzir do início, Reproduzir, Pausar, Parar, Início,
                    // Fim e quadros; estreita, fica só o essencial
                    let full = ui.available_width() >= 316.0;
                    let pad_x =
                        ((ui.available_width() - if full { 314.0 } else { 116.0 }) * 0.5).max(0.0);
                    ui.add_space(pad_x);
                    if full {
                        self.cmd_tool_toggle(
                            ui,
                            self.recorder.is_some(),
                            Icon::RecordDot,
                            None,
                            "transport.record",
                        );
                        let looping = self.opts.loop_playback;
                        self.cmd_tool_toggle(
                            ui,
                            looping,
                            Icon::LoopPlayback,
                            None,
                            "options.loop_playback",
                        );
                    }
                    self.cmd_tool_btn(ui, Icon::PlayFromStart, None, "transport.play_from_start");
                    self.play_pause_buttons(ui, playing);
                    self.cmd_tool_btn(ui, Icon::Stop, None, "transport.stop");
                    if full {
                        self.cmd_tool_btn(ui, Icon::GoStart, None, "transport.project_start");
                        self.cmd_tool_btn(ui, Icon::GoEnd, None, "transport.project_end");
                        self.cmd_tool_btn(ui, Icon::PrevFrame, None, "transport.frame_back");
                        self.cmd_tool_btn(ui, Icon::NextFrame, None, "transport.frame_fwd");
                        let ram_busy =
                            self.ram_preview_job.is_some() || !self.ram_preview.frames.is_empty();
                        if icons::small_icon_button(ui, Icon::RamCache, None, ram_busy, &self.theme)
                            .on_hover_text(tr(
                                lang,
                                "Prévia dinâmica em RAM na seleção de tempo (Shift+B)",
                            ))
                            .clicked()
                        {
                            self.start_ram_preview();
                        }
                        let sp_tip = if self.prefs.spacebar_play_pause {
                            tr(
                                lang,
                                "Barra de espaço: Reproduzir/Pausar (clique para alternar para Reproduzir/Parar)",
                            )
                        } else {
                            tr(
                                lang,
                                "Barra de espaço: Reproduzir/Parar retornando ao cursor (clique para alternar para Reproduzir/Pausar)",
                            )
                        };
                        if icons::small_icon_button(
                            ui,
                            Icon::SpacebarMode,
                            None,
                            !self.prefs.spacebar_play_pause,
                            &self.theme,
                        )
                        .on_hover_text(sp_tip)
                        .clicked()
                        {
                            self.prefs.spacebar_play_pause = !self.prefs.spacebar_play_pause;
                            let _ = self.prefs.save();
                        }
                    }
                },
            );
        }

        let s = &self.project.settings;
        let pq = self.opts.preview_quality.scale();
        let (pw, ph) = (
            ((s.width as f32 * pq).round() as u32).max(1),
            ((s.height as f32 * pq).round() as u32).max(1),
        );
        let (dw, dh) = (
            (img_rect.width().round() as u32).max(1),
            (img_rect.height().round() as u32).max(1),
        );
        let (prev_scaled, disp_scaled) =
            preview_footer_scaled_flags((s.width, s.height), (pw, ph), (dw, dh));
        let frame_no = s.fps.ticks_to_frame_round(self.view_time());
        let (info_rect, info_resp) =
            ui.allocate_exact_size(vec2(avail.width().max(80.0), info_h), Sense::hover());
        let ip = ui.painter_at(info_rect);
        let fg = self.theme.text_muted;
        // Aviso visual do VEGAS Pro (v9–v23, manual p. 51 / p. 197):
        // destaca a linha de Exibição em vermelho quando há redimensionamento
        // em relação à resolução do projeto ou da prévia.
        let disp_fg = if disp_scaled || prev_scaled {
            self.theme.offline_fg
        } else {
            fg
        };
        let w_s = s.width.to_string();
        let h_s = s.height.to_string();
        let pw_s = pw.to_string();
        let ph_s = ph.to_string();
        let dw_s = dw.to_string();
        let dh_s = dh.to_string();
        let frame_s = frame_no.to_string();
        let fps_s = s.fps.label_in(lang);
        let is_moderno = self.prefs.edit_layout.layout_type == neoveg_common::LayoutType::Moderno;
        if two_line_info {
            let y1 = info_rect.top() + 1.0;
            let y2 = y1 + line_h + 2.0;
            // fps realmente exibido durante a reprodução (manual, p. 197)
            let shown_fps = self.display_fps.fps().filter(|_| self.is_playing());
            if is_moderno {
                // Visual renovado (v23+): rodapé compacto com mini-ícones por métrica
                let ico_sz = 10.5_f32;
                let ico_y1 = y1 + (line_h - ico_sz) * 0.5;
                let ico_y2 = y2 + (line_h - ico_sz) * 0.5;
                let lx = info_rect.left() + 2.0;
                icons::paint(
                    &ip,
                    Rect::from_min_size(pos2(lx, ico_y1), vec2(ico_sz, ico_sz)),
                    Icon::GearProps,
                    &self.theme,
                    false,
                );
                ip.text(
                    pos2(lx + ico_sz + 4.0, y1),
                    egui::Align2::LEFT_TOP,
                    format!("{w_s}×{h_s}; {fps_s}p"),
                    font.clone(),
                    fg,
                );
                icons::paint(
                    &ip,
                    Rect::from_min_size(pos2(lx, ico_y2), vec2(ico_sz, ico_sz)),
                    Icon::VideoTrack,
                    &self.theme,
                    false,
                );
                ip.text(
                    pos2(lx + ico_sz + 4.0, y2),
                    egui::Align2::LEFT_TOP,
                    format!("{pw_s}×{ph_s}; {fps_s}p"),
                    font.clone(),
                    fg,
                );

                let right_disp = match shown_fps {
                    Some(fps) => {
                        let fps_cur = format!("{fps:.2}").replace('.', ",");
                        format!("{dw_s}×{dh_s}; {fps_cur} qps")
                    }
                    None => format!("{dw_s}×{dh_s}"),
                };
                let frame_g = ip.layout_no_wrap(frame_s.clone(), font.clone(), fg);
                let disp_g = ip.layout_no_wrap(right_disp, font, disp_fg);
                let rx_frame_txt = info_rect.right() - 2.0 - frame_g.size().x;
                icons::paint(
                    &ip,
                    Rect::from_min_size(
                        pos2(
                            (rx_frame_txt - ico_sz - 4.0).max(info_rect.center().x),
                            ico_y1,
                        ),
                        vec2(ico_sz, ico_sz),
                    ),
                    Icon::GeneratorFrame,
                    &self.theme,
                    false,
                );
                ip.galley(pos2(rx_frame_txt, y1), frame_g, fg);

                let rx_disp_txt = info_rect.right() - 2.0 - disp_g.size().x;
                icons::paint(
                    &ip,
                    Rect::from_min_size(
                        pos2(
                            (rx_disp_txt - ico_sz - 4.0).max(info_rect.center().x),
                            ico_y2,
                        ),
                        vec2(ico_sz, ico_sz),
                    ),
                    Icon::ExternalMonitor,
                    &self.theme,
                    disp_scaled || prev_scaled,
                );
                ip.galley(pos2(rx_disp_txt, y2), disp_g, disp_fg);
            } else {
                ip.text(
                    pos2(info_rect.left() + 2.0, y1),
                    egui::Align2::LEFT_TOP,
                    tr_args(
                        lang,
                        "Projeto: {w}x{h}; {fps}p",
                        &[("w", &w_s), ("h", &h_s), ("fps", &fps_s)],
                    ),
                    font.clone(),
                    fg,
                );
                ip.text(
                    pos2(info_rect.left() + 2.0, y2),
                    egui::Align2::LEFT_TOP,
                    tr_args(
                        lang,
                        "Prévia: {w}x{h}; {fps}p",
                        &[("w", &pw_s), ("h", &ph_s), ("fps", &fps_s)],
                    ),
                    font.clone(),
                    fg,
                );
                ip.text(
                    pos2(info_rect.right() - 2.0, y1),
                    egui::Align2::RIGHT_TOP,
                    tr_args(lang, "Quadro: {n}", &[("n", &frame_s)]),
                    font.clone(),
                    fg,
                );
                let shown = match shown_fps {
                    Some(fps) => {
                        let fps_cur = format!("{fps:.2}").replace('.', ",");
                        tr_args(
                            lang,
                            "Exibição: {w}x{h}; {fps} qps",
                            &[("w", &dw_s), ("h", &dh_s), ("fps", &fps_cur)],
                        )
                    }
                    None => tr_args(lang, "Exibição: {w}x{h}", &[("w", &dw_s), ("h", &dh_s)]),
                };
                ip.text(
                    pos2(info_rect.right() - 2.0, y2),
                    egui::Align2::RIGHT_TOP,
                    shown,
                    font,
                    disp_fg,
                );
            }
        } else {
            ip.text(
                pos2(info_rect.left() + 2.0, info_rect.center().y),
                egui::Align2::LEFT_CENTER,
                tr_args(
                    lang,
                    "Projeto: {pw}x{ph}  Prévia: {vw}x{vh}  Quadro: {n}",
                    &[
                        ("pw", &w_s),
                        ("ph", &h_s),
                        ("vw", &pw_s),
                        ("vh", &ph_s),
                        ("n", &frame_s),
                    ],
                ),
                font,
                fg,
            );
        }
        if disp_scaled || prev_scaled {
            info_resp.on_hover_text(tr(
                lang,
                "Exibição destacada: a resolução da prévia ou do monitor está redimensionada em relação ao projeto",
            ));
        }
    }

    pub(crate) fn sync_master_monitor_audio(&self) {
        {
            let mut s = crate::util::lock(&self.audio.shared);
            s.dim = self.master_dim;
            s.mono_downmix = self.master_mono;
            s.preview_muted = self.opts.preview_mute_audio;
            s.preview_bus_gain = if self.master_preview_mute {
                0.0
            } else {
                db_to_lin(self.master_preview_vol_db.clamp(-60.0, 12.0))
            };
        }
        self.audio
            .set_bypass_audio_fx(self.opts.bypass_all_audio_fx);
    }

    fn draw_preview_canvas(&mut self, ui: &mut egui::Ui, rect: Rect, interactive: bool) -> Rect {
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, self.theme.tl_preview_bg);
        let (proj_w, proj_h) = (self.project.settings.width, self.project.settings.height);
        let aspect = preview_display_aspect(
            proj_w,
            proj_h,
            self.project.settings.pixel_aspect,
            self.prefs.preview_simulate_device_aspect,
        );
        let (mut fw, mut fh) = (rect.width(), rect.width() / aspect);
        if fh > rect.height() {
            fh = rect.height();
            fw = fh * aspect;
        }
        let zoom = self.preview_zoom.clamp(0.25, 8.0);
        let w = (fw * zoom).max(8.0);
        let h = (fh * zoom).max(8.0);
        let max_px = ((w - rect.width()) * 0.5).max(0.0);
        let max_py = ((h - rect.height()) * 0.5).max(0.0);
        self.preview_pan.x = self.preview_pan.x.clamp(-max_px, max_px);
        self.preview_pan.y = self.preview_pan.y.clamp(-max_py, max_py);
        let img_rect = Rect::from_center_size(rect.center() + self.preview_pan, vec2(w, h));

        let split_mode = self.opts.preview_split_mode;
        if self.trimmer.show_in_preview && self.trimmer.has_focus && self.trimmer.media_id.is_some()
        {
            if let Some(tex) = &self.trimmer.tex {
                let [iw, ih] = tex.size();
                let t_asp = (iw.max(1) as f32) / (ih.max(1) as f32);
                let mut tw = img_rect.width();
                let mut th = tw / t_asp.max(0.01);
                if th > img_rect.height() {
                    th = img_rect.height();
                    tw = th * t_asp;
                }
                let tr_rect = Rect::from_center_size(img_rect.center(), vec2(tw, th));
                painter.image(
                    tex.id(),
                    tr_rect,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
                let badge_r =
                    Rect::from_min_size(img_rect.left_top() + vec2(6.0, 6.0), vec2(58.0, 16.0));
                painter.rect_filled(badge_r, 3.0, Color32::from_black_alpha(180));
                painter.text(
                    badge_r.center(),
                    egui::Align2::CENTER_CENTER,
                    "Trimmer",
                    egui::FontId::proportional(10.0),
                    Color32::from_rgb(240, 200, 40),
                );
            }
        } else if self.session.expanded_edit.is_some() {
            if let Some((lt_tex, rt_tex)) = &self.expanded_ab_tex {
                let mid_x = img_rect.center().x;
                let left_r = Rect::from_min_max(
                    img_rect.left_top(),
                    pos2((mid_x - 1.0).max(img_rect.left()), img_rect.bottom()),
                );
                let right_r = Rect::from_min_max(
                    pos2((mid_x + 1.0).min(img_rect.right()), img_rect.top()),
                    img_rect.right_bottom(),
                );
                let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                painter.image(lt_tex.id(), left_r, uv, Color32::WHITE);
                painter.image(rt_tex.id(), right_r, uv, Color32::WHITE);
                painter.line_segment(
                    [pos2(mid_x, img_rect.top()), pos2(mid_x, img_rect.bottom())],
                    Stroke::new(2.0_f32, self.theme.accent),
                );
                painter.text(
                    left_r.left_top() + vec2(4.0, 3.0),
                    egui::Align2::LEFT_TOP,
                    "A: Saída",
                    egui::FontId::proportional(10.0),
                    Color32::WHITE,
                );
                painter.text(
                    right_r.left_top() + vec2(4.0, 3.0),
                    egui::Align2::LEFT_TOP,
                    "B: Entrada",
                    egui::FontId::proportional(10.0),
                    Color32::WHITE,
                );
            }
        } else if split_mode != neoveg_engine::playback::PreviewSplitMode::Off
            && !self.opts.preview_mute_video
        {
            use neoveg_engine::playback::PreviewSplitMode;
            let cmp_tex = match split_mode {
                PreviewSplitMode::FxBypass => {
                    self.preview_split_tex.as_ref().map(|t| (t.id(), "Sem FX"))
                }
                PreviewSplitMode::Clipboard => self
                    .preview_clipboard
                    .as_ref()
                    .map(|(t, _)| (t.id(), "Transferência")),
                PreviewSplitMode::Off => None,
            };
            let k = self.opts.preview_split_pos.clamp(0.08, 0.92);
            if let (Some((alt_id, alt_lbl)), Some(main_tex)) = (cmp_tex, &self.preview_tex) {
                if self.opts.preview_split_horizontal {
                    let div_y = img_rect.top() + img_rect.height() * k;
                    let top_r =
                        Rect::from_min_max(img_rect.left_top(), pos2(img_rect.right(), div_y));
                    let bot_r =
                        Rect::from_min_max(pos2(img_rect.left(), div_y), img_rect.right_bottom());
                    painter.image(
                        alt_id,
                        top_r,
                        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, k)),
                        Color32::WHITE,
                    );
                    painter.image(
                        main_tex.id(),
                        bot_r,
                        Rect::from_min_max(pos2(0.0, k), pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                    painter.line_segment(
                        [pos2(img_rect.left(), div_y), pos2(img_rect.right(), div_y)],
                        Stroke::new(2.0_f32, self.theme.accent),
                    );
                } else {
                    let div_x = img_rect.left() + img_rect.width() * k;
                    let left_r =
                        Rect::from_min_max(img_rect.left_top(), pos2(div_x, img_rect.bottom()));
                    let right_r =
                        Rect::from_min_max(pos2(div_x, img_rect.top()), img_rect.right_bottom());
                    painter.image(
                        alt_id,
                        left_r,
                        Rect::from_min_max(pos2(0.0, 0.0), pos2(k, 1.0)),
                        Color32::WHITE,
                    );
                    painter.image(
                        main_tex.id(),
                        right_r,
                        Rect::from_min_max(pos2(k, 0.0), pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                    painter.line_segment(
                        [pos2(div_x, img_rect.top()), pos2(div_x, img_rect.bottom())],
                        Stroke::new(2.0_f32, self.theme.accent),
                    );
                }
                painter.text(
                    img_rect.left_top() + vec2(6.0, 4.0),
                    egui::Align2::LEFT_TOP,
                    alt_lbl,
                    egui::FontId::proportional(10.0),
                    Color32::WHITE,
                );
                painter.text(
                    img_rect.right_top() + vec2(-6.0, 4.0),
                    egui::Align2::RIGHT_TOP,
                    "Ativa",
                    egui::FontId::proportional(10.0),
                    Color32::WHITE,
                );
            } else if let Some(tex) = &self.preview_tex {
                painter.image(
                    tex.id(),
                    img_rect,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
                if split_mode == PreviewSplitMode::Clipboard {
                    painter.text(
                        img_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "Copie um quadro antes para comparar",
                        egui::FontId::proportional(11.0),
                        Color32::from_white_alpha(200),
                    );
                }
            }
        } else if let Some(tex) = &self.preview_tex {
            painter.image(
                tex.id(),
                img_rect,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }

        if interactive {
            let mut dragging_split = false;
            if split_mode != neoveg_engine::playback::PreviewSplitMode::Off
                && self.session.expanded_edit.is_none()
            {
                let k = self.opts.preview_split_pos.clamp(0.08, 0.92);
                let hit_rect = if self.opts.preview_split_horizontal {
                    let y = img_rect.top() + img_rect.height() * k;
                    Rect::from_min_max(
                        pos2(img_rect.left(), y - 6.0),
                        pos2(img_rect.right(), y + 6.0),
                    )
                } else {
                    let x = img_rect.left() + img_rect.width() * k;
                    Rect::from_min_max(
                        pos2(x - 6.0, img_rect.top()),
                        pos2(x + 6.0, img_rect.bottom()),
                    )
                };
                let div_resp = ui.interact(
                    hit_rect,
                    ui.id().with("pv_split_handle"),
                    Sense::click_and_drag(),
                );
                if div_resp.dragged() {
                    dragging_split = true;
                    if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                        let new_k = if self.opts.preview_split_horizontal {
                            (pos.y - img_rect.top()) / img_rect.height().max(1.0)
                        } else {
                            (pos.x - img_rect.left()) / img_rect.width().max(1.0)
                        };
                        self.opts.preview_split_pos = new_k.clamp(0.08, 0.92);
                    }
                }
            }
            let canvas_resp = ui.interact(
                rect,
                ui.id().with("pv_zoom_pan_canvas"),
                Sense::click_and_drag(),
            );
            canvas_resp.context_menu(|ui| {
                let floating = self.preview_detached;
                self.preview_context_menu(ui, floating);
            });
            if canvas_resp.hovered() {
                // roda: 10% por passo; com Ctrl, 1% (manual, p. 209). O egui
                // entrega Ctrl+roda como zoom_delta.
                let zd = ui.input(|i| i.zoom_delta());
                if zd.is_finite() && (zd - 1.0).abs() > 0.001 {
                    let factor = if zd > 1.0 { 1.01 } else { 1.0 / 1.01 };
                    self.preview_zoom_bypass = None;
                    self.preview_zoom = (self.preview_zoom * factor).clamp(0.25, 8.0);
                }
                let sy = ui.input(|i| i.smooth_scroll_delta.y);
                if sy.abs() > 0.5 {
                    if self.preview_360.enabled {
                        self.preview_360
                            .adjust_fov(if sy > 0.0 { -5.0 } else { 5.0 });
                    } else {
                        self.preview_zoom_bypass = None;
                        let factor = if sy > 0.0 { 1.10 } else { 1.0 / 1.10 };
                        self.preview_zoom = (self.preview_zoom * factor).clamp(0.25, 8.0);
                        if (self.preview_zoom - 1.0).abs() < 0.02 {
                            self.apply_preview_zoom(0.0);
                        }
                    }
                }
            }
            if !dragging_split && self.preview_360.enabled {
                if canvas_resp.hovered() || canvas_resp.dragged() {
                    self.set_cursor_shape(crate::cursors::NeovegCursor::Rotate);
                }
                if canvas_resp.dragged() {
                    let d = canvas_resp.drag_delta();
                    self.preview_360.orbit(d.x * 0.35, -d.y * 0.35);
                }
                if canvas_resp.double_clicked() {
                    self.preview_360.reset_view();
                }
            } else {
                // com zoom, a prévia aceita a mão: arrastar desloca a imagem e
                // Espaço segurado mostra a mão (soltar sem arrastar reproduz)
                let zoomed = self.preview_zoom > 1.01;
                let space_hand =
                    zoomed && !dragging_split && crate::hand::hand_active(ui, &canvas_resp, false);
                if !dragging_split && zoomed && canvas_resp.dragged() {
                    self.set_cursor_shape(crate::cursors::NeovegCursor::System(
                        egui::CursorIcon::Grabbing,
                    ));
                    self.preview_pan += canvas_resp.drag_delta();
                    if space_hand {
                        crate::hand::note_drag(ui.ctx());
                    }
                } else if space_hand && canvas_resp.hovered() {
                    self.set_cursor_shape(crate::cursors::NeovegCursor::System(
                        egui::CursorIcon::Grab,
                    ));
                }
                if canvas_resp.double_clicked() && (self.preview_zoom - 1.0).abs() > 0.02 {
                    self.apply_preview_zoom(0.0);
                }
            }
        }
        if self.preview_360.enabled {
            let badge = format!(
                "360°  Y:{:+.0}° P:{:+.0}° FOV:{:.0}°",
                self.preview_360.yaw_deg, self.preview_360.pitch_deg, self.preview_360.fov_deg
            );
            let b_rect = Rect::from_min_size(
                pos2(img_rect.left() + 6.0, img_rect.top() + 6.0),
                vec2(180.0, 18.0),
            );
            painter.rect_filled(
                b_rect,
                2.0,
                Color32::from_rgba_unmultiplied(12, 14, 18, 195),
            );
            painter.text(
                pos2(b_rect.left() + 5.0, b_rect.center().y),
                egui::Align2::LEFT_CENTER,
                badge,
                egui::FontId::monospace(10.0),
                self.theme.accent,
            );
        }
        if self.preview_cc_channel != crate::preview_overlays::ClosedCaptionChannel::Off {
            if let Some(cc_txt) = crate::preview_overlays::resolve_closed_caption(
                &self.project,
                self.play_pos(),
                self.preview_cc_channel,
            ) {
                let font = egui::FontId::monospace(13.0);
                let galley = painter.layout(
                    cc_txt,
                    font,
                    Color32::WHITE,
                    (img_rect.width() * 0.86).max(60.0),
                );
                let pad = vec2(8.0, 4.0);
                let sz = galley.size() + pad * 2.0;
                let center_x = img_rect.center().x;
                let bottom_y =
                    (img_rect.bottom() - img_rect.height() * 0.08).max(img_rect.top() + sz.y);
                let cc_rect = Rect::from_min_size(pos2(center_x - sz.x * 0.5, bottom_y - sz.y), sz);
                painter.rect_filled(cc_rect, 2.0, Color32::from_rgba_unmultiplied(0, 0, 0, 215));
                painter.galley(cc_rect.min + pad, galley, Color32::WHITE);
            }
        }

        if self.opts.safe_areas {
            let g = &self.prefs.safe_guides;
            if g.grid {
                let st = Stroke::new(1.0_f32, Color32::from_white_alpha(55));
                for k in crate::preview_overlays::grid_fractions(g.grid_v) {
                    let x = img_rect.left() + img_rect.width() * k;
                    painter.line_segment([pos2(x, img_rect.top()), pos2(x, img_rect.bottom())], st);
                }
                for k in crate::preview_overlays::grid_fractions(g.grid_h) {
                    let y = img_rect.top() + img_rect.height() * k;
                    painter.line_segment([pos2(img_rect.left(), y), pos2(img_rect.right(), y)], st);
                }
            }
            if g.action_title {
                let action_k = (self.prefs.action_safe_pct as f32 / 100.0).clamp(0.70, 0.98);
                let title_k = (self.prefs.title_safe_pct as f32 / 100.0).clamp(0.50, action_k);
                for k in [action_k, title_k] {
                    let r = Rect::from_center_size(img_rect.center(), img_rect.size() * k);
                    painter.rect_stroke(
                        r,
                        0.0,
                        Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 90)),
                        StrokeKind::Inside,
                    );
                }
            }
            if g.center_cross {
                let c = img_rect.center();
                painter.line_segment(
                    [c - vec2(8.0, 0.0), c + vec2(8.0, 0.0)],
                    Stroke::new(1.0_f32, Color32::from_white_alpha(120)),
                );
                painter.line_segment(
                    [c - vec2(0.0, 8.0), c + vec2(0.0, 8.0)],
                    Stroke::new(1.0_f32, Color32::from_white_alpha(120)),
                );
            }
            if g.thirds {
                let st = Stroke::new(1.0_f32, Color32::from_white_alpha(70));
                for k in [1.0 / 3.0, 2.0 / 3.0] {
                    let x = img_rect.left() + img_rect.width() * k;
                    let y = img_rect.top() + img_rect.height() * k;
                    painter.line_segment([pos2(x, img_rect.top()), pos2(x, img_rect.bottom())], st);
                    painter.line_segment([pos2(img_rect.left(), y), pos2(img_rect.right(), y)], st);
                }
            }
            let cut = Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 200, 60, 150));
            for (on, aspect) in [
                (g.cut_4x3, 4.0 / 3.0),
                (g.cut_9x16, 9.0 / 16.0),
                (g.cut_1x1, 1.0),
            ] {
                if let Some(r) = on.then(|| centered_aspect_rect(img_rect, aspect)).flatten() {
                    painter.rect_stroke(r, 0.0, cut, StrokeKind::Inside);
                }
            }
        }

        if self.prefs.safe_guides.rulers {
            self.draw_preview_rulers(&painter, rect, img_rect, proj_w, proj_h);
        }

        if self.opts.preview_mute_video {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "VÍDEO SILENCIADO",
                egui::FontId::proportional(13.0),
                self.theme.offline_fg,
            );
        } else {
            let mc_drawn = if interactive && self.opts.multicam_mode {
                if let Some((_, _, ev_id, active_idx, count)) =
                    self.session.active_multicam_event_at(self.cursor)
                {
                    self.draw_multicam_grid(ui, &painter, img_rect, ev_id, active_idx, count);
                    true
                } else {
                    false
                }
            } else {
                false
            };
            if !mc_drawn {
                if let Some(job) = &self.ram_preview_job {
                    let done = job.done.load(Ordering::Relaxed);
                    let total = job.total.load(Ordering::Relaxed).max(1);
                    let pct = (done.saturating_mul(100) / total).min(100);
                    painter.text(
                        rect.left_bottom() + vec2(6.0, -4.0),
                        egui::Align2::LEFT_BOTTOM,
                        format!("RAM Preview: {pct}% ({done}/{total})"),
                        egui::FontId::proportional(10.0),
                        Color32::WHITE,
                    );
                }
            }
        }
        img_rect
    }

    /// Réguas em pixels do projeto nas bordas de cima e da esquerda da área
    /// da prévia, alinhadas à imagem (já com zoom e pan).
    fn draw_preview_rulers(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        img_rect: Rect,
        proj_w: u32,
        proj_h: u32,
    ) {
        const T: f32 = 14.0;
        let bg = Color32::from_black_alpha(170);
        let fg = Color32::from_white_alpha(210);
        let font = egui::FontId::proportional(9.0);
        let top = Rect::from_min_max(rect.left_top(), pos2(rect.right(), rect.top() + T));
        let left = Rect::from_min_max(
            pos2(rect.left(), rect.top() + T),
            pos2(rect.left() + T, rect.bottom()),
        );
        painter.rect_filled(top, 0.0, bg);
        painter.rect_filled(left, 0.0, bg);
        for t in
            crate::preview_overlays::ruler_ticks(img_rect.left(), img_rect.width(), proj_w, 8.0)
        {
            if t.pos < top.left() + T || t.pos > top.right() {
                continue;
            }
            let h = if t.major { T } else { T * 0.35 };
            painter.line_segment(
                [pos2(t.pos, top.bottom() - h), pos2(t.pos, top.bottom())],
                Stroke::new(1.0_f32, fg),
            );
            if t.major {
                painter.text(
                    pos2(t.pos + 2.0, top.top()),
                    egui::Align2::LEFT_TOP,
                    t.value.to_string(),
                    font.clone(),
                    fg,
                );
            }
        }
        for t in
            crate::preview_overlays::ruler_ticks(img_rect.top(), img_rect.height(), proj_h, 8.0)
        {
            if t.pos < left.top() || t.pos > left.bottom() {
                continue;
            }
            let w = if t.major { T } else { T * 0.35 };
            painter.line_segment(
                [pos2(left.right() - w, t.pos), pos2(left.right(), t.pos)],
                Stroke::new(1.0_f32, fg),
            );
            if t.major {
                painter.text(
                    pos2(left.left() + 1.0, t.pos + 1.0),
                    egui::Align2::LEFT_TOP,
                    t.value.to_string(),
                    font.clone(),
                    fg,
                );
            }
        }
    }

    fn draw_multicam_grid(
        &mut self,
        ui: &mut egui::Ui,
        painter: &egui::Painter,
        rect: Rect,
        ev_id: Id,
        active_idx: usize,
        count: usize,
    ) {
        let shown = count.clamp(1, 16);
        let cols = if shown <= 4 {
            2
        } else if shown <= 9 {
            3
        } else {
            4
        };
        let rows = shown.div_ceil(cols).max(1);
        let gap = 3.0_f32;
        let cell_w = ((rect.width() - gap * (cols as f32 - 1.0)) / cols as f32).max(16.0);
        let cell_h = ((rect.height() - gap * (rows as f32 - 1.0)) / rows as f32).max(16.0);
        painter.rect_filled(rect, 0.0, Color32::BLACK);
        let qt = self.project.settings.quantize(self.cursor).0;
        let take_names: Vec<String> = self
            .project
            .event(ev_id)
            .map(|e| {
                neoveg_engine::takes::takes(e)
                    .into_iter()
                    .enumerate()
                    .map(|(i, tk)| {
                        if tk.name.is_empty() {
                            format!("Câm {}", i + 1)
                        } else {
                            tk.name
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        if self.multicam_thumbs.len() > 64 {
            self.multicam_thumbs.clear();
        }
        let mut clicked_angle: Option<usize> = None;
        for angle_i in 0..shown {
            let col = angle_i % cols;
            let row = angle_i / cols;
            let x0 = rect.left() + col as f32 * (cell_w + gap);
            let y0 = rect.top() + row as f32 * (cell_h + gap);
            let cr = Rect::from_min_size(pos2(x0, y0), vec2(cell_w, cell_h));
            let key = (ev_id, angle_i, qt);
            if !self.multicam_thumbs.contains_key(&key) {
                let frame = neoveg_engine::engine::video::render_event_take_still(
                    &self.project,
                    &self.media,
                    ev_id,
                    angle_i,
                    Ticks::from_raw(qt),
                    160,
                    90,
                    true,
                );
                if let Some(rgba) = frame.to_image() {
                    let ci = crate::app::rgba_to_color_image(&rgba);
                    let tex = ui.ctx().load_texture(
                        format!("mc_{ev_id}_{angle_i}_{qt}"),
                        ci,
                        egui::TextureOptions::LINEAR,
                    );
                    self.multicam_thumbs.insert(key, tex);
                }
            }
            if let Some(tex) = self.multicam_thumbs.get(&key) {
                painter.image(
                    tex.id(),
                    cr,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            let active = angle_i == active_idx;
            let border_col = if active {
                Color32::from_rgb(250, 210, 50)
            } else {
                Color32::from_gray(95)
            };
            painter.rect_stroke(
                cr,
                0.0,
                Stroke::new(if active { 2.5_f32 } else { 1.0_f32 }, border_col),
                StrokeKind::Inside,
            );
            let lbl = take_names
                .get(angle_i)
                .cloned()
                .unwrap_or_else(|| format!("Câm {}", angle_i + 1));
            painter.rect_filled(
                Rect::from_min_size(
                    cr.left_top() + vec2(3.0, 3.0),
                    vec2((cell_w - 6.0).min(120.0), 15.0),
                ),
                2.0,
                Color32::from_black_alpha(180),
            );
            painter.text(
                cr.left_top() + vec2(6.0, 5.0),
                egui::Align2::LEFT_TOP,
                format!("[{}] {}", angle_i + 1, lbl),
                egui::FontId::proportional(10.0),
                if active {
                    Color32::from_rgb(255, 225, 90)
                } else {
                    Color32::WHITE
                },
            );
            let resp = ui.interact(
                cr,
                ui.id().with(("mc_cell", ev_id, angle_i)),
                Sense::click(),
            );
            resp.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    true,
                    format!("Câmera {}", angle_i + 1),
                )
            });
            if resp.clicked() {
                clicked_angle = Some(angle_i);
            }
        }
        if let Some(angle_i) = clicked_angle {
            if self.session.switch_multicam_angle(self.cursor, angle_i) {
                self.sync_after_session();
            }
        }
    }

    fn master_bus_controls(&mut self, ui: &mut egui::Ui, tall_dock: bool) {
        let lang = self.lang();
        let mut changed = false;
        let mut monitor_changed = false;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            if ui
                .small_button("fx")
                .on_hover_text(tr(lang, "Efeitos do Master Bus (áudio)"))
                .clicked()
            {
                self.open_fx_target(FxTarget::MasterAudio);
            }
            if ui
                .selectable_label(self.master_mono, "Mono")
                .on_hover_text(tr(lang, "Downmix Stereo -> Mono na monitoração"))
                .clicked()
            {
                self.master_mono = !self.master_mono;
                monitor_changed = true;
            }
            if ui
                .selectable_label(self.master_dim, "Dim")
                .on_hover_text(tr(
                    lang,
                    "Atenuar saída de monitoração em -20 dB (Dim Output)",
                ))
                .clicked()
            {
                self.master_dim = !self.master_dim;
                monitor_changed = true;
            }
            if ui
                .selectable_label(self.project.master.mute, "M")
                .on_hover_text(tr(lang, "Silenciar Master Bus (Mute)"))
                .clicked()
            {
                self.checkpoint();
                self.project.master.mute = !self.project.master.mute;
                changed = true;
            }
            if tall_dock {
                let pv_btn = icons::small_icon_button(
                    ui,
                    Icon::BusTrack,
                    None,
                    self.master_show_preview_bus,
                    &self.theme,
                );
                pv_btn.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Button,
                        true,
                        tr(lang, "Barramento Preview"),
                    )
                });
                if pv_btn
                    .on_hover_text(tr(
                        lang,
                        "Mostrar/ocultar barramento Preview ao lado do Master",
                    ))
                    .clicked()
                {
                    self.master_show_preview_bus = !self.master_show_preview_bus;
                }
            }
        });
        if monitor_changed {
            self.sync_master_monitor_audio();
        }
        // Medidores: fader, canal L, escala e canal R formam um grupo centrado
        // na largura do painel e com a altura do painel, como no console do
        // VEGAS Pro. Antes o grupo ficava colado à esquerda e, num painel alto,
        // parava em 240 px e deixava o resto do painel vazio.
        let meter_h = if tall_dock {
            (ui.available_height() - MASTER_BOTTOM_ROWS_H).max(36.0)
        } else {
            110.0
        };
        let show_preview_strip =
            tall_dock && self.master_show_preview_bus && ui.available_width() >= 118.0;
        let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), meter_h), Sense::hover());
        let lay = master_strip_layout(row, show_preview_strip, tall_dock && meter_h >= 78.0);

        if let Some(pr) = lay.preview {
            ui.scope_builder(egui::UiBuilder::new().max_rect(pr), |ui| {
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new(tr(lang, "Prévia"))
                            .small()
                            .color(self.theme.text_muted),
                    );
                    if ui
                        .selectable_label(self.master_preview_mute, "M")
                        .on_hover_text(tr(lang, "Silenciar barramento Preview"))
                        .clicked()
                    {
                        self.master_preview_mute = !self.master_preview_mute;
                        monitor_changed = true;
                    }
                    let pv_s = ui
                        .add_sized(
                            [20.0, (meter_h - 34.0).max(28.0)],
                            egui::Slider::new(&mut self.master_preview_vol_db, -60.0..=12.0)
                                .vertical()
                                .show_value(false),
                        )
                        .on_hover_text(tr(
                            lang,
                            "Volume do barramento Preview (duplo clique para 0,0 dB)",
                        ));
                    if pv_s.double_clicked() {
                        self.master_preview_vol_db = 0.0;
                        monitor_changed = true;
                    } else if pv_s.changed() {
                        monitor_changed = true;
                    }
                    ui.label(
                        egui::RichText::new(format!("{:+.0}", self.master_preview_vol_db))
                            .monospace()
                            .small()
                            .color(self.theme.text_muted),
                    );
                });
            });
            let sep_x = pr.right() + MASTER_GAP;
            ui.painter().vline(
                sep_x,
                row.y_range(),
                egui::Stroke::new(1.0_f32, self.theme.text_muted.gamma_multiply(0.4)),
            );
        }

        let slider = ui.put(
            lay.fader,
            egui::Slider::new(&mut self.project.master.volume_db, -60.0..=12.0)
                .vertical()
                .show_value(false),
        );
        if slider.drag_started() {
            self.checkpoint();
        }
        if slider.double_clicked() {
            self.checkpoint();
            self.project.master.volume_db = 0.0;
            changed = true;
        }
        changed |= slider
            .on_hover_text(tr(lang, "Volume do Master Bus (duplo clique para 0,0 dB)"))
            .changed();
        if let Some(sr) = lay.scale {
            crate::timeline::draw_meter_channel(ui.painter(), lay.left, self.master_hold[0]);
            crate::timeline::draw_meter_channel(ui.painter(), lay.right, self.master_hold[1]);
            let sp = ui.painter_at(sr.expand2(vec2(0.0, 6.0)));
            let font = egui::FontId::monospace(8.0);
            for db in [0, -3, -6, -12, -18, -24, -36, -48, -57] {
                let n = ((db as f32 + 60.0) / 66.0).clamp(0.0, 1.0);
                let y = sr.bottom() - n * sr.height();
                sp.text(
                    pos2(sr.center().x, y),
                    egui::Align2::CENTER_CENTER,
                    format!("{db}"),
                    font.clone(),
                    self.theme.text_muted,
                );
            }
        } else {
            draw_meter(ui.painter(), lay.left.union(lay.right), self.master_hold);
        }

        // Leituras de pico L / R logo abaixo de cada canal, clicáveis para zerar
        // a retenção (VEGAS Pro 21, p. 50 / v23).
        let pk_l = lin_to_db(self.master_hold[0]);
        let pk_r = lin_to_db(self.master_hold[1]);
        let fmt_pk = |v: f32| {
            if !v.is_finite() || v < -100.0 {
                "-inf".to_string()
            } else {
                format!("{v:+.1}")
            }
        };
        let (pk_row, peak_resp) = ui.allocate_exact_size(
            vec2(ui.available_width(), MASTER_PEAK_ROW_H),
            Sense::click(),
        );
        let pk_font = egui::FontId::monospace(9.0);
        for (v, x) in [(pk_l, lay.left.center().x), (pk_r, lay.right.center().x)] {
            ui.painter().text(
                pos2(x, pk_row.center().y),
                egui::Align2::CENTER_CENTER,
                fmt_pk(v),
                pk_font.clone(),
                if v > -0.1 {
                    self.theme.offline_fg
                } else {
                    self.theme.text_muted
                },
            );
        }
        if peak_resp
            .on_hover_text(tr(lang, "Clique para zerar os picos retidos"))
            .clicked()
        {
            self.master_hold = [0.0; 2];
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.add_space((lay.fader.left() - ui.cursor().left()).max(0.0));
            let lock_tip = if self.master_channels_locked {
                tr(lang, "Travar canais L/R juntos")
            } else {
                tr(lang, "Destravar canais L/R")
            };
            if icons::small_icon_button(
                ui,
                Icon::LockClosed,
                None,
                self.master_channels_locked,
                &self.theme,
            )
            .on_hover_text(lock_tip)
            .clicked()
            {
                self.master_channels_locked = !self.master_channels_locked;
            }
            ui.label(
                egui::RichText::new(format!("{:+.1} dB", self.project.master.volume_db))
                    .monospace()
                    .small(),
            );
        });
        if monitor_changed {
            self.sync_master_monitor_audio();
        }
        if changed {
            self.changed();
        }
    }

    pub(crate) fn master_bus_panel(&mut self, ui: &mut egui::Ui, is_floating: bool) {
        let lang = self.lang();
        ui.vertical(|ui| {
            if !is_floating {
                ui.label(egui::RichText::new(tr(lang, "Master")).strong().small());
            }
            self.master_bus_controls(ui, true);
        });
    }

    // -----------------------------------------------------------------
    // Mídia do projeto
    // -----------------------------------------------------------------

    fn media_tab(&mut self, ui: &mut egui::Ui) {
        self.project_media_ui(ui);
    }

    // -----------------------------------------------------------------
    // Explorer (manual VEGAS Pro 21, p. 45)
    // -----------------------------------------------------------------

    fn explorer_tab(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang();
        let mut pending_nav: Option<PathBuf> = None;
        let mut pending_import_at_cursor: Vec<PathBuf> = Vec::new();
        let mut pending_import_only: Vec<PathBuf> = Vec::new();
        let mut pending_open_proj: Option<PathBuf> = None;

        // Barra superior: Voltar, Avançar, Acima, Favorito ★, Árvore, Lista/Detalhes e Filtro (p. 45)
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            if ui
                .add_enabled(
                    !self.explorer_ui.back_stack.is_empty(),
                    egui::Button::new("◀").small(),
                )
                .on_hover_text(tr(lang, "Voltar pasta anterior"))
                .clicked()
            {
                let mut cur = self.explorer_dir.clone();
                if self.explorer_ui.go_back(&mut cur) {
                    self.explorer_dir = cur;
                }
            }
            if ui
                .add_enabled(
                    !self.explorer_ui.forward_stack.is_empty(),
                    egui::Button::new("▶").small(),
                )
                .on_hover_text(tr(lang, "Avançar pasta"))
                .clicked()
            {
                let mut cur = self.explorer_dir.clone();
                if self.explorer_ui.go_forward(&mut cur) {
                    self.explorer_dir = cur;
                }
            }
            if icons::small_icon_button(ui, Icon::FolderUp, None, false, &self.theme)
                .on_hover_text(tr(lang, "Pasta acima"))
                .clicked()
            {
                if let Some(p) = self.explorer_dir.parent() {
                    pending_nav = Some(p.to_path_buf());
                }
            }
            let is_fav = self
                .explorer_ui
                .favorites
                .iter()
                .any(|p| p == &self.explorer_dir);
            if ui
                .selectable_label(is_fav, if is_fav { "★" } else { "☆" })
                .on_hover_text(tr(lang, "Adicionar ou remover pasta atual dos favoritos"))
                .clicked()
            {
                let cur = self.explorer_dir.clone();
                self.explorer_ui.toggle_favorite(&cur);
            }
            if ui
                .selectable_label(self.explorer_ui.show_tree, tr(lang, "Árvore"))
                .on_hover_text(tr(lang, "Mostrar/ocultar árvore de pastas e favoritos"))
                .clicked()
            {
                self.explorer_ui.show_tree = !self.explorer_ui.show_tree;
            }
            if ui
                .selectable_label(
                    self.explorer_ui.details_view,
                    if self.explorer_ui.details_view {
                        tr(lang, "Detalhes")
                    } else {
                        tr(lang, "Lista")
                    },
                )
                .on_hover_text(tr(lang, "Alternar visualização entre Lista e Detalhes"))
                .clicked()
            {
                self.explorer_ui.details_view = !self.explorer_ui.details_view;
            }
            ui.separator();
            ui.label(tr(lang, "Filtro:"));
            ui.add(egui::TextEdit::singleline(&mut self.explorer_filter).desired_width(96.0));
            if !self.explorer_filter.is_empty() && ui.small_button("×").clicked() {
                self.explorer_filter.clear();
            }
        });

        // Barra de endereço clicável em migalhas (breadcrumbs) ou edição direta (p. 45)
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if self.explorer_ui.editing_path {
                let r = ui.add(
                    egui::TextEdit::singleline(&mut self.explorer_ui.path_buf)
                        .desired_width((ui.available_width() - 56.0).max(120.0)),
                );
                let commit = ui.small_button("OK").clicked()
                    || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                if commit {
                    let p = PathBuf::from(self.explorer_ui.path_buf.trim());
                    if p.is_dir() {
                        pending_nav = Some(p);
                    }
                    self.explorer_ui.editing_path = false;
                }
                if ui.small_button("×").clicked() {
                    self.explorer_ui.editing_path = false;
                }
            } else {
                let mut accum = PathBuf::new();
                let comps: Vec<PathBuf> = self
                    .explorer_dir
                    .components()
                    .map(|c| {
                        accum.push(c.as_os_str());
                        accum.clone()
                    })
                    .collect();
                for (i, path_step) in comps.iter().enumerate() {
                    if i > 0 {
                        ui.label(
                            egui::RichText::new("›")
                                .small()
                                .color(self.theme.text_muted),
                        );
                    }
                    let label = path_step
                        .file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| path_step.to_string_lossy().to_string());
                    let is_last = i + 1 == comps.len();
                    let btn = ui.selectable_label(is_last, egui::RichText::new(label).small());
                    if btn.clicked() && !is_last {
                        pending_nav = Some(path_step.clone());
                    }
                }
                if ui
                    .small_button(tr(lang, "Editar"))
                    .on_hover_text(tr(lang, "Digitar caminho completo da pasta"))
                    .clicked()
                {
                    self.explorer_ui.path_buf = self.explorer_dir.to_string_lossy().to_string();
                    self.explorer_ui.editing_path = true;
                }
            }
        });
        ui.separator();

        // Leitura segura do diretório atual
        let mut entries: Vec<(bool, PathBuf, String, u64)> = std::fs::read_dir(&self.explorer_dir)
            .map(|rd| {
                rd.flatten()
                    .filter_map(|e| {
                        let p = e.path();
                        let name = p.file_name()?.to_string_lossy().to_string();
                        if name.starts_with('.') {
                            return None;
                        }
                        let meta = e.metadata().ok();
                        let is_dir = meta
                            .as_ref()
                            .map(|m| m.is_dir())
                            .unwrap_or_else(|| p.is_dir());
                        let size = if is_dir {
                            0
                        } else {
                            meta.map(|m| m.len()).unwrap_or(0)
                        };
                        Some((is_dir, p, name, size))
                    })
                    .collect()
            })
            .unwrap_or_default();

        let subdirs: Vec<(PathBuf, String)> = entries
            .iter()
            .filter(|(is_dir, _, _, _)| *is_dir)
            .map(|(_, p, n, _)| (p.clone(), n.clone()))
            .take(64)
            .collect();

        let sort_col = self.explorer_ui.sort_col;
        let sort_asc = self.explorer_ui.sort_asc;
        entries.sort_by(|a, b| {
            let dir_ord = b.0.cmp(&a.0);
            if dir_ord != std::cmp::Ordering::Equal {
                return dir_ord;
            }
            let ord = match sort_col {
                ExplorerSortCol::Name => a.2.to_lowercase().cmp(&b.2.to_lowercase()),
                ExplorerSortCol::Kind => {
                    let ea =
                        a.1.extension()
                            .map(|s| s.to_string_lossy().to_lowercase())
                            .unwrap_or_default();
                    let eb =
                        b.1.extension()
                            .map(|s| s.to_string_lossy().to_lowercase())
                            .unwrap_or_default();
                    ea.cmp(&eb)
                        .then_with(|| a.2.to_lowercase().cmp(&b.2.to_lowercase()))
                }
                ExplorerSortCol::Size => a.3.cmp(&b.3),
            };
            if sort_asc {
                ord
            } else {
                ord.reverse()
            }
        });

        let filter = self.explorer_filter.to_lowercase();
        let show_tree = self.explorer_ui.show_tree && ui.available_width() >= 420.0;
        let details_view = self.explorer_ui.details_view;

        ui.horizontal_top(|ui| {
            if show_tree {
                ui.vertical(|ui| {
                    ui.set_width(156.0);
                    egui::ScrollArea::vertical()
                        .id_salt("explorer_tree_rail")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            if !self.explorer_ui.favorites.is_empty() {
                                ui.label(
                                    egui::RichText::new(tr(lang, "Favoritos"))
                                        .strong()
                                        .small(),
                                );
                                let favs = self.explorer_ui.favorites.clone();
                                let mut rm_fav: Option<PathBuf> = None;
                                for fav in favs {
                                    let lbl = fav
                                        .file_name()
                                        .map(|s| s.to_string_lossy().to_string())
                                        .unwrap_or_else(|| fav.to_string_lossy().to_string());
                                    let r = ui.selectable_label(fav == self.explorer_dir, lbl);
                                    if r.clicked() {
                                        pending_nav = Some(fav.clone());
                                    }
                                    r.context_menu(|ui| {
                                        if ui.button(tr(lang, "Remover dos favoritos")).clicked() {
                                            rm_fav = Some(fav.clone());
                                            ui.close_menu();
                                        }
                                    });
                                }
                                if let Some(rf) = rm_fav {
                                    self.explorer_ui.favorites.retain(|p| p != &rf);
                                }
                                ui.separator();
                            }
                            ui.label(egui::RichText::new(tr(lang, "Locais")).strong().small());
                            #[cfg(windows)]
                            {
                                ui.horizontal_wrapped(|ui| {
                                    ui.spacing_mut().item_spacing.x = 2.0;
                                    for c in b'C'..=b'Z' {
                                        let d = format!("{}:\\", c as char);
                                        if std::path::Path::new(&d).exists()
                                            && ui.small_button(&d).clicked()
                                        {
                                            pending_nav = Some(PathBuf::from(&d));
                                        }
                                    }
                                });
                            }
                            #[cfg(not(windows))]
                            {
                                if ui.small_button("/").clicked() {
                                    pending_nav = Some(PathBuf::from("/"));
                                }
                                if let Ok(home) = std::env::var("HOME") {
                                    if ui.small_button("~").clicked() {
                                        pending_nav = Some(PathBuf::from(home));
                                    }
                                }
                            }
                            if !subdirs.is_empty() {
                                ui.separator();
                                ui.label(
                                    egui::RichText::new(tr(lang, "Subpastas"))
                                        .strong()
                                        .small(),
                                );
                                for (sp, sname) in &subdirs {
                                    ui.horizontal(|ui| {
                                        icons::paint_inline(ui, Icon::FolderItem, 12.0, &self.theme);
                                        if ui.selectable_label(false, sname).clicked() {
                                            pending_nav = Some(sp.clone());
                                        }
                                    });
                                }
                            }
                        });
                });
                ui.separator();
            }

            ui.vertical(|ui| {
                egui::ScrollArea::both()
                    .id_salt("explorer_files_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let visible: Vec<(bool, PathBuf, String, u64, bool)> = entries
                            .into_iter()
                            .filter_map(|(is_dir, p, name, size)| {
                                let is_proj = crate::model::is_project_file(&p);
                                if !is_dir && !crate::media::is_supported(&p) && !is_proj {
                                    return None;
                                }
                                if !filter.is_empty() && !name.to_lowercase().contains(&filter) {
                                    return None;
                                }
                                Some((is_dir, p, name, size, is_proj))
                            })
                            .collect();

                        if details_view {
                            let mut next_sort: Option<ExplorerSortCol> = None;
                            egui::Grid::new("explorer_details_grid")
                                .num_columns(3)
                                .striped(true)
                                .spacing([12.0, 3.0])
                                .show(ui, |ui| {
                                    let hdr = |ui: &mut egui::Ui,
                                               lbl: &str,
                                               col: ExplorerSortCol,
                                               out: &mut Option<ExplorerSortCol>| {
                                        let mark = if sort_col == col {
                                            if sort_asc { " ^" } else { " v" }
                                        } else {
                                            ""
                                        };
                                        if ui
                                            .button(egui::RichText::new(format!("{lbl}{mark}")).strong().small())
                                            .clicked()
                                        {
                                            *out = Some(col);
                                        }
                                    };
                                    hdr(ui, tr(lang, "Nome"), ExplorerSortCol::Name, &mut next_sort);
                                    hdr(ui, tr(lang, "Tipo"), ExplorerSortCol::Kind, &mut next_sort);
                                    hdr(
                                        ui,
                                        tr(lang, "Tamanho"),
                                        ExplorerSortCol::Size,
                                        &mut next_sort,
                                    );
                                    ui.end_row();

                                    for (is_dir, p, name, size, is_proj) in visible {
                                        let sel = self.explorer_ui.selected.as_ref() == Some(&p);
                                        let r = ui
                                            .horizontal(|ui| {
                                                icons::paint_inline(
                                                    ui,
                                                    if is_dir {
                                                        Icon::FolderItem
                                                    } else {
                                                        Icon::MediaItem
                                                    },
                                                    13.0,
                                                    &self.theme,
                                                );
                                                ui.selectable_label(sel, &name)
                                            })
                                            .inner;
                                        if r.clicked() {
                                            self.explorer_ui.selected = Some(p.clone());
                                        }
                                        if r.double_clicked() {
                                            if is_dir {
                                                pending_nav = Some(p.clone());
                                            } else if is_proj {
                                                pending_open_proj = Some(p.clone());
                                            } else {
                                                pending_import_at_cursor.push(p.clone());
                                            }
                                        }
                                        r.context_menu(|ui| {
                                            if is_dir {
                                                if ui.button(tr(lang, "Abrir pasta")).clicked() {
                                                    pending_nav = Some(p.clone());
                                                    ui.close_menu();
                                                }
                                                if ui
                                                    .button(tr(lang, "Adicionar aos favoritos"))
                                                    .clicked()
                                                {
                                                    self.explorer_ui.toggle_favorite(&p);
                                                    ui.close_menu();
                                                }
                                            } else if is_proj {
                                                if ui.button(tr(lang, "Abrir projeto")).clicked() {
                                                    pending_open_proj = Some(p.clone());
                                                    ui.close_menu();
                                                }
                                            } else {
                                                if ui
                                                    .button(tr(
                                                        lang,
                                                        "Importar e adicionar no cursor",
                                                    ))
                                                    .clicked()
                                                {
                                                    pending_import_at_cursor.push(p.clone());
                                                    ui.close_menu();
                                                }
                                                if ui
                                                    .button(tr(
                                                        lang,
                                                        "Adicionar à Mídia do Projeto",
                                                    ))
                                                    .clicked()
                                                {
                                                    pending_import_only.push(p.clone());
                                                    ui.close_menu();
                                                }
                                            }
                                        });
                                        let kind_txt = if is_dir {
                                            tr(lang, "Pasta").to_string()
                                        } else {
                                            p.extension()
                                                .map(|e| e.to_string_lossy().to_uppercase())
                                                .unwrap_or_else(|| "-".into())
                                        };
                                        ui.label(
                                            egui::RichText::new(kind_txt)
                                                .small()
                                                .color(self.theme.text_muted),
                                        );
                                        let sz_txt = if is_dir {
                                            String::new()
                                        } else {
                                            format_file_size(size)
                                        };
                                        ui.label(
                                            egui::RichText::new(sz_txt)
                                                .small()
                                                .monospace()
                                                .color(self.theme.text_muted),
                                        );
                                        ui.end_row();
                                    }
                                });
                            if let Some(col) = next_sort {
                                if self.explorer_ui.sort_col == col {
                                    self.explorer_ui.sort_asc = !self.explorer_ui.sort_asc;
                                } else {
                                    self.explorer_ui.sort_col = col;
                                    self.explorer_ui.sort_asc = true;
                                }
                            }
                        } else {
                            for (is_dir, p, name, _, is_proj) in visible {
                                let sel = self.explorer_ui.selected.as_ref() == Some(&p);
                                let r = ui
                                    .horizontal(|ui| {
                                        icons::paint_inline(
                                            ui,
                                            if is_dir {
                                                Icon::FolderItem
                                            } else {
                                                Icon::MediaItem
                                            },
                                            14.0,
                                            &self.theme,
                                        );
                                        ui.selectable_label(sel, name)
                                    })
                                    .inner;
                                if r.clicked() {
                                    self.explorer_ui.selected = Some(p.clone());
                                }
                                if r.double_clicked() {
                                    if is_dir {
                                        pending_nav = Some(p.clone());
                                    } else if is_proj {
                                        pending_open_proj = Some(p.clone());
                                    } else {
                                        pending_import_at_cursor.push(p.clone());
                                    }
                                }
                                if !is_dir {
                                    r.on_hover_text(tr(
                                        lang,
                                        "Duplo clique: importar e adicionar no cursor",
                                    ));
                                }
                            }
                        }
                    });
            });
        });

        if let Some(p) = pending_nav {
            let mut cur = self.explorer_dir.clone();
            self.explorer_ui.navigate(&mut cur, p);
            self.explorer_dir = cur;
        }
        if let Some(p) = pending_open_proj {
            self.open_project_path(p);
        }
        if !pending_import_at_cursor.is_empty() {
            let t = self.cursor;
            self.import_paths(pending_import_at_cursor, Some(t));
        }
        if !pending_import_only.is_empty() {
            self.import_paths(pending_import_only, None);
        }
    }

    // -----------------------------------------------------------------
    // Transições, efeitos e geradores
    // -----------------------------------------------------------------

    /// Janela Transições no arranjo do VEGAS Pro (manual 21, p. 53; ajuda
    /// "The Transitions Window"): busca, abas de categoria com Todos e
    /// Favoritos, a lista à esquerda e a grade de miniaturas à direita, que
    /// quebra de linha conforme a largura. Arrastar uma miniatura até um
    /// evento aplica; clique duplo aplica aos eventos selecionados.
    fn transitions_tab(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang();
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.transitions_ui.search)
                    .hint_text(tr(lang, "Buscar plug-ins"))
                    .desired_width(200.0),
            );
            ui.separator();
            let st = &mut self.transitions_ui;
            if ui
                .selectable_label(st.cat.is_none() && !st.favorites_only, tr(lang, "Todos"))
                .clicked()
            {
                st.cat = None;
                st.favorites_only = false;
            }
            for c in TransitionCategory::ALL {
                if ui
                    .selectable_label(st.cat == Some(c) && !st.favorites_only, c.name_in(lang))
                    .clicked()
                {
                    st.cat = Some(c);
                    st.favorites_only = false;
                }
            }
            if ui
                .selectable_label(st.favorites_only, format!("★ {}", tr(lang, "Favoritos")))
                .clicked()
            {
                st.cat = None;
                st.favorites_only = true;
            }
        });
        ui.separator();

        let q = self.transitions_ui.search.trim().to_lowercase();
        let favs = self.prefs.fx_favorites.clone();
        let list: Vec<(usize, TransitionKind)> = TransitionKind::ALL
            .into_iter()
            .enumerate()
            .filter(|(_, k)| {
                (self.transitions_ui.cat.is_none()
                    || self.transitions_ui.cat == Some(TransitionCategory::of(*k)))
                    && (!self.transitions_ui.favorites_only
                        || favs.contains(&transition_fav_id(*k)))
                    && (q.is_empty() || k.name_in(lang).to_lowercase().contains(&q))
            })
            .collect();
        if list.is_empty() {
            ui.label(
                egui::RichText::new(if self.transitions_ui.favorites_only {
                    tr(
                        lang,
                        "Nenhum favorito. Clique na estrela de uma transição para marcá-la.",
                    )
                } else {
                    tr(lang, "Nenhuma transição encontrada.")
                })
                .small()
                .weak(),
            );
            return;
        }
        if !self
            .transitions_ui
            .selected
            .is_some_and(|k| list.iter().any(|(_, x)| *x == k))
        {
            self.transitions_ui.selected = list.first().map(|(_, k)| *k);
        }

        let sel_events: Vec<Id> = self.sel_events.iter().cloned().collect();
        let left_w = (ui.available_width() * 0.30).clamp(140.0, 220.0);
        let visible_w = (ui.clip_rect().right() - ui.cursor().left()).min(ui.available_width());
        let right_w = (visible_w - left_w - 24.0).max(120.0);
        let body_h = (ui.available_height() - 28.0).max(80.0);
        let thumb = vec2(
            neoveg_engine::engine::fx_preview::THUMB_W as f32 * 1.25,
            neoveg_engine::engine::fx_preview::THUMB_H as f32 * 1.25,
        );
        let cell_w = thumb.x + 12.0;
        let per_row = ((right_w + 4.0) / cell_w).floor().max(1.0) as usize;
        let mut toggle_fav: Option<TransitionKind> = None;
        let mut select: Option<TransitionKind> = None;
        let mut apply: Option<TransitionKind> = None;
        let (accent, bg) = (self.theme.accent, self.theme.extreme_bg);

        ui.horizontal_top(|ui| {
            // esquerda: a lista com a estrela de favorito
            ui.vertical(|ui| {
                ui.set_width(left_w);
                egui::ScrollArea::vertical()
                    .id_salt("tr_lista")
                    .max_height(body_h)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for (_, k) in &list {
                            let fav = favs.contains(&transition_fav_id(*k));
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
                                let sel = self.transitions_ui.selected == Some(*k);
                                if ui.selectable_label(sel, k.name_in(lang)).clicked() {
                                    select = Some(*k);
                                }
                            });
                        }
                    });
            });
            ui.separator();
            // direita: grade de miniaturas (meio do progresso; anima ao passar o mouse)
            ui.vertical(|ui| {
                ui.set_width(right_w);
                egui::ScrollArea::vertical()
                    .id_salt("tr_grade")
                    .max_height(body_h)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for row in list.chunks(per_row) {
                            ui.horizontal_top(|ui| {
                                for (i, k) in row {
                                    let hover_id = egui::Id::new(("tr_hover", *i));
                                    let hovered = ui
                                        .ctx()
                                        .data(|d| d.get_temp::<bool>(hover_id))
                                        .unwrap_or(false);
                                    let ctx = ui.ctx().clone();
                                    let tex =
                                        self.fx_browser.transitions.texture(&ctx, *i, hovered);
                                    let selected = self.transitions_ui.selected == Some(*k);
                                    let id = egui::Id::new(("tr", *k as u32));
                                    let inner =
                                        ui.dnd_drag_source(id, DndPayload::Transition(*k), |ui| {
                                            ui.vertical(|ui| {
                                                ui.set_width(cell_w - 4.0);
                                                let (r, resp) =
                                                    ui.allocate_exact_size(thumb, Sense::click());
                                                match &tex {
                                                    Some(t) => {
                                                        ui.painter().image(
                                                            t.id(),
                                                            r,
                                                            Rect::from_min_max(
                                                                pos2(0.0, 0.0),
                                                                pos2(1.0, 1.0),
                                                            ),
                                                            Color32::WHITE,
                                                        );
                                                    }
                                                    None => {
                                                        ui.painter().rect_filled(r, 2.0, bg);
                                                    }
                                                }
                                                if selected {
                                                    ui.painter().rect_stroke(
                                                        r.expand(1.5),
                                                        2.0,
                                                        Stroke::new(2.0_f32, accent),
                                                        StrokeKind::Outside,
                                                    );
                                                }
                                                ui.add(
                                                    egui::Label::new(
                                                        egui::RichText::new(k.name_in(lang))
                                                            .small(),
                                                    )
                                                    .truncate(),
                                                );
                                                resp
                                            })
                                            .inner
                                        });
                                    let thumb_resp = inner.inner;
                                    let now_hover =
                                        inner.response.hovered() || thumb_resp.hovered();
                                    ui.ctx().data_mut(|d| d.insert_temp(hover_id, now_hover));
                                    if thumb_resp.clicked() {
                                        select = Some(*k);
                                    }
                                    if thumb_resp.double_clicked() {
                                        apply = Some(*k);
                                    }
                                    thumb_resp.on_hover_text(k.name_in(lang));
                                }
                            });
                            ui.add_space(4.0);
                        }
                    });
            });
        });

        // rodapé: a transição escolhida e o botão Aplicar
        ui.separator();
        ui.horizontal(|ui| {
            if let Some(k) = self.transitions_ui.selected {
                ui.label(
                    egui::RichText::new(format!(
                        "{} — {}",
                        k.name_in(lang),
                        TransitionCategory::of(k).name_in(lang)
                    ))
                    .small(),
                );
                if ui
                    .add_enabled(
                        !sel_events.is_empty(),
                        egui::Button::new(tr(lang, "Aplicar")),
                    )
                    .on_hover_text(tr(lang, "Aplicar aos eventos selecionados"))
                    .clicked()
                {
                    apply = Some(k);
                }
            }
        });

        if let Some(k) = select {
            self.transitions_ui.selected = Some(k);
        }
        if let Some(k) = apply {
            for e in &sel_events {
                self.apply_transition(*e, k);
            }
        }
        if let Some(k) = toggle_fav {
            let id = transition_fav_id(k);
            if let Some(pos) = self.prefs.fx_favorites.iter().position(|x| *x == id) {
                self.prefs.fx_favorites.remove(pos);
            } else {
                self.prefs.fx_favorites.push(id);
            }
            let _ = self.prefs.save();
        }
    }

    fn fx_tab(&mut self, ui: &mut egui::Ui, audio: bool) {
        let lang = self.lang();
        ui.label(
            egui::RichText::new(tr(
                lang,
                "Arraste um efeito até um evento ou trilha, ou use os botões para aplicar à seleção / trilha selecionada / master.",
            ))
            .small()
            .weak(),
        );
        ui.separator();
        let list: Vec<FxKind> = if audio {
            FxKind::AUDIO.to_vec()
        } else {
            FxKind::VIDEO.to_vec()
        };
        egui::Grid::new(("fx_grid", audio))
            .num_columns(4)
            .striped(true)
            .show(ui, |ui| {
                for k in list {
                    let id = egui::Id::new(("fx", k as u32));
                    ui.dnd_drag_source(id, DndPayload::Fx(k), |ui| {
                        icons::icon_label(
                            ui,
                            if audio {
                                Icon::FxAudio
                            } else {
                                Icon::FxSparkle
                            },
                            14.0,
                            k.name_in(lang),
                            &self.theme,
                        );
                    });
                    self.fx_apply_buttons(ui, k, audio);
                    ui.end_row();
                }
            });
    }

    /// Botões Eventos, Trilha e Master de um efeito (lista de áudio e
    /// janela Efeitos de Vídeo): aplicam à seleção do tipo certo, à trilha
    /// selecionada ou ao Master, com um passo de Desfazer.
    pub(crate) fn fx_apply_buttons(&mut self, ui: &mut egui::Ui, k: FxKind, audio: bool) {
        self.fx_apply_instance_buttons(ui, FxInstance::new(k), audio);
    }

    pub(crate) fn fx_apply_instance_buttons(
        &mut self,
        ui: &mut egui::Ui,
        fx: FxInstance,
        audio: bool,
    ) {
        let lang = self.lang();
        let want_kind = if audio {
            TrackKind::Audio
        } else {
            TrackKind::Video
        };
        let sel_ev: Vec<Id> = self
            .sel_events
            .iter()
            .filter(|id| {
                self.project
                    .find_event(**id)
                    .map(|(t, _)| self.project.tracks[t].kind == want_kind)
                    .unwrap_or(false)
            })
            .cloned()
            .collect();
        if ui
            .add_enabled(!sel_ev.is_empty(), egui::Button::new(tr(lang, "Eventos")))
            .clicked()
        {
            self.checkpoint();
            for (idx, id) in sel_ev.iter().enumerate() {
                if let Some(ev) = self.project.event_mut(*id) {
                    let mut inst = fx.clone();
                    if idx > 0 {
                        inst.id = new_id();
                    }
                    ev.fx.push(inst);
                }
            }
            self.dlg.fx_target = Some(FxTarget::Event(sel_ev[0]));
            self.changed();
        }
        let sel_tr = self
            .sel_track
            .filter(|&t| self.project.tracks.get(t).map(|x| x.kind) == Some(want_kind));
        if let (true, Some(t)) = (
            ui.add_enabled(sel_tr.is_some(), egui::Button::new(tr(lang, "Trilha")))
                .clicked(),
            sel_tr,
        ) {
            self.checkpoint();
            self.project.tracks[t].fx.push(fx.clone());
            self.dlg.fx_target = Some(FxTarget::Track(self.project.tracks[t].id));
            self.changed();
        }
        if ui.button(tr(lang, "Master")).clicked() {
            self.checkpoint();
            if audio {
                self.project.master.fx.push(fx);
                self.dlg.fx_target = Some(FxTarget::MasterAudio);
            } else {
                self.project.master.video_fx.push(fx);
                self.dlg.fx_target = Some(FxTarget::MasterVideo);
            }
            self.changed();
        }
    }

    // -----------------------------------------------------------------
    // Mixer
    // -----------------------------------------------------------------

    fn mixer_tab(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang();
        // teclas do Mixer (lista do VEGAS Pro 21, "Mixing Console") com o
        // mouse sobre o painel
        let area = ui.max_rect();
        self.mixer_ui.kb_rect = (ui.ctx().viewport_id() == egui::ViewportId::ROOT)
            .then(|| (area, ui.ctx().cumulative_pass_nr()));
        if !ui.ctx().wants_keyboard_input() && ui.rect_contains_pointer(area) {
            let keys: Vec<crate::panels::MixerKey> = ui.input_mut(|i| {
                let presses: Vec<(egui::Modifiers, egui::Key)> = i
                    .events
                    .iter()
                    .filter_map(|e| match e {
                        egui::Event::Key {
                            key,
                            pressed: true,
                            repeat: false,
                            modifiers,
                            ..
                        } => Some((*modifiers, *key)),
                        _ => None,
                    })
                    .collect();
                presses
                    .into_iter()
                    .filter_map(|(m, k)| {
                        let mk = mixer_key(m.command || m.ctrl, m.shift, m.alt, k)?;
                        i.consume_key(m, k).then_some(mk)
                    })
                    .collect()
            });
            if !keys.is_empty() {
                let tracks: Vec<Id> = self
                    .session
                    .project
                    .tracks
                    .iter()
                    .filter(|t| t.kind == TrackKind::Audio)
                    .map(|t| t.id)
                    .collect();
                let buses: Vec<Id> = self
                    .session
                    .project
                    .audio_buses
                    .iter()
                    .map(|b| b.id)
                    .collect();
                for k in keys {
                    self.mixer_ui.apply_key(k, &tracks, &buses);
                }
            }
        }
        let sr_s = self.project.settings.sample_rate.to_string();
        let bits_s = self.project.settings.audio_bit_depth.to_string();
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.label(
                egui::RichText::new(tr_args(
                    lang,
                    "{sr} Hz; {bits} bits",
                    &[("sr", &sr_s), ("bits", &bits_s)],
                ))
                .small()
                .monospace()
                .color(self.theme.text_muted),
            );
            ui.separator();
            if ui
                .selectable_label(self.mixer_ui.sidebar_open, tr(lang, "Canais"))
                .on_hover_text(tr(lang, "Mostrar/ocultar barra lateral de canais e seções"))
                .clicked()
            {
                self.mixer_ui.sidebar_open = !self.mixer_ui.sidebar_open;
            }
            if ui
                .button(tr(lang, "＋ Áudio"))
                .on_hover_text(tr(lang, "Inserir trilha de áudio"))
                .clicked()
            {
                let ctx = ui.ctx().clone();
                self.exec_cmd(Some(&ctx), "insert.track_audio");
            }
            let can_add = self.project.audio_buses.len() < crate::model::MAX_AUDIO_BUSES;
            if ui
                .add_enabled(can_add, egui::Button::new(tr(lang, "＋ Barramento")))
                .on_hover_text(tr(lang, "Inserir barramento auxiliar de áudio (até 26)"))
                .clicked()
            {
                let ctx = ui.ctx().clone();
                self.exec_cmd(Some(&ctx), "insert.audio_bus");
            }
            let n_s = self.project.audio_buses.len().to_string();
            let max_s = crate::model::MAX_AUDIO_BUSES.to_string();
            ui.label(
                egui::RichText::new(tr_args(
                    lang,
                    "{count}/{max} barramentos",
                    &[("count", &n_s), ("max", &max_s)],
                ))
                .small()
                .color(self.theme.text_muted),
            );
            ui.separator();
            let mut bypass = self.opts.bypass_all_audio_fx;
            if ui
                .toggle_value(&mut bypass, "Bypass FX")
                .on_hover_text(tr(
                    lang,
                    "Ignorar todos os plug-ins de efeito de áudio (Opções > Ignorar todos os FX de áudio)",
                ))
                .changed()
            {
                let ctx = ui.ctx().clone();
                self.exec_cmd(Some(&ctx), "options.bypass_all_audio_fx");
            }
        });
        ui.separator();

        // Remove IDs obsoletos de canais ocultos
        let live_track_ids: HashSet<Id> = self.project.tracks.iter().map(|t| t.id).collect();
        self.mixer_ui
            .hidden_tracks
            .retain(|id| live_track_ids.contains(id));
        let live_bus_ids: HashSet<Id> = self.project.audio_buses.iter().map(|b| b.id).collect();
        self.mixer_ui
            .hidden_buses
            .retain(|id| live_bus_ids.contains(id));

        let bus_choices: Vec<(Id, String)> = self
            .project
            .audio_buses
            .iter()
            .map(|b| (b.id, b.name.clone()))
            .collect();
        let mut remove_bus: Option<Id> = None;

        let show_sidebar = self.mixer_ui.sidebar_open && ui.available_width() >= 440.0;

        ui.horizontal_top(|ui| {
            if show_sidebar {
                ui.vertical(|ui| {
                    ui.set_width(168.0);
                    ui.label(
                        egui::RichText::new(tr(lang, "Visibilidade"))
                            .small()
                            .strong(),
                    );
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        for f in MixerChannelFilter::ALL {
                            if ui
                                .selectable_label(
                                    self.mixer_ui.channel_filter == f,
                                    egui::RichText::new(f.label_in(lang)).small(),
                                )
                                .clicked()
                            {
                                self.mixer_ui.channel_filter = f;
                            }
                        }
                    });
                    egui::ScrollArea::vertical()
                        .id_salt("mixer_sidebar_list")
                        .max_height(110.0)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            let mut audio_idx = 0usize;
                            for t in &self.session.project.tracks {
                                if t.kind != TrackKind::Audio {
                                    continue;
                                }
                                audio_idx += 1;
                                let mut vis = !self.mixer_ui.hidden_tracks.contains(&t.id);
                                ui.horizontal(|ui| {
                                    let badge_col =
                                        Color32::from_rgb(t.color[0], t.color[1], t.color[2]);
                                    let (br, _) =
                                        ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                                    ui.painter().rect_filled(br, 2.0, badge_col);
                                    ui.painter().text(
                                        br.center(),
                                        egui::Align2::CENTER_CENTER,
                                        format!("{audio_idx}"),
                                        egui::FontId::proportional(9.0),
                                        crate::theme::contrast_on(badge_col),
                                    );
                                    if ui.checkbox(&mut vis, &t.name).changed() {
                                        if vis {
                                            self.mixer_ui.hidden_tracks.remove(&t.id);
                                        } else {
                                            self.mixer_ui.hidden_tracks.insert(t.id);
                                        }
                                    }
                                });
                            }
                            for b in &self.session.project.audio_buses {
                                let mut vis = !self.mixer_ui.hidden_buses.contains(&b.id);
                                if ui.checkbox(&mut vis, &b.name).changed() {
                                    if vis {
                                        self.mixer_ui.hidden_buses.remove(&b.id);
                                    } else {
                                        self.mixer_ui.hidden_buses.insert(b.id);
                                    }
                                }
                            }
                            let mut m_vis = !self.mixer_ui.hide_master;
                            if ui.checkbox(&mut m_vis, tr(lang, "Master")).changed() {
                                self.mixer_ui.hide_master = !m_vis;
                            }
                        });
                    if (!self.mixer_ui.hidden_tracks.is_empty()
                        || !self.mixer_ui.hidden_buses.is_empty()
                        || self.mixer_ui.hide_master
                        || self.mixer_ui.channel_filter != MixerChannelFilter::All)
                        && ui.small_button(tr(lang, "Mostrar todos")).clicked()
                    {
                        self.mixer_ui.show_all_channels();
                    }
                    ui.separator();
                    ui.label(egui::RichText::new(tr(lang, "Seções")).small().strong());
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 3.0;
                        ui.toggle_value(&mut self.mixer_ui.show_io, "I/O");
                        ui.toggle_value(&mut self.mixer_ui.show_sends, tr(lang, "Envios"));
                        ui.toggle_value(&mut self.mixer_ui.show_automation, tr(lang, "Automação"));
                        ui.toggle_value(&mut self.mixer_ui.show_pan, "Pan");
                        ui.toggle_value(&mut self.mixer_ui.show_meters, tr(lang, "Medidores"));
                        ui.toggle_value(&mut self.mixer_ui.show_faders, tr(lang, "Faders"));
                    });
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(tr(lang, "Largura:")).small().weak());
                        if ui.small_button("-").clicked() {
                            self.mixer_ui.strip_width =
                                (self.mixer_ui.strip_width - 12.0).clamp(84.0, 156.0);
                        }
                        ui.add_sized(
                            [68.0, 16.0],
                            egui::Slider::new(&mut self.mixer_ui.strip_width, 84.0..=156.0)
                                .show_value(false),
                        );
                        if ui.small_button("+").clicked() {
                            self.mixer_ui.strip_width =
                                (self.mixer_ui.strip_width + 12.0).clamp(84.0, 156.0);
                        }
                    });
                });
                ui.separator();
            }

            let strip_w = self.mixer_ui.clamped_strip_width();
            let show_io = self.mixer_ui.show_io;
            let show_sends = self.mixer_ui.show_sends;
            let show_automation = self.mixer_ui.show_automation;
            let show_pan = self.mixer_ui.show_pan;
            let show_meters = self.mixer_ui.show_meters;
            let show_faders = self.mixer_ui.show_faders;
            let filter = self.mixer_ui.channel_filter;

            egui::ScrollArea::horizontal()
                .id_salt("mixer_channels_scroll")
                .show(ui, |ui| {
                    ui.horizontal_top(|ui| {
                        let show_tracks = matches!(
                            filter,
                            MixerChannelFilter::All | MixerChannelFilter::AudioTracks
                        );
                        let show_buses = matches!(
                            filter,
                            MixerChannelFilter::All | MixerChannelFilter::AudioBuses
                        );
                        let show_master = !self.mixer_ui.hide_master
                            && matches!(
                                filter,
                                MixerChannelFilter::All | MixerChannelFilter::MasterOnly
                            );

                        if show_tracks {
                            let n = self.project.tracks.len();
                            let mut audio_num = 0usize;
                            for ti in 0..n {
                                if self.project.tracks[ti].kind != TrackKind::Audio {
                                    continue;
                                }
                                audio_num += 1;
                                let tid = self.project.tracks[ti].id;
                                if self.mixer_ui.hidden_tracks.contains(&tid) {
                                    continue;
                                }
                                let lv = self.meters_hold.get(&tid).cloned().unwrap_or([0.0; 2]);
                                let pk_db = lin_to_db(lv[0].max(lv[1]));
                                let mut changed = false;
                                let mut checkpoint_needed = false;
                                let mut pending_toggle_mute = false;
                                let mut pending_toggle_solo = false;
                                let mut pending_toggle_phase = false;
                                let mut pending_toggle_rec = false;
                                let mut pending_output = self.project.tracks[ti].output_bus;
                                let mut pending_auto = self.project.tracks[ti].automation_mode;
                                let mut vol_reset = false;
                                let mut pan_reset = false;
                                let mut toggle_send_bus: Option<Id> = None;
                                ui.group(|ui| {
                                    ui.set_width(strip_w);
                                    ui.vertical(|ui| {
                                        let t_name = self.project.tracks[ti].name.clone();
                                        let t_col = self.project.tracks[ti].color;
                                        let t_mute = self.project.tracks[ti].mute;
                                        let t_solo = self.project.tracks[ti].solo;
                                        let t_phase = self.project.tracks[ti].invert_phase;
                                        let t_rec = self.project.tracks[ti].record_armed;
                                        let send_count = self.project.tracks[ti].sends.len();
                                        // Cabeçalho colorido numerado estilo VEGAS Pro (p. 63)
                                        let hdr_col = if self.theme.high_contrast {
                                            self.theme.selection_bg
                                        } else {
                                            Color32::from_rgb(t_col[0], t_col[1], t_col[2])
                                        };
                                        let hdr_fg = crate::theme::contrast_on(hdr_col);
                                        let (hdr_r, _) = ui.allocate_exact_size(
                                            vec2((strip_w - 6.0).max(60.0), 18.0),
                                            Sense::hover(),
                                        );
                                        ui.painter().rect_filled(hdr_r, 2.0, hdr_col);
                                        ui.painter().text(
                                            pos2(hdr_r.left() + 4.0, hdr_r.center().y),
                                            egui::Align2::LEFT_CENTER,
                                            format!("{audio_num}  {t_name}"),
                                            egui::FontId::proportional(10.5),
                                            hdr_fg,
                                        );

                                        if show_io && !bus_choices.is_empty() {
                                            let sel_lbl = pending_output
                                                .and_then(|bid| {
                                                    bus_choices
                                                        .iter()
                                                        .find(|(id, _)| *id == bid)
                                                        .map(|(_, nm)| nm.as_str())
                                                })
                                                .unwrap_or_else(|| tr(lang, "Master"));
                                            let before = pending_output;
                                            egui::ComboBox::from_id_salt(("mix_trk_bus", tid))
                                                .width((strip_w - 10.0).max(64.0))
                                                .selected_text(sel_lbl)
                                                .show_ui(ui, |ui| {
                                                    ui.selectable_value(
                                                        &mut pending_output,
                                                        None,
                                                        tr(lang, "Master"),
                                                    );
                                                    for (bid, bname) in &bus_choices {
                                                        ui.selectable_value(
                                                            &mut pending_output,
                                                            Some(*bid),
                                                            bname,
                                                        );
                                                    }
                                                });
                                            if pending_output != before {
                                                checkpoint_needed = true;
                                                changed = true;
                                            }
                                        }

                                        if show_automation {
                                            let before_a = pending_auto;
                                            egui::ComboBox::from_id_salt(("mix_trk_auto", tid))
                                                .width((strip_w - 10.0).max(64.0))
                                                .selected_text(pending_auto.short_label())
                                                .show_ui(ui, |ui| {
                                                    for am in AutomationMode::ALL {
                                                        ui.selectable_value(
                                                            &mut pending_auto,
                                                            am,
                                                            am.label(),
                                                        );
                                                    }
                                                });
                                            if pending_auto != before_a {
                                                checkpoint_needed = true;
                                                changed = true;
                                            }
                                        }

                                        if show_pan {
                                            let mut pan = self.project.tracks[ti].pan;
                                            let pan_r = ui.add_sized(
                                                [(strip_w - 10.0).max(60.0), 16.0],
                                                egui::Slider::new(&mut pan, -1.0..=1.0)
                                                    .show_value(false),
                                            );
                                            if pan_r.drag_started() {
                                                checkpoint_needed = true;
                                            }
                                            if pan_r.double_clicked() {
                                                checkpoint_needed = true;
                                                pan = 0.0;
                                                pan_reset = true;
                                            } else if pan_r.changed() {
                                                self.session.project.tracks[ti].pan = pan;
                                                changed = true;
                                            }
                                            ui.label(
                                                egui::RichText::new(EnvKind::Pan.format(pan))
                                                    .small()
                                                    .monospace()
                                                    .color(self.theme.text_muted),
                                            );
                                        }

                                        ui.horizontal_wrapped(|ui| {
                                            ui.spacing_mut().item_spacing.x = 2.0;
                                            if ui.selectable_label(t_mute, "M").clicked() {
                                                checkpoint_needed = true;
                                                pending_toggle_mute = true;
                                            }
                                            if ui.selectable_label(t_solo, "S").clicked() {
                                                checkpoint_needed = true;
                                                pending_toggle_solo = true;
                                            }
                                            if ui.small_button("FX").clicked() {
                                                self.open_fx_target(FxTarget::Track(tid));
                                            }
                                            if ui
                                                .selectable_label(t_phase, "Ø")
                                                .on_hover_text(tr(lang, "Inverter fase (180°)"))
                                                .clicked()
                                            {
                                                checkpoint_needed = true;
                                                pending_toggle_phase = true;
                                            }
                                            if ui
                                                .selectable_label(t_rec, "Rec")
                                                .on_hover_text(tr(
                                                    lang,
                                                    "Armar trilha para gravação (Rec)",
                                                ))
                                                .clicked()
                                            {
                                                checkpoint_needed = true;
                                                pending_toggle_rec = true;
                                            }
                                        });

                                        if show_meters {
                                            let pk_txt = if !pk_db.is_finite() || pk_db < -100.0 {
                                                "-inf".to_string()
                                            } else {
                                                format!("{pk_db:+.1}")
                                            };
                                            let pk_resp = ui
                                                .label(
                                                    egui::RichText::new(pk_txt)
                                                        .monospace()
                                                        .small()
                                                        .color(if pk_db > -0.1 {
                                                            self.theme.offline_fg
                                                        } else {
                                                            self.theme.text_muted
                                                        }),
                                                )
                                                .interact(Sense::click())
                                                .on_hover_text(tr(
                                                    lang,
                                                    "Clique para zerar os picos retidos",
                                                ));
                                            if pk_resp.clicked() {
                                                self.meters_hold.insert(tid, [0.0; 2]);
                                            }
                                        }

                                        let mut vol = self.project.tracks[ti].volume_db;
                                        let mut vol_new: Option<f32> = None;
                                        if show_faders || show_meters {
                                            ui.horizontal(|ui| {
                                                if show_faders {
                                                    let r = ui.add_sized(
                                                        [22.0, 102.0],
                                                        egui::Slider::new(&mut vol, -60.0..=12.0)
                                                            .vertical()
                                                            .show_value(false),
                                                    );
                                                    if r.drag_started() {
                                                        checkpoint_needed = true;
                                                    }
                                                    if r.double_clicked() {
                                                        checkpoint_needed = true;
                                                        vol = 0.0;
                                                        vol_reset = true;
                                                    } else if r.changed() {
                                                        vol_new = Some(vol);
                                                    }
                                                }
                                                if show_meters {
                                                    let (mr, _) = ui.allocate_exact_size(
                                                        vec2(14.0, 102.0),
                                                        Sense::hover(),
                                                    );
                                                    draw_meter(ui.painter(), mr, lv);
                                                }
                                            });
                                        }
                                        ui.label(
                                            egui::RichText::new(format!("{vol:+.1} dB"))
                                                .small()
                                                .monospace(),
                                        );

                                        if show_sends && !bus_choices.is_empty() {
                                            let send_lbl = if send_count > 0 {
                                                let sc_s = send_count.to_string();
                                                tr_args(lang, "Send ({n})", &[("n", &sc_s)])
                                            } else {
                                                tr(lang, "Send").into()
                                            };
                                            ui.menu_button(send_lbl, |ui| {
                                                for (bid, bname) in &bus_choices {
                                                    let active = self.session.project.tracks[ti]
                                                        .sends
                                                        .iter()
                                                        .any(|s| s.bus_id == *bid);
                                                    if ui.selectable_label(active, bname).clicked()
                                                    {
                                                        toggle_send_bus = Some(*bid);
                                                    }
                                                }
                                                let sc =
                                                    self.session.project.tracks[ti].sends.len();
                                                for si in 0..sc {
                                                    ui.separator();
                                                    let bid = self.session.project.tracks[ti].sends
                                                        [si]
                                                        .bus_id;
                                                    let bn = bus_choices
                                                        .iter()
                                                        .find(|(id, _)| *id == bid)
                                                        .map(|(_, nm)| nm.as_str())
                                                        .unwrap_or_else(|| tr(lang, "Bus"));
                                                    let mut pre = self.session.project.tracks[ti]
                                                        .sends[si]
                                                        .pre_fader;
                                                    let mut sdb = self.session.project.tracks[ti]
                                                        .sends[si]
                                                        .level_db;
                                                    ui.horizontal(|ui| {
                                                        ui.label(egui::RichText::new(bn).small());
                                                        if ui
                                                            .toggle_value(&mut pre, tr(lang, "Pré"))
                                                            .changed()
                                                        {
                                                            self.checkpoint();
                                                            self.session.project.tracks[ti].sends
                                                                [si]
                                                                .pre_fader = pre;
                                                            changed = true;
                                                        }
                                                    });
                                                    let sr = ui.add(
                                                        egui::Slider::new(&mut sdb, -60.0..=12.0)
                                                            .suffix(" dB")
                                                            .fixed_decimals(1),
                                                    );
                                                    if sr.drag_started() {
                                                        self.checkpoint();
                                                    }
                                                    if sr.changed() {
                                                        self.session.project.tracks[ti].sends[si]
                                                            .level_db = sdb;
                                                        changed = true;
                                                    }
                                                }
                                            });
                                        }
                                        if let Some(v) = vol_new {
                                            if !checkpoint_needed {
                                                self.checkpoint();
                                            }
                                            self.session.project.tracks[ti].volume_db = v;
                                            changed = true;
                                        }
                                    });
                                });
                                if checkpoint_needed {
                                    self.checkpoint();
                                }
                                if pending_toggle_mute {
                                    self.session.project.tracks[ti].mute =
                                        !self.session.project.tracks[ti].mute;
                                    changed = true;
                                }
                                if pending_toggle_solo {
                                    self.session.project.tracks[ti].solo =
                                        !self.session.project.tracks[ti].solo;
                                    changed = true;
                                }
                                if pending_toggle_phase {
                                    self.session.project.tracks[ti].invert_phase =
                                        !self.session.project.tracks[ti].invert_phase;
                                    changed = true;
                                }
                                if pending_toggle_rec {
                                    self.session.project.tracks[ti].record_armed =
                                        !self.session.project.tracks[ti].record_armed;
                                    changed = true;
                                }
                                if self.session.project.tracks[ti].output_bus != pending_output {
                                    self.session.project.tracks[ti].output_bus = pending_output;
                                    changed = true;
                                }
                                if self.session.project.tracks[ti].automation_mode != pending_auto {
                                    self.session.project.tracks[ti].automation_mode = pending_auto;
                                    changed = true;
                                }
                                if vol_reset {
                                    self.session.project.tracks[ti].volume_db = 0.0;
                                    changed = true;
                                }
                                if pan_reset {
                                    self.session.project.tracks[ti].pan = 0.0;
                                    changed = true;
                                }
                                if let Some(bid) = toggle_send_bus {
                                    self.checkpoint();
                                    let sends = &mut self.session.project.tracks[ti].sends;
                                    if sends.iter().any(|s| s.bus_id == bid) {
                                        sends.retain(|s| s.bus_id != bid);
                                    } else {
                                        sends.push(crate::model::AudioSend {
                                            bus_id: bid,
                                            level_db: -6.0,
                                            pre_fader: false,
                                            mute: false,
                                        });
                                    }
                                    changed = true;
                                }
                                if changed {
                                    self.changed();
                                }
                            }
                        }

                        // Barramentos auxiliares (Bus A .. Bus Z)
                        if show_buses {
                            let nb = self.project.audio_buses.len();
                            for bi in 0..nb {
                                let bid = self.project.audio_buses[bi].id;
                                if self.mixer_ui.hidden_buses.contains(&bid) {
                                    continue;
                                }
                                let lv = self.meters_hold.get(&bid).cloned().unwrap_or([0.0; 2]);
                                let pk_db = lin_to_db(lv[0].max(lv[1]));
                                let valid_targets: Vec<(Id, String)> = bus_choices
                                    .iter()
                                    .filter(|(cand, _)| {
                                        *cand != bid
                                            && self
                                                .session
                                                .project
                                                .validate_bus_route(bid, Some(*cand))
                                    })
                                    .cloned()
                                    .collect();
                                let mut changed = false;
                                let mut checkpoint_needed = false;
                                let mut toggle_mute = false;
                                let mut toggle_solo = false;
                                let mut toggle_phase = false;
                                let mut pending_out =
                                    self.session.project.audio_buses[bi].output_bus;
                                let mut pending_auto =
                                    self.session.project.audio_buses[bi].automation_mode;
                                let mut vol_reset = false;
                                let mut pan_reset = false;
                                let mut commit_name: Option<String> = None;
                                ui.group(|ui| {
                                    ui.set_width(strip_w);
                                    ui.vertical(|ui| {
                                        let cur_name =
                                            self.session.project.audio_buses[bi].name.clone();
                                        let buf_id = egui::Id::new(("mix_bus_name", bid));
                                        let mut draft = ui.data_mut(|d| {
                                            d.get_temp::<String>(buf_id)
                                                .unwrap_or_else(|| cur_name.clone())
                                        });
                                        ui.horizontal(|ui| {
                                            let nr = ui.add(
                                                egui::TextEdit::singleline(&mut draft)
                                                    .desired_width((strip_w - 34.0).max(52.0))
                                                    .font(egui::TextStyle::Small),
                                            );
                                            if nr.has_focus() {
                                                ui.data_mut(|d| {
                                                    d.insert_temp(buf_id, draft.clone())
                                                });
                                            } else if nr.lost_focus() {
                                                commit_name = Some(draft.clone());
                                                ui.data_mut(|d| d.remove::<String>(buf_id));
                                            } else if draft != cur_name {
                                                ui.data_mut(|d| d.remove::<String>(buf_id));
                                            }
                                            if ui
                                                .small_button("×")
                                                .on_hover_text(tr(lang, "Remover barramento"))
                                                .clicked()
                                            {
                                                remove_bus = Some(bid);
                                            }
                                        });
                                        if show_io {
                                            let sel_lbl = pending_out
                                                .and_then(|out_id| {
                                                    bus_choices
                                                        .iter()
                                                        .find(|(id, _)| *id == out_id)
                                                        .map(|(_, nm)| nm.as_str())
                                                })
                                                .unwrap_or_else(|| tr(lang, "Master"));
                                            let before = pending_out;
                                            egui::ComboBox::from_id_salt(("mix_bus_out", bid))
                                                .width((strip_w - 10.0).max(64.0))
                                                .selected_text(sel_lbl)
                                                .show_ui(ui, |ui| {
                                                    ui.selectable_value(
                                                        &mut pending_out,
                                                        None,
                                                        tr(lang, "Master"),
                                                    );
                                                    for (cand, cname) in &valid_targets {
                                                        ui.selectable_value(
                                                            &mut pending_out,
                                                            Some(*cand),
                                                            cname,
                                                        );
                                                    }
                                                });
                                            if pending_out != before {
                                                checkpoint_needed = true;
                                                changed = true;
                                            }
                                        }
                                        if show_automation {
                                            let before_a = pending_auto;
                                            egui::ComboBox::from_id_salt(("mix_bus_auto", bid))
                                                .width((strip_w - 10.0).max(64.0))
                                                .selected_text(pending_auto.short_label())
                                                .show_ui(ui, |ui| {
                                                    for am in AutomationMode::ALL {
                                                        ui.selectable_value(
                                                            &mut pending_auto,
                                                            am,
                                                            am.label(),
                                                        );
                                                    }
                                                });
                                            if pending_auto != before_a {
                                                checkpoint_needed = true;
                                                changed = true;
                                            }
                                        }
                                        if show_pan {
                                            let mut pan = self.session.project.audio_buses[bi].pan;
                                            let pan_r = ui.add_sized(
                                                [(strip_w - 10.0).max(60.0), 16.0],
                                                egui::Slider::new(&mut pan, -1.0..=1.0)
                                                    .show_value(false),
                                            );
                                            if pan_r.drag_started() {
                                                checkpoint_needed = true;
                                            }
                                            if pan_r.double_clicked() {
                                                checkpoint_needed = true;
                                                pan = 0.0;
                                                pan_reset = true;
                                            }
                                            if pan_r.changed() {
                                                self.session.project.audio_buses[bi].pan = pan;
                                                changed = true;
                                            }
                                            ui.label(
                                                egui::RichText::new(EnvKind::Pan.format(pan))
                                                    .small()
                                                    .monospace()
                                                    .color(self.theme.text_muted),
                                            );
                                        }
                                        let b_mute = self.session.project.audio_buses[bi].mute;
                                        let b_solo = self.session.project.audio_buses[bi].solo;
                                        let b_phase =
                                            self.session.project.audio_buses[bi].invert_phase;
                                        ui.horizontal_wrapped(|ui| {
                                            ui.spacing_mut().item_spacing.x = 2.0;
                                            if ui.selectable_label(b_mute, "M").clicked() {
                                                checkpoint_needed = true;
                                                toggle_mute = true;
                                            }
                                            if ui.selectable_label(b_solo, "S").clicked() {
                                                checkpoint_needed = true;
                                                toggle_solo = true;
                                            }
                                            if ui.small_button("FX").clicked() {
                                                self.open_fx_target(FxTarget::AudioBus(bid));
                                            }
                                            if ui
                                                .selectable_label(b_phase, "Ø")
                                                .on_hover_text(tr(lang, "Inverter fase (180°)"))
                                                .clicked()
                                            {
                                                checkpoint_needed = true;
                                                toggle_phase = true;
                                            }
                                        });
                                        if show_meters {
                                            let pk_txt = if !pk_db.is_finite() || pk_db < -100.0 {
                                                "-inf".to_string()
                                            } else {
                                                format!("{pk_db:+.1}")
                                            };
                                            let pk_resp = ui
                                                .label(
                                                    egui::RichText::new(pk_txt)
                                                        .monospace()
                                                        .small()
                                                        .color(if pk_db > -0.1 {
                                                            self.theme.offline_fg
                                                        } else {
                                                            self.theme.text_muted
                                                        }),
                                                )
                                                .interact(Sense::click())
                                                .on_hover_text(tr(
                                                    lang,
                                                    "Clique para zerar os picos retidos",
                                                ));
                                            if pk_resp.clicked() {
                                                self.meters_hold.insert(bid, [0.0; 2]);
                                            }
                                        }
                                        let mut vol =
                                            self.session.project.audio_buses[bi].volume_db;
                                        if show_faders || show_meters {
                                            ui.horizontal(|ui| {
                                                if show_faders {
                                                    let r = ui.add_sized(
                                                        [22.0, 102.0],
                                                        egui::Slider::new(&mut vol, -60.0..=12.0)
                                                            .vertical()
                                                            .show_value(false),
                                                    );
                                                    if r.drag_started() {
                                                        checkpoint_needed = true;
                                                    }
                                                    if r.double_clicked() {
                                                        checkpoint_needed = true;
                                                        vol = 0.0;
                                                        vol_reset = true;
                                                    }
                                                    if r.changed() {
                                                        self.session.project.audio_buses[bi]
                                                            .volume_db = vol;
                                                        changed = true;
                                                    }
                                                }
                                                if show_meters {
                                                    let (mr, _) = ui.allocate_exact_size(
                                                        vec2(14.0, 102.0),
                                                        Sense::hover(),
                                                    );
                                                    draw_meter(ui.painter(), mr, lv);
                                                }
                                            });
                                        }
                                        ui.label(
                                            egui::RichText::new(format!("{vol:+.1} dB"))
                                                .small()
                                                .monospace(),
                                        );
                                    });
                                });
                                if let Some(new_name) = commit_name {
                                    self.session.rename_audio_bus(bid, new_name);
                                }
                                if checkpoint_needed {
                                    self.checkpoint();
                                }
                                if toggle_mute {
                                    self.session.project.audio_buses[bi].mute =
                                        !self.session.project.audio_buses[bi].mute;
                                    changed = true;
                                }
                                if toggle_solo {
                                    self.session.project.audio_buses[bi].solo =
                                        !self.session.project.audio_buses[bi].solo;
                                    changed = true;
                                }
                                if toggle_phase {
                                    self.session.project.audio_buses[bi].invert_phase =
                                        !self.session.project.audio_buses[bi].invert_phase;
                                    changed = true;
                                }
                                if self.session.project.audio_buses[bi].output_bus != pending_out {
                                    self.session.project.audio_buses[bi].output_bus = pending_out;
                                    changed = true;
                                }
                                if self.session.project.audio_buses[bi].automation_mode
                                    != pending_auto
                                {
                                    self.session.project.audio_buses[bi].automation_mode =
                                        pending_auto;
                                    changed = true;
                                }
                                if vol_reset {
                                    self.session.project.audio_buses[bi].volume_db = 0.0;
                                    changed = true;
                                }
                                if pan_reset {
                                    self.session.project.audio_buses[bi].pan = 0.0;
                                    changed = true;
                                }
                                if changed {
                                    self.changed();
                                }
                            }
                        }

                        // Master Bus no Console
                        if show_master {
                            ui.group(|ui| {
                                ui.set_width(strip_w.max(114.0));
                                ui.vertical(|ui| {
                                    ui.label(egui::RichText::new(tr(lang, "Master")).strong());
                                    self.master_bus_controls(ui, false);
                                });
                            });
                        }
                    });
                });
        });

        if let Some(bid) = remove_bus {
            self.session.remove_audio_bus(bid);
        }
    }

    // -----------------------------------------------------------------
    // Medidores de Loudness (EBU R128 / ITU-R BS.1770-4)
    // -----------------------------------------------------------------

    pub fn cancel_loudness_scan(&mut self) {
        if let Some(job) = &self.loudness_job {
            job.cancel.store(true, Ordering::Relaxed);
        }
    }

    pub fn start_loudness_scan(&mut self, range: Option<(Ticks, Ticks)>) {
        if self.loudness_job.is_some() {
            return;
        }
        let lang = self.lang();
        let proj = self.project.clone();
        let media = self.media.clone();
        let sr = self.audio.sr.max(8_000);
        let cancel = Arc::new(AtomicBool::new(false));
        let done = Arc::new(AtomicU64::new(0));
        let total = Arc::new(AtomicU64::new(1));
        let cancel_bg = cancel.clone();
        let done_bg = done.clone();
        let total_bg = total.clone();
        let tx = self.loader_tx.clone();
        self.loading += 1;
        self.loudness_job = Some(LoudnessJob {
            cancel,
            done,
            total,
        });
        self.set_status(tr(lang, "Analisando loudness EBU R128 do projeto..."));
        let err_msg = tr(lang, "falha interna isolada ao analisar loudness").to_string();
        if let Err(e) = util::spawn("loudness-scan", move || {
            let res = crate::util::catch_isolated(|| {
                analyze_project_loudness(
                    &proj,
                    &media,
                    range,
                    sr,
                    &cancel_bg,
                    Some((&done_bg, &total_bg)),
                )
            });
            let msg = match res {
                Ok(r) => r,
                Err(_) => Err(err_msg),
            };
            let _ = tx.send(LoadMsg::LoudnessAnalyzed(msg));
        }) {
            self.loading = self.loading.saturating_sub(1);
            self.loudness_job = None;
            self.set_status(e);
        }
    }

    fn loudness_tab(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang();
        let target = self.loudness_target;
        let target_lufs = target.target_lufs();
        let max_tp = target.max_true_peak_dbtp();

        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(tr(lang, "Padrão:")).strong().small());
            egui::ComboBox::from_id_salt("loudness_target_preset")
                .selected_text(self.loudness_target.name_in(lang))
                .show_ui(ui, |ui| {
                    for preset in LoudnessTarget::ALL {
                        ui.selectable_value(
                            &mut self.loudness_target,
                            preset,
                            preset.name_in(lang),
                        );
                    }
                });
            if ui
                .button(tr(lang, "Zerar"))
                .on_hover_text(tr(lang, "Zera a integração EBU R128 e os picos acumulados"))
                .clicked()
            {
                self.audio.reset_loudness();
                self.loudness_live = LoudnessSnapshot::default();
            }
            if let Some(job) = &self.loudness_job {
                let done = job.done.load(Ordering::Relaxed);
                let total = job.total.load(Ordering::Relaxed).max(1);
                let frac = (done as f32 / total as f32).clamp(0.0, 1.0);
                ui.add(
                    egui::ProgressBar::new(frac)
                        .desired_width(120.0)
                        .show_percentage(),
                );
                if ui.button(tr(lang, "Cancelar")).clicked() {
                    job.cancel.store(true, Ordering::Relaxed);
                }
            } else {
                if ui
                    .button(tr(lang, "Analisar projeto"))
                    .on_hover_text(tr(lang, "Varredura offline EBU R128 de todo o projeto"))
                    .clicked()
                {
                    self.start_loudness_scan(None);
                }
                if let Some(sel) = self.time_sel {
                    if sel.1 > sel.0
                        && ui
                            .button(tr(lang, "Analisar seleção"))
                            .on_hover_text(tr(lang, "Varredura offline apenas na seleção de tempo"))
                            .clicked()
                    {
                        self.start_loudness_scan(Some(sel));
                    }
                }
            }
        });
        ui.separator();

        let snap = self.loudness_live.clone();
        let meter_col = |val: f64, limit: f64| -> Color32 {
            if !val.is_finite() {
                self.theme.text_muted
            } else if val > limit + 0.5 {
                self.theme.offline_fg
            } else if val >= limit - 1.5 {
                Color32::from_rgb(230, 180, 45)
            } else {
                self.theme.accent
            }
        };

        let draw_vert_bar = |ui: &mut egui::Ui,
                             label: &str,
                             val: f64,
                             min_db: f64,
                             max_db: f64,
                             ref_db: f64,
                             ticks: &[i32],
                             is_tp: bool| {
            ui.vertical(|ui| {
                ui.set_width(40.0);
                let top_txt = if !val.is_finite() {
                    "-inf".to_string()
                } else {
                    format!("{val:.1}")
                };
                ui.label(
                    egui::RichText::new(top_txt)
                        .monospace()
                        .small()
                        .color(meter_col(val, ref_db)),
                );
                let (rect, _) = ui.allocate_exact_size(vec2(30.0, 128.0), Sense::hover());
                let p = ui.painter_at(rect);
                p.rect_filled(rect, 2.0, self.theme.extreme_bg);
                p.rect_stroke(
                    rect,
                    2.0,
                    Stroke::new(1.0_f32, self.theme.border),
                    StrokeKind::Inside,
                );
                let norm = |v: f64| -> f32 {
                    if !v.is_finite() {
                        0.0
                    } else {
                        ((v - min_db) / (max_db - min_db).max(1.0)).clamp(0.0, 1.0) as f32
                    }
                };
                let fill_n = norm(val);
                if fill_n > 0.0 {
                    let fill_h = (rect.height() - 2.0) * fill_n;
                    let fr = Rect::from_min_max(
                        pos2(rect.left() + 2.0, rect.bottom() - 1.0 - fill_h),
                        pos2(rect.right() - 2.0, rect.bottom() - 1.0),
                    );
                    let bar_col = if self.theme.high_contrast {
                        meter_col(val, ref_db)
                    } else if is_tp {
                        if val > ref_db {
                            self.theme.offline_fg
                        } else if val > ref_db - 3.0 {
                            Color32::from_rgb(235, 210, 45)
                        } else {
                            Color32::from_rgb(45, 210, 75)
                        }
                    } else if val > ref_db + 0.5 {
                        Color32::from_rgb(205, 90, 240)
                    } else {
                        Color32::from_rgb(45, 215, 235)
                    };
                    p.rect_filled(fr, 1.0, bar_col);
                }
                let ref_y = rect.bottom() - 1.0 - (rect.height() - 2.0) * norm(ref_db);
                p.line_segment(
                    [pos2(rect.left(), ref_y), pos2(rect.right(), ref_y)],
                    Stroke::new(1.5_f32, self.theme.text_primary),
                );
                let tick_font = egui::FontId::monospace(8.0);
                let tick_col = if self.theme.high_contrast {
                    self.theme.text_primary
                } else {
                    Color32::from_white_alpha(200)
                };
                for &tk in ticks {
                    let y = rect.bottom() - 1.0 - (rect.height() - 2.0) * norm(f64::from(tk));
                    if y >= rect.top() + 5.0 && y <= rect.bottom() - 5.0 {
                        p.text(
                            pos2(rect.right() - 3.0, y),
                            egui::Align2::RIGHT_CENTER,
                            format!("{tk}"),
                            tick_font.clone(),
                            tick_col,
                        );
                    }
                }
                ui.label(egui::RichText::new(label).strong().small());
            });
        };

        let draw_led = |ui: &mut egui::Ui, over: bool, active: bool| {
            let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
            let col = if over {
                self.theme.offline_fg
            } else if active {
                Color32::from_rgb(65, 200, 95)
            } else {
                self.theme.border
            };
            ui.painter().circle_filled(r.center(), 4.5, col);
        };

        egui::ScrollArea::vertical()
            .id_salt("loudness_tab_scroll")
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    // Coluna esquerda numérica + LEDs de conformidade (VEGAS Pro 21, p. 64)
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new(tr(lang, "Tempo real (Master)"))
                                .strong()
                                .small(),
                        );
                        egui::Grid::new("loudness_live_grid")
                            .num_columns(3)
                            .spacing([8.0, 4.0])
                            .show(ui, |ui| {
                                ui.label(tr(lang, "Momentâneo (M):"));
                                ui.label(
                                    egui::RichText::new(format_lufs(snap.momentary_lufs))
                                        .monospace(),
                                );
                                ui.label("");
                                ui.end_row();
                                ui.label(tr(lang, "Curto prazo (S):"));
                                ui.label(
                                    egui::RichText::new(format_lufs(snap.short_term_lufs))
                                        .monospace(),
                                );
                                ui.label("");
                                ui.end_row();
                                ui.label(tr(lang, "Integrado (I):"));
                                let int_col = meter_col(snap.integrated_lufs, target_lufs);
                                ui.label(
                                    egui::RichText::new(format_lufs(snap.integrated_lufs))
                                        .monospace()
                                        .strong()
                                        .color(int_col),
                                );
                                draw_led(
                                    ui,
                                    snap.integrated_lufs.is_finite()
                                        && snap.integrated_lufs > target_lufs + 0.5,
                                    snap.integrated_lufs.is_finite(),
                                );
                                ui.end_row();
                                ui.label(tr(lang, "LRA:"));
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{:.1} LU",
                                        snap.loudness_range_lu
                                    ))
                                    .monospace(),
                                );
                                ui.label("");
                                ui.end_row();
                                ui.label(tr(lang, "True Peak (L / R):"));
                                let tp_max = snap.true_peak_dbtp[0].max(snap.true_peak_dbtp[1]);
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{} / {}",
                                        format_db(snap.true_peak_dbtp[0], "dBTP"),
                                        format_db(snap.true_peak_dbtp[1], "dBTP"),
                                    ))
                                    .monospace()
                                    .color(meter_col(tp_max, max_tp)),
                                );
                                draw_led(
                                    ui,
                                    tp_max.is_finite() && tp_max > max_tp,
                                    tp_max.is_finite(),
                                );
                                ui.end_row();
                                ui.label(tr(lang, "RMS (L / R):"));
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{} / {}",
                                        format_db(snap.rms_dbfs[0], "dBFS"),
                                        format_db(snap.rms_dbfs[1], "dBFS"),
                                    ))
                                    .monospace(),
                                );
                                ui.label("");
                                ui.end_row();
                                if snap.integrated_lufs.is_finite() {
                                    let delta = snap.integrated_lufs - target_lufs;
                                    ui.label(tr(lang, "Desvio do alvo:"));
                                    ui.label(
                                        egui::RichText::new(format!("{delta:+.1} LU"))
                                            .monospace()
                                            .color(meter_col(snap.integrated_lufs, target_lufs)),
                                    );
                                    ui.label("");
                                    ui.end_row();
                                }
                            });
                    });
                    ui.add_space(10.0);
                    // Banco de 6 barras verticais graduadas (M, S, I, LRA, TP L, TP R — p. 64)
                    let lu_ticks = [-42, -36, -30, -23, -18, -12, -6];
                    let lra_ticks = [0, 5, 10, 15, 20];
                    let tp_ticks = [-48, -36, -24, -12, -6, -3, 0];
                    draw_vert_bar(
                        ui,
                        "M",
                        snap.momentary_lufs,
                        -50.0,
                        0.0,
                        target_lufs,
                        &lu_ticks,
                        false,
                    );
                    draw_vert_bar(
                        ui,
                        "S",
                        snap.short_term_lufs,
                        -50.0,
                        0.0,
                        target_lufs,
                        &lu_ticks,
                        false,
                    );
                    draw_vert_bar(
                        ui,
                        "I",
                        snap.integrated_lufs,
                        -50.0,
                        0.0,
                        target_lufs,
                        &lu_ticks,
                        false,
                    );
                    draw_vert_bar(
                        ui,
                        "LRA",
                        snap.loudness_range_lu,
                        0.0,
                        25.0,
                        12.0,
                        &lra_ticks,
                        false,
                    );
                    draw_vert_bar(
                        ui,
                        "TP L",
                        snap.true_peak_dbtp[0],
                        -57.0,
                        3.0,
                        max_tp,
                        &tp_ticks,
                        true,
                    );
                    draw_vert_bar(
                        ui,
                        "TP R",
                        snap.true_peak_dbtp[1],
                        -57.0,
                        3.0,
                        max_tp,
                        &tp_ticks,
                        true,
                    );
                });

                if let Some(rep) = &self.loudness_report {
                    ui.separator();
                    ui.group(|ui| {
                        ui.horizontal_wrapped(|ui| {
                            let a_s = self.fmt_time(rep.start);
                            let b_s = self.fmt_time(rep.end);
                            ui.label(
                                egui::RichText::new(tr_args(
                                    lang,
                                    "Varredura offline ({start} – {end})",
                                    &[("start", &a_s), ("end", &b_s)],
                                ))
                                .strong()
                                .small(),
                            );
                            let over_i = rep.snapshot.integrated_lufs.is_finite()
                                && rep.snapshot.integrated_lufs > target_lufs + 1.0;
                            let tp_max =
                                rep.snapshot.true_peak_dbtp[0].max(rep.snapshot.true_peak_dbtp[1]);
                            let over_tp = tp_max.is_finite() && tp_max > max_tp;
                            let (badge, col) = if over_i || over_tp {
                                (tr(lang, "Acima do limite"), self.theme.offline_fg)
                            } else if rep.snapshot.integrated_lufs.is_finite() {
                                (tr(lang, "Em conformidade"), self.theme.accent)
                            } else {
                                (tr(lang, "Silêncio"), self.theme.text_muted)
                            };
                            ui.label(egui::RichText::new(badge).strong().small().color(col));
                        });
                        egui::Grid::new("loudness_report_grid")
                            .num_columns(4)
                            .spacing([12.0, 4.0])
                            .show(ui, |ui| {
                                ui.label(tr(lang, "Integrado:"));
                                ui.label(
                                    egui::RichText::new(format_lufs(rep.snapshot.integrated_lufs))
                                        .monospace(),
                                );
                                ui.label(tr(lang, "LRA:"));
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{:.1} LU",
                                        rep.snapshot.loudness_range_lu
                                    ))
                                    .monospace(),
                                );
                                ui.end_row();
                                ui.label(tr(lang, "Máx. S:"));
                                ui.label(
                                    egui::RichText::new(format_lufs(rep.max_short_term_lufs))
                                        .monospace(),
                                );
                                ui.label(tr(lang, "Máx. M:"));
                                ui.label(
                                    egui::RichText::new(format_lufs(rep.max_momentary_lufs))
                                        .monospace(),
                                );
                                ui.end_row();
                                ui.label(tr(lang, "True Peak L:"));
                                ui.label(
                                    egui::RichText::new(format_db(
                                        rep.snapshot.true_peak_dbtp[0],
                                        "dBTP",
                                    ))
                                    .monospace(),
                                );
                                ui.label(tr(lang, "True Peak R:"));
                                ui.label(
                                    egui::RichText::new(format_db(
                                        rep.snapshot.true_peak_dbtp[1],
                                        "dBTP",
                                    ))
                                    .monospace(),
                                );
                                ui.end_row();
                            });
                    });
                }
            });
    }

    // -----------------------------------------------------------------
    // Escopos de Vídeo (manual VEGAS Pro 21, p. 61)
    // -----------------------------------------------------------------

    fn scopes_tab(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang();
        let mut changed = false;
        // Barra superior compacta estilo VEGAS Pro 21 (p. 61): Layout ("Todos" ou individual), Escala e Matriz
        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_id_salt("sc_layout_mode")
                .selected_text(self.scopes_layout.label_in(lang))
                .show_ui(ui, |ui| {
                    for ly in ScopeViewLayout::ALL {
                        ui.selectable_value(&mut self.scopes_layout, ly, ly.label_in(lang));
                    }
                });

            ui.separator();
            ui.label(tr(lang, "Escala:"));
            egui::ComboBox::from_id_salt("sc_scale")
                .selected_text(self.scopes_config.waveform_scale.label())
                .show_ui(ui, |ui| {
                    for s in neoveg_engine::scopes::WaveformScale::ALL {
                        if ui
                            .selectable_value(&mut self.scopes_config.waveform_scale, s, s.label())
                            .changed()
                        {
                            changed = true;
                        }
                    }
                });

            ui.separator();
            ui.label(tr(lang, "Matriz:"));
            egui::ComboBox::from_id_salt("sc_matrix")
                .selected_text(self.scopes_config.matrix.label())
                .show_ui(ui, |ui| {
                    for m in neoveg_engine::scopes::ScopeColorMatrix::ALL {
                        if ui
                            .selectable_value(&mut self.scopes_config.matrix, m, m.label())
                            .changed()
                        {
                            changed = true;
                        }
                    }
                });
        });

        ui.separator();

        let Some(t) = self.scopes_tex.clone() else {
            ui.label(tr(
                lang,
                "Os escopos são calculados a partir do quadro da prévia...",
            ));
            if changed {
                self.last_req = crate::app::PreviewReqKey::default();
            }
            return;
        };

        // Ordem dos 4 quadrantes fiel ao VEGAS Pro 21 (p. 61):
        // [0] Vectorscope (tex 2) | [1] Waveform (tex 1)
        // [2] Histograma  (tex 0) | [3] RGB Parade (tex 3)
        let draw_scope_tile = |ui: &mut egui::Ui,
                               cfg: &mut neoveg_engine::scopes::ScopeConfig,
                               changed: &mut bool,
                               kind: ScopeViewLayout,
                               tile_w: f32,
                               max_h: f32| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| match kind {
                    ScopeViewLayout::Vectorscope => {
                        ui.label(
                            egui::RichText::new(tr(lang, "Vectorscope"))
                                .small()
                                .strong(),
                        );
                        egui::ComboBox::from_id_salt("sc_vec_targets")
                            .width(58.0)
                            .selected_text(cfg.vectorscope_targets.label())
                            .show_ui(ui, |ui| {
                                if ui
                                    .selectable_value(
                                        &mut cfg.vectorscope_targets,
                                        neoveg_engine::scopes::VectorscopeTarget::Percent75,
                                        "75%",
                                    )
                                    .changed()
                                {
                                    *changed = true;
                                }
                                if ui
                                    .selectable_value(
                                        &mut cfg.vectorscope_targets,
                                        neoveg_engine::scopes::VectorscopeTarget::Percent100,
                                        "100%",
                                    )
                                    .changed()
                                {
                                    *changed = true;
                                }
                            });
                        if ui
                            .checkbox(&mut cfg.vectorscope_skin_line, tr(lang, "Tom de pele (I)"))
                            .changed()
                        {
                            *changed = true;
                        }
                    }
                    ScopeViewLayout::Waveform => {
                        let lbl = match cfg.waveform_mode {
                            neoveg_engine::scopes::WaveformMode::RgbOverlay => {
                                tr(lang, "Forma de onda (RGB Overlay)")
                            }
                            neoveg_engine::scopes::WaveformMode::YC => {
                                tr(lang, "Forma de onda (YC Envelope)")
                            }
                            _ => tr(lang, "Forma de onda (Luma)"),
                        };
                        ui.label(egui::RichText::new(lbl).small().strong());
                        egui::ComboBox::from_id_salt("sc_mode")
                            .width(92.0)
                            .selected_text(cfg.waveform_mode.label())
                            .show_ui(ui, |ui| {
                                for m in neoveg_engine::scopes::WaveformMode::ALL {
                                    if ui
                                        .selectable_value(&mut cfg.waveform_mode, m, m.label())
                                        .changed()
                                    {
                                        *changed = true;
                                    }
                                }
                            });
                    }
                    ScopeViewLayout::Histogram => {
                        ui.label(
                            egui::RichText::new(tr(lang, "Histograma (RGB + luma)"))
                                .small()
                                .strong(),
                        );
                    }
                    ScopeViewLayout::RgbParade => {
                        let lbl = match cfg.parade_mode {
                            neoveg_engine::scopes::ParadeMode::Yuv => tr(lang, "YUV Parade"),
                            neoveg_engine::scopes::ParadeMode::RgbWhite => {
                                tr(lang, "RGB + Branco Parade")
                            }
                            _ => tr(lang, "RGB Parade"),
                        };
                        ui.label(egui::RichText::new(lbl).small().strong());
                        egui::ComboBox::from_id_salt("sc_parade")
                            .width(92.0)
                            .selected_text(cfg.parade_mode.label())
                            .show_ui(ui, |ui| {
                                for p in neoveg_engine::scopes::ParadeMode::ALL {
                                    if ui
                                        .selectable_value(&mut cfg.parade_mode, p, p.label())
                                        .changed()
                                    {
                                        *changed = true;
                                    }
                                }
                            });
                    }
                    ScopeViewLayout::All => {}
                });
                let tex_idx = match kind {
                    ScopeViewLayout::Histogram => 0,
                    ScopeViewLayout::Waveform => 1,
                    ScopeViewLayout::Vectorscope => 2,
                    ScopeViewLayout::RgbParade | ScopeViewLayout::All => 3,
                };
                let s = t[tex_idx].size_vec2();
                let is_vec = kind == ScopeViewLayout::Vectorscope;
                // cada quadro cabe na altura que sobrou (os 4 aparecem inteiros)
                ui.image((t[tex_idx].id(), fit_scope_image(s, tile_w, max_h, is_vec)));
            });
        };

        let avail = ui.available_width();
        // altura de cada quadro: o que sobra na aba, menos o título de cada
        // linha, dividido por 2 na grade (VEGAS Pro 21, p. 61)
        let title_h = ui.spacing().interact_size.y + ui.spacing().item_spacing.y * 2.0;
        let avail_h = ui.available_height();
        if self.scopes_layout == ScopeViewLayout::All {
            let w = ((avail - 24.0) / 2.0).max(80.0);
            let max_h = ((avail_h - 2.0 * title_h - 8.0) / 2.0).max(48.0);
            egui::Grid::new("scopes").num_columns(2).show(ui, |ui| {
                draw_scope_tile(
                    ui,
                    &mut self.scopes_config,
                    &mut changed,
                    ScopeViewLayout::Vectorscope,
                    w,
                    max_h,
                );
                draw_scope_tile(
                    ui,
                    &mut self.scopes_config,
                    &mut changed,
                    ScopeViewLayout::Waveform,
                    w,
                    max_h,
                );
                ui.end_row();
                draw_scope_tile(
                    ui,
                    &mut self.scopes_config,
                    &mut changed,
                    ScopeViewLayout::Histogram,
                    w,
                    max_h,
                );
                draw_scope_tile(
                    ui,
                    &mut self.scopes_config,
                    &mut changed,
                    ScopeViewLayout::RgbParade,
                    w,
                    max_h,
                );
                ui.end_row();
            });
        } else {
            let w = (avail - 16.0).max(120.0);
            let max_h = (avail_h - title_h - 4.0).max(48.0);
            let chosen = self.scopes_layout;
            draw_scope_tile(ui, &mut self.scopes_config, &mut changed, chosen, w, max_h);
        }

        if changed {
            self.last_req = crate::app::PreviewReqKey::default();
        }
    }
}

/// Categorias da janela Transições, nas abas do VEGAS Pro (Efeitos 3D,
/// Revelar, Wipes, Fades e Dobras e páginas).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum TransitionCategory {
    ThreeD,
    Reveals,
    Wipes,
    Fades,
    Pages,
}

impl TransitionCategory {
    pub(crate) const ALL: [TransitionCategory; 5] = [
        TransitionCategory::ThreeD,
        TransitionCategory::Reveals,
        TransitionCategory::Wipes,
        TransitionCategory::Fades,
        TransitionCategory::Pages,
    ];

    pub(crate) fn name_in(self, lang: Language) -> &'static str {
        let key = match self {
            TransitionCategory::ThreeD => "Efeitos 3D",
            TransitionCategory::Reveals => "Revelar",
            TransitionCategory::Wipes => "Wipes",
            TransitionCategory::Fades => "Fades",
            TransitionCategory::Pages => "Dobras e páginas",
        };
        tr(lang, key)
    }

    /// Categoria de cada transição (exaustivo: uma transição nova obriga a
    /// escolher a aba dela).
    pub(crate) fn of(k: TransitionKind) -> Self {
        use TransitionKind as T;
        match k {
            T::Zoom
            | T::FlyIn
            | T::Blinds3D
            | T::Cascade3D
            | T::Shuffle3D
            | T::WarpFlow
            | T::Swap => TransitionCategory::ThreeD,
            T::PushLeft
            | T::PushRight
            | T::PushUp
            | T::PushDown
            | T::SlideOver
            | T::Squeeze
            | T::IrisOpen
            | T::IrisClose
            | T::BarnDoor
            | T::SplitHorizontal
            | T::Portals => TransitionCategory::Reveals,
            T::WipeLeft
            | T::WipeRight
            | T::WipeUp
            | T::WipeDown
            | T::ClockWipe
            | T::Blinds
            | T::Checker
            | T::StarWipe
            | T::DiagonalWipe
            | T::GradientWipe
            | T::VenetianBlinds
            | T::Spiral
            | T::Cascade => TransitionCategory::Wipes,
            T::Dissolve | T::FadeThroughBlack | T::RandomDissolve | T::FlashWhite => {
                TransitionCategory::Fades
            }
            T::PagePeel | T::PageLoop | T::PageRoll => TransitionCategory::Pages,
        }
    }
}

/// Identificador de favorito de uma transição (lista `fx_favorites`).
pub(crate) fn transition_fav_id(k: TransitionKind) -> String {
    format!("tr:{k:?}")
}

/// Estado da janela Transições.
#[derive(Default)]
pub(crate) struct TransitionsUiState {
    pub(crate) search: String,
    /// `None` = Todos.
    pub(crate) cat: Option<TransitionCategory>,
    pub(crate) favorites_only: bool,
    pub(crate) selected: Option<TransitionKind>,
}

/// Indica se a prévia está redimensionada em relação ao projeto (`prev_scaled`)
/// ou se o tamanho de exibição no monitor diverge da prévia (`disp_scaled`).
/// Usado pelo rodapé da Prévia de Vídeo para aplicar o destaque visual do VEGAS Pro.
pub(crate) fn preview_footer_scaled_flags(
    proj: (u32, u32),
    prev: (u32, u32),
    disp: (u32, u32),
) -> (bool, bool) {
    let prev_scaled = prev.0 != proj.0 || prev.1 != proj.1;
    let disp_scaled = disp.0 != prev.0 || disp.1 != prev.1;
    (prev_scaled, disp_scaled)
}

/// Tamanho de um quadro da aba Escopos: cabe em `tile_w` × `max_h`
/// mantendo a proporção da textura; o vectorscope é quadrado.
pub(crate) fn fit_scope_image(tex: Vec2, tile_w: f32, max_h: f32, square: bool) -> Vec2 {
    let tile_w = if tile_w.is_finite() {
        tile_w.max(1.0)
    } else {
        1.0
    };
    let max_h = if max_h.is_finite() {
        max_h.max(1.0)
    } else {
        1.0
    };
    if square {
        let side = tile_w.min(max_h);
        return vec2(side, side);
    }
    let aspect = if tex.x > 0.0 && tex.y > 0.0 && tex.x.is_finite() && tex.y.is_finite() {
        tex.x / tex.y
    } else {
        1.0
    };
    let mut w = tile_w;
    let mut h = w / aspect;
    if h > max_h {
        h = max_h;
        w = h * aspect;
    }
    vec2(w, h)
}

/// Maior retângulo de proporção `aspect` (largura/altura) centrado em `r`.
/// `None` para proporção ou retângulo inválidos (§0.4).
fn centered_aspect_rect(r: Rect, aspect: f32) -> Option<Rect> {
    if !aspect.is_finite() || aspect <= 0.0 || !r.is_finite() || r.width() < 1.0 || r.height() < 1.0
    {
        return None;
    }
    let (w, h) = if r.width() / r.height() > aspect {
        (r.height() * aspect, r.height())
    } else {
        (r.width(), r.width() / aspect)
    };
    Some(Rect::from_center_size(r.center(), vec2(w, h)))
}

// Medidas do grupo de medidores do Master Bus (fader, canal L, escala, canal R).
const MASTER_GAP: f32 = 4.0;
const MASTER_FADER_W: f32 = 22.0;
const MASTER_CHANNEL_W: f32 = 8.0;
const MASTER_SCALE_W: f32 = 22.0;
const MASTER_COMPACT_W: f32 = 18.0;
const MASTER_PREVIEW_W: f32 = 34.0;
const MASTER_PEAK_ROW_H: f32 = 14.0;
/// Altura reservada abaixo dos medidores: leituras de pico e a linha do cadeado.
const MASTER_BOTTOM_ROWS_H: f32 = MASTER_PEAK_ROW_H + 30.0;

/// Retângulos do grupo de medidores do Master Bus dentro de `row`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct MasterStripRects {
    pub preview: Option<Rect>,
    pub fader: Rect,
    pub left: Rect,
    pub scale: Option<Rect>,
    pub right: Rect,
}

/// Distribui o barramento Preview (opcional), o fader e os medidores L/R
/// centrados na largura de `row`, todos com a altura de `row`. Sem escala
/// (painel baixo), os dois canais ficam juntos numa barra estreita.
pub(crate) fn master_strip_layout(
    row: Rect,
    with_preview: bool,
    with_scale: bool,
) -> MasterStripRects {
    let preview_w = if with_preview {
        MASTER_PREVIEW_W + 2.0 * MASTER_GAP + 1.0
    } else {
        0.0
    };
    let meters_w = if with_scale {
        2.0 * MASTER_CHANNEL_W + MASTER_SCALE_W + 2.0 * MASTER_GAP
    } else {
        MASTER_COMPACT_W
    };
    let total = preview_w + MASTER_FADER_W + MASTER_GAP + meters_w;
    let x0 = row.left() + ((row.width() - total) * 0.5).max(0.0);
    let col = |x: f32, w: f32| Rect::from_min_size(pos2(x, row.top()), vec2(w, row.height()));
    let preview = with_preview.then(|| col(x0, MASTER_PREVIEW_W));
    let mut x = x0 + preview_w;
    let fader = col(x, MASTER_FADER_W);
    x += MASTER_FADER_W + MASTER_GAP;
    if with_scale {
        let left = col(x, MASTER_CHANNEL_W);
        x += MASTER_CHANNEL_W + MASTER_GAP;
        let scale = col(x, MASTER_SCALE_W);
        x += MASTER_SCALE_W + MASTER_GAP;
        let right = col(x, MASTER_CHANNEL_W);
        MasterStripRects {
            preview,
            fader,
            left,
            scale: Some(scale),
            right,
        }
    } else {
        let half = MASTER_COMPACT_W * 0.5;
        MasterStripRects {
            preview,
            fader,
            left: col(x, half),
            scale: None,
            right: col(x + half, half),
        }
    }
}

#[cfg(test)]
mod master_strip_tests {
    use super::*;

    fn group_center(l: &MasterStripRects) -> f32 {
        let left = l.preview.map_or(l.fader.left(), |p| p.left());
        (left + l.right.right()) * 0.5
    }

    /// O grupo de medidores ficava colado à esquerda do painel.
    #[test]
    fn grupo_de_medidores_fica_centrado_no_painel() {
        let row = Rect::from_min_size(pos2(10.0, 50.0), vec2(150.0, 400.0));
        for (pv, sc) in [(false, true), (true, true), (false, false)] {
            let l = master_strip_layout(row, pv, sc);
            assert!(
                (group_center(&l) - row.center().x).abs() < 1.0,
                "grupo fora do centro (prévia {pv}, escala {sc})"
            );
        }
    }

    /// Fader e canais ocupam a altura inteira e não se sobrepõem.
    #[test]
    fn medidores_ocupam_a_altura_toda_e_em_ordem() {
        let row = Rect::from_min_size(pos2(0.0, 0.0), vec2(150.0, 600.0));
        let l = master_strip_layout(row, true, true);
        for r in [l.fader, l.left, l.right] {
            assert_eq!(r.height(), 600.0);
        }
        let s = l.scale.expect("escala no painel alto");
        let pv = l.preview.expect("barramento Preview");
        assert!(pv.right() < l.fader.left());
        assert!(l.fader.right() <= l.left.left());
        assert!(l.left.right() <= s.left());
        assert!(s.right() <= l.right.left());
    }

    /// Painel mais estreito que o grupo: o grupo começa na borda, sem sair por
    /// ela com deslocamento negativo.
    #[test]
    fn painel_estreito_nao_desloca_para_fora() {
        let row = Rect::from_min_size(pos2(5.0, 0.0), vec2(20.0, 100.0));
        let l = master_strip_layout(row, false, true);
        assert_eq!(l.fader.left(), 5.0);
    }
}

#[cfg(test)]
mod safe_guide_tests {
    use super::*;

    #[test]
    fn preview_footer_scaled_flags_detecta_redimensionamento() {
        assert_eq!(
            preview_footer_scaled_flags((1920, 1080), (1920, 1080), (1920, 1080)),
            (false, false)
        );
        assert_eq!(
            preview_footer_scaled_flags((1920, 1080), (960, 540), (960, 540)),
            (true, false)
        );
        assert_eq!(
            preview_footer_scaled_flags((1920, 1080), (1920, 1080), (640, 360)),
            (false, true)
        );
        assert_eq!(
            preview_footer_scaled_flags((1920, 1080), (960, 540), (640, 360)),
            (true, true)
        );
    }

    #[test]
    fn proporcao_da_previa_simula_o_pixel_do_dispositivo() {
        // DV NTSC com PAR 0,9091: a área ativa de 704x480 fica em 4:3 só com
        // a simulação (os 720 pixels incluem 16 de margem analógica)
        let sim = preview_display_aspect(704, 480, 0.9091, true);
        assert!((sim - 4.0 / 3.0).abs() < 0.001, "{sim}");
        let full = preview_display_aspect(720, 480, 0.9091, true);
        assert!((full - 1.5 * 0.9091).abs() < 1e-4, "{full}");
        assert!((preview_display_aspect(720, 480, 0.9091, false) - 1.5).abs() < 1e-6);
        // pixels quadrados não mudam; valores hostis caem para quadrado
        assert_eq!(
            preview_display_aspect(1920, 1080, 1.0, true),
            1920.0 / 1080.0
        );
        assert_eq!(
            preview_display_aspect(1920, 1080, f32::NAN, true),
            1920.0 / 1080.0
        );
        assert_eq!(
            preview_display_aspect(1920, 1080, -2.0, true),
            1920.0 / 1080.0
        );
        assert!(preview_display_aspect(0, 0, 1.0, true).is_finite());
    }

    #[test]
    fn teclas_do_mixer_seguem_o_vegas() {
        use egui::Key as K;
        assert_eq!(
            mixer_key(false, true, false, K::C),
            Some(MixerKey::ChannelList)
        );
        assert_eq!(
            mixer_key(false, false, false, K::Q),
            Some(MixerKey::AllStrips)
        );
        assert_eq!(
            mixer_key(false, false, false, K::W),
            Some(MixerKey::WidthWide)
        );
        // Ctrl+Q (nova trilha de áudio) e Alt+... continuam globais
        assert_eq!(mixer_key(true, false, false, K::Q), None);
        assert_eq!(mixer_key(false, false, true, K::M), None);
        assert_eq!(mixer_key(false, false, false, K::E), None);

        let mut st = MixerConsoleState::default();
        let (tracks, buses) = ([1, 2], [7]);
        st.apply_key(MixerKey::TrackStrips, &tracks, &buses);
        assert!(st.hidden_tracks.contains(&1) && st.hidden_tracks.contains(&2));
        st.apply_key(MixerKey::TrackStrips, &tracks, &buses);
        assert!(st.hidden_tracks.is_empty());
        st.apply_key(MixerKey::AllStrips, &tracks, &buses);
        assert!(st.hide_master && st.hidden_buses.contains(&7) && st.hidden_tracks.len() == 2);
        st.apply_key(MixerKey::AllStrips, &tracks, &buses);
        assert!(!st.hide_master && st.hidden_buses.is_empty() && st.hidden_tracks.is_empty());
        st.apply_key(MixerKey::WidthNarrow, &tracks, &buses);
        assert_eq!(st.clamped_strip_width(), 84.0);
        // projeto sem barramentos: U não trava nem muda nada estranho
        st.apply_key(MixerKey::BusStrips, &tracks, &[]);
        assert!(st.hidden_buses.is_empty());
    }

    #[test]
    fn mixer_console_state_saneia_largura_e_restaura_canais() {
        let mut st = MixerConsoleState {
            strip_width: f32::NAN,
            hide_master: true,
            channel_filter: MixerChannelFilter::AudioBuses,
            ..Default::default()
        };
        st.hidden_tracks.insert(42);
        st.hidden_buses.insert(99);
        assert_eq!(st.clamped_strip_width(), 112.0);
        st.strip_width = 999.0;
        assert_eq!(st.clamped_strip_width(), 156.0);
        st.show_all_channels();
        assert!(st.hidden_tracks.is_empty() && st.hidden_buses.is_empty());
        assert!(!st.hide_master);
        assert_eq!(st.channel_filter, MixerChannelFilter::All);
    }

    #[test]
    fn explorer_state_historico_favoritos_e_formatacao_tamanho() {
        let mut ex = ExplorerState::default();
        let mut cur = PathBuf::from("a");
        ex.navigate(&mut cur, PathBuf::from("b"));
        ex.navigate(&mut cur, PathBuf::from("c"));
        assert_eq!(cur, PathBuf::from("c"));
        assert!(ex.go_back(&mut cur));
        assert_eq!(cur, PathBuf::from("b"));
        assert!(ex.go_forward(&mut cur));
        assert_eq!(cur, PathBuf::from("c"));
        assert!(ex.toggle_favorite(&cur));
        assert_eq!(ex.favorites.len(), 1);
        assert!(!ex.toggle_favorite(&cur));
        assert!(ex.favorites.is_empty());
        assert_eq!(format_file_size(512), "512 B");
        assert_eq!(format_file_size(4096), "4 KB");
        assert_eq!(format_file_size(2 * 1024 * 1024), "2.0 MB");
    }

    #[test]
    fn recorte_central_cabe_no_quadro_e_recusa_entrada_invalida() {
        let r = Rect::from_min_size(pos2(0.0, 0.0), vec2(1920.0, 1080.0));
        let c = centered_aspect_rect(r, 9.0 / 16.0).expect("9:16");
        assert!((c.height() - 1080.0).abs() < 0.01);
        assert!((c.width() - 607.5).abs() < 0.01);
        assert!((c.center().x - 960.0).abs() < 0.01);
        let q = centered_aspect_rect(r, 1.0).expect("1:1");
        assert!((q.width() - 1080.0).abs() < 0.01);
        // quadro vertical: o 4:3 é limitado pela largura
        let v = Rect::from_min_size(pos2(0.0, 0.0), vec2(1080.0, 1920.0));
        let w = centered_aspect_rect(v, 4.0 / 3.0).expect("4:3");
        assert!((w.width() - 1080.0).abs() < 0.01 && w.height() < 1920.0);
        assert!(centered_aspect_rect(r, f32::NAN).is_none());
        assert!(centered_aspect_rect(r, 0.0).is_none());
        assert!(centered_aspect_rect(Rect::NOTHING, 1.0).is_none());
    }
}
