//! Sobreposições da Prévia de Vídeo (manual do VEGAS Pro 21, p. 197-198):
//! isolamento de canal de cor, grade, réguas em pixels do projeto e medidor
//! do fps exibido. Funções puras, sem estado de UI, para poderem ser testadas
//! com entradas hostis (§0.4).

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Canal mostrado sozinho na prévia (*Color Channel Isolation*).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ChannelIsolation {
    #[default]
    Off,
    Red,
    Green,
    Blue,
    /// Alfa sempre em escala de cinza (*Alpha as grayscale*).
    Alpha,
}

impl ChannelIsolation {
    pub const ALL: [ChannelIsolation; 5] = [
        ChannelIsolation::Off,
        ChannelIsolation::Red,
        ChannelIsolation::Green,
        ChannelIsolation::Blue,
        ChannelIsolation::Alpha,
    ];

    /// Rótulo em pt-BR (chave do catálogo).
    pub fn label(self) -> &'static str {
        match self {
            ChannelIsolation::Off => "Todos os canais",
            ChannelIsolation::Red => "Canal vermelho",
            ChannelIsolation::Green => "Canal verde",
            ChannelIsolation::Blue => "Canal azul",
            ChannelIsolation::Alpha => "Canal alfa (em cinza)",
        }
    }
}

/// Isola um canal de uma imagem RGBA (bytes `[r, g, b, a]` não
/// pré-multiplicados). Com `gray`, o canal vira cinza; sem, fica na própria
/// cor. O alfa é sempre mostrado em cinza. O resultado é opaco. Um buffer
/// com tamanho que não é múltiplo de 4 só tem os pixels inteiros alterados.
pub fn isolate_channel(rgba: &mut [u8], mode: ChannelIsolation, gray: bool) {
    if mode == ChannelIsolation::Off {
        return;
    }
    for px in rgba.as_chunks_mut::<4>().0 {
        let v = match mode {
            ChannelIsolation::Red => px[0],
            ChannelIsolation::Green => px[1],
            ChannelIsolation::Blue => px[2],
            ChannelIsolation::Alpha => px[3],
            ChannelIsolation::Off => return,
        };
        let out = if gray || mode == ChannelIsolation::Alpha {
            [v, v, v]
        } else {
            match mode {
                ChannelIsolation::Red => [v, 0, 0],
                ChannelIsolation::Green => [0, v, 0],
                _ => [0, 0, v],
            }
        };
        px[0] = out[0];
        px[1] = out[1];
        px[2] = out[2];
        px[3] = 255;
    }
}

/// Uma marca da régua: posição na tela, valor em pixels do projeto e se é
/// uma marca maior (com número).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RulerTick {
    pub pos: f32,
    pub value: u32,
    pub major: bool,
}

/// Máximo de marcas por régua (limita a alocação em zoom extremo).
pub const MAX_RULER_TICKS: usize = 400;

/// Marcas de uma régua ao longo de um eixo: `start` e `len` são a posição e o
/// tamanho da imagem na tela, `project_px` o tamanho do quadro do projeto
/// nesse eixo, e `min_gap` o espaço mínimo na tela entre marcas menores. O
/// passo é 1, 2 ou 5 vezes uma potência de 10 em pixels do projeto; a cada 5
/// marcas menores há uma maior. Entrada inválida dá lista vazia.
pub fn ruler_ticks(start: f32, len: f32, project_px: u32, min_gap: f32) -> Vec<RulerTick> {
    if !start.is_finite()
        || !len.is_finite()
        || !min_gap.is_finite()
        || len < 1.0
        || min_gap <= 0.0
        || project_px == 0
    {
        return Vec::new();
    }
    let screen_per_px = len / project_px as f32;
    let mut step: u32 = 1;
    let mut i = 0usize;
    while (step as f32) * screen_per_px < min_gap {
        let base = 10u32.saturating_pow((i / 3) as u32);
        step = base.saturating_mul([1, 2, 5][i % 3]);
        i += 1;
        if step >= project_px || i > 30 {
            break;
        }
    }
    let step = step.max(1);
    let mut out = Vec::new();
    let mut v: u32 = 0;
    let mut k: u32 = 0;
    while v <= project_px && out.len() < MAX_RULER_TICKS {
        out.push(RulerTick {
            pos: start + v as f32 * screen_per_px,
            value: v,
            major: k.is_multiple_of(5),
        });
        k = k.saturating_add(1);
        v = match v.checked_add(step) {
            Some(n) => n,
            None => break,
        };
    }
    out
}

/// Frações `k / n` (`k = 1..n`) das linhas da grade com `n` divisões; `n`
/// limitado a 2..=64.
pub fn grid_fractions(divisions: u8) -> Vec<f32> {
    let n = divisions.clamp(2, 64);
    (1..n).map(|k| f32::from(k) / f32::from(n)).collect()
}

/// Fps exibido: conta os quadros que chegaram na prévia no último segundo.
#[derive(Debug, Default)]
pub struct FpsMeter {
    stamps: VecDeque<Instant>,
}

impl FpsMeter {
    const WINDOW: Duration = Duration::from_secs(1);
    const MAX_STAMPS: usize = 512;

    pub fn clear(&mut self) {
        self.stamps.clear();
    }

    pub fn push(&mut self, at: Instant) {
        if self.stamps.back().is_some_and(|last| at < *last) {
            // relógio voltou: recomeça
            self.stamps.clear();
        }
        self.stamps.push_back(at);
        while self.stamps.len() > Self::MAX_STAMPS {
            self.stamps.pop_front();
        }
        while let (Some(first), Some(last)) = (self.stamps.front(), self.stamps.back()) {
            if last.saturating_duration_since(*first) > Self::WINDOW {
                self.stamps.pop_front();
            } else {
                break;
            }
        }
    }

    /// Quadros por segundo nos últimos quadros, ou `None` com menos de dois
    /// quadros ou intervalo nulo.
    pub fn fps(&self) -> Option<f64> {
        let (first, last) = (self.stamps.front()?, self.stamps.back()?);
        let secs = last.saturating_duration_since(*first).as_secs_f64();
        let n = self.stamps.len().checked_sub(1)?;
        if n == 0 || secs <= 0.0 {
            return None;
        }
        let fps = n as f64 / secs;
        fps.is_finite().then_some(fps)
    }
}

/// Canal de legendas fechadas (CEA-608 CC1–CC4) exibido como sobreposição na
/// Prévia de Vídeo (manual do VEGAS Pro 21, p. 199).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ClosedCaptionChannel {
    #[default]
    Off,
    Cc1,
    Cc2,
    Cc3,
    Cc4,
}

impl ClosedCaptionChannel {
    pub const ALL: [ClosedCaptionChannel; 5] =
        [Self::Off, Self::Cc1, Self::Cc2, Self::Cc3, Self::Cc4];

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Legendas fechadas: Desligado",
            Self::Cc1 => "CC1",
            Self::Cc2 => "CC2",
            Self::Cc3 => "CC3",
            Self::Cc4 => "CC4",
        }
    }

    pub fn prefix(self) -> Option<&'static str> {
        match self {
            Self::Off => None,
            Self::Cc1 => Some("CC1"),
            Self::Cc2 => Some("CC2"),
            Self::Cc3 => Some("CC3"),
            Self::Cc4 => Some("CC4"),
        }
    }
}

/// Resolve o texto de legenda fechada ativo em `t` para o canal `channel`,
/// procurando em marcadores de comando (`CC1:...`..`CC4:...`) e regiões
/// rotuladas, limitando o texto exibido para proteção contra entradas hostis.
pub fn resolve_closed_caption(
    project: &neoveg_engine::model::Project,
    t: neoveg_engine::model::Ticks,
    channel: ClosedCaptionChannel,
) -> Option<String> {
    let tag = channel.prefix()?;
    // 1. Procura região ativa no cursor cuja etiqueta corresponda ao canal
    // (ou qualquer região em CC1 quando não tem prefixo CC2..CC4).
    for r in &project.regions {
        if t >= r.start && t <= r.end {
            let trimmed = r.label.trim();
            if let Some(rest) = strip_cc_prefix(trimmed, tag) {
                if !rest.is_empty() {
                    return Some(rest.chars().take(160).collect());
                }
            } else if channel == ClosedCaptionChannel::Cc1
                && !trimmed.is_empty()
                && !has_any_cc_prefix(trimmed)
            {
                return Some(trimmed.chars().take(160).collect());
            }
        }
    }
    // 2. Procura o marcador de comando mais recente antes/em `t` (até 5 s).
    let max_age = neoveg_engine::model::Ticks::from_secs_i64(5);
    let mut best: Option<(neoveg_engine::model::Ticks, String)> = None;
    for m in &project.markers {
        if m.kind != neoveg_engine::model::MarkerKind::Command || m.t > t {
            continue;
        }
        if t - m.t > max_age {
            continue;
        }
        let text = if !m.command_arg.trim().is_empty() {
            m.command_arg.trim()
        } else {
            m.label.trim()
        };
        if let Some(rest) = strip_cc_prefix(text, tag) {
            if !rest.is_empty() && best.as_ref().is_none_or(|(bt, _)| m.t >= *bt) {
                best = Some((m.t, rest.chars().take(160).collect()));
            }
        }
    }
    best.map(|(_, s)| s)
}

fn has_any_cc_prefix(s: &str) -> bool {
    ["CC1", "CC2", "CC3", "CC4"]
        .iter()
        .any(|p| strip_cc_prefix(s, p).is_some())
}

fn strip_cc_prefix<'a>(s: &'a str, tag: &str) -> Option<&'a str> {
    let head = s.get(..tag.len())?;
    if !head.eq_ignore_ascii_case(tag) {
        return None;
    }
    let rest = s.get(tag.len()..)?;
    if let Some(after) = rest.strip_prefix(':').or_else(|| rest.strip_prefix(' ')) {
        Some(after.trim())
    } else {
        None
    }
}

/// Estado da câmera virtual da Prévia de Vídeo em modo 360° equiretangular.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Preview360State {
    pub enabled: bool,
    /// Rotação horizontal em graus (-180..=180).
    pub yaw_deg: f32,
    /// Inclinação vertical em graus (-85..=85).
    pub pitch_deg: f32,
    /// Campo de visão horizontal em graus (30..=130).
    pub fov_deg: f32,
}

impl Default for Preview360State {
    fn default() -> Self {
        Self {
            enabled: false,
            yaw_deg: 0.0,
            pitch_deg: 0.0,
            fov_deg: 90.0,
        }
    }
}

impl Preview360State {
    pub fn reset_view(&mut self) {
        self.yaw_deg = 0.0;
        self.pitch_deg = 0.0;
        self.fov_deg = 90.0;
    }

    pub fn orbit(&mut self, dx_deg: f32, dy_deg: f32) {
        if dx_deg.is_finite() {
            self.yaw_deg = (self.yaw_deg + dx_deg).rem_euclid(360.0);
            if self.yaw_deg > 180.0 {
                self.yaw_deg -= 360.0;
            }
        }
        if dy_deg.is_finite() {
            self.pitch_deg = (self.pitch_deg + dy_deg).clamp(-85.0, 85.0);
        }
    }

    pub fn adjust_fov(&mut self, delta_deg: f32) {
        if delta_deg.is_finite() {
            self.fov_deg = (self.fov_deg + delta_deg).clamp(30.0, 130.0);
        }
    }
}

/// Projeta um quadro equiretangular 360° (`src`, `w × h` RGBA) numa visão
/// retilínea segundo `state` (yaw/pitch/FOV). Devolve o quadro original sem
/// pânico quando dimensões ou parâmetros forem inválidos (§0.4).
pub fn project_equirectangular_360(
    src: &[u8],
    w: usize,
    h: usize,
    state: Preview360State,
) -> Vec<u8> {
    let Some(expected) = w.checked_mul(h).and_then(|n| n.checked_mul(4)) else {
        return Vec::new();
    };
    if w == 0 || h == 0 || src.len() < expected || expected > 64 * 1024 * 1024 {
        return src.get(..expected.min(src.len())).unwrap_or(&[]).to_vec();
    }
    let yaw = if state.yaw_deg.is_finite() {
        state.yaw_deg.to_radians()
    } else {
        0.0
    };
    let pitch = if state.pitch_deg.is_finite() {
        state.pitch_deg.clamp(-85.0, 85.0).to_radians()
    } else {
        0.0
    };
    let fov = if state.fov_deg.is_finite() {
        state.fov_deg.clamp(30.0, 130.0).to_radians()
    } else {
        90.0_f32.to_radians()
    };

    let (cy, sy) = (yaw.cos(), yaw.sin());
    let (cp, sp) = (pitch.cos(), pitch.sin());
    let half_w = (fov * 0.5).tan();
    let aspect = h as f32 / w as f32;
    let half_h = half_w * aspect;

    let mut out = vec![0u8; expected];
    let wf = w as f32;
    let hf = h as f32;
    let inv_tau = 1.0 / std::f32::consts::TAU;
    let inv_pi = 1.0 / std::f32::consts::PI;

    for y in 0..h {
        let ny = 1.0 - 2.0 * ((y as f32 + 0.5) / hf);
        let ry0 = ny * half_h;
        let row_off = y * w * 4;
        for x in 0..w {
            let nx = 2.0 * ((x as f32 + 0.5) / wf) - 1.0;
            let rx0 = nx * half_w;
            let rz0 = 1.0_f32;
            // Rotação pitch (em torno de X)
            let ry1 = ry0 * cp + rz0 * sp;
            let rz1 = -ry0 * sp + rz0 * cp;
            // Rotação yaw (em torno de Y)
            let dx = rx0 * cy + rz1 * sy;
            let dy = ry1;
            let dz = -rx0 * sy + rz1 * cy;
            let inv_len = (dx * dx + dy * dy + dz * dz).sqrt().recip();
            let ux = dx * inv_len;
            let uy = (dy * inv_len).clamp(-1.0, 1.0);
            let uz = dz * inv_len;

            let lon = ux.atan2(uz); // -pi..pi
            let lat = uy.asin(); // -pi/2..pi/2
            let u = (lon * inv_tau + 0.5).rem_euclid(1.0);
            let v = (0.5 - lat * inv_pi).clamp(0.0, 1.0);
            let sx = ((u * wf) as usize).min(w - 1);
            let sy_i = ((v * hf) as usize).min(h - 1);
            let si = (sy_i * w + sx) * 4;
            let di = row_off + x * 4;
            out[di..di + 4].copy_from_slice(&src[si..si + 4]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isolamento_de_canal_cores_cinza_alfa_e_buffer_torto() {
        let px = [200u8, 100, 50, 128];
        let mut b = px.to_vec();
        isolate_channel(&mut b, ChannelIsolation::Off, false);
        assert_eq!(b, px);
        let mut b = px.to_vec();
        isolate_channel(&mut b, ChannelIsolation::Red, false);
        assert_eq!(b, [200, 0, 0, 255]);
        let mut b = px.to_vec();
        isolate_channel(&mut b, ChannelIsolation::Green, true);
        assert_eq!(b, [100, 100, 100, 255]);
        let mut b = px.to_vec();
        isolate_channel(&mut b, ChannelIsolation::Blue, false);
        assert_eq!(b, [0, 0, 50, 255]);
        let mut b = px.to_vec();
        isolate_channel(&mut b, ChannelIsolation::Alpha, false);
        assert_eq!(b, [128, 128, 128, 255]);
        // tamanho que não é múltiplo de 4: sobra intacta, sem pânico
        let mut b = vec![1u8, 2, 3, 4, 9, 9];
        isolate_channel(&mut b, ChannelIsolation::Red, true);
        assert_eq!(b, [1, 1, 1, 255, 9, 9]);
        let mut vazio: Vec<u8> = Vec::new();
        isolate_channel(&mut vazio, ChannelIsolation::Alpha, true);
        assert!(vazio.is_empty());
    }

    #[test]
    fn regua_passos_redondos_limites_e_entrada_hostil() {
        // 1920 px do projeto em 960 px de tela, marcas a cada >= 40 px
        let t = ruler_ticks(10.0, 960.0, 1920, 40.0);
        assert!(!t.is_empty());
        assert_eq!(t[0].value, 0);
        assert!((t[0].pos - 10.0).abs() < 1e-3);
        assert!(t[0].major);
        let step = t[1].value;
        assert!(
            [1, 2, 5, 10, 20, 50, 100, 200, 500].contains(&step),
            "{step}"
        );
        assert!(step as f32 * 0.5 >= 40.0);
        assert!(t.last().is_some_and(|k| k.value <= 1920));
        // zoom extremo: limite de marcas
        let t = ruler_ticks(0.0, 1.0e9, 100_000, 1.0);
        assert!(t.len() <= MAX_RULER_TICKS);
        // entradas inválidas
        assert!(ruler_ticks(0.0, f32::NAN, 1920, 40.0).is_empty());
        assert!(ruler_ticks(f32::INFINITY, 100.0, 1920, 40.0).is_empty());
        assert!(ruler_ticks(0.0, 0.0, 1920, 40.0).is_empty());
        assert!(ruler_ticks(0.0, 100.0, 0, 40.0).is_empty());
        assert!(ruler_ticks(0.0, 100.0, 1920, 0.0).is_empty());
        assert!(ruler_ticks(0.0, 100.0, u32::MAX, 40.0).len() <= MAX_RULER_TICKS);
        // passo maior que o quadro: só a marca do zero (e talvez a do fim)
        let t = ruler_ticks(0.0, 10.0, 3, 500.0);
        assert!(!t.is_empty() && t.len() <= 2);
    }

    #[test]
    fn grade_divisoes_limitadas() {
        assert_eq!(grid_fractions(2), vec![0.5]);
        assert_eq!(grid_fractions(0), vec![0.5]);
        assert_eq!(grid_fractions(4).len(), 3);
        assert_eq!(grid_fractions(255).len(), 63);
        assert!(grid_fractions(8).iter().all(|f| *f > 0.0 && *f < 1.0));
    }

    #[test]
    fn fps_exibido_janela_de_um_segundo_e_relogio_que_volta() {
        let mut m = FpsMeter::default();
        assert_eq!(m.fps(), None);
        let t0 = Instant::now();
        m.push(t0);
        assert_eq!(m.fps(), None);
        for i in 1..=30u64 {
            m.push(t0 + Duration::from_millis(i * 40));
        }
        let fps = m.fps().expect("fps");
        assert!((fps - 25.0).abs() < 0.5, "{fps}");
        // dois carimbos iguais: intervalo nulo
        let mut z = FpsMeter::default();
        z.push(t0);
        z.push(t0);
        assert_eq!(z.fps(), None);
        // relógio voltou: recomeça sem pânico
        m.push(t0);
        assert_eq!(m.fps(), None);
        m.clear();
        assert_eq!(m.fps(), None);
    }

    #[test]
    fn legendas_fechadas_cc1_a_cc4_resolvem_regioes_e_comandos() {
        let mut p = neoveg_engine::model::Project::new_default();
        p.regions.push(neoveg_engine::model::Region {
            id: 1,
            start: neoveg_engine::model::Ticks::ZERO,
            end: neoveg_engine::model::Ticks::from_secs_i64(4),
            label: "CC2: Olá mundo".into(),
        });
        p.markers.push(neoveg_engine::model::Marker {
            id: 2,
            t: neoveg_engine::model::Ticks::from_secs_i64(1),
            label: "cmd".into(),
            kind: neoveg_engine::model::MarkerKind::Command,
            command_type: "CAPTION".into(),
            command_arg: "CC3: Legenda de comando".into(),
        });
        let t = neoveg_engine::model::Ticks::from_secs_i64(2);
        assert_eq!(
            resolve_closed_caption(&p, t, ClosedCaptionChannel::Off),
            None
        );
        assert_eq!(
            resolve_closed_caption(&p, t, ClosedCaptionChannel::Cc2).as_deref(),
            Some("Olá mundo")
        );
        assert_eq!(
            resolve_closed_caption(&p, t, ClosedCaptionChannel::Cc3).as_deref(),
            Some("Legenda de comando")
        );
        assert_eq!(
            resolve_closed_caption(&p, t, ClosedCaptionChannel::Cc4),
            None
        );
    }

    #[test]
    fn projecao_360_respeita_limites_e_nao_entra_em_panico_com_entradas_hostis() {
        let src = vec![200u8; 16 * 8 * 4];
        let mut st = Preview360State {
            enabled: true,
            yaw_deg: 45.0,
            pitch_deg: 20.0,
            fov_deg: 90.0,
        };
        let out = project_equirectangular_360(&src, 16, 8, st);
        assert_eq!(out.len(), src.len());
        assert_eq!(out[0], 200);
        // Entradas hostis (§0.4)
        st.orbit(f32::NAN, f32::INFINITY);
        st.adjust_fov(f32::NEG_INFINITY);
        assert!(project_equirectangular_360(&src, 0, 8, st).is_empty());
        assert!(project_equirectangular_360(&[], 16, 8, st).is_empty());
        let hostil = Preview360State {
            enabled: true,
            yaw_deg: f32::NAN,
            pitch_deg: f32::INFINITY,
            fov_deg: f32::NAN,
        };
        assert_eq!(
            project_equirectangular_360(&src, 16, 8, hostil).len(),
            src.len()
        );
    }
}
