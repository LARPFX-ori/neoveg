//! Identificadores únicos e curvas/keyframes/fades usados por timeline,
//! envelopes, Pan/Crop e Track Motion.

use crate::time::Ticks;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

pub type Id = u64;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

pub fn new_id() -> Id {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// Garante que os próximos ids gerados sejam maiores que `max`.
pub fn bump_ids(max: Id) {
    NEXT_ID.fetch_max(max + 1, Ordering::Relaxed);
}

/// Consulta o próximo id sem alocá-lo (útil para invariantes atômicas, §3.1).
pub fn peek_next_id() -> Id {
    NEXT_ID.load(Ordering::Relaxed)
}

/// Tipos de curva de fade / segmento de envelope (Linear, Fast, Slow, Smooth,
/// Sharp e Hold, como no VEGAS Pro).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Curve {
    #[default]
    Linear,
    Fast,
    Slow,
    Smooth,
    Sharp,
    Hold,
}

impl Curve {
    pub const ALL: [Curve; 6] = [
        Curve::Linear,
        Curve::Fast,
        Curve::Slow,
        Curve::Smooth,
        Curve::Sharp,
        Curve::Hold,
    ];

    pub fn name_in(self, lang: crate::i18n::Language) -> &'static str {
        let pt = match self {
            Curve::Linear => "Linear",
            Curve::Fast => "Rápida",
            Curve::Slow => "Lenta",
            Curve::Smooth => "Suave",
            Curve::Sharp => "Abrupta",
            Curve::Hold => "Manter",
        };
        crate::i18n::tr(lang, pt)
    }

    pub fn name(self) -> &'static str {
        self.name_in(crate::i18n::Language::PtBr)
    }

    /// Mapeia `x` em `[0, 1]` para `y` em `[0, 1]` segundo a curva.
    pub fn shape(self, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        match self {
            Curve::Linear => x,
            Curve::Fast => 1.0 - (1.0 - x) * (1.0 - x) * (1.0 - x),
            Curve::Slow => x * x * x,
            Curve::Smooth => x * x * (3.0 - 2.0 * x),
            Curve::Sharp => {
                let s = x * x * (3.0 - 2.0 * x);
                2.0 * x - s
            }
            Curve::Hold => {
                if x >= 1.0 {
                    1.0
                } else {
                    0.0
                }
            }
        }
    }
}

pub trait Lerp: Clone {
    fn lerp(&self, other: &Self, t: f32) -> Self;
}

impl Lerp for f32 {
    fn lerp(&self, other: &Self, t: f32) -> Self {
        self + (other - self) * t
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Keyframe<T> {
    /// Tempo local em ticks relativo ao início do evento (ou da timeline,
    /// para track motion).
    pub t: Ticks,
    pub value: T,
    #[serde(default)]
    pub curve: Curve,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Keyframes<T> {
    pub keys: Vec<Keyframe<T>>,
}

impl<T: Lerp + Default> Default for Keyframes<T> {
    fn default() -> Self {
        Keyframes {
            keys: vec![Keyframe {
                t: Ticks::ZERO,
                value: T::default(),
                curve: Curve::Smooth,
            }],
        }
    }
}

impl<T: Lerp + Default> Keyframes<T> {
    pub fn eval(&self, t: Ticks) -> T {
        if self.keys.is_empty() {
            return T::default();
        }
        if t <= self.keys[0].t {
            return self.keys[0].value.clone();
        }
        for w in self.keys.windows(2) {
            let (a, b) = (&w[0], &w[1]);
            if t >= a.t && t < b.t {
                let x = Ticks::lerp_factor(t, a.t, b.t);
                return a.value.lerp(&b.value, a.curve.shape(x));
            }
        }
        self.keys
            .last()
            .map(|k| k.value.clone())
            .unwrap_or_default()
    }

    /// Insere ou substitui o keyframe no tempo `t` (tolerância de meio quadro).
    pub fn set(&mut self, t: Ticks, value: T, tol: Ticks) {
        if let Some(k) = self.keys.iter_mut().find(|k| k.t.abs_diff(t) <= tol) {
            k.value = value;
            return;
        }
        let curve = self.keys.first().map(|k| k.curve).unwrap_or(Curve::Smooth);
        self.keys.push(Keyframe { t, value, curve });
        self.keys.sort_by_key(|k| k.t);
    }

    pub fn is_identity(&self) -> bool
    where
        T: PartialEq,
    {
        self.keys.iter().all(|k| k.value == T::default())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Fade {
    pub len: Ticks,
    #[serde(default)]
    pub curve: Curve,
}
