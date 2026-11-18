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
}
