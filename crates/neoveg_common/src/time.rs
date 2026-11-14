//! Representação exata de tempo em ticks inteiros, taxas de quadros racionais
//! e formatação/interpretação de timecode SMPTE (incluindo Drop-Frame ST 12M).
//!
//! Regra Rust 5 do AGENTS.md: todo cálculo de timeline, régua SMPTE, quadros
//! e áudio usa inteiros exatos (ticks/amostras). Nunca usamos `f64` em segundos
//! na aritmética de edição; conversões para/de `f64` acontecem apenas na
//! fronteira de exibição, coordenadas em pixels da UI ou compatibilidade legada.

use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::iter::Sum;
use std::ops::{Add, AddAssign, Neg, Sub, SubAssign};

/// Unidades exatas por segundo (`1_411_200_000`).
///
/// Divisível exatamente por:
/// - todas as taxas de quadro inteiras comuns (1, 10, 12, 15, 20, 24, 25, 30,
///   48, 50, 60, 75, 100, 120, 144, 240 fps);
/// - multiplicado por `1001`, pelas taxas NTSC (`24000/1001`, `30000/1001`,
///   `60000/1001`, `120000/1001`);
/// - todas as taxas de áudio padrão de 8 kHz a 384 kHz (`8000`, `11025`,
///   `16000`, `22050`, `24000`, `32000`, `44100`, `48000`, `88200`, `96000`,
///   `176400`, `192000`, `352800`, `384000` Hz);
/// - milissegundos exatos (`1_411_200` ticks/ms).
pub const TICKS_PER_SEC: i64 = 1_411_200_000;

/// Maior duração aceita na timeline (48 horas em segundos).
pub const MAX_SECONDS: i64 = 48 * 3600;

/// Maior quantidade de ticks aceita em projetos (48 horas).
pub const MAX_TICKS: i64 = MAX_SECONDS * TICKS_PER_SEC;

/// Ticks por milissegundo exato.
pub const TICKS_PER_MS: i64 = TICKS_PER_SEC / 1000;

fn gcd_u64(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let r = a % b;
        a = b;
        b = r;
    }
    a.max(1)
}

fn gcd_u32(a: u32, b: u32) -> u32 {
    gcd_u64(a as u64, b as u64) as u32
}

fn clamp_i128_to_ticks(v: i128) -> Ticks {
    Ticks(v.clamp(-MAX_TICKS as i128, MAX_TICKS as i128) as i64)
}

fn div_round_i128(num: i128, den: i128) -> i128 {
    if den <= 0 {
        return 0;
    }
    if num >= 0 {
        (num + den / 2) / den
    } else {
        -(((-num) + den / 2) / den)
    }
}

/// Posição ou duração exata na timeline, em unidades de `1 / TICKS_PER_SEC` s.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ticks(pub i64);

impl Ticks {
    pub const ZERO: Ticks = Ticks(0);
    pub const MAX: Ticks = Ticks(MAX_TICKS);
    pub const MIN_NEGATIVE: Ticks = Ticks(-MAX_TICKS);

    #[inline]
    pub const fn from_raw(ticks: i64) -> Self {
        Ticks(ticks)
    }

    #[inline]
    pub fn from_secs_i64(s: i64) -> Self {
        let v = (s as i128).saturating_mul(TICKS_PER_SEC as i128);
        clamp_i128_to_ticks(v)
    }

    #[inline]
    pub fn from_millis_i64(ms: i64) -> Self {
        let v = (ms as i128).saturating_mul(TICKS_PER_MS as i128);
        clamp_i128_to_ticks(v)
    }

    /// Converte uma fração em milissegundos `(num / den)` para ticks.
    pub fn from_millis_ratio(num: u32, den: u32) -> Self {
        if den == 0 {
            return Ticks::ZERO;
        }
        let ticks = div_round_i128(num as i128 * TICKS_PER_MS as i128, den as i128);
        clamp_i128_to_ticks(ticks)
    }

    /// Converte segundos `f64` da fronteira de UI/arquivo legado para `Ticks`.
    /// Retorna `None` se `NaN`, infinito ou muito fora de escala.
    pub fn from_secs_f64(secs: f64) -> Option<Self> {
        if !secs.is_finite() {
            return None;
        }
        let lim = (MAX_SECONDS * 2) as f64;
        if secs.abs() > lim {
            return None;
        }
        let raw = (secs * TICKS_PER_SEC as f64).round();
        if !raw.is_finite() {
            return None;
        }
        Some(Ticks((raw as i64).clamp(-MAX_TICKS, MAX_TICKS)))
    }

    /// Versão saturante para entradas de UI ou JSON legado: `NaN` vira zero e
    /// valores extremos ficam dentro de `[-MAX_TICKS, MAX_TICKS]`.
    pub fn from_secs_f64_clamped(secs: f64) -> Self {
        if !secs.is_finite() {
            return Ticks::ZERO;
        }
        let s = secs.clamp(-(MAX_SECONDS as f64), MAX_SECONDS as f64);
        let raw = (s * TICKS_PER_SEC as f64).round() as i64;
        Ticks(raw.clamp(-MAX_TICKS, MAX_TICKS))
    }

    /// Converte para segundos em `f64` apenas para exibição, coordenadas de UI
    /// ou parâmetros de shaders/geradores.
    #[inline]
    pub fn as_secs_f64(self) -> f64 {
        self.0 as f64 / TICKS_PER_SEC as f64
    }

    /// Converte um índice/contagem de amostras na taxa `sample_rate` para ticks exatos.
    pub fn from_samples(samples: i64, sample_rate: u32) -> Self {
        if sample_rate == 0 {
            return Ticks::ZERO;
        }
        let n = samples as i128 * TICKS_PER_SEC as i128;
        let d = sample_rate as i128;
        clamp_i128_to_ticks(div_round_i128(n, d))
    }

    /// Converte ticks para índice de amostra (arredondado para baixo).
    pub fn to_samples_floor(self, sample_rate: u32) -> i64 {
        if sample_rate == 0 {
            return 0;
        }
        let q = (self.0 as i128 * sample_rate as i128).div_euclid(TICKS_PER_SEC as i128);
        q.clamp(i64::MIN as i128, i64::MAX as i128) as i64
    }

    /// Converte ticks para índice de amostra (arredondado ao mais próximo).
    pub fn to_samples_round(self, sample_rate: u32) -> i64 {
        if sample_rate == 0 {
            return 0;
        }
        let q = div_round_i128(self.0 as i128 * sample_rate as i128, TICKS_PER_SEC as i128);
        q.clamp(i64::MIN as i128, i64::MAX as i128) as i64
    }

    /// Quantidade inteira de amostra mais fração `[0, 1)` para reamostragem.
    pub fn to_sample_pos(self, sample_rate: u32) -> (i64, f32) {
        if sample_rate == 0 {
            return (0, 0.0);
        }
        let total = self.0 as i128 * sample_rate as i128;
        let q = total.div_euclid(TICKS_PER_SEC as i128);
        let r = total.rem_euclid(TICKS_PER_SEC as i128);
        (
            q.clamp(i64::MIN as i128, i64::MAX as i128) as i64,
            r as f32 / TICKS_PER_SEC as f32,
        )
    }

    #[inline]
    pub fn abs(self) -> Self {
        Ticks(self.0.saturating_abs())
    }

    #[inline]
    pub fn min(self, other: Self) -> Self {
        Ticks(self.0.min(other.0))
    }

    #[inline]
    pub fn max(self, other: Self) -> Self {
        Ticks(self.0.max(other.0))
    }

    /// Clamp seguro mesmo se `lo > hi` (§0.4).
    #[inline]
    pub fn clamp_safe(self, a: Self, b: Self) -> Self {
        let (lo, hi) = if a.0 <= b.0 { (a.0, b.0) } else { (b.0, a.0) };
        Ticks(self.0.clamp(lo, hi))
    }

    #[inline]
    pub fn abs_diff(self, other: Self) -> Self {
        Ticks(self.0.saturating_sub(other.0).saturating_abs())
    }

    #[inline]
    pub fn saturating_mul(self, k: i64) -> Self {
        clamp_i128_to_ticks(self.0 as i128 * k as i128)
    }

    #[inline]
    pub fn div_i64(self, k: i64) -> Self {
        if k == 0 {
            Ticks::ZERO
        } else {
            Ticks(self.0 / k)
        }
    }

    pub fn rem_euclid(self, d: Self) -> Self {
        if d.0 <= 0 {
            Ticks::ZERO
        } else {
            Ticks(self.0.rem_euclid(d.0))
        }
    }

    /// Fração normalizada em `[0.0, 1.0]` dentro de um intervalo `[start, start + len)`.
    /// Segura contra `len <= 0`.
    pub fn fraction_in(self, start: Ticks, len: Ticks) -> f32 {
        if len.0 <= 0 {
            return if self >= start { 1.0 } else { 0.0 };
        }
        let d = self.0.saturating_sub(start.0);
        (d as f64 / len.0 as f64).clamp(0.0, 1.0) as f32
    }

    /// Fração normalizada em `[0.0, 1.0]` dentro do segmento `[a, b]`.
    pub fn lerp_factor(t: Ticks, a: Ticks, b: Ticks) -> f32 {
        let span = b.0.saturating_sub(a.0);
        if span <= 0 {
            return if t >= a { 1.0 } else { 0.0 };
        }
        let d = t.0.saturating_sub(a.0);
        (d as f64 / span as f64).clamp(0.0, 1.0) as f32
    }

    /// Quantos ticks cada amostra ocupa quando `TICKS_PER_SEC % sr == 0`.
    pub fn exact_step_for_sample_rate(sr: u32) -> Option<i64> {
        if sr > 0 && (TICKS_PER_SEC as u64).is_multiple_of(sr as u64) {
            Some(TICKS_PER_SEC / sr as i64)
        } else {
            None
        }
    }
}

impl Add for Ticks {
    type Output = Ticks;
    #[inline]
    fn add(self, rhs: Ticks) -> Ticks {
        Ticks(self.0.saturating_add(rhs.0))
    }
}

impl Sub for Ticks {
    type Output = Ticks;
    #[inline]
    fn sub(self, rhs: Ticks) -> Ticks {
        Ticks(self.0.saturating_sub(rhs.0))
    }
}

impl Neg for Ticks {
    type Output = Ticks;
    #[inline]
    fn neg(self) -> Ticks {
        Ticks(self.0.saturating_neg())
    }
}

impl AddAssign for Ticks {
    #[inline]
    fn add_assign(&mut self, rhs: Ticks) {
        self.0 = self.0.saturating_add(rhs.0);
    }
}

impl SubAssign for Ticks {
    #[inline]
    fn sub_assign(&mut self, rhs: Ticks) {
        self.0 = self.0.saturating_sub(rhs.0);
    }
}

impl Sum for Ticks {
    fn sum<I: Iterator<Item = Ticks>>(iter: I) -> Self {
        iter.fold(Ticks::ZERO, |a, b| a + b)
    }
}

#[derive(Serialize)]
struct TicksSer {
    ticks: i64,
}

impl Serialize for Ticks {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        TicksSer { ticks: self.0 }.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Ticks {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct TicksVisitor;
        impl<'de> Visitor<'de> for TicksVisitor {
            type Value = Ticks;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("objeto {\"ticks\": inteiro} ou duração numérica em segundos")
            }

            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Ticks, E> {
                Ok(Ticks::from_secs_i64(v))
            }

            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Ticks, E> {
                Ok(Ticks::from_secs_i64(v.min(i64::MAX as u64) as i64))
            }

            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Ticks, E> {
                Ok(Ticks::from_secs_f64_clamped(v))
            }

            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Ticks, M::Error> {
                let mut ticks: Option<i64> = None;
                while let Some(key) = map.next_key::<String>()? {
                    if key == "ticks" {
                        if ticks.is_some() {
                            return Err(de::Error::duplicate_field("ticks"));
                        }
                        ticks = Some(map.next_value()?);
                    } else {
                        let _: de::IgnoredAny = map.next_value()?;
                    }
                }
                let raw = ticks.ok_or_else(|| de::Error::missing_field("ticks"))?;
                Ok(Ticks(raw.clamp(-MAX_TICKS, MAX_TICKS)))
            }
        }
        deserializer.deserialize_any(TicksVisitor)
    }
}

// ---------------------------------------------------------------------------
// Taxa de quadros racional exata (FrameRate)
// ---------------------------------------------------------------------------

/// Taxa de quadros racional exata (`num / den` quadros por segundo).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FrameRate {
    pub num: u32,
    pub den: u32,
}

impl FrameRate {
    pub const FPS_23_976: FrameRate = FrameRate {
        num: 24_000,
        den: 1001,
    };
    pub const FPS_24: FrameRate = FrameRate { num: 24, den: 1 };
    pub const FPS_25: FrameRate = FrameRate { num: 25, den: 1 };
    pub const FPS_29_97: FrameRate = FrameRate {
        num: 30_000,
        den: 1001,
    };
    pub const FPS_30: FrameRate = FrameRate { num: 30, den: 1 };
    pub const FPS_50: FrameRate = FrameRate { num: 50, den: 1 };
    pub const FPS_59_94: FrameRate = FrameRate {
        num: 60_000,
        den: 1001,
    };
    pub const FPS_60: FrameRate = FrameRate { num: 60, den: 1 };

    pub const STANDARD: [FrameRate; 8] = [
        Self::FPS_23_976,
        Self::FPS_24,
        Self::FPS_25,
        Self::FPS_29_97,
        Self::FPS_30,
        Self::FPS_50,
        Self::FPS_59_94,
        Self::FPS_60,
    ];

    /// Constrói uma fração reduzida válida dentro da faixa `[0.1, 1000.0]` qps.
    pub fn new(num: u32, den: u32) -> Option<Self> {
        if num == 0 || den == 0 {
            return None;
        }
        let g = gcd_u32(num, den);
        let r = FrameRate {
            num: num / g,
            den: den / g,
        };
        let f = r.as_f64();
        if (0.1..=1000.0).contains(&f) {
            Some(r)
        } else {
            None
        }
    }

    /// Converte um valor `f64` (ex.: vindo da UI ou de JSON legado `.nvp`),
    /// reconhecendo automaticamente taxas NTSC (`23.976`, `29.97`, `59.94`).
    pub fn from_f64(fps: f64) -> Option<Self> {
        if !fps.is_finite() || !(0.1..=1000.0).contains(&fps) {
            return None;
        }
        if (fps - 23.976_023_976).abs() < 0.01 {
            return Some(Self::FPS_23_976);
        }
        if (fps - 29.970_029_97).abs() < 0.01 {
            return Some(Self::FPS_29_97);
        }
        if (fps - 59.940_059_94).abs() < 0.01 {
            return Some(Self::FPS_59_94);
        }
        if (fps - 119.880_119_88).abs() < 0.02 {
            return Some(FrameRate {
                num: 120_000,
                den: 1001,
            });
        }
        let rounded = fps.round();
        if (fps - rounded).abs() < 1e-4 && rounded >= 1.0 {
            return Self::new(rounded as u32, 1);
        }
        let scaled = (fps * 1000.0).round();
        if scaled >= 1.0 && scaled <= (u32::MAX / 2) as f64 {
            Self::new(scaled as u32, 1000)
        } else {
            None
        }
    }

    #[inline]
    pub fn as_f64(self) -> f64 {
        self.num as f64 / self.den.max(1) as f64
    }

    /// Taxa nominal inteira associada ao timecode SMPTE (ex.: 29,97 -> 30).
    pub fn nominal_fps(self) -> u32 {
        if self.den == 1001 && self.num.is_multiple_of(1000) {
            (self.num / 1000).max(1)
        } else {
            self.as_f64().round().max(1.0) as u32
        }
    }

    /// Indica se a taxa suporta contagem SMPTE Drop-Frame (29,97 ou 59,94 NTSC).
    pub fn is_ntsc_drop_eligible(self) -> bool {
        self == Self::FPS_29_97 || self == Self::FPS_59_94
    }

    /// Duração aproximada (exata para todas as taxas padrão) de 1 quadro em ticks.
    pub fn ticks_per_frame(self) -> Ticks {
        let num = self.num.max(1) as i128;
        let den = self.den.max(1) as i128;
        let t = div_round_i128(TICKS_PER_SEC as i128 * den, num).max(1);
        clamp_i128_to_ticks(t)
    }

    /// Converte um índice de quadro exato para ticks (sem erro acumulado).
    pub fn frame_to_ticks(self, frame: i64) -> Ticks {
        let num = self.num.max(1) as i128;
        let den = self.den.max(1) as i128;
        let t = div_round_i128(frame as i128 * TICKS_PER_SEC as i128 * den, num);
        clamp_i128_to_ticks(t)
    }

    /// Converte ticks para o índice de quadro (arredondado para baixo).
    pub fn ticks_to_frame_floor(self, t: Ticks) -> i64 {
        let num = self.num.max(1) as i128;
        let den = self.den.max(1) as i128;
        let f = (t.0 as i128 * num).div_euclid(TICKS_PER_SEC as i128 * den);
        f.clamp(i64::MIN as i128, i64::MAX as i128) as i64
    }

    /// Converte ticks para o índice de quadro mais próximo.
    pub fn ticks_to_frame_round(self, t: Ticks) -> i64 {
        let num = self.num.max(1) as i128;
        let den = self.den.max(1) as i128;
        let f = div_round_i128(t.0 as i128 * num, TICKS_PER_SEC as i128 * den);
        f.clamp(i64::MIN as i128, i64::MAX as i128) as i64
    }

    /// Arredonda `t` para o limite exato de quadro mais próximo (Quantize to Frames).
    pub fn quantize(self, t: Ticks) -> Ticks {
        self.frame_to_ticks(self.ticks_to_frame_round(t))
    }

    pub fn label(self) -> String {
        match (self.num, self.den) {
            (24_000, 1001) => "23,976 qps (IVTC NTSC)".into(),
            (30_000, 1001) => "29,970 qps (NTSC)".into(),
            (60_000, 1001) => "59,940 qps (NTSC)".into(),
            (n, 1) => format!("{n} qps"),
            _ => format!("{:.3} qps", self.as_f64()),
        }
    }
}

impl Default for FrameRate {
    fn default() -> Self {
        Self::FPS_29_97
    }
}

#[derive(Serialize)]
struct FrameRateSer {
    num: u32,
    den: u32,
}

impl Serialize for FrameRate {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        FrameRateSer {
            num: self.num,
            den: self.den,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for FrameRate {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct FpsVisitor;
        impl<'de> Visitor<'de> for FpsVisitor {
            type Value = FrameRate;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("objeto {\"num\": u32, \"den\": u32} ou número em qps")
            }

            fn visit_i64<E: de::Error>(self, v: i64) -> Result<FrameRate, E> {
                Ok(FrameRate::from_f64(v as f64).unwrap_or(FrameRate::FPS_29_97))
            }

            fn visit_u64<E: de::Error>(self, v: u64) -> Result<FrameRate, E> {
                Ok(FrameRate::from_f64(v as f64).unwrap_or(FrameRate::FPS_29_97))
            }

            fn visit_f64<E: de::Error>(self, v: f64) -> Result<FrameRate, E> {
                Ok(FrameRate::from_f64(v).unwrap_or(FrameRate::FPS_29_97))
            }

            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<FrameRate, M::Error> {
                let mut num: Option<u32> = None;
                let mut den: Option<u32> = None;
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "num" => num = Some(map.next_value()?),
                        "den" => den = Some(map.next_value()?),
                        _ => {
                            let _: de::IgnoredAny = map.next_value()?;
                        }
                    }
                }
                let num = num.ok_or_else(|| de::Error::missing_field("num"))?;
                let den = den.ok_or_else(|| de::Error::missing_field("den"))?;
                Ok(FrameRate::new(num, den).unwrap_or(FrameRate::FPS_29_97))
            }
        }
        deserializer.deserialize_any(FpsVisitor)
    }
}

// ---------------------------------------------------------------------------
// Playback Rate racional exato (Rate)
// ---------------------------------------------------------------------------

const MAX_RATE_DEN: u64 = 1_000_000;

/// Velocidade de reprodução de um evento em fração racional (`num / den`).
/// `1/1` = 100%; `0/1` = quadro congelado; `1/2` = 50%; `2/1` = 200%.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rate {
    pub num: u64,
    pub den: u64,
}

impl Rate {
    pub const NORMAL: Rate = Rate { num: 1, den: 1 };
    pub const FROZEN: Rate = Rate { num: 0, den: 1 };

    /// Constrói uma razão reduzida. Se `den == 0` retorna `None`.
    pub fn from_ratio(num: u64, den: u64) -> Option<Self> {
        if den == 0 {
            return None;
        }
        if num == 0 {
            return Some(Self::FROZEN);
        }
        let g = gcd_u64(num, den);
        let (mut n, mut d) = (num / g, den / g);
        if d > MAX_RATE_DEN {
            let scaled = (n as u128 * MAX_RATE_DEN as u128) / d as u128;
            let g2 = gcd_u64(scaled.min(u64::MAX as u128) as u64, MAX_RATE_DEN);
            n = (scaled.min(u64::MAX as u128) as u64) / g2;
            d = MAX_RATE_DEN / g2;
        }
        Some(Rate { num: n, den: d })
    }

    pub fn from_f64(v: f64) -> Option<Self> {
        if !v.is_finite() || !(0.0..=100.0).contains(&v) {
            return None;
        }
        if v == 0.0 {
            return Some(Self::FROZEN);
        }
        let n = (v * MAX_RATE_DEN as f64)
            .round()
            .clamp(0.0, 100.0 * MAX_RATE_DEN as f64) as u64;
        Self::from_ratio(n, MAX_RATE_DEN)
    }

    #[inline]
    pub fn is_normal(self) -> bool {
        self.num == self.den
    }

    #[inline]
    pub fn as_f64(self) -> f64 {
        self.num as f64 / self.den.max(1) as f64
    }

    #[inline]
    pub fn percent_f64(self) -> f64 {
        self.as_f64() * 100.0
    }

    /// Escala uma duração em ticks pela velocidade: `dt * num / den`.
    pub fn scale_ticks(self, dt: Ticks) -> Ticks {
        if self.den == 0 || self.num == 0 {
            return Ticks::ZERO;
        }
        let n = dt.0 as i128 * self.num as i128;
        clamp_i128_to_ticks(div_round_i128(n, self.den as i128))
    }

    /// Operação inversa: `dt * den / num`. Retorna `None` se `rate == 0`.
    pub fn unscale_ticks(self, dt: Ticks) -> Option<Ticks> {
        if self.num == 0 || self.den == 0 {
            return None;
        }
        let n = dt.0 as i128 * self.den as i128;
        Some(clamp_i128_to_ticks(div_round_i128(n, self.num as i128)))
    }
}

impl Default for Rate {
    fn default() -> Self {
        Self::NORMAL
    }
}

#[derive(Serialize)]
struct RateSer {
    num: u64,
    den: u64,
}

impl Serialize for Rate {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        RateSer {
            num: self.num,
            den: self.den,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Rate {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct RateVisitor;
        impl<'de> Visitor<'de> for RateVisitor {
            type Value = Rate;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("objeto {\"num\": u64, \"den\": u64} ou número de velocidade")
            }

            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Rate, E> {
                Ok(Rate::from_f64(v as f64).unwrap_or(Rate::NORMAL))
            }

            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Rate, E> {
                Ok(Rate::from_f64(v as f64).unwrap_or(Rate::NORMAL))
            }

            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Rate, E> {
                Ok(Rate::from_f64(v).unwrap_or(Rate::NORMAL))
            }

            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Rate, M::Error> {
                let mut num: Option<u64> = None;
                let mut den: Option<u64> = None;
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "num" => num = Some(map.next_value()?),
                        "den" => den = Some(map.next_value()?),
                        _ => {
                            let _: de::IgnoredAny = map.next_value()?;
                        }
                    }
                }
                let num = num.ok_or_else(|| de::Error::missing_field("num"))?;
                let den = den.ok_or_else(|| de::Error::missing_field("den"))?;
                Ok(Rate::from_ratio(num, den).unwrap_or(Rate::NORMAL))
            }
        }
        deserializer.deserialize_any(RateVisitor)
    }
}

// ---------------------------------------------------------------------------
// Régua de tempo e SMPTE ST 12M (Drop-Frame e Non-Drop)
// ---------------------------------------------------------------------------

/// Formatos de exibição e entrada de tempo na régua (espelhando o VEGAS Pro).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TimeFormat {
    /// SMPTE Drop-Frame (`HH:MM:SS;FF` em 29,97/59,94 qps; Non-Drop nas demais taxas).
    #[default]
    #[serde(alias = "Timecode")]
    SmpteDrop,
    /// SMPTE Non-Drop (`HH:MM:SS:FF`).
    SmpteNonDrop,
    /// Tempo e quadros (`HH:MM:SS.FF`).
    TimeAndFrames,
    /// Horas, minutos, segundos e milissegundos (`HH:MM:SS.mmm`).
    #[serde(alias = "Seconds")]
    Time,
    /// Segundos decimais (`SS.mmm`).
    DecimalSeconds,
    /// Contagem absoluta de quadros (`0, 1, 2, ...`).
    Frames,
    /// Contagem absoluta de amostras na taxa do projeto.
    Samples,
}

impl TimeFormat {
    pub const ALL: [TimeFormat; 7] = [
        TimeFormat::SmpteDrop,
        TimeFormat::SmpteNonDrop,
        TimeFormat::TimeAndFrames,
        TimeFormat::Time,
        TimeFormat::DecimalSeconds,
        TimeFormat::Frames,
        TimeFormat::Samples,
    ];

    pub fn name(self) -> &'static str {
        match self {
            TimeFormat::SmpteDrop => "SMPTE Drop-Frame",
            TimeFormat::SmpteNonDrop => "SMPTE Non-Drop",
            TimeFormat::TimeAndFrames => "Tempo e quadros",
            TimeFormat::Time => "Tempo (HH:MM:SS.mmm)",
            TimeFormat::DecimalSeconds => "Segundos",
            TimeFormat::Frames => "Quadros absolutos",
            TimeFormat::Samples => "Amostras",
        }
    }
}

/// Decompõe um índice de quadro `frame_idx >= 0` em `(h, m, s, f)` no padrão
/// SMPTE ST 12M Drop-Frame (para 29,97 qps pula 2 números por minuto exceto
/// múltiplos de 10; para 59,94 qps pula 4 números por minuto exceto múltiplos de 10).
fn drop_frame_components(frame_idx: u64, nominal: u64, drop: u64) -> (u64, u64, u64, u64) {
    let frames_per_min = nominal * 60 - drop;
    let frames_per_10min = nominal * 600 - 9 * drop;
    let d = frame_idx / frames_per_10min;
    let m = frame_idx % frames_per_10min;
    let extra = if m >= drop {
        drop * ((m - drop) / frames_per_min)
    } else {
        0
    };
    let display = frame_idx + drop * 9 * d + extra;
    let f = display % nominal;
    let secs = display / nominal;
    (secs / 3600, (secs / 60) % 60, secs % 60, f)
}

/// Inverte `drop_frame_components` para obter o índice de quadro real.
fn drop_frame_to_index(h: u64, m: u64, s: u64, mut f: u64, nominal: u64, drop: u64) -> u64 {
    let total_mins = h.saturating_mul(60).saturating_add(m);
    if s == 0 && !total_mins.is_multiple_of(10) && f < drop {
        f = drop;
    }
    let dropped = drop.saturating_mul(total_mins - total_mins / 10);
    let display = total_mins
        .saturating_mul(60)
        .saturating_add(s)
        .saturating_mul(nominal)
        .saturating_add(f.min(nominal.saturating_sub(1)));
    display.saturating_sub(dropped)
}

/// Formata `t` (em ticks exatos) usando `fps`, `sample_rate` e `fmt`.
pub fn format_ticks(t: Ticks, fps: FrameRate, sample_rate: u32, fmt: TimeFormat) -> String {
    let neg = t.0 < 0;
    let abs = t.abs();
    let s = match fmt {
        TimeFormat::SmpteDrop => {
            let total_frames = fps.ticks_to_frame_round(abs).max(0) as u64;
            if fps == FrameRate::FPS_29_97 {
                let (h, m, sec, f) = drop_frame_components(total_frames, 30, 2);
                format!("{h:02}:{m:02}:{sec:02};{f:02}")
            } else if fps == FrameRate::FPS_59_94 {
                let (h, m, sec, f) = drop_frame_components(total_frames, 60, 4);
                format!("{h:02}:{m:02}:{sec:02};{f:02}")
            } else {
                let nom = fps.nominal_fps().max(1) as u64;
                let f = total_frames % nom;
                let secs = total_frames / nom;
                format!(
                    "{:02}:{:02}:{:02}:{:02}",
                    secs / 3600,
                    (secs / 60) % 60,
                    secs % 60,
                    f
                )
            }
        }
        TimeFormat::SmpteNonDrop => {
            let total_frames = fps.ticks_to_frame_round(abs).max(0) as u64;
            let nom = fps.nominal_fps().max(1) as u64;
            let f = total_frames % nom;
            let secs = total_frames / nom;
            format!(
                "{:02}:{:02}:{:02}:{:02}",
                secs / 3600,
                (secs / 60) % 60,
                secs % 60,
                f
            )
        }
        TimeFormat::TimeAndFrames => {
            let total_frames = fps.ticks_to_frame_round(abs).max(0) as u64;
            let nom = fps.nominal_fps().max(1) as u64;
            let f = total_frames % nom;
            let secs = total_frames / nom;
            format!(
                "{:02}:{:02}:{:02}.{:02}",
                secs / 3600,
                (secs / 60) % 60,
                secs % 60,
                f
            )
        }
        TimeFormat::Time => {
            let ms = div_round_i128(abs.0 as i128, TICKS_PER_MS as i128).max(0) as u64;
            format!(
                "{:02}:{:02}:{:02}.{:03}",
                ms / 3_600_000,
                (ms / 60_000) % 60,
                (ms / 1000) % 60,
                ms % 1000
            )
        }
        TimeFormat::DecimalSeconds => {
            let ms = div_round_i128(abs.0 as i128, TICKS_PER_MS as i128).max(0) as u64;
            format!("{}.{:03}", ms / 1000, ms % 1000)
        }
        TimeFormat::Frames => format!("{}", fps.ticks_to_frame_round(abs)),
        TimeFormat::Samples => format!("{}", abs.to_samples_round(sample_rate)),
    };
    if neg && abs.0 != 0 {
        format!("-{s}")
    } else {
        s
    }
}

/// Interpreta um tempo digitado em qualquer formato suportado (`HH:MM:SS;FF`,
/// `HH:MM:SS:FF`, `HH:MM:SS.mmm`, segundos decimais, `"Nf"` ou `"Nsm"`),
/// sempre sem divisão por zero nem conversão de float inválido.
pub fn parse_time(s: &str, fps: FrameRate, sample_rate: u32, fmt: TimeFormat) -> Option<Ticks> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Some(f) = s.strip_suffix('f').or_else(|| s.strip_suffix('F')) {
        let frames: i64 = f.trim().parse().ok()?;
        return Some(fps.frame_to_ticks(frames));
    }
    if let Some(smp) = s
        .strip_suffix("sm")
        .or_else(|| s.strip_suffix("sa"))
        .or_else(|| s.strip_suffix('a'))
    {
        let samples: i64 = smp.trim().parse().ok()?;
        return Some(Ticks::from_samples(samples, sample_rate));
    }
    if !s.contains(':') && !s.contains(';') {
        if fmt == TimeFormat::Frames && !s.contains('.') && !s.contains(',') {
            if let Ok(frames) = s.parse::<i64>() {
                return Some(fps.frame_to_ticks(frames));
            }
        }
        if fmt == TimeFormat::Samples && !s.contains('.') && !s.contains(',') {
            if let Ok(samples) = s.parse::<i64>() {
                return Some(Ticks::from_samples(samples, sample_rate));
            }
        }
        let secs: f64 = s.replace(',', ".").parse().ok()?;
        return Ticks::from_secs_f64(secs);
    }

    let (neg, body) = match s.strip_prefix('-') {
        Some(rest) => (true, rest.trim()),
        None => (false, s),
    };

    let res = if let Some((hms, ff_str)) = body.split_once(';') {
        // `;` indica explicitamente SMPTE Drop-Frame (quando aplicável)
        let f: u64 = ff_str.trim().parse().ok()?;
        let (h, m, sec) = parse_hms(hms)?;
        let idx = if fps == FrameRate::FPS_29_97 {
            drop_frame_to_index(h, m, sec, f, 30, 2)
        } else if fps == FrameRate::FPS_59_94 {
            drop_frame_to_index(h, m, sec, f, 60, 4)
        } else {
            let nom = fps.nominal_fps().max(1) as u64;
            h.saturating_mul(3600)
                .saturating_add(m.saturating_mul(60))
                .saturating_add(sec)
                .saturating_mul(nom)
                .saturating_add(f)
        };
        fps.frame_to_ticks(idx.min(i64::MAX as u64) as i64)
    } else {
        let parts: Vec<&str> = body.split(':').collect();
        if parts.len() == 4 && !parts[3].contains('.') && !parts[3].contains(',') {
            // HH:MM:SS:FF (SMPTE Non-Drop)
            let h: u64 = parts[0].trim().parse().ok()?;
            let m: u64 = parts[1].trim().parse().ok()?;
            let sec: u64 = parts[2].trim().parse().ok()?;
            let f: u64 = parts[3].trim().parse().ok()?;
            let nom = fps.nominal_fps().max(1) as u64;
            let idx = h
                .saturating_mul(3600)
                .saturating_add(m.saturating_mul(60))
                .saturating_add(sec)
                .saturating_mul(nom)
                .saturating_add(f);
            fps.frame_to_ticks(idx.min(i64::MAX as u64) as i64)
        } else if let Some((hms, frac)) = body.split_once('.').or_else(|| body.split_once(',')) {
            let (h, m, sec) = parse_hms(hms)?;
            let frac_trimmed = frac.trim();
            if frac_trimmed.is_empty() || !frac_trimmed.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            if fmt == TimeFormat::TimeAndFrames && frac_trimmed.len() <= 2 {
                let f: u64 = frac_trimmed.parse().ok()?;
                let nom = fps.nominal_fps().max(1) as u64;
                let idx = h
                    .saturating_mul(3600)
                    .saturating_add(m.saturating_mul(60))
                    .saturating_add(sec)
                    .saturating_mul(nom)
                    .saturating_add(f);
                fps.frame_to_ticks(idx.min(i64::MAX as u64) as i64)
            } else {
                let secs = h
                    .saturating_mul(3600)
                    .saturating_add(m.saturating_mul(60))
                    .saturating_add(sec);
                let base = Ticks::from_secs_i64(secs.min(MAX_SECONDS as u64) as i64);
                let ms_str = format!("{frac_trimmed:0<3}");
                let ms: i64 = ms_str.get(..3).unwrap_or("0").parse().ok()?;
                base + Ticks::from_millis_i64(ms)
            }
        } else {
            let (h, m, sec) = parse_hms(body)?;
            let secs = h
                .saturating_mul(3600)
                .saturating_add(m.saturating_mul(60))
                .saturating_add(sec);
            Ticks::from_secs_i64(secs.min(MAX_SECONDS as u64) as i64)
        }
    };
    Some(if neg { -res } else { res })
}

fn parse_hms(s: &str) -> Option<(u64, u64, u64)> {
    let parts: Vec<&str> = s.split(':').collect();
    match parts.as_slice() {
        [sec] => Some((0, 0, sec.trim().parse().ok()?)),
        [m, sec] => Some((0, m.trim().parse().ok()?, sec.trim().parse().ok()?)),
        [h, m, sec] => Some((
            h.trim().parse().ok()?,
            m.trim().parse().ok()?,
            sec.trim().parse().ok()?,
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_divisivel_por_taxas_padrao() {
        // Todos os formatos padrão de quadro e áudio devem dividir de modo exato.
        for fps_int in [24u32, 25, 30, 48, 50, 60, 120, 240] {
            let fps = FrameRate::new(fps_int, 1).unwrap();
            let tpf = fps.ticks_per_frame();
            assert_eq!(tpf.0 * fps_int as i64, TICKS_PER_SEC);
            // 10.000 quadros devem retornar exatos 10.000 no roundtrip.
            assert_eq!(fps.ticks_to_frame_round(fps.frame_to_ticks(10_000)), 10_000);
        }
        for ntsc in [
            FrameRate::FPS_23_976,
            FrameRate::FPS_29_97,
            FrameRate::FPS_59_94,
        ] {
            assert_eq!(
                (TICKS_PER_SEC as i128 * ntsc.den as i128) % ntsc.num as i128,
                0
            );
            assert_eq!(
                ntsc.ticks_to_frame_round(ntsc.frame_to_ticks(17_982)),
                17_982
            );
        }
        for sr in [
            8_000u32, 22_050, 44_100, 48_000, 88_200, 96_000, 192_000, 384_000,
        ] {
            assert!(Ticks::exact_step_for_sample_rate(sr).is_some());
            assert_eq!(
                Ticks::from_samples(sr as i64 * 123, sr).to_samples_round(sr),
                sr as i64 * 123
            );
        }
    }

    #[test]
    fn smpte_drop_frame_29_97_e_59_94() {
        let fps29 = FrameRate::FPS_29_97;
        // Frame 1799 -> 00:00:59;29; próximo quadro (1800) pula ;00 e ;01 -> 00:01:00;02
        assert_eq!(
            format_ticks(
                fps29.frame_to_ticks(1799),
                fps29,
                48000,
                TimeFormat::SmpteDrop
            ),
            "00:00:59;29"
        );
        assert_eq!(
            format_ticks(
                fps29.frame_to_ticks(1800),
                fps29,
                48000,
                TimeFormat::SmpteDrop
            ),
            "00:01:00;02"
        );
        // Frame 17981 -> 00:09:59;29; Frame 17982 (10 minutos) NÃO pula -> 00:10:00;00
        assert_eq!(
            format_ticks(
                fps29.frame_to_ticks(17_981),
                fps29,
                48000,
                TimeFormat::SmpteDrop
            ),
            "00:09:59;29"
        );
        assert_eq!(
            format_ticks(
                fps29.frame_to_ticks(17_982),
                fps29,
                48000,
                TimeFormat::SmpteDrop
            ),
            "00:10:00;00"
        );
        // 1 hora real exata (6 * 17982 = 107_892 quadros) -> 01:00:00;00
        assert_eq!(
            format_ticks(
                fps29.frame_to_ticks(107_892),
                fps29,
                48000,
                TimeFormat::SmpteDrop
            ),
            "01:00:00;00"
        );
        // Non-Drop no mesmo frame 1800 não pula -> 00:01:00:00
        assert_eq!(
            format_ticks(
                fps29.frame_to_ticks(1800),
                fps29,
                48000,
                TimeFormat::SmpteNonDrop
            ),
            "00:01:00:00"
        );

        // 59,94 Drop-Frame: pula 4 números (;00..;03), menos aos 10 minutos
        let fps59 = FrameRate::FPS_59_94;
        assert_eq!(
            format_ticks(
                fps59.frame_to_ticks(3599),
                fps59,
                48000,
                TimeFormat::SmpteDrop
            ),
            "00:00:59;59"
        );
        assert_eq!(
            format_ticks(
                fps59.frame_to_ticks(3600),
                fps59,
                48000,
                TimeFormat::SmpteDrop
            ),
            "00:01:00;04"
        );
        assert_eq!(
            format_ticks(
                fps59.frame_to_ticks(35_964),
                fps59,
                48000,
                TimeFormat::SmpteDrop
            ),
            "00:10:00;00"
        );
    }

    #[test]
    fn roundtrip_parse_time() {
        let fps = FrameRate::FPS_29_97;
        for f in [0i64, 1, 1799, 1800, 1801, 17_981, 17_982, 107_892] {
            let t = fps.frame_to_ticks(f);
            let s = format_ticks(t, fps, 48000, TimeFormat::SmpteDrop);
            let back = parse_time(&s, fps, 48000, TimeFormat::SmpteDrop).unwrap();
            assert_eq!(back, t, "falhou no roundtrip de {s}");
        }
        assert_eq!(
            parse_time(
                "01:02:03:15",
                FrameRate::FPS_30,
                48000,
                TimeFormat::SmpteNonDrop
            )
            .unwrap(),
            FrameRate::FPS_30.frame_to_ticks((3723 * 30) + 15)
        );
        assert_eq!(
            parse_time("90f", FrameRate::FPS_30, 48000, TimeFormat::Time).unwrap(),
            Ticks::from_secs_i64(3)
        );
        assert_eq!(
            parse_time("48000sm", FrameRate::FPS_30, 48000, TimeFormat::Samples).unwrap(),
            Ticks::from_secs_i64(1)
        );
    }

    #[test]
    fn entradas_hostis_parse_time_e_ticks() {
        assert_eq!(Ticks::from_secs_f64(f64::NAN), None);
        assert_eq!(Ticks::from_secs_f64(f64::INFINITY), None);
        assert_eq!(Ticks::from_secs_f64_clamped(f64::NAN), Ticks::ZERO);
        assert_eq!(Ticks::from_samples(10, 0), Ticks::ZERO);
        assert_eq!(Ticks::from_raw(5).to_samples_round(0), 0);
        assert_eq!(Ticks::from_raw(5).clamp_safe(Ticks(10), Ticks(2)), Ticks(5));
        for bad in [
            "",
            ":",
            ";;",
            "9999999999999999999999999:00:00;00",
            "abc",
            "-:;",
            "f",
            "sm",
        ] {
            let _ = parse_time(bad, FrameRate::FPS_29_97, 48000, TimeFormat::SmpteDrop);
        }
    }
}
