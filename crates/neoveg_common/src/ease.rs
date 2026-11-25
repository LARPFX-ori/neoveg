//! Interpolação temporal Bézier dos quadros-chave (velocidade e influência).
//!
//! Complementa as curvas do VEGAS Pro (`Curve`) com o modelo de interpolação
//! temporal descrito na documentação pública de editores de vídeo (Bézier,
//! Bézier contínuo, Bézier automático, suavizar entrada/saída): cada
//! quadro-chave guarda, para o lado que chega (`incoming`) e para o lado que
//! sai (`outgoing`), uma **velocidade** e uma **influência**.
//!
//! - A velocidade é normalizada pelo trecho: `1.0` é a velocidade média do
//!   trecho (o mesmo que a interpolação linear), `0.0` para o movimento no
//!   quadro-chave e valores acima de `1.0` aceleram.
//! - A influência é a fração do trecho (`0.01..=1.0`) em que a alça age.
//!
//! O trecho entre dois quadros-chave vira uma curva de tempo cúbica de Bézier
//! com pontos de controle `(influência_saída, velocidade_saída ·
//! influência_saída)` e `(1 − influência_entrada, 1 − velocidade_entrada ·
//! influência_entrada)`: a inclinação no início do trecho é a velocidade de
//! saída e a do fim é a velocidade de entrada.
//!
//! O tempo continua exato (`Ticks`); só a fração dentro do trecho é `f32`,
//! como nas curvas existentes. Toda entrada vem de arquivo de projeto e é
//! tratada como hostil: valores não finitos viram o padrão e tudo é limitado.

use crate::time::Ticks;
use serde::{Deserialize, Serialize};

/// Influência padrão de uma alça (um terço do trecho).
pub const DEFAULT_INFLUENCE: f32 = 1.0 / 3.0;
/// Menor influência aceita (evita alça de comprimento zero).
pub const MIN_INFLUENCE: f32 = 0.01;
/// Maior velocidade normalizada aceita, nos dois sentidos.
pub const MAX_SPEED: f32 = 10.0;
/// Iterações da bissecção que inverte `x(s)`: erro abaixo de 1e-7.
const SOLVE_ITERS: usize = 24;

/// Tipo de interpolação temporal Bézier de um quadro-chave.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum EaseMode {
    /// Alças independentes: entrada e saída editadas separadamente.
    #[default]
    Bezier,
    /// Alças ligadas: a velocidade real é a mesma dos dois lados.
    Continuous,
    /// Alças automáticas, calculadas pelos vizinhos (curva suave sem ajuste).
    Auto,
}

impl EaseMode {
    pub const ALL: [EaseMode; 3] = [EaseMode::Bezier, EaseMode::Continuous, EaseMode::Auto];

    pub fn name_in(self, lang: crate::i18n::Language) -> &'static str {
        let pt = match self {
            EaseMode::Bezier => "Bézier",
            EaseMode::Continuous => "Bézier contínuo",
            EaseMode::Auto => "Bézier automático",
        };
        crate::i18n::tr(lang, pt)
    }
}

/// Uma alça de quadro-chave: velocidade normalizada e influência.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct EaseSide {
    pub speed: f32,
    pub influence: f32,
}

impl Default for EaseSide {
    fn default() -> Self {
        EaseSide::LINEAR
    }
}

impl EaseSide {
    /// Alça que reproduz a interpolação linear.
    pub const LINEAR: EaseSide = EaseSide {
        speed: 1.0,
        influence: DEFAULT_INFLUENCE,
    };
    /// Alça que para o movimento no quadro-chave (suavizar entrada/saída).
    pub const STOP: EaseSide = EaseSide {
        speed: 0.0,
        influence: DEFAULT_INFLUENCE,
    };

    /// Valores limitados e finitos (entrada hostil, AGENTS.md §0.4).
    pub fn sanitized(self) -> EaseSide {
        let speed = if self.speed.is_finite() {
            self.speed.clamp(-MAX_SPEED, MAX_SPEED)
        } else {
            1.0
        };
        let influence = if self.influence.is_finite() {
            self.influence.clamp(MIN_INFLUENCE, 1.0)
        } else {
            DEFAULT_INFLUENCE
        };
        EaseSide { speed, influence }
    }
}

/// Interpolação temporal Bézier de um quadro-chave.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct KeyEase {
    #[serde(default)]
    pub mode: EaseMode,
    /// Alça do lado que chega ao quadro-chave (fim do trecho anterior).
    #[serde(default)]
    pub incoming: EaseSide,
    /// Alça do lado que sai do quadro-chave (início do trecho seguinte).
    #[serde(default)]
    pub outgoing: EaseSide,
}

impl KeyEase {
    /// Bézier no modo pedido, começando com alças lineares.
    pub fn new(mode: EaseMode) -> KeyEase {
        KeyEase {
            mode,
            incoming: EaseSide::LINEAR,
            outgoing: EaseSide::LINEAR,
        }
    }

    /// Suavizar entrada: o movimento desacelera até parar no quadro-chave.
    /// Mantém a alça de saída que já existia.
    pub fn ease_in(prev: Option<KeyEase>) -> KeyEase {
        let base = prev.unwrap_or_else(|| KeyEase::new(EaseMode::Bezier));
        KeyEase {
            mode: EaseMode::Bezier,
            incoming: EaseSide::STOP,
            outgoing: base.outgoing,
        }
    }

    /// Suavizar saída: o movimento parte do repouso no quadro-chave.
    /// Mantém a alça de entrada que já existia.
    pub fn ease_out(prev: Option<KeyEase>) -> KeyEase {
        let base = prev.unwrap_or_else(|| KeyEase::new(EaseMode::Bezier));
        KeyEase {
            mode: EaseMode::Bezier,
            incoming: base.incoming,
            outgoing: EaseSide::STOP,
        }
    }

    /// Suavização dos dois lados (parar e partir do repouso).
    pub fn easy_ease() -> KeyEase {
        KeyEase {
            mode: EaseMode::Bezier,
            incoming: EaseSide::STOP,
            outgoing: EaseSide::STOP,
        }
    }

    pub fn sanitized(self) -> KeyEase {
        KeyEase {
            mode: self.mode,
            incoming: self.incoming.sanitized(),
            outgoing: self.outgoing.sanitized(),
        }
    }
}

/// Fração do valor (`0` no primeiro quadro-chave, `1` no segundo) na fração
/// de tempo `u ∈ [0, 1]` de um trecho com as alças `out` (saída do primeiro)
/// e `inn` (entrada do segundo). Pode passar de `[0, 1]` (ultrapassagem)
/// quando as velocidades são altas ou negativas.
pub fn bezier_progress(out: EaseSide, inn: EaseSide, u: f32) -> f32 {
    let out = out.sanitized();
    let inn = inn.sanitized();
    let u = if u.is_finite() {
        u.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let x1 = out.influence;
    let y1 = out.speed * out.influence;
    let x2 = 1.0 - inn.influence;
    let y2 = 1.0 - inn.speed * inn.influence;
    // x(s) é monótona com x1, x2 ∈ [0, 1]: a bissecção sempre converge.
    let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
    for _ in 0..SOLVE_ITERS {
        let mid = 0.5 * (lo + hi);
        if cubic(x1, x2, mid) < u {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    cubic(y1, y2, 0.5 * (lo + hi))
}

/// Bézier cúbica 1D com extremos 0 e 1 e pontos de controle `c1`, `c2`.
fn cubic(c1: f32, c2: f32, s: f32) -> f32 {
    let r = 1.0 - s;
    3.0 * r * r * s * c1 + 3.0 * r * s * s * c2 + s * s * s
}

/// Um quadro-chave visto pelo resolvedor de alças: tempo, valor escalar (se
/// a propriedade tiver um só número) e a interpolação Bézier, se houver.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EaseKey {
    pub t: Ticks,
    pub scalar: Option<f32>,
    pub ease: Option<KeyEase>,
}

/// Variação do trecho `i → i+1` usada para converter velocidades
/// normalizadas em reais: a diferença de valor nas propriedades escalares e
/// `1` nas compostas (posição, recorte, rotação...).
pub fn segment_delta(keys: &[EaseKey], i: usize) -> f32 {
    match (
        keys.get(i).and_then(|k| k.scalar),
        keys.get(i.saturating_add(1)).and_then(|k| k.scalar),
    ) {
        (Some(a), Some(b)) if a.is_finite() && b.is_finite() => b - a,
        _ => 1.0,
    }
}

/// Duração do trecho `i → i+1` em ticks (nunca menor que 1).
fn segment_ticks(keys: &[EaseKey], i: usize) -> f64 {
    match (keys.get(i), keys.get(i.saturating_add(1))) {
        (Some(a), Some(b)) => (b.t.0.saturating_sub(a.t.0)).max(1) as f64,
        _ => 1.0,
    }
}

/// Velocidade real média do trecho `i → i+1` (variação por tick).
fn segment_rate(keys: &[EaseKey], i: usize) -> f64 {
    f64::from(segment_delta(keys, i)) / segment_ticks(keys, i)
}

/// Alças efetivas do quadro-chave `k`: as gravadas ou, no modo automático,
/// as calculadas pelos vizinhos. `None` quando o quadro-chave não usa Bézier.
pub fn resolved_ease(keys: &[EaseKey], k: usize) -> Option<KeyEase> {
    let e = keys.get(k)?.ease?.sanitized();
    if e.mode != EaseMode::Auto {
        return Some(e);
    }
    let (inn, out) = auto_speeds(keys, k);
    Some(KeyEase {
        mode: EaseMode::Auto,
        incoming: EaseSide {
            speed: inn,
            influence: DEFAULT_INFLUENCE,
        },
        outgoing: EaseSide {
            speed: out,
            influence: DEFAULT_INFLUENCE,
        },
    })
}

/// Velocidades normalizadas `(entrada, saída)` do Bézier automático: a
/// inclinação real no quadro-chave é a média ponderada dos trechos vizinhos
/// (como uma spline de Catmull-Rom) e zera nos picos e vales de valores
/// escalares, para não ultrapassar. Nas pontas, a alça é linear.
pub fn auto_speeds(keys: &[EaseKey], k: usize) -> (f32, f32) {
    if k == 0 || k.saturating_add(1) >= keys.len() {
        return (1.0, 1.0);
    }
    let prev = k - 1;
    let d_in = segment_delta(keys, prev);
    let d_out = segment_delta(keys, k);
    let both_scalar = keys.get(k).and_then(|x| x.scalar).is_some();
    if both_scalar && (d_in == 0.0 || d_out == 0.0 || (d_in > 0.0) != (d_out > 0.0)) {
        return (0.0, 0.0);
    }
    let dt_in = segment_ticks(keys, prev);
    let dt_out = segment_ticks(keys, k);
    let slope = (f64::from(d_in) + f64::from(d_out)) / (dt_in + dt_out);
    let norm = |rate: f64| -> f32 {
        if rate.abs() < 1e-30 || !rate.is_finite() {
            1.0
        } else {
            ((slope / rate) as f32).clamp(-MAX_SPEED, MAX_SPEED)
        }
    };
    (norm(segment_rate(keys, prev)), norm(segment_rate(keys, k)))
}

/// Alças `(saída de i, entrada de i+1)` do trecho `i → i+1`, ou `None`
/// quando nenhum dos dois quadros-chave usa Bézier (vale a `Curve` do VEGAS).
/// Um lado sem Bézier conta como linear.
pub fn segment_sides(keys: &[EaseKey], i: usize) -> Option<(EaseSide, EaseSide)> {
    let a = resolved_ease(keys, i);
    let b = resolved_ease(keys, i.saturating_add(1));
    if a.is_none() && b.is_none() {
        return None;
    }
    Some((
        a.map(|e| e.outgoing).unwrap_or(EaseSide::LINEAR),
        b.map(|e| e.incoming).unwrap_or(EaseSide::LINEAR),
    ))
}

/// Ajusta o lado oposto de um quadro-chave Bézier contínuo para que a
/// velocidade real seja igual dos dois lados. `edited_out` diz qual lado o
/// usuário mexeu (o outro é recalculado). Outros modos voltam inalterados.
pub fn enforce_continuous(keys: &[EaseKey], k: usize, ease: KeyEase, edited_out: bool) -> KeyEase {
    let ease = ease.sanitized();
    if ease.mode != EaseMode::Continuous || k == 0 || k.saturating_add(1) >= keys.len() {
        return ease;
    }
    let rate_in = segment_rate(keys, k - 1);
    let rate_out = segment_rate(keys, k);
    if rate_in.abs() < 1e-30 || rate_out.abs() < 1e-30 {
        return ease;
    }
    let mut e = ease;
    if edited_out {
        let real = f64::from(e.outgoing.speed) * rate_out;
        e.incoming.speed = (real / rate_in) as f32;
    } else {
        let real = f64::from(e.incoming.speed) * rate_in;
        e.outgoing.speed = (real / rate_out) as f32;
    }
    e.sanitized()
}

/// Fração do valor no trecho `i → i+1` na fração de tempo `u`, usando as
/// alças Bézier quando houver e, senão, a forma da `curve` do VEGAS.
/// `Hold` sempre segura o valor (vale o do VEGAS e o do modelo Bézier).
pub fn segment_progress(keys: &[EaseKey], i: usize, curve: crate::curve::Curve, u: f32) -> f32 {
    if curve == crate::curve::Curve::Hold {
        return curve.shape(u);
    }
    match segment_sides(keys, i) {
        Some((out, inn)) => bezier_progress(out, inn, u),
        None => curve.shape(u),
    }
}

/// Velocidade real (variação por segundo) no trecho `i → i+1` na fração de
/// tempo `u`, para desenhar o gráfico de velocidade. Só para exibição.
pub fn segment_speed_per_sec(
    keys: &[EaseKey],
    i: usize,
    curve: crate::curve::Curve,
    u: f32,
) -> f32 {
    if curve == crate::curve::Curve::Hold {
        return 0.0;
    }
    let h = 1e-3_f32;
    let (u0, u1) = ((u - h).max(0.0), (u + h).min(1.0));
    if u1 <= u0 {
        return 0.0;
    }
    let dp = segment_progress(keys, i, curve, u1) - segment_progress(keys, i, curve, u0);
    let secs = segment_ticks(keys, i) / crate::time::TICKS_PER_SEC as f64;
    let v = f64::from(dp / (u1 - u0)) * f64::from(segment_delta(keys, i)) / secs.max(1e-12);
    if v.is_finite() {
        v as f32
    } else {
        0.0
    }
}

/// Converte uma velocidade real (variação por segundo) do lado de saída do
/// trecho `i` em velocidade normalizada. Usado ao arrastar uma alça.
pub fn normalize_speed(keys: &[EaseKey], i: usize, real_per_sec: f32) -> f32 {
    let secs = segment_ticks(keys, i) / crate::time::TICKS_PER_SEC as f64;
    let avg = f64::from(segment_delta(keys, i)) / secs.max(1e-12);
    if avg.abs() < 1e-12 || !real_per_sec.is_finite() {
        return 0.0;
    }
    ((f64::from(real_per_sec) / avg) as f32).clamp(-MAX_SPEED, MAX_SPEED)
}

/// Velocidade real média (variação por segundo) do trecho `i → i+1`.
pub fn segment_avg_per_sec(keys: &[EaseKey], i: usize) -> f32 {
    let secs = segment_ticks(keys, i) / crate::time::TICKS_PER_SEC as f64;
    let v = f64::from(segment_delta(keys, i)) / secs.max(1e-12);
    if v.is_finite() {
        v as f32
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::Curve;

    fn key(secs: i64, v: Option<f32>, ease: Option<KeyEase>) -> EaseKey {
        EaseKey {
            t: Ticks::from_secs_i64(secs),
            scalar: v,
            ease,
        }
    }

    #[test]
    fn alcas_lineares_reproduzem_interpolacao_linear() {
        for i in 0..=20 {
            let u = i as f32 / 20.0;
            let p = bezier_progress(EaseSide::LINEAR, EaseSide::LINEAR, u);
            assert!((p - u).abs() < 1e-4, "u={u} p={p}");
        }
    }

    #[test]
    fn suavizar_para_nas_pontas_e_respeita_extremos() {
        let e = KeyEase::easy_ease();
        let p = |u| bezier_progress(e.outgoing, e.incoming, u);
        assert!(p(0.0).abs() < 1e-5);
        assert!((p(1.0) - 1.0).abs() < 1e-5);
        assert!((p(0.5) - 0.5).abs() < 1e-3);
        // começa devagar: no primeiro décimo andou bem menos que 10 %
        assert!(p(0.1) < 0.05);
        assert!(p(0.9) > 0.95);
    }

    #[test]
    fn velocidade_de_saida_e_a_inclinacao_no_inicio() {
        let out = EaseSide {
            speed: 2.0,
            influence: 0.5,
        };
        let u = 1e-3;
        let slope = bezier_progress(out, EaseSide::LINEAR, u) / u;
        assert!((slope - 2.0).abs() < 0.05, "slope={slope}");
    }

    #[test]
    fn entrada_hostil_nao_trava_e_fica_limitada() {
        let bad = EaseSide {
            speed: f32::NAN,
            influence: f32::INFINITY,
        };
        let p = bezier_progress(bad, bad, f32::NAN);
        assert!(p.is_finite());
        let huge = EaseSide {
            speed: 1e30,
            influence: -5.0,
        };
        assert!(bezier_progress(huge, huge, 0.5).is_finite());
        assert_eq!(huge.sanitized().speed, MAX_SPEED);
        assert_eq!(huge.sanitized().influence, MIN_INFLUENCE);
        // índices fora do vetor não entram em pânico
        assert!(segment_sides(&[], 7).is_none());
        assert_eq!(auto_speeds(&[], 3), (1.0, 1.0));
        assert_eq!(segment_speed_per_sec(&[], 2, Curve::Linear, 0.5), 0.0);
    }

    #[test]
    fn auto_zera_em_picos_e_suaviza_em_rampas() {
        let auto = Some(KeyEase::new(EaseMode::Auto));
        let pico = [
            key(0, Some(0.0), auto),
            key(1, Some(10.0), auto),
            key(2, Some(0.0), auto),
        ];
        assert_eq!(auto_speeds(&pico, 1), (0.0, 0.0));
        let rampa = [
            key(0, Some(0.0), auto),
            key(1, Some(10.0), auto),
            key(3, Some(30.0), auto),
        ];
        let (i, o) = auto_speeds(&rampa, 1);
        // inclinação média = 30/3 = 10/s; trecho de entrada 10/s, de saída 10/s
        assert!((i - 1.0).abs() < 1e-4 && (o - 1.0).abs() < 1e-4, "{i} {o}");
    }

    #[test]
    fn continuo_iguala_a_velocidade_real_dos_dois_lados() {
        let c = Some(KeyEase::new(EaseMode::Continuous));
        // entrada: 0→10 em 1 s (10/s); saída: 10→30 em 4 s (5/s)
        let keys = [
            key(0, Some(0.0), c),
            key(1, Some(10.0), c),
            key(5, Some(30.0), c),
        ];
        let mut e = KeyEase::new(EaseMode::Continuous);
        e.outgoing.speed = 2.0; // 10/s reais na saída
        let r = enforce_continuous(&keys, 1, e, true);
        assert!((r.incoming.speed - 1.0).abs() < 1e-4, "{r:?}");
        // Bézier comum não liga os lados
        let mut b = KeyEase::new(EaseMode::Bezier);
        b.outgoing.speed = 2.0;
        assert_eq!(enforce_continuous(&keys, 1, b, true).incoming.speed, 1.0);
    }

    #[test]
    fn sem_bezier_vale_a_curva_do_vegas_e_hold_segura() {
        let keys = [key(0, Some(0.0), None), key(1, Some(1.0), None)];
        assert!(
            (segment_progress(&keys, 0, Curve::Smooth, 0.25) - Curve::Smooth.shape(0.25)).abs()
                < 1e-6
        );
        let eased = [
            key(0, Some(0.0), Some(KeyEase::easy_ease())),
            key(1, Some(1.0), None),
        ];
        assert_eq!(segment_progress(&eased, 0, Curve::Hold, 0.5), 0.0);
    }

    #[test]
    fn velocidade_por_segundo_do_trecho_linear() {
        let keys = [
            key(0, Some(0.0), Some(KeyEase::new(EaseMode::Bezier))),
            key(2, Some(10.0), None),
        ];
        let v = segment_speed_per_sec(&keys, 0, Curve::Linear, 0.5);
        assert!((v - 5.0).abs() < 0.01, "v={v}");
        assert!((normalize_speed(&keys, 0, 10.0) - 2.0).abs() < 1e-4);
        assert!((segment_avg_per_sec(&keys, 0) - 5.0).abs() < 1e-4);
    }

    #[test]
    fn serde_aceita_campos_ausentes() {
        let e: KeyEase = serde_json::from_str("{}").unwrap_or_default();
        assert_eq!(e, KeyEase::default());
        let e2: KeyEase = serde_json::from_str(r#"{"mode":"Auto"}"#).unwrap_or_default();
        assert_eq!(e2.mode, EaseMode::Auto);
        assert_eq!(e2.outgoing, EaseSide::LINEAR);
    }
}
