//! Ícones vetoriais originais do Neoveg desenhados nativamente via `egui::Painter`
//! (AGENTS.md §1.1 e §3.6: zero dependências extras, resolução infinita e adaptação automática
//! ao Alto Contraste WCAG AAA).

use crate::theme::ThemeColors;
use eframe::egui;
use egui::{pos2, vec2, Color32, Pos2, Rect, Sense, Shape, Stroke, StrokeKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Icon {
    NewFile,
    OpenFolder,
    SaveDisk,
    RenderClapper,
    GearProps,
    ImportMedia,
    Cut,
    Copy,
    Paste,
    Undo,
    Redo,
    Split,
    DeleteTrash,
    MagnetSnap,
    AutoRipple,
    IgnoreGrouping,
    Envelopes,
    LayoutWindow,
    GoStart,
    PlayFromStart,
    PrevFrame,
    Play,
    Pause,
    Stop,
    NextFrame,
    GoEnd,
    LoopPlayback,
    MasterBus,
    SpeedGauge,
    ZoomGlass,
    VideoTrack,
    AudioTrack,
    TrackMotion,
    PanCrop,
    LockClosed,
    SafeAreas,
    CameraSnap,
    FolderUp,
    FolderItem,
    MediaItem,
    TransitionSplit,
    FxSparkle,
    ColorWheels,
    FxAudio,
    GeneratorFrame,
    MixerFaders,
    ScopesWave,
    NotesPad,
    PopOut,
    DockIn,
    KeyframeFilled,
    KeyframeEmpty,
    MoveUp,
    MoveDown,
    ResetSpin,
    CloseX,
    Backspace,
    LoudnessMeter,
    BusTrack,
    TrimmerBlade,
    EditDetailsTable,
    SurroundSpeakers,
    PluginPlug,
    Compositors,
    EncoderQueue,
    TrimSelection,
    TrimStart,
    TrimEnd,
    InsertMarker,
    InsertRegion,
    ExternalApps,
    NormalTool,
    EnvelopeTool,
    SelectionTool,
    ShuffleTool,
    SlipTool,
    SlideTool,
    StretchTool,
    SplitTrimTool,
    RollTool,
    SmartSplit,
    HealEvents,
    PasteAttributes,
    RecordDot,
    ExpandedEdit,
    SceneDetect,
    StabilizeVideo,
    ViewThumbnails,
    ViewList,
    ViewDetails,
    StoryboardGrid,
    SmartFolder,
    ReplaceMedia,
    SplitPreview,
    RamCache,
    CopyFrame,
    MuteAudio,
    MuteVideo,
    ExternalMonitor,
    TrackFolder,
    ParentChild,
    TrackMask,
    NestedTimeline,
    Multicam,
    Stereo3d,
    RenderTrack,
    TimelineGrid,
}

#[cfg(test)]
impl Icon {
    pub const ALL: [Icon; 107] = [
        Icon::NewFile,
        Icon::OpenFolder,
        Icon::SaveDisk,
        Icon::RenderClapper,
        Icon::GearProps,
        Icon::ImportMedia,
        Icon::Cut,
        Icon::Copy,
        Icon::Paste,
        Icon::Undo,
        Icon::Redo,
        Icon::Split,
        Icon::DeleteTrash,
        Icon::MagnetSnap,
        Icon::AutoRipple,
        Icon::IgnoreGrouping,
        Icon::Envelopes,
        Icon::LayoutWindow,
        Icon::GoStart,
        Icon::PlayFromStart,
        Icon::PrevFrame,
        Icon::Play,
        Icon::Pause,
        Icon::Stop,
        Icon::NextFrame,
        Icon::GoEnd,
        Icon::LoopPlayback,
        Icon::MasterBus,
        Icon::SpeedGauge,
        Icon::ZoomGlass,
        Icon::VideoTrack,
        Icon::AudioTrack,
        Icon::TrackMotion,
        Icon::PanCrop,
        Icon::LockClosed,
        Icon::SafeAreas,
        Icon::CameraSnap,
        Icon::FolderUp,
        Icon::FolderItem,
        Icon::MediaItem,
        Icon::TransitionSplit,
        Icon::FxSparkle,
        Icon::ColorWheels,
        Icon::FxAudio,
        Icon::GeneratorFrame,
        Icon::MixerFaders,
        Icon::ScopesWave,
        Icon::NotesPad,
        Icon::PopOut,
        Icon::DockIn,
        Icon::KeyframeFilled,
        Icon::KeyframeEmpty,
        Icon::MoveUp,
        Icon::MoveDown,
        Icon::ResetSpin,
        Icon::CloseX,
        Icon::Backspace,
        Icon::LoudnessMeter,
        Icon::BusTrack,
        Icon::TrimmerBlade,
        Icon::EditDetailsTable,
        Icon::SurroundSpeakers,
        Icon::PluginPlug,
        Icon::Compositors,
        Icon::EncoderQueue,
        Icon::TrimSelection,
        Icon::TrimStart,
        Icon::TrimEnd,
        Icon::InsertMarker,
        Icon::InsertRegion,
        Icon::ExternalApps,
        Icon::NormalTool,
        Icon::EnvelopeTool,
        Icon::SelectionTool,
        Icon::ShuffleTool,
        Icon::SlipTool,
        Icon::SlideTool,
        Icon::StretchTool,
        Icon::SplitTrimTool,
        Icon::RollTool,
        Icon::SmartSplit,
        Icon::HealEvents,
        Icon::PasteAttributes,
        Icon::RecordDot,
        Icon::ExpandedEdit,
        Icon::SceneDetect,
        Icon::StabilizeVideo,
        Icon::ViewThumbnails,
        Icon::ViewList,
        Icon::ViewDetails,
        Icon::StoryboardGrid,
        Icon::SmartFolder,
        Icon::ReplaceMedia,
        Icon::SplitPreview,
        Icon::RamCache,
        Icon::CopyFrame,
        Icon::MuteAudio,
        Icon::MuteVideo,
        Icon::ExternalMonitor,
        Icon::TrackFolder,
        Icon::ParentChild,
        Icon::TrackMask,
        Icon::NestedTimeline,
        Icon::Multicam,
        Icon::Stereo3d,
        Icon::RenderTrack,
        Icon::TimelineGrid,
    ];
}

pub fn normal_mode_icon(mode: neoveg_engine::session::EditMode) -> Icon {
    use neoveg_engine::session::EditMode;
    match mode {
        EditMode::Normal | EditMode::Ripple => Icon::NormalTool,
        EditMode::Shuffle => Icon::ShuffleTool,
        EditMode::Slip => Icon::SlipTool,
        EditMode::Slide => Icon::SlideTool,
        EditMode::Stretch => Icon::StretchTool,
        EditMode::SplitTrim => Icon::SplitTrimTool,
        EditMode::Roll => Icon::RollTool,
    }
}

#[derive(Clone, Copy)]
struct Pal {
    ink: Color32,
    muted: Color32,
    paper: Color32,
    blue: Color32,
    amber: Color32,
    green: Color32,
    red: Color32,
    cyan: Color32,
    purple: Color32,
}

impl Pal {
    fn from_theme(theme: &ThemeColors, enabled: bool) -> Self {
        let mut pal = if theme.high_contrast {
            Self {
                ink: theme.text_primary,
                muted: theme.text_muted,
                paper: theme.extreme_bg,
                blue: theme.accent,
                amber: theme.text_primary,
                green: theme.accent,
                red: theme.accent,
                cyan: theme.accent,
                purple: theme.accent,
            }
        } else if theme.dark_mode {
            Self {
                ink: Color32::from_rgb(228, 232, 238),
                muted: Color32::from_rgb(152, 162, 176),
                paper: Color32::from_rgb(238, 242, 248),
                blue: Color32::from_rgb(76, 166, 255),
                amber: Color32::from_rgb(245, 188, 66),
                green: Color32::from_rgb(92, 214, 134),
                red: Color32::from_rgb(240, 98, 92),
                cyan: Color32::from_rgb(78, 214, 224),
                purple: Color32::from_rgb(186, 136, 250),
            }
        } else {
            Self {
                ink: Color32::from_rgb(44, 52, 62),
                muted: Color32::from_rgb(104, 114, 128),
                paper: Color32::from_rgb(252, 253, 255),
                blue: Color32::from_rgb(32, 110, 214),
                amber: Color32::from_rgb(218, 146, 16),
                green: Color32::from_rgb(30, 152, 74),
                red: Color32::from_rgb(206, 52, 52),
                cyan: Color32::from_rgb(14, 148, 168),
                purple: Color32::from_rgb(130, 74, 214),
            }
        };
        if !enabled {
            let dim = |c: Color32| c.gamma_multiply(0.38);
            pal = Self {
                ink: dim(pal.ink),
                muted: dim(pal.muted),
                paper: dim(pal.paper),
                blue: dim(pal.blue),
                amber: dim(pal.amber),
                green: dim(pal.green),
                red: dim(pal.red),
                cyan: dim(pal.cyan),
                purple: dim(pal.purple),
            };
        }
        pal
    }
}

struct Canvas<'a> {
    p: &'a egui::Painter,
    r: Rect,
}

impl<'a> Canvas<'a> {
    fn pt(&self, x: f32, y: f32) -> Pos2 {
        pos2(
            self.r.left() + x * self.r.width(),
            self.r.top() + y * self.r.height(),
        )
    }

    fn rect(&self, x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
        Rect::from_min_max(self.pt(x0, y0), self.pt(x1, y1))
    }

    fn u(&self, frac: f32) -> f32 {
        (self.r.width().min(self.r.height()) * frac).max(1.0)
    }

    fn stroke(&self, frac: f32, col: Color32) -> Stroke {
        Stroke::new(self.u(frac), col)
    }

    fn line(&self, x0: f32, y0: f32, x1: f32, y1: f32, col: Color32, width_frac: f32) {
        self.p.line_segment(
            [self.pt(x0, y0), self.pt(x1, y1)],
            self.stroke(width_frac, col),
        );
    }

    fn poly(&self, pts: &[(f32, f32)], fill: Color32, stroke: Option<(f32, Color32)>) {
        let points: Vec<Pos2> = pts.iter().map(|&(x, y)| self.pt(x, y)).collect();
        let st = match stroke {
            Some((wf, col)) => self.stroke(wf, col),
            None => Stroke::NONE,
        };
        self.p.add(Shape::convex_polygon(points, fill, st));
    }

    fn polyline(&self, pts: &[(f32, f32)], col: Color32, width_frac: f32) {
        let points: Vec<Pos2> = pts.iter().map(|&(x, y)| self.pt(x, y)).collect();
        self.p
            .add(Shape::line(points, self.stroke(width_frac, col)));
    }
}

/// Pinta um ícone escalável dentro de `rect`.
pub fn paint(painter: &egui::Painter, rect: Rect, icon: Icon, theme: &ThemeColors, enabled: bool) {
    if rect.width() <= 1.0 || rect.height() <= 1.0 {
        return;
    }
    let side = rect.width().min(rect.height());
    let sq = Rect::from_center_size(rect.center(), vec2(side, side));
    let c = Canvas { p: painter, r: sq };
    let pal = Pal::from_theme(theme, enabled);

    match icon {
        Icon::NewFile => {
            c.poly(
                &[
                    (0.18, 0.08),
                    (0.64, 0.08),
                    (0.84, 0.28),
                    (0.84, 0.92),
                    (0.18, 0.92),
                ],
                pal.paper,
                Some((0.08, pal.ink)),
            );
            c.poly(
                &[(0.64, 0.08), (0.84, 0.28), (0.64, 0.28)],
                pal.blue,
                Some((0.06, pal.ink)),
            );
            c.line(0.30, 0.50, 0.72, 0.50, pal.blue, 0.08);
            c.line(0.30, 0.68, 0.64, 0.68, pal.muted, 0.08);
        }
        Icon::OpenFolder => {
            c.poly(
                &[
                    (0.08, 0.20),
                    (0.38, 0.20),
                    (0.46, 0.30),
                    (0.84, 0.30),
                    (0.84, 0.84),
                    (0.08, 0.84),
                ],
                pal.amber.gamma_multiply(0.85),
                Some((0.07, pal.ink)),
            );
            c.poly(
                &[(0.08, 0.84), (0.20, 0.42), (0.94, 0.42), (0.82, 0.84)],
                pal.amber,
                Some((0.07, pal.ink)),
            );
        }
        Icon::SaveDisk => {
            let body = c.rect(0.12, 0.12, 0.88, 0.88);
            c.p.rect_filled(body, c.u(0.08), pal.blue);
            c.p.rect_stroke(body, c.u(0.08), c.stroke(0.07, pal.ink), StrokeKind::Inside);
            let shutter = c.rect(0.26, 0.12, 0.72, 0.40);
            c.p.rect_filled(shutter, 0.0, pal.paper);
            c.p.rect_filled(c.rect(0.56, 0.17, 0.66, 0.34), 0.0, pal.blue);
            let label = c.rect(0.24, 0.54, 0.76, 0.86);
            c.p.rect_filled(label, c.u(0.04), pal.paper);
            c.line(0.32, 0.68, 0.68, 0.68, pal.muted, 0.06);
        }
        Icon::RenderClapper => {
            let box_r = c.rect(0.12, 0.36, 0.88, 0.88);
            c.p.rect_filled(box_r, c.u(0.06), pal.blue.gamma_multiply(0.85));
            c.p.rect_stroke(
                box_r,
                c.u(0.06),
                c.stroke(0.07, pal.ink),
                StrokeKind::Inside,
            );
            let top_r = c.rect(0.12, 0.14, 0.88, 0.36);
            c.p.rect_filled(top_r, c.u(0.04), pal.ink);
            for s in [0.22, 0.46, 0.70] {
                c.poly(
                    &[
                        (s, 0.14),
                        (s + 0.12, 0.14),
                        (s + 0.04, 0.36),
                        (s - 0.08, 0.36),
                    ],
                    pal.amber,
                    None,
                );
            }
            c.poly(&[(0.42, 0.48), (0.66, 0.62), (0.42, 0.76)], pal.paper, None);
        }
        Icon::GearProps => {
            let ctr = c.pt(0.5, 0.5);
            for i in 0..8 {
                let ang = (i as f32) * std::f32::consts::FRAC_PI_4;
                let (s, cs) = ang.sin_cos();
                let p1 = pos2(ctr.x + cs * c.u(0.26), ctr.y + s * c.u(0.26));
                let p2 = pos2(ctr.x + cs * c.u(0.42), ctr.y + s * c.u(0.42));
                c.p.line_segment([p1, p2], c.stroke(0.15, pal.blue));
            }
            c.p.circle_filled(ctr, c.u(0.30), pal.blue);
            c.p.circle_stroke(ctr, c.u(0.30), c.stroke(0.07, pal.ink));
            c.p.circle_filled(ctr, c.u(0.12), pal.paper);
        }
        Icon::ImportMedia => {
            let frame = c.rect(0.10, 0.18, 0.74, 0.82);
            c.p.rect_filled(frame, c.u(0.06), pal.blue);
            c.p.rect_stroke(
                frame,
                c.u(0.06),
                c.stroke(0.07, pal.ink),
                StrokeKind::Inside,
            );
            c.poly(&[(0.30, 0.36), (0.56, 0.50), (0.30, 0.64)], pal.paper, None);
            let badge = c.pt(0.74, 0.72);
            c.p.circle_filled(badge, c.u(0.20), pal.green);
            c.p.circle_stroke(badge, c.u(0.20), c.stroke(0.06, pal.ink));
            c.line(0.64, 0.72, 0.84, 0.72, pal.paper, 0.08);
            c.line(0.74, 0.62, 0.74, 0.82, pal.paper, 0.08);
        }
        Icon::Cut => {
            c.line(0.30, 0.64, 0.80, 0.16, pal.ink, 0.10);
            c.line(0.30, 0.36, 0.80, 0.84, pal.ink, 0.10);
            c.p.circle_filled(c.pt(0.24, 0.28), c.u(0.12), pal.red);
            c.p.circle_stroke(c.pt(0.24, 0.28), c.u(0.12), c.stroke(0.06, pal.ink));
            c.p.circle_filled(c.pt(0.24, 0.72), c.u(0.12), pal.blue);
            c.p.circle_stroke(c.pt(0.24, 0.72), c.u(0.12), c.stroke(0.06, pal.ink));
        }
        Icon::Copy => {
            let back = c.rect(0.14, 0.12, 0.64, 0.72);
            c.p.rect_filled(back, c.u(0.05), pal.blue);
            c.p.rect_stroke(back, c.u(0.05), c.stroke(0.07, pal.ink), StrokeKind::Inside);
            let front = c.rect(0.34, 0.28, 0.84, 0.88);
            c.p.rect_filled(front, c.u(0.05), pal.paper);
            c.p.rect_stroke(
                front,
                c.u(0.05),
                c.stroke(0.07, pal.ink),
                StrokeKind::Inside,
            );
            c.line(0.44, 0.50, 0.74, 0.50, pal.blue, 0.07);
            c.line(0.44, 0.68, 0.74, 0.68, pal.muted, 0.07);
        }
        Icon::Paste => {
            let board = c.rect(0.16, 0.18, 0.84, 0.90);
            c.p.rect_filled(board, c.u(0.07), pal.amber);
            c.p.rect_stroke(
                board,
                c.u(0.07),
                c.stroke(0.07, pal.ink),
                StrokeKind::Inside,
            );
            let clip = c.rect(0.36, 0.08, 0.64, 0.26);
            c.p.rect_filled(clip, c.u(0.05), pal.muted);
            c.p.rect_stroke(clip, c.u(0.05), c.stroke(0.06, pal.ink), StrokeKind::Inside);
            let sheet = c.rect(0.28, 0.34, 0.74, 0.80);
            c.p.rect_filled(sheet, 0.0, pal.paper);
            c.line(0.36, 0.52, 0.66, 0.52, pal.blue, 0.07);
            c.line(0.36, 0.68, 0.62, 0.68, pal.muted, 0.07);
        }
        Icon::Undo => {
            c.polyline(
                &[
                    (0.22, 0.40),
                    (0.58, 0.34),
                    (0.80, 0.48),
                    (0.76, 0.74),
                    (0.48, 0.80),
                ],
                pal.blue,
                0.13,
            );
            c.poly(
                &[(0.12, 0.40), (0.38, 0.18), (0.36, 0.58)],
                pal.cyan,
                Some((0.05, pal.ink)),
            );
        }
        Icon::Redo => {
            c.polyline(
                &[
                    (0.78, 0.40),
                    (0.42, 0.34),
                    (0.20, 0.48),
                    (0.24, 0.74),
                    (0.52, 0.80),
                ],
                pal.blue,
                0.13,
            );
            c.poly(
                &[(0.88, 0.40), (0.64, 0.58), (0.62, 0.18)],
                pal.cyan,
                Some((0.05, pal.ink)),
            );
        }
        Icon::Split => {
            let l_r = c.rect(0.08, 0.28, 0.42, 0.74);
            let r_r = c.rect(0.58, 0.28, 0.92, 0.74);
            c.p.rect_filled(l_r, c.u(0.04), pal.blue);
            c.p.rect_filled(r_r, c.u(0.04), pal.cyan);
            c.p.rect_stroke(l_r, c.u(0.04), c.stroke(0.06, pal.ink), StrokeKind::Inside);
            c.p.rect_stroke(r_r, c.u(0.04), c.stroke(0.06, pal.ink), StrokeKind::Inside);
            c.line(0.50, 0.10, 0.50, 0.90, pal.red, 0.11);
        }
        Icon::DeleteTrash => {
            c.poly(
                &[(0.22, 0.32), (0.78, 0.32), (0.72, 0.88), (0.28, 0.88)],
                pal.muted,
                Some((0.07, pal.ink)),
            );
            c.line(0.14, 0.24, 0.86, 0.24, pal.red, 0.10);
            c.line(0.38, 0.14, 0.62, 0.14, pal.red, 0.09);
            c.line(0.40, 0.44, 0.42, 0.76, pal.paper, 0.07);
            c.line(0.60, 0.44, 0.58, 0.76, pal.paper, 0.07);
        }
        Icon::MagnetSnap => {
            c.polyline(
                &[
                    (0.26, 0.24),
                    (0.26, 0.64),
                    (0.36, 0.82),
                    (0.64, 0.82),
                    (0.74, 0.64),
                    (0.74, 0.24),
                ],
                pal.red,
                0.16,
            );
            c.p.rect_filled(c.rect(0.18, 0.14, 0.34, 0.32), 0.0, pal.paper);
            c.p.rect_filled(c.rect(0.66, 0.14, 0.82, 0.32), 0.0, pal.paper);
            c.line(0.50, 0.10, 0.50, 0.44, pal.cyan, 0.07);
        }
        Icon::AutoRipple => {
            let b1 = c.rect(0.08, 0.20, 0.40, 0.48);
            let b2 = c.rect(0.50, 0.20, 0.84, 0.48);
            c.p.rect_filled(b1, c.u(0.04), pal.blue);
            c.p.rect_filled(b2, c.u(0.04), pal.cyan);
            c.line(0.16, 0.72, 0.78, 0.72, pal.amber, 0.11);
            c.poly(&[(0.90, 0.72), (0.68, 0.56), (0.68, 0.88)], pal.amber, None);
        }
        Icon::IgnoreGrouping => {
            let l1 = c.rect(0.10, 0.34, 0.48, 0.66);
            let l2 = c.rect(0.52, 0.34, 0.90, 0.66);
            c.p.rect_stroke(l1, c.u(0.12), c.stroke(0.10, pal.amber), StrokeKind::Inside);
            c.p.rect_stroke(l2, c.u(0.12), c.stroke(0.10, pal.blue), StrokeKind::Inside);
            c.line(0.32, 0.20, 0.68, 0.82, pal.red, 0.09);
        }
        Icon::Envelopes => {
            c.polyline(
                &[(0.12, 0.76), (0.42, 0.32), (0.68, 0.56), (0.90, 0.22)],
                pal.green,
                0.11,
            );
            for (x, y) in [(0.14, 0.76), (0.42, 0.32), (0.68, 0.56), (0.88, 0.22)] {
                c.p.rect_filled(
                    Rect::from_center_size(c.pt(x, y), vec2(c.u(0.18), c.u(0.18))),
                    0.0,
                    pal.amber,
                );
            }
        }
        Icon::LayoutWindow => {
            let outer = c.rect(0.10, 0.14, 0.90, 0.86);
            c.p.rect_filled(outer, c.u(0.06), pal.paper.gamma_multiply(0.2));
            c.p.rect_filled(c.rect(0.12, 0.16, 0.88, 0.46), c.u(0.03), pal.blue);
            c.p.rect_filled(c.rect(0.12, 0.52, 0.54, 0.84), c.u(0.03), pal.cyan);
            c.p.rect_filled(c.rect(0.58, 0.52, 0.88, 0.84), c.u(0.03), pal.amber);
            c.p.rect_stroke(
                outer,
                c.u(0.06),
                c.stroke(0.07, pal.ink),
                StrokeKind::Inside,
            );
        }
        Icon::GoStart => {
            c.line(0.18, 0.20, 0.18, 0.80, pal.cyan, 0.12);
            c.poly(&[(0.24, 0.50), (0.56, 0.22), (0.56, 0.78)], pal.blue, None);
            c.poly(&[(0.54, 0.50), (0.86, 0.22), (0.86, 0.78)], pal.blue, None);
        }
        Icon::PlayFromStart => {
            c.line(0.18, 0.18, 0.18, 0.82, pal.cyan, 0.12);
            c.poly(
                &[(0.32, 0.18), (0.84, 0.50), (0.32, 0.82)],
                pal.green,
                Some((0.06, pal.ink)),
            );
        }
        Icon::PrevFrame => {
            c.line(0.24, 0.20, 0.24, 0.80, pal.cyan, 0.12);
            c.poly(&[(0.32, 0.50), (0.80, 0.20), (0.80, 0.80)], pal.blue, None);
        }
        Icon::Play => {
            c.poly(
                &[(0.24, 0.16), (0.84, 0.50), (0.24, 0.84)],
                pal.green,
                Some((0.06, pal.ink)),
            );
        }
        Icon::Pause => {
            c.p.rect_filled(c.rect(0.22, 0.18, 0.42, 0.82), c.u(0.04), pal.amber);
            c.p.rect_filled(c.rect(0.58, 0.18, 0.78, 0.82), c.u(0.04), pal.amber);
        }
        Icon::Stop => {
            let b = c.rect(0.20, 0.20, 0.80, 0.80);
            c.p.rect_filled(b, c.u(0.06), pal.red);
            c.p.rect_stroke(b, c.u(0.06), c.stroke(0.06, pal.ink), StrokeKind::Inside);
        }
        Icon::NextFrame => {
            c.poly(&[(0.20, 0.20), (0.68, 0.50), (0.20, 0.80)], pal.blue, None);
            c.line(0.76, 0.20, 0.76, 0.80, pal.cyan, 0.12);
        }
        Icon::GoEnd => {
            c.poly(&[(0.14, 0.22), (0.46, 0.50), (0.14, 0.78)], pal.blue, None);
            c.poly(&[(0.44, 0.22), (0.76, 0.50), (0.44, 0.78)], pal.blue, None);
            c.line(0.82, 0.20, 0.82, 0.80, pal.cyan, 0.12);
        }
        Icon::LoopPlayback => {
            c.polyline(
                &[(0.16, 0.52), (0.20, 0.26), (0.74, 0.26), (0.82, 0.46)],
                pal.cyan,
                0.11,
            );
            c.poly(&[(0.90, 0.52), (0.68, 0.36), (0.84, 0.22)], pal.cyan, None);
            c.polyline(
                &[(0.84, 0.56), (0.78, 0.78), (0.26, 0.78), (0.18, 0.60)],
                pal.green,
                0.11,
            );
            c.poly(&[(0.10, 0.52), (0.32, 0.68), (0.16, 0.80)], pal.green, None);
        }
        Icon::MasterBus => {
            c.p.rect_filled(c.rect(0.18, 0.16, 0.34, 0.84), 0.0, pal.green);
            c.p.rect_filled(c.rect(0.40, 0.26, 0.56, 0.84), 0.0, pal.amber);
            c.line(0.74, 0.16, 0.74, 0.84, pal.muted, 0.09);
            c.p.rect_filled(c.rect(0.62, 0.38, 0.86, 0.54), c.u(0.04), pal.ink);
        }
        Icon::SpeedGauge => {
            c.polyline(
                &[
                    (0.14, 0.74),
                    (0.22, 0.36),
                    (0.50, 0.22),
                    (0.78, 0.36),
                    (0.86, 0.74),
                ],
                pal.cyan,
                0.11,
            );
            c.line(0.50, 0.74, 0.74, 0.38, pal.amber, 0.10);
            c.p.circle_filled(c.pt(0.50, 0.74), c.u(0.10), pal.ink);
        }
        Icon::ZoomGlass => {
            c.p.circle_filled(c.pt(0.42, 0.42), c.u(0.28), pal.cyan.gamma_multiply(0.35));
            c.p.circle_stroke(c.pt(0.42, 0.42), c.u(0.28), c.stroke(0.10, pal.blue));
            c.line(0.62, 0.62, 0.88, 0.88, pal.ink, 0.13);
        }
        Icon::VideoTrack => {
            let r = c.rect(0.10, 0.18, 0.90, 0.82);
            c.p.rect_filled(r, c.u(0.06), pal.blue);
            c.p.rect_stroke(r, c.u(0.06), c.stroke(0.06, pal.ink), StrokeKind::Inside);
            for y in [0.28, 0.50, 0.72] {
                c.p.rect_filled(
                    Rect::from_center_size(c.pt(0.20, y), vec2(c.u(0.10), c.u(0.10))),
                    0.0,
                    pal.paper,
                );
                c.p.rect_filled(
                    Rect::from_center_size(c.pt(0.80, y), vec2(c.u(0.10), c.u(0.10))),
                    0.0,
                    pal.paper,
                );
            }
            c.poly(&[(0.42, 0.34), (0.64, 0.50), (0.42, 0.66)], pal.paper, None);
        }
        Icon::AudioTrack => {
            c.poly(
                &[
                    (0.14, 0.38),
                    (0.32, 0.38),
                    (0.54, 0.18),
                    (0.54, 0.82),
                    (0.32, 0.62),
                    (0.14, 0.62),
                ],
                pal.green,
                Some((0.05, pal.ink)),
            );
            c.polyline(&[(0.66, 0.34), (0.74, 0.50), (0.66, 0.66)], pal.cyan, 0.09);
            c.polyline(&[(0.78, 0.22), (0.90, 0.50), (0.78, 0.78)], pal.cyan, 0.09);
        }
        Icon::TrackMotion => {
            let back = c.rect(0.16, 0.16, 0.66, 0.66);
            c.p.rect_stroke(back, 0.0, c.stroke(0.07, pal.muted), StrokeKind::Inside);
            let front = c.rect(0.32, 0.32, 0.84, 0.84);
            c.p.rect_filled(front, 0.0, pal.blue.gamma_multiply(0.35));
            c.p.rect_stroke(front, 0.0, c.stroke(0.08, pal.cyan), StrokeKind::Inside);
            c.line(0.16, 0.16, 0.32, 0.32, pal.amber, 0.07);
        }
        Icon::PanCrop => {
            c.line(0.14, 0.38, 0.14, 0.14, pal.cyan, 0.11);
            c.line(0.14, 0.14, 0.38, 0.14, pal.cyan, 0.11);
            c.line(0.62, 0.14, 0.86, 0.14, pal.cyan, 0.11);
            c.line(0.86, 0.14, 0.86, 0.38, pal.cyan, 0.11);
            c.line(0.14, 0.62, 0.14, 0.86, pal.cyan, 0.11);
            c.line(0.14, 0.86, 0.38, 0.86, pal.cyan, 0.11);
            c.line(0.62, 0.86, 0.86, 0.86, pal.cyan, 0.11);
            c.line(0.86, 0.86, 0.86, 0.62, pal.cyan, 0.11);
            c.p.circle_filled(c.pt(0.50, 0.50), c.u(0.12), pal.amber);
        }
        Icon::LockClosed => {
            c.polyline(
                &[
                    (0.32, 0.44),
                    (0.32, 0.24),
                    (0.50, 0.14),
                    (0.68, 0.24),
                    (0.68, 0.44),
                ],
                pal.paper,
                0.10,
            );
            let body = c.rect(0.22, 0.44, 0.78, 0.88);
            c.p.rect_filled(body, c.u(0.06), pal.amber);
            c.p.rect_stroke(body, c.u(0.06), c.stroke(0.06, pal.ink), StrokeKind::Inside);
        }
        Icon::SafeAreas => {
            c.p.rect_stroke(
                c.rect(0.10, 0.16, 0.90, 0.84),
                0.0,
                c.stroke(0.07, pal.cyan),
                StrokeKind::Inside,
            );
            c.p.rect_stroke(
                c.rect(0.24, 0.28, 0.76, 0.72),
                0.0,
                c.stroke(0.07, pal.amber),
                StrokeKind::Inside,
            );
            c.line(0.50, 0.40, 0.50, 0.60, pal.ink, 0.07);
            c.line(0.40, 0.50, 0.60, 0.50, pal.ink, 0.07);
        }
        Icon::CameraSnap => {
            let body = c.rect(0.10, 0.28, 0.90, 0.84);
            c.p.rect_filled(body, c.u(0.07), pal.muted);
            c.p.rect_stroke(body, c.u(0.07), c.stroke(0.06, pal.ink), StrokeKind::Inside);
            c.poly(
                &[(0.34, 0.28), (0.40, 0.16), (0.60, 0.16), (0.66, 0.28)],
                pal.paper,
                None,
            );
            c.p.circle_filled(c.pt(0.50, 0.56), c.u(0.18), pal.cyan);
            c.p.circle_stroke(c.pt(0.50, 0.56), c.u(0.18), c.stroke(0.06, pal.ink));
        }
        Icon::FolderUp => {
            c.line(0.50, 0.84, 0.50, 0.24, pal.green, 0.13);
            c.poly(
                &[(0.50, 0.12), (0.20, 0.44), (0.80, 0.44)],
                pal.green,
                Some((0.05, pal.ink)),
            );
        }
        Icon::FolderItem => {
            c.poly(
                &[
                    (0.10, 0.22),
                    (0.40, 0.22),
                    (0.48, 0.34),
                    (0.90, 0.34),
                    (0.90, 0.84),
                    (0.10, 0.84),
                ],
                pal.amber,
                Some((0.06, pal.ink)),
            );
        }
        Icon::MediaItem => {
            let box_r = c.rect(0.12, 0.18, 0.88, 0.82);
            c.p.rect_filled(box_r, c.u(0.06), pal.blue);
            c.p.rect_stroke(
                box_r,
                c.u(0.06),
                c.stroke(0.06, pal.ink),
                StrokeKind::Inside,
            );
            c.poly(&[(0.38, 0.34), (0.68, 0.50), (0.38, 0.66)], pal.paper, None);
        }
        Icon::TransitionSplit => {
            c.poly(&[(0.12, 0.16), (0.88, 0.16), (0.12, 0.84)], pal.blue, None);
            c.poly(&[(0.88, 0.16), (0.88, 0.84), (0.12, 0.84)], pal.amber, None);
            c.p.rect_stroke(
                c.rect(0.12, 0.16, 0.88, 0.84),
                0.0,
                c.stroke(0.07, pal.ink),
                StrokeKind::Inside,
            );
        }
        Icon::FxSparkle => {
            c.poly(
                &[
                    (0.44, 0.10),
                    (0.52, 0.38),
                    (0.80, 0.46),
                    (0.52, 0.54),
                    (0.44, 0.84),
                    (0.36, 0.54),
                    (0.08, 0.46),
                    (0.36, 0.38),
                ],
                pal.purple,
                None,
            );
            c.p.circle_filled(c.pt(0.78, 0.22), c.u(0.10), pal.amber);
            c.p.circle_filled(c.pt(0.76, 0.78), c.u(0.09), pal.cyan);
        }
        Icon::ColorWheels => {
            let r = c.u(0.20);
            for (cx, cy, col) in [
                (0.50, 0.30, pal.red),
                (0.32, 0.66, pal.green),
                (0.68, 0.66, pal.blue),
            ] {
                let p = c.pt(cx, cy);
                c.p.circle_filled(p, r, col);
                c.p.circle_stroke(p, r, c.stroke(0.06, pal.ink));
            }
            c.p.circle_filled(c.pt(0.50, 0.54), c.u(0.07), pal.paper);
        }
        Icon::FxAudio => {
            for (x, top, col) in [
                (0.22, 0.44, pal.green),
                (0.40, 0.18, pal.cyan),
                (0.58, 0.34, pal.amber),
                (0.76, 0.24, pal.blue),
            ] {
                c.p.rect_filled(c.rect(x - 0.06, top, x + 0.06, 0.86), c.u(0.03), col);
            }
        }
        Icon::GeneratorFrame => {
            let r = c.rect(0.12, 0.16, 0.88, 0.84);
            c.p.rect_filled(r, c.u(0.05), pal.purple);
            c.poly(&[(0.14, 0.82), (0.42, 0.44), (0.64, 0.82)], pal.green, None);
            c.poly(&[(0.48, 0.82), (0.70, 0.52), (0.86, 0.82)], pal.cyan, None);
            c.p.circle_filled(c.pt(0.70, 0.32), c.u(0.10), pal.amber);
            c.p.rect_stroke(r, c.u(0.05), c.stroke(0.06, pal.ink), StrokeKind::Inside);
        }
        Icon::MixerFaders => {
            for (x, y, col) in [
                (0.26, 0.36, pal.green),
                (0.50, 0.62, pal.cyan),
                (0.74, 0.28, pal.amber),
            ] {
                c.line(x, 0.14, x, 0.86, pal.muted, 0.08);
                c.p.rect_filled(
                    c.rect(x - 0.09, y - 0.07, x + 0.09, y + 0.07),
                    c.u(0.03),
                    col,
                );
            }
        }
        Icon::ScopesWave => {
            c.p.rect_filled(
                c.rect(0.10, 0.14, 0.90, 0.86),
                c.u(0.05),
                pal.paper.gamma_multiply(0.15),
            );
            c.p.rect_stroke(
                c.rect(0.10, 0.14, 0.90, 0.86),
                c.u(0.05),
                c.stroke(0.06, pal.ink),
                StrokeKind::Inside,
            );
            c.polyline(
                &[
                    (0.14, 0.66),
                    (0.32, 0.28),
                    (0.50, 0.64),
                    (0.68, 0.34),
                    (0.86, 0.56),
                ],
                pal.green,
                0.10,
            );
        }
        Icon::NotesPad => {
            let r = c.rect(0.18, 0.12, 0.82, 0.88);
            c.p.rect_filled(r, c.u(0.05), pal.amber);
            c.p.rect_stroke(r, c.u(0.05), c.stroke(0.06, pal.ink), StrokeKind::Inside);
            c.line(0.28, 0.34, 0.72, 0.34, pal.ink, 0.07);
            c.line(0.28, 0.52, 0.72, 0.52, pal.ink, 0.07);
            c.line(0.28, 0.70, 0.58, 0.70, pal.ink, 0.07);
        }
        Icon::PopOut => {
            c.p.rect_stroke(
                c.rect(0.14, 0.32, 0.68, 0.86),
                0.0,
                c.stroke(0.08, pal.muted),
                StrokeKind::Inside,
            );
            c.line(0.46, 0.54, 0.84, 0.16, pal.cyan, 0.11);
            c.poly(&[(0.88, 0.12), (0.58, 0.14), (0.86, 0.42)], pal.cyan, None);
        }
        Icon::DockIn => {
            c.p.rect_stroke(
                c.rect(0.14, 0.32, 0.68, 0.86),
                0.0,
                c.stroke(0.08, pal.muted),
                StrokeKind::Inside,
            );
            c.line(0.86, 0.14, 0.46, 0.54, pal.green, 0.11);
            c.poly(&[(0.40, 0.60), (0.70, 0.58), (0.42, 0.30)], pal.green, None);
        }
        Icon::KeyframeFilled => {
            c.poly(
                &[(0.50, 0.12), (0.88, 0.50), (0.50, 0.88), (0.12, 0.50)],
                pal.amber,
                Some((0.06, pal.ink)),
            );
        }
        Icon::KeyframeEmpty => {
            c.polyline(
                &[
                    (0.50, 0.14),
                    (0.86, 0.50),
                    (0.50, 0.86),
                    (0.14, 0.50),
                    (0.50, 0.14),
                ],
                pal.ink,
                0.10,
            );
        }
        Icon::MoveUp => {
            c.poly(&[(0.50, 0.22), (0.82, 0.72), (0.18, 0.72)], pal.blue, None);
        }
        Icon::MoveDown => {
            c.poly(&[(0.18, 0.28), (0.82, 0.28), (0.50, 0.78)], pal.blue, None);
        }
        Icon::ResetSpin => {
            c.polyline(
                &[
                    (0.76, 0.34),
                    (0.50, 0.20),
                    (0.24, 0.38),
                    (0.28, 0.72),
                    (0.64, 0.80),
                    (0.80, 0.60),
                ],
                pal.cyan,
                0.11,
            );
            c.poly(&[(0.84, 0.42), (0.64, 0.18), (0.86, 0.18)], pal.cyan, None);
        }
        Icon::CloseX => {
            c.line(0.22, 0.22, 0.78, 0.78, pal.red, 0.13);
            c.line(0.78, 0.22, 0.22, 0.78, pal.red, 0.13);
        }
        Icon::Backspace => {
            c.poly(
                &[
                    (0.12, 0.50),
                    (0.34, 0.22),
                    (0.88, 0.22),
                    (0.88, 0.78),
                    (0.34, 0.78),
                ],
                pal.muted,
                Some((0.06, pal.ink)),
            );
            c.line(0.46, 0.36, 0.72, 0.64, pal.red, 0.09);
            c.line(0.72, 0.36, 0.46, 0.64, pal.red, 0.09);
        }
        Icon::LoudnessMeter => {
            c.p.rect_filled(c.rect(0.14, 0.24, 0.34, 0.86), c.u(0.03), pal.green);
            c.p.rect_filled(c.rect(0.40, 0.16, 0.60, 0.86), c.u(0.03), pal.amber);
            c.p.rect_filled(c.rect(0.66, 0.34, 0.86, 0.86), c.u(0.03), pal.cyan);
            c.line(0.10, 0.36, 0.90, 0.36, pal.ink, 0.08);
        }
        Icon::BusTrack => {
            let r = c.rect(0.12, 0.16, 0.88, 0.84);
            c.p.rect_filled(r, c.u(0.07), pal.cyan.gamma_multiply(0.30));
            c.p.rect_stroke(r, c.u(0.07), c.stroke(0.07, pal.cyan), StrokeKind::Inside);
            c.line(0.24, 0.42, 0.76, 0.42, pal.ink, 0.10);
            c.line(0.24, 0.62, 0.62, 0.62, pal.ink, 0.10);
            c.poly(&[(0.78, 0.62), (0.62, 0.52), (0.62, 0.72)], pal.amber, None);
        }
        Icon::TrimmerBlade => {
            c.p.rect_filled(c.rect(0.24, 0.24, 0.76, 0.76), c.u(0.04), pal.blue);
            c.line(0.22, 0.16, 0.22, 0.84, pal.green, 0.11);
            c.line(0.78, 0.16, 0.78, 0.84, pal.amber, 0.11);
            c.line(0.50, 0.12, 0.50, 0.88, pal.red, 0.08);
        }
        Icon::EditDetailsTable => {
            let r = c.rect(0.12, 0.16, 0.88, 0.84);
            c.p.rect_filled(r, c.u(0.05), pal.paper.gamma_multiply(0.18));
            c.p.rect_filled(c.rect(0.12, 0.16, 0.88, 0.34), 0.0, pal.blue);
            c.p.rect_stroke(r, c.u(0.05), c.stroke(0.06, pal.ink), StrokeKind::Inside);
            c.line(0.14, 0.52, 0.86, 0.52, pal.muted, 0.06);
            c.line(0.14, 0.70, 0.86, 0.70, pal.muted, 0.06);
            c.line(0.42, 0.18, 0.42, 0.84, pal.muted, 0.06);
        }
        Icon::SurroundSpeakers => {
            // sala vista de cima: círculo, ouvinte no centro e 5 caixas ao redor
            c.p.circle_stroke(c.pt(0.50, 0.52), c.u(0.34), c.stroke(0.06, pal.muted));
            c.p.circle_filled(c.pt(0.50, 0.52), c.u(0.08), pal.amber);
            for (x, y) in [
                (0.50, 0.14),
                (0.22, 0.26),
                (0.78, 0.26),
                (0.18, 0.80),
                (0.82, 0.80),
            ] {
                c.p.rect_filled(
                    Rect::from_center_size(c.pt(x, y), vec2(c.u(0.16), c.u(0.12))),
                    c.u(0.02),
                    pal.blue,
                );
            }
        }
        Icon::PluginPlug => {
            // tomada: corpo arredondado, dois pinos e cabo
            c.p.rect_filled(c.rect(0.24, 0.34, 0.76, 0.66), c.u(0.08), pal.purple);
            c.line(0.38, 0.14, 0.38, 0.34, pal.ink, 0.09);
            c.line(0.62, 0.14, 0.62, 0.34, pal.ink, 0.09);
            c.polyline(&[(0.50, 0.66), (0.50, 0.78), (0.70, 0.88)], pal.ink, 0.08);
        }
        Icon::Compositors => {
            // duas camadas sobrepostas, a de cima translúcida
            c.p.rect_filled(c.rect(0.12, 0.30, 0.62, 0.80), c.u(0.04), pal.blue);
            c.p.rect_filled(
                c.rect(0.38, 0.18, 0.88, 0.68),
                c.u(0.04),
                pal.amber.gamma_multiply(0.75),
            );
            c.p.rect_stroke(
                c.rect(0.38, 0.18, 0.88, 0.68),
                c.u(0.04),
                c.stroke(0.05, pal.ink),
                StrokeKind::Inside,
            );
        }
        Icon::EncoderQueue => {
            // pilhas de fila + triângulo verde de codificação (arte original)
            c.p.rect_filled(c.rect(0.14, 0.16, 0.78, 0.34), c.u(0.03), pal.blue);
            c.p.rect_filled(c.rect(0.14, 0.42, 0.60, 0.60), c.u(0.03), pal.cyan);
            c.p.rect_filled(c.rect(0.14, 0.68, 0.56, 0.86), c.u(0.03), pal.muted);
            c.poly(
                &[(0.64, 0.44), (0.90, 0.64), (0.64, 0.84)],
                pal.green,
                Some((0.04, pal.ink)),
            );
        }
        Icon::TrimSelection => {
            // evento: só o trecho entre as duas marcas fica
            c.p.rect_filled(c.rect(0.06, 0.38, 0.30, 0.66), c.u(0.03), pal.muted);
            c.p.rect_filled(c.rect(0.70, 0.38, 0.94, 0.66), c.u(0.03), pal.muted);
            let keep = c.rect(0.30, 0.34, 0.70, 0.70);
            c.p.rect_filled(keep, c.u(0.03), pal.blue);
            c.p.rect_stroke(keep, c.u(0.03), c.stroke(0.06, pal.ink), StrokeKind::Inside);
            c.line(0.30, 0.16, 0.30, 0.86, pal.amber, 0.09);
            c.line(0.70, 0.16, 0.70, 0.86, pal.amber, 0.09);
        }
        Icon::TrimStart => {
            // o começo do evento sai até o cursor
            c.p.rect_filled(c.rect(0.08, 0.38, 0.44, 0.66), c.u(0.03), pal.muted);
            let keep = c.rect(0.44, 0.34, 0.92, 0.70);
            c.p.rect_filled(keep, c.u(0.03), pal.blue);
            c.p.rect_stroke(keep, c.u(0.03), c.stroke(0.06, pal.ink), StrokeKind::Inside);
            c.line(0.44, 0.12, 0.44, 0.90, pal.red, 0.10);
            c.poly(&[(0.16, 0.20), (0.32, 0.12), (0.32, 0.28)], pal.red, None);
        }
        Icon::TrimEnd => {
            // o fim do evento sai a partir do cursor
            let keep = c.rect(0.08, 0.34, 0.56, 0.70);
            c.p.rect_filled(keep, c.u(0.03), pal.blue);
            c.p.rect_stroke(keep, c.u(0.03), c.stroke(0.06, pal.ink), StrokeKind::Inside);
            c.p.rect_filled(c.rect(0.56, 0.38, 0.92, 0.66), c.u(0.03), pal.muted);
            c.line(0.56, 0.12, 0.56, 0.90, pal.red, 0.10);
            c.poly(&[(0.84, 0.20), (0.68, 0.12), (0.68, 0.28)], pal.red, None);
        }
        Icon::InsertMarker => {
            // marca numerada no alto da régua, com a linha descendo
            c.poly(
                &[
                    (0.30, 0.12),
                    (0.70, 0.12),
                    (0.70, 0.36),
                    (0.50, 0.48),
                    (0.30, 0.36),
                ],
                pal.amber,
                Some((0.06, pal.ink)),
            );
            c.line(0.50, 0.48, 0.50, 0.92, pal.amber, 0.09);
        }
        Icon::InsertRegion => {
            // duas marcas nas pontas do trecho, ligadas no alto
            c.p.rect_filled(
                c.rect(0.12, 0.22, 0.88, 0.30),
                0.0,
                pal.green.gamma_multiply(0.6),
            );
            for x in [0.16_f32, 0.84] {
                c.poly(
                    &[
                        (x - 0.10, 0.12),
                        (x + 0.10, 0.12),
                        (x + 0.10, 0.32),
                        (x - 0.10, 0.32),
                    ],
                    pal.green,
                    Some((0.05, pal.ink)),
                );
                c.line(x, 0.32, x, 0.92, pal.green, 0.08);
            }
        }
        Icon::ExternalApps => {
            // dois quadros interligados por elo de ida e volta (integração externa)
            let back = c.rect(0.12, 0.24, 0.62, 0.78);
            c.p.rect_filled(back, c.u(0.08), pal.blue.gamma_multiply(0.28));
            c.p.rect_stroke(
                back,
                c.u(0.08),
                c.stroke(0.08, pal.cyan),
                StrokeKind::Inside,
            );
            let front = c.rect(0.40, 0.14, 0.88, 0.64);
            c.p.rect_filled(front, c.u(0.08), pal.paper.gamma_multiply(0.18));
            c.p.rect_stroke(
                front,
                c.u(0.08),
                c.stroke(0.08, pal.amber),
                StrokeKind::Inside,
            );
            c.line(0.46, 0.56, 0.78, 0.24, pal.ink, 0.09);
            c.line(0.60, 0.24, 0.78, 0.24, pal.ink, 0.09);
            c.line(0.78, 0.24, 0.78, 0.42, pal.ink, 0.09);
        }
        Icon::NormalTool => {
            // seta de seleção
            c.poly(
                &[
                    (0.26, 0.10),
                    (0.26, 0.78),
                    (0.42, 0.63),
                    (0.54, 0.90),
                    (0.66, 0.84),
                    (0.54, 0.58),
                    (0.76, 0.58),
                ],
                pal.paper,
                Some((0.06, pal.ink)),
            );
        }
        Icon::EnvelopeTool => {
            // linha de envelope com pontos
            let pts = [(0.10, 0.72), (0.36, 0.34), (0.64, 0.56), (0.90, 0.24)];
            c.polyline(&pts, pal.green, 0.08);
            for (x, y) in pts {
                c.p.circle_filled(c.pt(x, y), c.u(0.08), pal.amber);
            }
        }
        Icon::SelectionTool => {
            // caixa de seleção tracejada sobre dois eventos
            c.p.rect_filled(c.rect(0.14, 0.40, 0.48, 0.60), c.u(0.03), pal.blue);
            c.p.rect_filled(c.rect(0.54, 0.40, 0.88, 0.60), c.u(0.03), pal.cyan);
            let r = c.rect(0.08, 0.22, 0.94, 0.80);
            let corners = [
                r.left_top(),
                r.right_top(),
                r.right_bottom(),
                r.left_bottom(),
                r.left_top(),
            ];
            c.p.extend(Shape::dashed_line(
                &corners,
                c.stroke(0.06, pal.ink),
                c.u(0.10),
                c.u(0.07),
            ));
        }
        Icon::ShuffleTool => {
            // dois blocos alternando ordem com setas opostas
            c.p.rect_filled(c.rect(0.12, 0.22, 0.44, 0.44), c.u(0.04), pal.blue);
            c.p.rect_filled(c.rect(0.56, 0.56, 0.88, 0.78), c.u(0.04), pal.cyan);
            c.line(0.52, 0.30, 0.86, 0.30, pal.amber, 0.08);
            c.line(0.76, 0.20, 0.86, 0.30, pal.amber, 0.08);
            c.line(0.76, 0.40, 0.86, 0.30, pal.amber, 0.08);
            c.line(0.14, 0.68, 0.48, 0.68, pal.amber, 0.08);
            c.line(0.24, 0.58, 0.14, 0.68, pal.amber, 0.08);
            c.line(0.24, 0.78, 0.14, 0.68, pal.amber, 0.08);
        }
        Icon::SlipTool => {
            // evento fixo com setas de conteúdo interno deslizando
            c.p.rect_stroke(
                c.rect(0.16, 0.26, 0.84, 0.74),
                c.u(0.05),
                c.stroke(0.08, pal.ink),
                StrokeKind::Inside,
            );
            c.line(0.24, 0.50, 0.76, 0.50, pal.cyan, 0.09);
            c.line(0.36, 0.38, 0.24, 0.50, pal.cyan, 0.08);
            c.line(0.36, 0.62, 0.24, 0.50, pal.cyan, 0.08);
            c.line(0.64, 0.38, 0.76, 0.50, pal.cyan, 0.08);
            c.line(0.64, 0.62, 0.76, 0.50, pal.cyan, 0.08);
        }
        Icon::SlideTool => {
            // barra central deslizando entre dois blocos adjacentes
            c.p.rect_filled(c.rect(0.10, 0.32, 0.26, 0.68), c.u(0.03), pal.muted);
            c.p.rect_filled(c.rect(0.30, 0.26, 0.70, 0.74), c.u(0.04), pal.amber);
            c.p.rect_filled(c.rect(0.74, 0.32, 0.90, 0.68), c.u(0.03), pal.muted);
            c.line(0.38, 0.50, 0.62, 0.50, pal.ink, 0.08);
        }
        Icon::StretchTool => {
            // onda comprimida/esticada entre colchetes móveis
            c.line(0.16, 0.22, 0.16, 0.78, pal.ink, 0.08);
            c.line(0.84, 0.22, 0.84, 0.78, pal.ink, 0.08);
            c.polyline(
                &[
                    (0.24, 0.50),
                    (0.34, 0.28),
                    (0.46, 0.72),
                    (0.58, 0.28),
                    (0.70, 0.72),
                    (0.76, 0.50),
                ],
                pal.green,
                0.08,
            );
        }
        Icon::SplitTrimTool => {
            // lâmina com seta de descarte de trecho
            c.line(0.40, 0.14, 0.40, 0.86, pal.amber, 0.09);
            c.p.rect_filled(
                c.rect(0.44, 0.32, 0.86, 0.68),
                c.u(0.04),
                pal.red.gamma_multiply(0.55),
            );
            c.line(0.52, 0.38, 0.78, 0.62, pal.ink, 0.08);
            c.line(0.78, 0.38, 0.52, 0.62, pal.ink, 0.08);
        }
        Icon::RollTool => {
            // linha vertical de corte com setas duplas esquerda/direita
            c.p.rect_filled(c.rect(0.12, 0.30, 0.46, 0.70), c.u(0.03), pal.blue);
            c.p.rect_filled(c.rect(0.54, 0.30, 0.88, 0.70), c.u(0.03), pal.cyan);
            c.line(0.50, 0.16, 0.50, 0.84, pal.ink, 0.09);
            c.line(0.24, 0.50, 0.76, 0.50, pal.amber, 0.08);
        }
        Icon::SmartSplit => {
            // dois segmentos convergindo sobre região de corte com transição
            c.p.rect_filled(c.rect(0.10, 0.26, 0.44, 0.74), c.u(0.04), pal.blue);
            c.p.rect_filled(c.rect(0.56, 0.26, 0.90, 0.74), c.u(0.04), pal.cyan);
            c.poly(
                &[(0.42, 0.22), (0.58, 0.50), (0.42, 0.78)],
                pal.amber,
                Some((0.05, pal.ink)),
            );
        }
        Icon::HealEvents => {
            // duas metades se unindo no centro
            c.p.rect_filled(c.rect(0.12, 0.28, 0.46, 0.72), c.u(0.04), pal.green);
            c.p.rect_filled(c.rect(0.54, 0.28, 0.88, 0.72), c.u(0.04), pal.green);
            c.line(0.36, 0.50, 0.64, 0.50, pal.paper, 0.10);
            c.line(0.50, 0.36, 0.50, 0.64, pal.paper, 0.10);
        }
        Icon::PasteAttributes => {
            // prancheta com estrela/atributo
            let r = c.rect(0.20, 0.18, 0.76, 0.86);
            c.p.rect_filled(r, c.u(0.06), pal.paper);
            c.p.rect_stroke(r, c.u(0.06), c.stroke(0.07, pal.ink), StrokeKind::Inside);
            c.line(0.36, 0.14, 0.60, 0.14, pal.ink, 0.10);
            c.line(0.32, 0.46, 0.64, 0.46, pal.blue, 0.08);
            c.line(0.32, 0.64, 0.58, 0.64, pal.amber, 0.08);
        }
        Icon::RecordDot => {
            c.p.circle_filled(c.pt(0.50, 0.50), c.u(0.28), pal.red);
            c.p.circle_stroke(c.pt(0.50, 0.50), c.u(0.36), c.stroke(0.07, pal.ink));
        }
        Icon::ExpandedEdit => {
            c.p.rect_filled(c.rect(0.10, 0.24, 0.45, 0.76), c.u(0.03), pal.blue);
            c.p.rect_filled(c.rect(0.55, 0.24, 0.90, 0.76), c.u(0.03), pal.cyan);
            c.line(0.49, 0.14, 0.49, 0.86, pal.amber, 0.09);
            c.line(0.36, 0.18, 0.62, 0.18, pal.ink, 0.07);
            c.line(0.36, 0.82, 0.62, 0.82, pal.ink, 0.07);
        }
        Icon::SceneDetect => {
            c.p.rect_stroke(
                c.rect(0.14, 0.20, 0.86, 0.80),
                c.u(0.05),
                c.stroke(0.07, pal.ink),
                StrokeKind::Inside,
            );
            c.line(0.42, 0.14, 0.42, 0.86, pal.amber, 0.09);
            c.line(0.68, 0.14, 0.68, 0.86, pal.amber, 0.09);
        }
        Icon::StabilizeVideo => {
            c.p.rect_stroke(
                c.rect(0.14, 0.18, 0.86, 0.82),
                c.u(0.05),
                c.stroke(0.07, pal.ink),
                StrokeKind::Inside,
            );
            c.p.rect_stroke(
                c.rect(0.28, 0.30, 0.72, 0.70),
                c.u(0.03),
                c.stroke(0.08, pal.green),
                StrokeKind::Inside,
            );
        }
        Icon::ViewThumbnails => {
            for (rx, ry) in [(0.16, 0.16), (0.54, 0.16), (0.16, 0.54), (0.54, 0.54)] {
                c.p.rect_filled(c.rect(rx, ry, rx + 0.28, ry + 0.28), c.u(0.04), pal.ink);
            }
        }
        Icon::ViewList => {
            for ry in [0.22, 0.46, 0.70] {
                c.p.rect_filled(c.rect(0.14, ry, 0.24, ry + 0.10), c.u(0.03), pal.blue);
                c.line(0.32, ry + 0.05, 0.86, ry + 0.05, pal.ink, 0.08);
            }
        }
        Icon::ViewDetails => {
            c.p.rect_filled(c.rect(0.14, 0.16, 0.86, 0.28), c.u(0.03), pal.blue);
            for ry in [0.42, 0.60, 0.78] {
                c.line(0.14, ry, 0.44, ry, pal.ink, 0.07);
                c.line(0.52, ry, 0.68, ry, pal.muted, 0.07);
                c.line(0.74, ry, 0.86, ry, pal.muted, 0.07);
            }
        }
        Icon::StoryboardGrid => {
            c.p.rect_stroke(
                c.rect(0.12, 0.22, 0.42, 0.76),
                c.u(0.04),
                c.stroke(0.07, pal.blue),
                StrokeKind::Inside,
            );
            c.p.rect_stroke(
                c.rect(0.58, 0.22, 0.88, 0.76),
                c.u(0.04),
                c.stroke(0.07, pal.green),
                StrokeKind::Inside,
            );
            c.line(0.42, 0.49, 0.58, 0.49, pal.amber, 0.08);
        }
        Icon::SmartFolder => {
            c.p.rect_stroke(
                c.rect(0.14, 0.24, 0.86, 0.82),
                c.u(0.06),
                c.stroke(0.07, pal.amber),
                StrokeKind::Inside,
            );
            c.line(0.18, 0.24, 0.44, 0.24, pal.amber, 0.09);
            c.p.circle_filled(c.pt(0.62, 0.55), c.u(0.12), pal.cyan);
        }
        Icon::ReplaceMedia => {
            c.line(0.20, 0.34, 0.76, 0.34, pal.blue, 0.08);
            c.line(0.62, 0.22, 0.78, 0.34, pal.blue, 0.08);
            c.line(0.80, 0.66, 0.24, 0.66, pal.green, 0.08);
            c.line(0.38, 0.78, 0.22, 0.66, pal.green, 0.08);
        }
        Icon::SplitPreview => {
            c.p.rect_stroke(
                c.rect(0.14, 0.20, 0.86, 0.80),
                c.u(0.04),
                c.stroke(0.07, pal.ink),
                StrokeKind::Inside,
            );
            c.p.rect_filled(c.rect(0.50, 0.24, 0.82, 0.76), c.u(0.02), pal.blue);
            c.line(0.50, 0.14, 0.50, 0.86, pal.amber, 0.09);
        }
        Icon::RamCache => {
            c.p.rect_stroke(
                c.rect(0.16, 0.26, 0.84, 0.74),
                c.u(0.05),
                c.stroke(0.07, pal.green),
                StrokeKind::Inside,
            );
            for rx in [0.28, 0.46, 0.64] {
                c.p.rect_filled(c.rect(rx, 0.36, rx + 0.10, 0.64), c.u(0.02), pal.ink);
            }
            for rx in [0.24, 0.42, 0.60, 0.74] {
                c.line(rx, 0.14, rx, 0.26, pal.amber, 0.07);
                c.line(rx, 0.74, rx, 0.86, pal.amber, 0.07);
            }
        }
        Icon::CopyFrame => {
            c.p.rect_stroke(
                c.rect(0.14, 0.16, 0.66, 0.68),
                c.u(0.04),
                c.stroke(0.07, pal.muted),
                StrokeKind::Inside,
            );
            c.p.rect_filled(c.rect(0.34, 0.32, 0.86, 0.84), c.u(0.04), pal.paper);
            c.p.rect_stroke(
                c.rect(0.34, 0.32, 0.86, 0.84),
                c.u(0.04),
                c.stroke(0.07, pal.blue),
                StrokeKind::Inside,
            );
        }
        Icon::MuteAudio => {
            c.line(0.18, 0.42, 0.34, 0.42, pal.ink, 0.09);
            c.line(0.34, 0.42, 0.54, 0.24, pal.ink, 0.08);
            c.line(0.54, 0.24, 0.54, 0.76, pal.ink, 0.08);
            c.line(0.54, 0.76, 0.34, 0.58, pal.ink, 0.08);
            c.line(0.18, 0.82, 0.84, 0.18, pal.red, 0.09);
        }
        Icon::MuteVideo => {
            c.p.rect_stroke(
                c.rect(0.14, 0.24, 0.86, 0.76),
                c.u(0.05),
                c.stroke(0.07, pal.ink),
                StrokeKind::Inside,
            );
            c.line(0.16, 0.82, 0.84, 0.18, pal.red, 0.09);
        }
        Icon::ExternalMonitor => {
            c.p.rect_stroke(
                c.rect(0.12, 0.18, 0.88, 0.68),
                c.u(0.05),
                c.stroke(0.07, pal.blue),
                StrokeKind::Inside,
            );
            c.line(0.50, 0.68, 0.50, 0.84, pal.ink, 0.08);
            c.line(0.30, 0.84, 0.70, 0.84, pal.ink, 0.08);
        }
        Icon::TrackFolder => {
            c.p.rect_filled(c.rect(0.12, 0.22, 0.44, 0.34), c.u(0.04), pal.amber);
            c.p.rect_filled(
                c.rect(0.12, 0.32, 0.88, 0.82),
                c.u(0.06),
                pal.amber.gamma_multiply(0.85),
            );
            c.line(0.24, 0.52, 0.76, 0.52, pal.ink, 0.07);
            c.line(0.24, 0.68, 0.64, 0.68, pal.ink, 0.07);
        }
        Icon::ParentChild => {
            c.p.rect_filled(c.rect(0.14, 0.18, 0.54, 0.38), c.u(0.04), pal.blue);
            c.polyline(&[(0.28, 0.38), (0.28, 0.68), (0.48, 0.68)], pal.ink, 0.08);
            c.p.rect_filled(c.rect(0.48, 0.56, 0.88, 0.78), c.u(0.04), pal.cyan);
        }
        Icon::TrackMask => {
            c.p.rect_stroke(
                c.rect(0.14, 0.20, 0.86, 0.80),
                c.u(0.06),
                c.stroke(0.07, pal.ink),
                StrokeKind::Inside,
            );
            c.p.circle_filled(c.pt(0.50, 0.50), c.u(0.18), pal.paper);
        }
        Icon::NestedTimeline => {
            c.p.rect_filled(
                c.rect(0.12, 0.20, 0.88, 0.80),
                c.u(0.06),
                pal.blue.gamma_multiply(0.45),
            );
            c.p.rect_stroke(
                c.rect(0.12, 0.20, 0.88, 0.80),
                c.u(0.06),
                c.stroke(0.06, pal.cyan),
                StrokeKind::Inside,
            );
            c.line(0.22, 0.42, 0.78, 0.42, pal.green, 0.09);
            c.line(0.30, 0.60, 0.70, 0.60, pal.amber, 0.09);
        }
        Icon::Multicam => {
            for (x0, y0, x1, y1, col) in [
                (0.14, 0.16, 0.46, 0.46, pal.blue),
                (0.54, 0.16, 0.86, 0.46, pal.cyan),
                (0.14, 0.54, 0.46, 0.84, pal.green),
                (0.54, 0.54, 0.86, 0.84, pal.amber),
            ] {
                c.p.rect_filled(c.rect(x0, y0, x1, y1), c.u(0.04), col);
            }
        }
        Icon::Stereo3d => {
            c.p.rect_stroke(
                c.rect(0.12, 0.28, 0.48, 0.72),
                c.u(0.05),
                c.stroke(0.08, pal.red),
                StrokeKind::Inside,
            );
            c.p.rect_stroke(
                c.rect(0.52, 0.28, 0.88, 0.72),
                c.u(0.05),
                c.stroke(0.08, pal.cyan),
                StrokeKind::Inside,
            );
            c.line(0.48, 0.46, 0.52, 0.46, pal.ink, 0.08);
        }
        Icon::RenderTrack => {
            c.p.rect_filled(c.rect(0.14, 0.22, 0.86, 0.42), c.u(0.04), pal.muted);
            c.polyline(&[(0.50, 0.44), (0.50, 0.64)], pal.green, 0.09);
            c.poly(&[(0.38, 0.56), (0.62, 0.56), (0.50, 0.70)], pal.green, None);
            c.p.rect_filled(c.rect(0.14, 0.74, 0.86, 0.90), c.u(0.04), pal.blue);
        }
        Icon::TimelineGrid => {
            c.p.rect_stroke(
                c.rect(0.14, 0.14, 0.86, 0.86),
                c.u(0.04),
                c.stroke(0.07, pal.ink),
                StrokeKind::Inside,
            );
            for t in [0.38, 0.62] {
                c.line(t, 0.14, t, 0.86, pal.blue, 0.06);
                c.line(0.14, t, 0.86, t, pal.muted, 0.06);
            }
        }
    }
}

/// Pinta apenas o ícone vetorial como widget inline (para cabeçalhos, listas e abas).
pub fn paint_inline(
    ui: &mut egui::Ui,
    icon: Icon,
    size: f32,
    theme: &ThemeColors,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    paint(ui.painter(), rect, icon, theme, ui.is_enabled());
    resp
}

/// Rótulo horizontal com ícone vetorial seguido de texto (sem caracteres Unicode ausentes).
pub fn icon_label(
    ui: &mut egui::Ui,
    icon: Icon,
    size: f32,
    text: impl Into<egui::WidgetText>,
    theme: &ThemeColors,
) {
    ui.horizontal(|ui| {
        paint_inline(ui, icon, size, theme);
        ui.add(egui::Label::new(text).extend());
    });
}

struct BtnOpts<'a> {
    icon: Icon,
    label: Option<&'a str>,
    icon_size: f32,
    pad: egui::Vec2,
    active: bool,
    enabled: bool,
    draggable: bool,
}

fn draw_button(ui: &mut egui::Ui, opts: BtnOpts<'_>, theme: &ThemeColors) -> egui::Response {
    let BtnOpts {
        icon,
        label,
        icon_size,
        pad,
        active,
        enabled,
        draggable,
    } = opts;
    let sel_fg = crate::theme::contrast_on(theme.selection_bg);
    let label_fg = if !enabled {
        theme.text_muted
    } else if active {
        sel_fg
    } else {
        theme.text_primary
    };
    let galley = label.map(|t| {
        ui.painter().layout_no_wrap(
            t.to_string(),
            egui::TextStyle::Button.resolve(ui.style()),
            label_fg,
        )
    });
    let text_w = galley.as_ref().map(|g| g.size().x + 5.0).unwrap_or(0.0);
    let text_h = galley.as_ref().map(|g| g.size().y).unwrap_or(0.0);
    let w = (pad.x * 2.0 + icon_size + text_w).max(icon_size + 8.0);
    let h = (pad.y * 2.0 + icon_size.max(text_h)).max(icon_size + 6.0);
    let sense = if !enabled {
        Sense::hover()
    } else if draggable {
        Sense::click_and_drag()
    } else {
        Sense::click()
    };
    let (rect, resp) = ui.allocate_exact_size(vec2(w, h), sense);
    resp.widget_info(|| {
        if let Some(lbl) = label {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, lbl)
        } else {
            egui::WidgetInfo::new(egui::WidgetType::Button)
        }
    });

    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let hovered = enabled && resp.hovered();
        let down = enabled && resp.is_pointer_button_down_on();
        let fill = if active {
            theme.selection_bg
        } else if down || hovered {
            theme.widget_hover_bg
        } else {
            Color32::TRANSPARENT
        };
        if fill != Color32::TRANSPARENT {
            p.rect_filled(rect, 3.0_f32, fill);
        }
        if active || hovered || theme.high_contrast {
            let border = if active { theme.accent } else { theme.border };
            p.rect_stroke(
                rect,
                3.0_f32,
                Stroke::new(1.0_f32, border),
                StrokeKind::Inside,
            );
        }
        let iy = rect.center().y - icon_size * 0.5;
        let ix = if galley.is_some() {
            rect.left() + pad.x
        } else {
            rect.center().x - icon_size * 0.5
        };
        let icon_rect = Rect::from_min_size(pos2(ix, iy), vec2(icon_size, icon_size));
        if active && crate::theme::relative_luminance(theme.selection_bg) < 0.35 {
            let mut active_theme = theme.clone();
            active_theme.dark_mode = true;
            if active_theme.high_contrast {
                active_theme.text_primary = sel_fg;
                active_theme.accent = sel_fg;
            }
            paint(p, icon_rect, icon, &active_theme, enabled);
        } else {
            paint(p, icon_rect, icon, theme, enabled);
        }
        if let Some(g) = galley {
            let tx = icon_rect.right() + 5.0;
            let ty = rect.center().y - g.size().y * 0.5;
            p.galley(pos2(tx, ty), g, label_fg);
        }
    }
    resp
}

pub fn icon_button(
    ui: &mut egui::Ui,
    icon: Icon,
    label: Option<&str>,
    active: bool,
    enabled: bool,
    theme: &ThemeColors,
) -> egui::Response {
    draw_button(
        ui,
        BtnOpts {
            icon,
            label,
            icon_size: 16.0,
            pad: vec2(5.0, 3.0),
            active,
            enabled,
            draggable: false,
        },
        theme,
    )
}

pub fn small_icon_button(
    ui: &mut egui::Ui,
    icon: Icon,
    label: Option<&str>,
    active: bool,
    theme: &ThemeColors,
) -> egui::Response {
    draw_button(
        ui,
        BtnOpts {
            icon,
            label,
            icon_size: 13.0,
            pad: vec2(3.0, 2.0),
            active,
            enabled: ui.is_enabled(),
            draggable: false,
        },
        theme,
    )
}

pub fn small_icon_drag_button(
    ui: &mut egui::Ui,
    icon: Icon,
    label: Option<&str>,
    active: bool,
    theme: &ThemeColors,
) -> egui::Response {
    draw_button(
        ui,
        BtnOpts {
            icon,
            label,
            icon_size: 13.0,
            pad: vec2(3.0, 2.0),
            active,
            enabled: ui.is_enabled(),
            draggable: true,
        },
        theme,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{palette, ThemeId};

    #[test]
    fn todos_os_icones_desenham_em_todos_os_temas_sem_panico() {
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                for tid in ThemeId::ALL {
                    let th = palette(tid);
                    for icon in Icon::ALL {
                        let (rect, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                        paint(ui.painter(), rect, icon, &th, true);
                        paint(ui.painter(), rect, icon, &th, false);
                    }
                }
            });
        });
    }
}
