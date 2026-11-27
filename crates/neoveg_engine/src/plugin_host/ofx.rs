//! Host OpenFX em processo, a partir dos cabeçalhos públicos `ofxCore.h`,
//! `ofxProperty.h`, `ofxImageEffect.h`, `ofxParam.h`, `ofxMemory.h`,
//! `ofxMultiThread.h`, `ofxMessage.h`, `ofxTimeLine.h` e `ofxProgress.h`
//! (BSD-3-Clause), usados como especificação da API.
//!
//! **Só roda no subprocesso `neoveg --plugin-host serve-ofx`**: o plug-in é
//! código de terceiros e pode travar ou derrubar o processo.
//!
//! Escopo: contextos *General* (com `Source`, `Output` e clipes auxiliares
//! conectados sob demanda), *Filter*, *Generator* e *Transition* (`SourceFrom`,
//! `SourceTo`, `Output`), renderização por tiles
//! (`OfxImageEffectPropSupportsTiles = 1`), profundidade float ou byte,
//! parâmetros com keyframes (`getValueAtTime`, `getDerivative`, `getIntegral`,
//! chaves) e execução *multithread* sequencial.
//!
//! As funções variádicas de `ofxParam.h` (`paramGetValue`, `paramSetValue` e
//! afins) são implementadas como funções de aridade fixa. Isso depende da
//! convenção de chamada Win64 x86_64, em que argumentos variádicos usam os
//! mesmos registradores e posições de pilha que os fixos (valores `double`
//! vão também no registrador inteiro correspondente). Por isso este host só
//! é compilado em `windows` + `x86_64`.
#![allow(unsafe_code)]
// `Box<Vec<u8>>` é intencional: o `OfxImageMemoryHandle` precisa ser um
// ponteiro fino e estável, e a caixa fornece isso.
#![allow(clippy::vec_box)]

use super::{bounded_cstr, OfxParamInfo, OfxPluginRaw};
use crate::model::{OfxParamValue, Ticks, MAX_OFX_KEYS, TICKS_PER_SEC};
use std::collections::HashMap;
use std::ffi::{c_char, c_int, c_uint, c_void, CString};

type OfxStatus = c_int;
const STAT_OK: OfxStatus = 0;
const STAT_FAILED: OfxStatus = 1;
const STAT_ERR_UNKNOWN: OfxStatus = 3;
const STAT_ERR_UNSUPPORTED: OfxStatus = 5;
const STAT_ERR_EXISTS: OfxStatus = 6;
const STAT_ERR_MEMORY: OfxStatus = 8;
const STAT_ERR_BAD_HANDLE: OfxStatus = 9;
const STAT_ERR_BAD_INDEX: OfxStatus = 10;
const STAT_ERR_VALUE: OfxStatus = 11;
const STAT_REPLY_DEFAULT: OfxStatus = 14;

/// Maior alocação aceita de um plug-in (§0.4).
const MAX_ALLOC: usize = 1 << 31;
/// Maior número de valores numa propriedade.
const MAX_PROP_DIM: usize = 4096;
const MAX_PARAMS: usize = 4096;
const MAX_CLIPS: usize = 64;

// ---------------------------------------------------------------------------
// Conjuntos de propriedades
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
enum PropVal {
    Ptr(Vec<usize>),
    Str(Vec<CString>),
    Dbl(Vec<f64>),
    Int(Vec<c_int>),
}

/// `OfxPropertySetHandle` aponta para um `PropSet` encaixotado (endereço fixo).
#[derive(Clone, Debug, Default)]
pub(crate) struct PropSet {
    map: HashMap<String, PropVal>,
}

fn cstring(s: &str) -> CString {
    CString::new(s.replace('\0', "")).unwrap_or_default()
}

impl PropSet {
    fn set_str(&mut self, k: &str, v: &[&str]) {
        self.map.insert(
            k.into(),
            PropVal::Str(v.iter().map(|s| cstring(s)).collect()),
        );
    }
    fn set_int(&mut self, k: &str, v: &[c_int]) {
        self.map.insert(k.into(), PropVal::Int(v.to_vec()));
    }
    fn set_dbl(&mut self, k: &str, v: &[f64]) {
        self.map.insert(k.into(), PropVal::Dbl(v.to_vec()));
    }
    fn set_ptr(&mut self, k: &str, v: usize) {
        self.map.insert(k.into(), PropVal::Ptr(vec![v]));
    }
    fn strs(&self, k: &str) -> Vec<String> {
        match self.map.get(k) {
            Some(PropVal::Str(v)) => v.iter().map(|c| c.to_string_lossy().into_owned()).collect(),
            _ => vec![],
        }
    }
    fn str0(&self, k: &str) -> Option<String> {
        self.strs(k).into_iter().next()
    }
    fn nums(&self, k: &str) -> Vec<f64> {
        match self.map.get(k) {
            Some(PropVal::Dbl(v)) => v.clone(),
            Some(PropVal::Int(v)) => v.iter().map(|i| f64::from(*i)).collect(),
            _ => vec![],
        }
    }
}

/// # Safety
/// `p` nulo ou string C válida (lida com limite).
unsafe fn key(p: *const c_char) -> Option<String> {
    if p.is_null() {
        None
    } else {
        // SAFETY: pré-condição da função.
        Some(unsafe { bounded_cstr(p) })
    }
}

/// # Safety
/// `h` nulo ou ponteiro para `PropSet` vivo, criado por este host.
unsafe fn props<'a>(h: *mut PropSet) -> Option<&'a mut PropSet> {
    // SAFETY: pré-condição da função; o plug-in só recebe handles deste host.
    unsafe { h.as_mut() }
}

macro_rules! prop_set_one {
    ($fname:ident, $ty:ty, $variant:ident, $conv:expr, $default:expr) => {
        unsafe extern "C" fn $fname(
            h: *mut PropSet,
            name: *const c_char,
            idx: c_int,
            v: $ty,
        ) -> OfxStatus {
            // SAFETY: handle e nome vindos do plug-in conforme `ofxProperty.h`.
            let (Some(ps), Some(k)) = (unsafe { props(h) }, unsafe { key(name) }) else {
                return STAT_ERR_BAD_HANDLE;
            };
            let Ok(i) = usize::try_from(idx) else {
                return STAT_ERR_BAD_INDEX;
            };
            if i >= MAX_PROP_DIM {
                return STAT_ERR_BAD_INDEX;
            }
            let val = ($conv)(v);
            let entry = ps.map.entry(k).or_insert(PropVal::$variant(vec![]));
            if !matches!(entry, PropVal::$variant(_)) {
                *entry = PropVal::$variant(vec![]);
            }
            if let PropVal::$variant(vec) = entry {
                if vec.len() <= i {
                    vec.resize(i + 1, $default);
                }
                vec[i] = val;
            }
            STAT_OK
        }
    };
}

prop_set_one!(
    prop_set_pointer,
    *mut c_void,
    Ptr,
    |v: *mut c_void| v as usize,
    0usize
);
prop_set_one!(
    prop_set_string,
    *const c_char,
    Str,
    |v: *const c_char| cstring(&unsafe { key(v) }.unwrap_or_default()),
    CString::default()
);
prop_set_one!(prop_set_double, f64, Dbl, |v: f64| v, 0.0f64);
prop_set_one!(prop_set_int, c_int, Int, |v: c_int| v, 0);

/// # Safety
/// `v` aponta para `count` elementos legíveis.
unsafe fn slice<'a, T>(v: *const T, count: c_int) -> Option<&'a [T]> {
    let n = usize::try_from(count).ok().filter(|n| *n <= MAX_PROP_DIM)?;
    if n == 0 {
        return Some(&[]);
    }
    if v.is_null() {
        return None;
    }
    // SAFETY: pré-condição da função.
    Some(unsafe { std::slice::from_raw_parts(v, n) })
}

unsafe extern "C" fn prop_set_pointer_n(
    h: *mut PropSet,
    name: *const c_char,
    count: c_int,
    v: *const *mut c_void,
) -> OfxStatus {
    // SAFETY: argumentos conforme `ofxProperty.h`.
    let (Some(ps), Some(k), Some(s)) = (unsafe { props(h) }, unsafe { key(name) }, unsafe {
        slice(v, count)
    }) else {
        return STAT_ERR_BAD_HANDLE;
    };
    ps.map
        .insert(k, PropVal::Ptr(s.iter().map(|p| *p as usize).collect()));
    STAT_OK
}

unsafe extern "C" fn prop_set_string_n(
    h: *mut PropSet,
    name: *const c_char,
    count: c_int,
    v: *const *const c_char,
) -> OfxStatus {
    // SAFETY: argumentos conforme `ofxProperty.h`.
    let (Some(ps), Some(k), Some(s)) = (unsafe { props(h) }, unsafe { key(name) }, unsafe {
        slice(v, count)
    }) else {
        return STAT_ERR_BAD_HANDLE;
    };
    // SAFETY: cada item é uma string C do plug-in.
    let vals = s
        .iter()
        .map(|p| cstring(&unsafe { key(*p) }.unwrap_or_default()))
        .collect();
    ps.map.insert(k, PropVal::Str(vals));
    STAT_OK
}

unsafe extern "C" fn prop_set_double_n(
    h: *mut PropSet,
    name: *const c_char,
    count: c_int,
    v: *const f64,
) -> OfxStatus {
    // SAFETY: argumentos conforme `ofxProperty.h`.
    let (Some(ps), Some(k), Some(s)) = (unsafe { props(h) }, unsafe { key(name) }, unsafe {
        slice(v, count)
    }) else {
        return STAT_ERR_BAD_HANDLE;
    };
    ps.map.insert(k, PropVal::Dbl(s.to_vec()));
    STAT_OK
}

unsafe extern "C" fn prop_set_int_n(
    h: *mut PropSet,
    name: *const c_char,
    count: c_int,
    v: *const c_int,
) -> OfxStatus {
    // SAFETY: argumentos conforme `ofxProperty.h`.
    let (Some(ps), Some(k), Some(s)) = (unsafe { props(h) }, unsafe { key(name) }, unsafe {
        slice(v, count)
    }) else {
        return STAT_ERR_BAD_HANDLE;
    };
    ps.map.insert(k, PropVal::Int(s.to_vec()));
    STAT_OK
}

/// Lê o item `idx` de uma propriedade; devolve status OFX.
fn get_item<'a>(ps: &'a PropSet, k: &str, idx: c_int) -> Result<(&'a PropVal, usize), OfxStatus> {
    let v = ps.map.get(k).ok_or(STAT_ERR_UNKNOWN)?;
    let i = usize::try_from(idx).map_err(|_| STAT_ERR_BAD_INDEX)?;
    let len = match v {
        PropVal::Ptr(x) => x.len(),
        PropVal::Str(x) => x.len(),
        PropVal::Dbl(x) => x.len(),
        PropVal::Int(x) => x.len(),
    };
    if i >= len {
        return Err(STAT_ERR_BAD_INDEX);
    }
    Ok((v, i))
}

unsafe extern "C" fn prop_get_pointer(
    h: *mut PropSet,
    name: *const c_char,
    idx: c_int,
    out: *mut *mut c_void,
) -> OfxStatus {
    // SAFETY: argumentos conforme `ofxProperty.h`.
    let (Some(ps), Some(k)) = (unsafe { props(h) }, unsafe { key(name) }) else {
        return STAT_ERR_BAD_HANDLE;
    };
    if out.is_null() {
        return STAT_ERR_VALUE;
    }
    match get_item(ps, &k, idx) {
        Ok((PropVal::Ptr(v), i)) => {
            // SAFETY: `out` não nulo, fornecido pelo plug-in para receber o valor.
            unsafe { *out = v[i] as *mut c_void };
            STAT_OK
        }
        Ok(_) => STAT_ERR_VALUE,
        Err(e) => e,
    }
}

unsafe extern "C" fn prop_get_string(
    h: *mut PropSet,
    name: *const c_char,
    idx: c_int,
    out: *mut *const c_char,
) -> OfxStatus {
    // SAFETY: argumentos conforme `ofxProperty.h`.
    let (Some(ps), Some(k)) = (unsafe { props(h) }, unsafe { key(name) }) else {
        return STAT_ERR_BAD_HANDLE;
    };
    if out.is_null() {
        return STAT_ERR_VALUE;
    }
    match get_item(ps, &k, idx) {
        Ok((PropVal::Str(v), i)) => {
            // SAFETY: a string vive no conjunto até a propriedade mudar (contrato OFX).
            unsafe { *out = v[i].as_ptr() };
            STAT_OK
        }
        Ok(_) => STAT_ERR_VALUE,
        Err(e) => e,
    }
}

unsafe extern "C" fn prop_get_double(
    h: *mut PropSet,
    name: *const c_char,
    idx: c_int,
    out: *mut f64,
) -> OfxStatus {
    // SAFETY: argumentos conforme `ofxProperty.h`.
    let (Some(ps), Some(k)) = (unsafe { props(h) }, unsafe { key(name) }) else {
        return STAT_ERR_BAD_HANDLE;
    };
    if out.is_null() {
        return STAT_ERR_VALUE;
    }
    let v = match get_item(ps, &k, idx) {
        Ok((PropVal::Dbl(v), i)) => v[i],
        Ok((PropVal::Int(v), i)) => f64::from(v[i]),
        Ok(_) => return STAT_ERR_VALUE,
        Err(e) => return e,
    };
    // SAFETY: `out` não nulo.
    unsafe { *out = v };
    STAT_OK
}

unsafe extern "C" fn prop_get_int(
    h: *mut PropSet,
    name: *const c_char,
    idx: c_int,
    out: *mut c_int,
) -> OfxStatus {
    // SAFETY: argumentos conforme `ofxProperty.h`.
    let (Some(ps), Some(k)) = (unsafe { props(h) }, unsafe { key(name) }) else {
        return STAT_ERR_BAD_HANDLE;
    };
    if out.is_null() {
        return STAT_ERR_VALUE;
    }
    let v = match get_item(ps, &k, idx) {
        Ok((PropVal::Int(v), i)) => v[i],
        Ok((PropVal::Dbl(v), i)) if v[i].is_finite() => {
            v[i].round()
                .clamp(f64::from(c_int::MIN), f64::from(c_int::MAX)) as c_int
        }
        Ok(_) => return STAT_ERR_VALUE,
        Err(e) => return e,
    };
    // SAFETY: `out` não nulo.
    unsafe { *out = v };
    STAT_OK
}

macro_rules! prop_get_n {
    ($fname:ident, $ty:ty, $one:ident) => {
        unsafe extern "C" fn $fname(
            h: *mut PropSet,
            name: *const c_char,
            count: c_int,
            out: *mut $ty,
        ) -> OfxStatus {
            let Ok(n) = usize::try_from(count) else {
                return STAT_ERR_BAD_INDEX;
            };
            if out.is_null() || n > MAX_PROP_DIM {
                return STAT_ERR_VALUE;
            }
            for i in 0..n {
                // SAFETY: `out` tem `count` posições, conforme `ofxProperty.h`.
                let st = unsafe { $one(h, name, i as c_int, out.add(i)) };
                if st != STAT_OK {
                    return st;
                }
            }
            STAT_OK
        }
    };
}

prop_get_n!(prop_get_pointer_n, *mut c_void, prop_get_pointer);
prop_get_n!(prop_get_string_n, *const c_char, prop_get_string);
prop_get_n!(prop_get_double_n, f64, prop_get_double);
prop_get_n!(prop_get_int_n, c_int, prop_get_int);

unsafe extern "C" fn prop_reset(h: *mut PropSet, name: *const c_char) -> OfxStatus {
    // SAFETY: argumentos conforme `ofxProperty.h`.
    let (Some(ps), Some(k)) = (unsafe { props(h) }, unsafe { key(name) }) else {
        return STAT_ERR_BAD_HANDLE;
    };
    if ps.map.contains_key(&k) {
        STAT_OK
    } else {
        STAT_ERR_UNKNOWN
    }
}

unsafe extern "C" fn prop_get_dimension(
    h: *mut PropSet,
    name: *const c_char,
    out: *mut c_int,
) -> OfxStatus {
    // SAFETY: argumentos conforme `ofxProperty.h`.
    let (Some(ps), Some(k)) = (unsafe { props(h) }, unsafe { key(name) }) else {
        return STAT_ERR_BAD_HANDLE;
    };
    let Some(v) = ps.map.get(&k) else {
        return STAT_ERR_UNKNOWN;
    };
    let n = match v {
        PropVal::Ptr(x) => x.len(),
        PropVal::Str(x) => x.len(),
        PropVal::Dbl(x) => x.len(),
        PropVal::Int(x) => x.len(),
    };
    if out.is_null() {
        return STAT_ERR_VALUE;
    }
    // SAFETY: `out` não nulo.
    unsafe { *out = n as c_int };
    STAT_OK
}

#[repr(C)]
struct PropertySuiteV1 {
    set_pointer: unsafe extern "C" fn(*mut PropSet, *const c_char, c_int, *mut c_void) -> OfxStatus,
    set_string:
        unsafe extern "C" fn(*mut PropSet, *const c_char, c_int, *const c_char) -> OfxStatus,
    set_double: unsafe extern "C" fn(*mut PropSet, *const c_char, c_int, f64) -> OfxStatus,
    set_int: unsafe extern "C" fn(*mut PropSet, *const c_char, c_int, c_int) -> OfxStatus,
    set_pointer_n:
        unsafe extern "C" fn(*mut PropSet, *const c_char, c_int, *const *mut c_void) -> OfxStatus,
    set_string_n:
        unsafe extern "C" fn(*mut PropSet, *const c_char, c_int, *const *const c_char) -> OfxStatus,
    set_double_n: unsafe extern "C" fn(*mut PropSet, *const c_char, c_int, *const f64) -> OfxStatus,
    set_int_n: unsafe extern "C" fn(*mut PropSet, *const c_char, c_int, *const c_int) -> OfxStatus,
    get_pointer:
        unsafe extern "C" fn(*mut PropSet, *const c_char, c_int, *mut *mut c_void) -> OfxStatus,
    get_string:
        unsafe extern "C" fn(*mut PropSet, *const c_char, c_int, *mut *const c_char) -> OfxStatus,
    get_double: unsafe extern "C" fn(*mut PropSet, *const c_char, c_int, *mut f64) -> OfxStatus,
    get_int: unsafe extern "C" fn(*mut PropSet, *const c_char, c_int, *mut c_int) -> OfxStatus,
    get_pointer_n:
        unsafe extern "C" fn(*mut PropSet, *const c_char, c_int, *mut *mut c_void) -> OfxStatus,
    get_string_n:
        unsafe extern "C" fn(*mut PropSet, *const c_char, c_int, *mut *const c_char) -> OfxStatus,
    get_double_n: unsafe extern "C" fn(*mut PropSet, *const c_char, c_int, *mut f64) -> OfxStatus,
    get_int_n: unsafe extern "C" fn(*mut PropSet, *const c_char, c_int, *mut c_int) -> OfxStatus,
    reset: unsafe extern "C" fn(*mut PropSet, *const c_char) -> OfxStatus,
    get_dimension: unsafe extern "C" fn(*mut PropSet, *const c_char, *mut c_int) -> OfxStatus,
}

static PROPERTY_SUITE: PropertySuiteV1 = PropertySuiteV1 {
    set_pointer: prop_set_pointer,
    set_string: prop_set_string,
    set_double: prop_set_double,
    set_int: prop_set_int,
    set_pointer_n: prop_set_pointer_n,
    set_string_n: prop_set_string_n,
    set_double_n: prop_set_double_n,
    set_int_n: prop_set_int_n,
    get_pointer: prop_get_pointer,
    get_string: prop_get_string,
    get_double: prop_get_double,
    get_int: prop_get_int,
    get_pointer_n: prop_get_pointer_n,
    get_string_n: prop_get_string_n,
    get_double_n: prop_get_double_n,
    get_int_n: prop_get_int_n,
    reset: prop_reset,
    get_dimension: prop_get_dimension,
};

// ---------------------------------------------------------------------------
// Efeito, clipes e parâmetros
// ---------------------------------------------------------------------------

struct Param {
    kind: String,
    props: Box<PropSet>,
    nums: Vec<f64>,
    text: CString,
    /// Keyframes `(tempo_em_quadros, valores)`, ordenados por tempo.
    keys: Vec<(f64, Vec<f64>)>,
    integer: bool,
    hold: bool,
}

const KEY_TIME_EPS: f64 = 1.0e-6;

fn ticks_to_ofx_time(t: Ticks, fps: f64) -> f64 {
    if !fps.is_finite() || fps <= 0.0 {
        return 0.0;
    }
    (t.0 as f64 * fps) / (TICKS_PER_SEC as f64)
}

/// Quantos valores e se são inteiros, por tipo de parâmetro (`ofxParam.h`).
fn param_shape(kind: &str) -> (usize, bool) {
    match kind {
        "OfxParamTypeInteger" | "OfxParamTypeBoolean" | "OfxParamTypeChoice" => (1, true),
        "OfxParamTypeDouble" => (1, false),
        "OfxParamTypeInteger2D" => (2, true),
        "OfxParamTypeInteger3D" => (3, true),
        "OfxParamTypeDouble2D" => (2, false),
        "OfxParamTypeDouble3D" | "OfxParamTypeRGB" => (3, false),
        "OfxParamTypeRGBA" => (4, false),
        _ => (0, false),
    }
}

fn is_hold_kind(kind: &str) -> bool {
    matches!(kind, "OfxParamTypeBoolean" | "OfxParamTypeChoice")
}

impl Param {
    fn new_empty(kind: String, props: Box<PropSet>) -> Self {
        let (_, is_int) = param_shape(&kind);
        let hold = is_hold_kind(&kind);
        Self {
            kind,
            props,
            nums: vec![],
            text: CString::default(),
            keys: vec![],
            integer: is_int,
            hold,
        }
    }

    fn reset_to_default(&mut self) {
        let (n, is_int) = param_shape(&self.kind);
        let d = self.props.nums("OfxParamPropDefault");
        self.nums = (0..n).map(|i| d.get(i).copied().unwrap_or(0.0)).collect();
        self.keys.clear();
        self.integer = is_int;
        self.hold = is_hold_kind(&self.kind);
        if let Some(s) = self.props.str0("OfxParamPropDefault") {
            self.text = cstring(&s);
        }
    }

    fn eval_unrounded_at(&self, t: f64) -> Vec<f64> {
        let (n, _) = param_shape(&self.kind);
        if n == 0 {
            return vec![];
        }
        let fit = |vals: &[f64]| -> Vec<f64> {
            (0..n)
                .map(|i| {
                    vals.get(i)
                        .copied()
                        .filter(|v| v.is_finite())
                        .unwrap_or(0.0)
                })
                .collect()
        };
        let (Some(first), Some(last)) = (self.keys.first(), self.keys.last()) else {
            return fit(&self.nums);
        };
        if !t.is_finite() || t <= first.0 {
            return fit(&first.1);
        }
        if t >= last.0 {
            return fit(&last.1);
        }
        for w in self.keys.windows(2) {
            let (a, b) = (&w[0], &w[1]);
            if t >= a.0 && t < b.0 {
                if self.hold || (b.0 - a.0).abs() <= KEY_TIME_EPS {
                    return fit(&a.1);
                }
                let x = ((t - a.0) / (b.0 - a.0)).clamp(0.0, 1.0);
                return (0..n)
                    .map(|i| {
                        let va = a.1.get(i).copied().filter(|v| v.is_finite()).unwrap_or(0.0);
                        let vb = b.1.get(i).copied().filter(|v| v.is_finite()).unwrap_or(va);
                        va + (vb - va) * x
                    })
                    .collect();
            }
        }
        fit(&last.1)
    }

    fn eval_at(&self, t: f64) -> Vec<f64> {
        let mut out = self.eval_unrounded_at(t);
        if self.integer {
            for v in &mut out {
                *v = v.round();
            }
        }
        out
    }

    fn derivative_at(&self, t: f64) -> Vec<f64> {
        let (n, _) = param_shape(&self.kind);
        if n == 0 || !t.is_finite() || self.keys.len() < 2 || self.hold || self.integer {
            return vec![0.0; n];
        }
        let (Some(first), Some(last)) = (self.keys.first(), self.keys.last()) else {
            return vec![0.0; n];
        };
        if t < first.0 || t > last.0 {
            return vec![0.0; n];
        }
        for w in self.keys.windows(2) {
            let (a, b) = (&w[0], &w[1]);
            if t >= a.0 && t <= b.0 {
                let dt = b.0 - a.0;
                if dt <= KEY_TIME_EPS {
                    return vec![0.0; n];
                }
                return (0..n)
                    .map(|i| {
                        let va = a.1.get(i).copied().filter(|v| v.is_finite()).unwrap_or(0.0);
                        let vb = b.1.get(i).copied().filter(|v| v.is_finite()).unwrap_or(va);
                        (vb - va) / dt
                    })
                    .collect();
            }
        }
        vec![0.0; n]
    }

    fn integral(&self, t1: f64, t2: f64) -> Vec<f64> {
        let (n, _) = param_shape(&self.kind);
        if n == 0 || !t1.is_finite() || !t2.is_finite() || (t2 - t1).abs() <= f64::EPSILON {
            return vec![0.0; n];
        }
        if self.keys.is_empty() {
            let dt = t2 - t1;
            return (0..n)
                .map(|i| self.nums.get(i).copied().unwrap_or(0.0) * dt)
                .collect();
        }
        let (sign, a, b) = if t1 <= t2 {
            (1.0, t1, t2)
        } else {
            (-1.0, t2, t1)
        };
        let mut pts = Vec::with_capacity(self.keys.len() + 2);
        pts.push(a);
        for (kt, _) in &self.keys {
            if *kt > a + KEY_TIME_EPS && *kt < b - KEY_TIME_EPS {
                pts.push(*kt);
            }
        }
        pts.push(b);
        let mut acc = vec![0.0; n];
        for seg in pts.windows(2) {
            let (x0, x1) = (seg[0], seg[1]);
            let dt = x1 - x0;
            if dt <= 0.0 {
                continue;
            }
            if self.hold {
                let v = self.eval_unrounded_at(x0);
                for (dst, s) in acc.iter_mut().zip(v.iter()) {
                    *dst += *s * dt;
                }
            } else {
                let mid = 0.5 * (x0 + x1);
                // Dentro de [x0, x1] não há keyframe intermediário, então é linear.
                let va = self.eval_unrounded_at(x0);
                let vb = self.eval_unrounded_at(x1);
                let _ = mid;
                for i in 0..n {
                    acc[i] += 0.5 * (va[i] + vb[i]) * dt;
                }
            }
        }
        if sign < 0.0 {
            for dst in &mut acc {
                *dst = -*dst;
            }
        }
        acc
    }

    fn set_key(&mut self, t: f64, vals: Vec<f64>) {
        if !t.is_finite() {
            return;
        }
        if let Some(k) = self
            .keys
            .iter_mut()
            .find(|k| (k.0 - t).abs() <= KEY_TIME_EPS)
        {
            k.1 = vals;
        } else if self.keys.len() < MAX_OFX_KEYS {
            self.keys.push((t, vals));
            self.keys
                .sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        }
    }
}

struct ParamSet {
    props: Box<PropSet>,
    params: Vec<Box<Param>>,
}

struct Clip {
    name: String,
    props: Box<PropSet>,
    images: Vec<Box<PropSet>>,
    owner: *mut Effect,
}

/// Buffers do quadro em renderização (OFX: linhas de baixo para cima).
#[derive(Default)]
struct Buffers {
    w: i32,
    h: i32,
    float: bool,
    src: Vec<u8>,
    dst: Vec<u8>,
    /// Entradas auxiliares do contexto *General* (ex.: `Mask`, `Background`), por nome do clipe.
    aux_srcs: HashMap<String, Vec<u8>>,
}

pub(crate) struct Effect {
    props: Box<PropSet>,
    params: Box<ParamSet>,
    clips: Vec<Box<Clip>>,
    bufs: Buffers,
}

impl Effect {
    fn new() -> Box<Self> {
        Box::new(Effect {
            props: Box::default(),
            params: Box::new(ParamSet {
                props: Box::default(),
                params: vec![],
            }),
            clips: vec![],
            bufs: Buffers::default(),
        })
    }

    /// Instância criada a partir do descritor (cópia de clipes e parâmetros).
    fn instance_of(desc: &Effect) -> Box<Self> {
        let mut e = Effect::new();
        e.props = desc.props.clone();
        e.params.props = desc.params.props.clone();
        for p in &desc.params.params {
            let mut np = Box::new(Param::new_empty(p.kind.clone(), p.props.clone()));
            np.reset_to_default();
            e.params.params.push(np);
        }
        let owner: *mut Effect = &mut *e;
        for c in &desc.clips {
            e.clips.push(Box::new(Clip {
                name: c.name.clone(),
                props: c.props.clone(),
                images: vec![],
                owner,
            }));
        }
        e
    }
}

unsafe extern "C" fn ie_get_property_set(e: *mut Effect, out: *mut *mut PropSet) -> OfxStatus {
    // SAFETY: `e` é um handle deste host; `out` recebe o ponteiro.
    let Some(e) = (unsafe { e.as_mut() }) else {
        return STAT_ERR_BAD_HANDLE;
    };
    if out.is_null() {
        return STAT_ERR_VALUE;
    }
    // SAFETY: `out` não nulo.
    unsafe { *out = &mut *e.props };
    STAT_OK
}

unsafe extern "C" fn ie_get_param_set(e: *mut Effect, out: *mut *mut ParamSet) -> OfxStatus {
    // SAFETY: handle deste host.
    let Some(e) = (unsafe { e.as_mut() }) else {
        return STAT_ERR_BAD_HANDLE;
    };
    if out.is_null() {
        return STAT_ERR_VALUE;
    }
    // SAFETY: `out` não nulo.
    unsafe { *out = &mut *e.params };
    STAT_OK
}

unsafe extern "C" fn ie_clip_define(
    e: *mut Effect,
    name: *const c_char,
    out: *mut *mut PropSet,
) -> OfxStatus {
    // SAFETY: handle e nome do plug-in.
    let (Some(eff), Some(n)) = (unsafe { e.as_mut() }, unsafe { key(name) }) else {
        return STAT_ERR_BAD_HANDLE;
    };
    if eff.clips.iter().any(|c| c.name == n) {
        return STAT_ERR_EXISTS;
    }
    if eff.clips.len() >= MAX_CLIPS {
        return STAT_ERR_MEMORY;
    }
    let mut props = Box::<PropSet>::default();
    props.set_str("OfxPropName", &[&n]);
    eff.clips.push(Box::new(Clip {
        name: n,
        props,
        images: vec![],
        owner: e,
    }));
    if !out.is_null() {
        if let Some(c) = eff.clips.last_mut() {
            // SAFETY: `out` não nulo.
            unsafe { *out = &mut *c.props };
        }
    }
    STAT_OK
}

unsafe extern "C" fn ie_clip_get_handle(
    e: *mut Effect,
    name: *const c_char,
    clip: *mut *mut Clip,
    out: *mut *mut PropSet,
) -> OfxStatus {
    // SAFETY: handle e nome do plug-in.
    let (Some(eff), Some(n)) = (unsafe { e.as_mut() }, unsafe { key(name) }) else {
        return STAT_ERR_BAD_HANDLE;
    };
    let Some(c) = eff.clips.iter_mut().find(|c| c.name == n) else {
        return STAT_ERR_UNKNOWN;
    };
    if !clip.is_null() {
        // SAFETY: `clip` não nulo.
        unsafe { *clip = &mut **c };
    }
    if !out.is_null() {
        // SAFETY: `out` não nulo.
        unsafe { *out = &mut *c.props };
    }
    STAT_OK
}

unsafe extern "C" fn ie_clip_get_property_set(
    clip: *mut Clip,
    out: *mut *mut PropSet,
) -> OfxStatus {
    // SAFETY: handle deste host.
    let Some(c) = (unsafe { clip.as_mut() }) else {
        return STAT_ERR_BAD_HANDLE;
    };
    if out.is_null() {
        return STAT_ERR_VALUE;
    }
    // SAFETY: `out` não nulo.
    unsafe { *out = &mut *c.props };
    STAT_OK
}

unsafe extern "C" fn ie_clip_get_image(
    clip: *mut Clip,
    _time: f64,
    _region: *const c_void,
    out: *mut *mut PropSet,
) -> OfxStatus {
    // SAFETY: handle deste host; `owner` aponta para o efeito dono do clipe.
    let Some(c) = (unsafe { clip.as_mut() }) else {
        return STAT_ERR_BAD_HANDLE;
    };
    // SAFETY: o efeito vive enquanto seus clipes vivem.
    let Some(eff) = (unsafe { c.owner.as_mut() }) else {
        return STAT_ERR_BAD_HANDLE;
    };
    if out.is_null() {
        return STAT_ERR_VALUE;
    }
    let b = &mut eff.bufs;
    if b.w <= 0 || b.h <= 0 {
        return STAT_FAILED;
    }
    let data = match c.name.as_str() {
        "Output" => b.dst.as_mut_ptr(),
        "Source" | "SourceFrom" => {
            if b.src.is_empty() {
                return STAT_FAILED;
            }
            b.src.as_mut_ptr()
        }
        other => match b.aux_srcs.get_mut(other) {
            Some(buf) if !buf.is_empty() => buf.as_mut_ptr(),
            _ => return STAT_FAILED,
        },
    };
    let bpp: c_int = if b.float { 16 } else { 4 };
    let mut img = Box::<PropSet>::default();
    img.set_ptr("OfxImagePropData", data as usize);
    img.set_int("OfxImagePropBounds", &[0, 0, b.w, b.h]);
    img.set_int("OfxImagePropRegionOfDefinition", &[0, 0, b.w, b.h]);
    img.set_int("OfxImagePropRowBytes", &[b.w.saturating_mul(bpp)]);
    img.set_str(
        "OfxImageEffectPropPixelDepth",
        &[if b.float {
            "OfxBitDepthFloat"
        } else {
            "OfxBitDepthByte"
        }],
    );
    img.set_str("OfxImageEffectPropComponents", &["OfxImageComponentRGBA"]);
    img.set_str(
        "OfxImageEffectPropPreMultiplication",
        &["OfxImagePreMultiplied"],
    );
    img.set_dbl("OfxImagePropPixelAspectRatio", &[1.0]);
    img.set_dbl("OfxImageEffectPropRenderScale", &[1.0, 1.0]);
    img.set_str("OfxImagePropField", &["OfxImageFieldNone"]);
    img.set_str(
        "OfxImagePropUniqueIdentifier",
        &[&format!("neoveg-{}", c.name)],
    );
    c.images.push(img);
    if let Some(last) = c.images.last_mut() {
        // SAFETY: `out` não nulo; a caixa vive até o fim da renderização.
        unsafe { *out = &mut **last };
    }
    STAT_OK
}

unsafe extern "C" fn ie_clip_release_image(_img: *mut PropSet) -> OfxStatus {
    STAT_OK
}

#[repr(C)]
struct RectD {
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
}

unsafe extern "C" fn ie_clip_get_rod(clip: *mut Clip, _time: f64, out: *mut RectD) -> OfxStatus {
    // SAFETY: handle deste host.
    let Some(c) = (unsafe { clip.as_mut() }) else {
        return STAT_ERR_BAD_HANDLE;
    };
    // SAFETY: dono vivo enquanto o clipe vive.
    let Some(eff) = (unsafe { c.owner.as_ref() }) else {
        return STAT_ERR_BAD_HANDLE;
    };
    if out.is_null() {
        return STAT_ERR_VALUE;
    }
    // SAFETY: `out` não nulo.
    unsafe {
        *out = RectD {
            x1: 0.0,
            y1: 0.0,
            x2: f64::from(eff.bufs.w),
            y2: f64::from(eff.bufs.h),
        }
    };
    STAT_OK
}

unsafe extern "C" fn ie_abort(_e: *mut Effect) -> c_int {
    0
}

unsafe extern "C" fn ie_image_memory_alloc(
    _e: *mut Effect,
    n: usize,
    out: *mut *mut Vec<u8>,
) -> OfxStatus {
    if out.is_null() || n > MAX_ALLOC {
        return STAT_ERR_MEMORY;
    }
    let b = Box::new(vec![0u8; n]);
    // SAFETY: `out` não nulo.
    unsafe { *out = Box::into_raw(b) };
    STAT_OK
}

unsafe extern "C" fn ie_image_memory_free(h: *mut Vec<u8>) -> OfxStatus {
    if h.is_null() {
        return STAT_ERR_BAD_HANDLE;
    }
    // SAFETY: `h` veio de `ie_image_memory_alloc` (Box::into_raw).
    drop(unsafe { Box::from_raw(h) });
    STAT_OK
}

unsafe extern "C" fn ie_image_memory_lock(h: *mut Vec<u8>, out: *mut *mut c_void) -> OfxStatus {
    // SAFETY: `h` veio de `ie_image_memory_alloc`.
    let Some(v) = (unsafe { h.as_mut() }) else {
        return STAT_ERR_BAD_HANDLE;
    };
    if out.is_null() {
        return STAT_ERR_VALUE;
    }
    // SAFETY: `out` não nulo.
    unsafe { *out = v.as_mut_ptr().cast() };
    STAT_OK
}

unsafe extern "C" fn ie_image_memory_unlock(_h: *mut Vec<u8>) -> OfxStatus {
    STAT_OK
}

#[repr(C)]
struct ImageEffectSuiteV1 {
    get_property_set: unsafe extern "C" fn(*mut Effect, *mut *mut PropSet) -> OfxStatus,
    get_param_set: unsafe extern "C" fn(*mut Effect, *mut *mut ParamSet) -> OfxStatus,
    clip_define: unsafe extern "C" fn(*mut Effect, *const c_char, *mut *mut PropSet) -> OfxStatus,
    clip_get_handle: unsafe extern "C" fn(
        *mut Effect,
        *const c_char,
        *mut *mut Clip,
        *mut *mut PropSet,
    ) -> OfxStatus,
    clip_get_property_set: unsafe extern "C" fn(*mut Clip, *mut *mut PropSet) -> OfxStatus,
    clip_get_image:
        unsafe extern "C" fn(*mut Clip, f64, *const c_void, *mut *mut PropSet) -> OfxStatus,
    clip_release_image: unsafe extern "C" fn(*mut PropSet) -> OfxStatus,
    clip_get_region_of_definition: unsafe extern "C" fn(*mut Clip, f64, *mut RectD) -> OfxStatus,
    abort: unsafe extern "C" fn(*mut Effect) -> c_int,
    image_memory_alloc: unsafe extern "C" fn(*mut Effect, usize, *mut *mut Vec<u8>) -> OfxStatus,
    image_memory_free: unsafe extern "C" fn(*mut Vec<u8>) -> OfxStatus,
    image_memory_lock: unsafe extern "C" fn(*mut Vec<u8>, *mut *mut c_void) -> OfxStatus,
    image_memory_unlock: unsafe extern "C" fn(*mut Vec<u8>) -> OfxStatus,
}

static IMAGE_EFFECT_SUITE: ImageEffectSuiteV1 = ImageEffectSuiteV1 {
    get_property_set: ie_get_property_set,
    get_param_set: ie_get_param_set,
    clip_define: ie_clip_define,
    clip_get_handle: ie_clip_get_handle,
    clip_get_property_set: ie_clip_get_property_set,
    clip_get_image: ie_clip_get_image,
    clip_release_image: ie_clip_release_image,
    clip_get_region_of_definition: ie_clip_get_rod,
    abort: ie_abort,
    image_memory_alloc: ie_image_memory_alloc,
    image_memory_free: ie_image_memory_free,
    image_memory_lock: ie_image_memory_lock,
    image_memory_unlock: ie_image_memory_unlock,
};

// --- parâmetros -------------------------------------------------------------

unsafe extern "C" fn pa_define(
    ps: *mut ParamSet,
    kind: *const c_char,
    name: *const c_char,
    out: *mut *mut PropSet,
) -> OfxStatus {
    // SAFETY: handle e strings do plug-in.
    let (Some(set), Some(kind), Some(name)) =
        (unsafe { ps.as_mut() }, unsafe { key(kind) }, unsafe {
            key(name)
        })
    else {
        return STAT_ERR_BAD_HANDLE;
    };
    if set
        .params
        .iter()
        .any(|p| p.props.str0("OfxPropName").as_deref() == Some(name.as_str()))
    {
        return STAT_ERR_EXISTS;
    }
    if set.params.len() >= MAX_PARAMS {
        return STAT_ERR_MEMORY;
    }
    let mut props = Box::<PropSet>::default();
    props.set_str("OfxPropName", &[&name]);
    props.set_str("OfxParamPropType", &[&kind]);
    set.params.push(Box::new(Param::new_empty(kind, props)));
    if !out.is_null() {
        if let Some(p) = set.params.last_mut() {
            // SAFETY: `out` não nulo.
            unsafe { *out = &mut *p.props };
        }
    }
    STAT_OK
}

unsafe extern "C" fn pa_get_handle(
    ps: *mut ParamSet,
    name: *const c_char,
    param: *mut *mut Param,
    out: *mut *mut PropSet,
) -> OfxStatus {
    // SAFETY: handle e nome do plug-in.
    let (Some(set), Some(name)) = (unsafe { ps.as_mut() }, unsafe { key(name) }) else {
        return STAT_ERR_BAD_HANDLE;
    };
    let Some(p) = set
        .params
        .iter_mut()
        .find(|p| p.props.str0("OfxPropName").as_deref() == Some(name.as_str()))
    else {
        return STAT_ERR_UNKNOWN;
    };
    if !param.is_null() {
        // SAFETY: `param` não nulo.
        unsafe { *param = &mut **p };
    }
    if !out.is_null() {
        // SAFETY: `out` não nulo.
        unsafe { *out = &mut *p.props };
    }
    STAT_OK
}

unsafe extern "C" fn pa_set_get_property_set(
    ps: *mut ParamSet,
    out: *mut *mut PropSet,
) -> OfxStatus {
    // SAFETY: handle do host.
    let Some(set) = (unsafe { ps.as_mut() }) else {
        return STAT_ERR_BAD_HANDLE;
    };
    if out.is_null() {
        return STAT_ERR_VALUE;
    }
    // SAFETY: `out` não nulo.
    unsafe { *out = &mut *set.props };
    STAT_OK
}

unsafe extern "C" fn pa_get_property_set(p: *mut Param, out: *mut *mut PropSet) -> OfxStatus {
    // SAFETY: handle do host.
    let Some(p) = (unsafe { p.as_mut() }) else {
        return STAT_ERR_BAD_HANDLE;
    };
    if out.is_null() {
        return STAT_ERR_VALUE;
    }
    // SAFETY: `out` não nulo.
    unsafe { *out = &mut *p.props };
    STAT_OK
}

/// # Safety
/// Os ponteiros usados (conforme o tipo do parâmetro) apontam para variáveis
/// do tipo certo, como pede `ofxParam.h`.
unsafe fn write_vals(
    kind: &str,
    nums: &[f64],
    text: &CString,
    outs: [*mut c_void; 4],
) -> OfxStatus {
    if kind == "OfxParamTypeString" || kind == "OfxParamTypeCustom" {
        let o = outs[0].cast::<*const c_char>();
        if o.is_null() {
            return STAT_ERR_VALUE;
        }
        // SAFETY: pré-condição; a string vive no parâmetro.
        unsafe { *o = text.as_ptr() };
        return STAT_OK;
    }
    let (n, is_int) = param_shape(kind);
    for (i, &o) in outs.iter().enumerate().take(n) {
        let v = nums.get(i).copied().unwrap_or(0.0);
        if o.is_null() {
            return STAT_ERR_VALUE;
        }
        if is_int {
            // SAFETY: pré-condição (ponteiro para `int`).
            unsafe { *o.cast::<c_int>() = v.round() as c_int };
        } else {
            // SAFETY: pré-condição (ponteiro para `double`).
            unsafe { *o.cast::<f64>() = v };
        }
    }
    STAT_OK
}

/// Escreve o valor do parâmetro nos ponteiros de saída variádicos.
///
/// # Safety
/// Ver [`write_vals`].
unsafe fn write_value(p: &Param, outs: [*mut c_void; 4]) -> OfxStatus {
    // SAFETY: pré-condição encaminhada.
    unsafe { write_vals(&p.kind, &p.nums, &p.text, outs) }
}

unsafe extern "C" fn pa_get_value(
    p: *mut Param,
    a: *mut c_void,
    b: *mut c_void,
    c: *mut c_void,
    d: *mut c_void,
) -> OfxStatus {
    // SAFETY: handle do host; saídas conforme o tipo (ver doc do módulo sobre a ABI).
    match unsafe { p.as_ref() } {
        Some(p) => unsafe { write_value(p, [a, b, c, d]) },
        None => STAT_ERR_BAD_HANDLE,
    }
}

unsafe extern "C" fn pa_get_value_at_time(
    p: *mut Param,
    t: f64,
    a: *mut c_void,
    b: *mut c_void,
    c: *mut c_void,
    d: *mut c_void,
) -> OfxStatus {
    // SAFETY: handle do host; avalia keyframes no tempo `t` (ou `nums` sem keys).
    let Some(p) = (unsafe { p.as_ref() }) else {
        return STAT_ERR_BAD_HANDLE;
    };
    if !t.is_finite() {
        return STAT_ERR_VALUE;
    }
    let vals = p.eval_at(t);
    // SAFETY: saídas conforme o tipo.
    unsafe { write_vals(&p.kind, &vals, &p.text, [a, b, c, d]) }
}

unsafe extern "C" fn pa_get_derivative(
    p: *mut Param,
    t: f64,
    a: *mut c_void,
    b: *mut c_void,
    c: *mut c_void,
    d: *mut c_void,
) -> OfxStatus {
    // SAFETY: handle do host; derivada analítica ao longo dos keyframes.
    let Some(p) = (unsafe { p.as_ref() }) else {
        return STAT_ERR_BAD_HANDLE;
    };
    if !t.is_finite() {
        return STAT_ERR_VALUE;
    }
    let deriv = p.derivative_at(t);
    // SAFETY: saídas conforme o tipo.
    unsafe { write_vals(&p.kind, &deriv, &p.text, [a, b, c, d]) }
}

unsafe extern "C" fn pa_get_integral(
    p: *mut Param,
    t1: f64,
    t2: f64,
    a: *mut c_void,
    b: *mut c_void,
    c: *mut c_void,
    d: *mut c_void,
) -> OfxStatus {
    // SAFETY: handle do host; integral analítica entre `t1` e `t2`.
    let Some(p) = (unsafe { p.as_ref() }) else {
        return STAT_ERR_BAD_HANDLE;
    };
    if !t1.is_finite() || !t2.is_finite() {
        return STAT_ERR_VALUE;
    }
    let vals = p.integral(t1, t2);
    // SAFETY: saídas conforme o tipo.
    unsafe { write_vals(&p.kind, &vals, &p.text, [a, b, c, d]) }
}

/// Lê valores variádicos como palavras de 64 bits (Win64: `double` também
/// chega no registrador inteiro; `int` ocupa a palavra inteira).
///
/// # Safety
/// Para string, `a` deve ser `const char*` válido.
unsafe fn read_value(p: &mut Param, words: [usize; 4]) -> OfxStatus {
    if p.kind == "OfxParamTypeString" || p.kind == "OfxParamTypeCustom" {
        // SAFETY: pré-condição.
        p.text = cstring(&unsafe { key(words[0] as *const c_char) }.unwrap_or_default());
        return STAT_OK;
    }
    let (n, is_int) = param_shape(&p.kind);
    p.nums = (0..n)
        .map(|i| {
            let w = words[i] as u64;
            if is_int {
                f64::from(w as u32 as i32)
            } else {
                let v = f64::from_bits(w);
                if v.is_finite() {
                    v
                } else {
                    0.0
                }
            }
        })
        .collect();
    STAT_OK
}

unsafe extern "C" fn pa_set_value(
    p: *mut Param,
    a: usize,
    b: usize,
    c: usize,
    d: usize,
) -> OfxStatus {
    // SAFETY: handle do host; valores conforme o tipo (ver doc do módulo).
    match unsafe { p.as_mut() } {
        Some(p) => unsafe { read_value(p, [a, b, c, d]) },
        None => STAT_ERR_BAD_HANDLE,
    }
}

unsafe extern "C" fn pa_set_value_at_time(
    p: *mut Param,
    t: f64,
    a: usize,
    b: usize,
    c: usize,
    d: usize,
) -> OfxStatus {
    // SAFETY: handle do host; grava o valor e registra keyframe no tempo `t`.
    let Some(p) = (unsafe { p.as_mut() }) else {
        return STAT_ERR_BAD_HANDLE;
    };
    if !t.is_finite() {
        return STAT_ERR_VALUE;
    }
    // SAFETY: leitura conforme o tipo.
    let st = unsafe { read_value(p, [a, b, c, d]) };
    if st == STAT_OK {
        let vals = p.nums.clone();
        p.set_key(t, vals);
    }
    st
}

unsafe extern "C" fn pa_get_num_keys(p: *mut Param, out: *mut c_uint) -> OfxStatus {
    // SAFETY: handle deste host.
    let Some(p) = (unsafe { p.as_ref() }) else {
        return STAT_ERR_BAD_HANDLE;
    };
    if out.is_null() {
        return STAT_ERR_VALUE;
    }
    // SAFETY: `out` não nulo.
    unsafe { *out = p.keys.len() as c_uint };
    STAT_OK
}

unsafe extern "C" fn pa_get_key_time(p: *mut Param, n: c_uint, out: *mut f64) -> OfxStatus {
    // SAFETY: handle deste host.
    let Some(p) = (unsafe { p.as_ref() }) else {
        return STAT_ERR_BAD_HANDLE;
    };
    if out.is_null() {
        return STAT_ERR_VALUE;
    }
    let Some(&(kt, _)) = p.keys.get(n as usize) else {
        return STAT_ERR_BAD_INDEX;
    };
    // SAFETY: `out` não nulo.
    unsafe { *out = kt };
    STAT_OK
}

unsafe extern "C" fn pa_get_key_index(
    p: *mut Param,
    t: f64,
    dir: c_int,
    out: *mut c_int,
) -> OfxStatus {
    // SAFETY: handle deste host (`kOfxKeyBefore` < 0, `kOfxKeyAt` == 0, `kOfxKeyAfter` > 0).
    let Some(p) = (unsafe { p.as_ref() }) else {
        return STAT_ERR_BAD_HANDLE;
    };
    if out.is_null() || !t.is_finite() {
        return STAT_ERR_VALUE;
    }
    let found = if dir == 0 {
        p.keys
            .iter()
            .position(|(kt, _)| (*kt - t).abs() <= KEY_TIME_EPS)
    } else if dir < 0 {
        p.keys.iter().rposition(|(kt, _)| *kt < t - KEY_TIME_EPS)
    } else {
        p.keys.iter().position(|(kt, _)| *kt > t + KEY_TIME_EPS)
    };
    match found.and_then(|i| c_int::try_from(i).ok()) {
        Some(idx) => {
            // SAFETY: `out` não nulo.
            unsafe { *out = idx };
            STAT_OK
        }
        None => STAT_FAILED,
    }
}

unsafe extern "C" fn pa_delete_key(p: *mut Param, t: f64) -> OfxStatus {
    // SAFETY: handle deste host.
    let Some(p) = (unsafe { p.as_mut() }) else {
        return STAT_ERR_BAD_HANDLE;
    };
    if !t.is_finite() {
        return STAT_ERR_VALUE;
    }
    if let Some(pos) = p
        .keys
        .iter()
        .position(|(kt, _)| (*kt - t).abs() <= KEY_TIME_EPS)
    {
        p.keys.remove(pos);
        STAT_OK
    } else {
        STAT_ERR_BAD_INDEX
    }
}

unsafe extern "C" fn pa_delete_all_keys(p: *mut Param) -> OfxStatus {
    // SAFETY: handle deste host.
    let Some(p) = (unsafe { p.as_mut() }) else {
        return STAT_ERR_BAD_HANDLE;
    };
    p.keys.clear();
    STAT_OK
}

unsafe extern "C" fn pa_copy(
    _to: *mut Param,
    _from: *mut Param,
    _off: f64,
    _range: *const c_void,
) -> OfxStatus {
    STAT_ERR_UNSUPPORTED
}

unsafe extern "C" fn pa_edit_begin(_ps: *mut ParamSet, _name: *const c_char) -> OfxStatus {
    STAT_OK
}

unsafe extern "C" fn pa_edit_end(_ps: *mut ParamSet) -> OfxStatus {
    STAT_OK
}

type ValueGetFn = unsafe extern "C" fn(
    *mut Param,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
) -> OfxStatus;
type ValueGetTFn = unsafe extern "C" fn(
    *mut Param,
    f64,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
) -> OfxStatus;
type ValueIntegralFn = unsafe extern "C" fn(
    *mut Param,
    f64,
    f64,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
) -> OfxStatus;
type ValueSetFn = unsafe extern "C" fn(*mut Param, usize, usize, usize, usize) -> OfxStatus;
type ValueSetTFn = unsafe extern "C" fn(*mut Param, f64, usize, usize, usize, usize) -> OfxStatus;

#[repr(C)]
struct ParameterSuiteV1 {
    define: unsafe extern "C" fn(
        *mut ParamSet,
        *const c_char,
        *const c_char,
        *mut *mut PropSet,
    ) -> OfxStatus,
    get_handle: unsafe extern "C" fn(
        *mut ParamSet,
        *const c_char,
        *mut *mut Param,
        *mut *mut PropSet,
    ) -> OfxStatus,
    set_get_property_set: unsafe extern "C" fn(*mut ParamSet, *mut *mut PropSet) -> OfxStatus,
    get_property_set: unsafe extern "C" fn(*mut Param, *mut *mut PropSet) -> OfxStatus,
    get_value: ValueGetFn,
    get_value_at_time: ValueGetTFn,
    get_derivative: ValueGetTFn,
    get_integral: ValueIntegralFn,
    set_value: ValueSetFn,
    set_value_at_time: ValueSetTFn,
    get_num_keys: unsafe extern "C" fn(*mut Param, *mut c_uint) -> OfxStatus,
    get_key_time: unsafe extern "C" fn(*mut Param, c_uint, *mut f64) -> OfxStatus,
    get_key_index: unsafe extern "C" fn(*mut Param, f64, c_int, *mut c_int) -> OfxStatus,
    delete_key: unsafe extern "C" fn(*mut Param, f64) -> OfxStatus,
    delete_all_keys: unsafe extern "C" fn(*mut Param) -> OfxStatus,
    copy: unsafe extern "C" fn(*mut Param, *mut Param, f64, *const c_void) -> OfxStatus,
    edit_begin: unsafe extern "C" fn(*mut ParamSet, *const c_char) -> OfxStatus,
    edit_end: unsafe extern "C" fn(*mut ParamSet) -> OfxStatus,
}

static PARAMETER_SUITE: ParameterSuiteV1 = ParameterSuiteV1 {
    define: pa_define,
    get_handle: pa_get_handle,
    set_get_property_set: pa_set_get_property_set,
    get_property_set: pa_get_property_set,
    get_value: pa_get_value,
    get_value_at_time: pa_get_value_at_time,
    get_derivative: pa_get_derivative,
    get_integral: pa_get_integral,
    set_value: pa_set_value,
    set_value_at_time: pa_set_value_at_time,
    get_num_keys: pa_get_num_keys,
    get_key_time: pa_get_key_time,
    get_key_index: pa_get_key_index,
    delete_key: pa_delete_key,
    delete_all_keys: pa_delete_all_keys,
    copy: pa_copy,
    edit_begin: pa_edit_begin,
    edit_end: pa_edit_end,
};

// --- memória, threads, mensagens, linha do tempo, progresso -----------------

const MEM_HEADER: usize = 16;

unsafe extern "C" fn mem_alloc(_h: *mut c_void, n: usize, out: *mut *mut c_void) -> OfxStatus {
    if out.is_null() || n > MAX_ALLOC {
        return STAT_ERR_MEMORY;
    }
    let Ok(layout) = std::alloc::Layout::from_size_align(n + MEM_HEADER, 16) else {
        return STAT_ERR_MEMORY;
    };
    // SAFETY: tamanho não nulo (cabeçalho) e alinhamento válido.
    let p = unsafe { std::alloc::alloc_zeroed(layout) };
    if p.is_null() {
        return STAT_ERR_MEMORY;
    }
    // SAFETY: `p` tem pelo menos MEM_HEADER bytes alinhados a 16.
    unsafe {
        p.cast::<usize>().write(n);
        *out = p.add(MEM_HEADER).cast();
    }
    STAT_OK
}

unsafe extern "C" fn mem_free(p: *mut c_void) -> OfxStatus {
    if p.is_null() {
        return STAT_ERR_BAD_HANDLE;
    }
    // SAFETY: `p` veio de `mem_alloc`, que guarda o tamanho antes do bloco.
    unsafe {
        let base = p.cast::<u8>().sub(MEM_HEADER);
        let n = base.cast::<usize>().read();
        if let Ok(layout) = std::alloc::Layout::from_size_align(n + MEM_HEADER, 16) {
            std::alloc::dealloc(base, layout);
        }
    }
    STAT_OK
}

#[repr(C)]
struct MemorySuiteV1 {
    alloc: unsafe extern "C" fn(*mut c_void, usize, *mut *mut c_void) -> OfxStatus,
    free: unsafe extern "C" fn(*mut c_void) -> OfxStatus,
}

static MEMORY_SUITE: MemorySuiteV1 = MemorySuiteV1 {
    alloc: mem_alloc,
    free: mem_free,
};

type ThreadFn = unsafe extern "C" fn(c_uint, c_uint, *mut c_void);

unsafe extern "C" fn mt_multi_thread(
    f: Option<ThreadFn>,
    n: c_uint,
    arg: *mut c_void,
) -> OfxStatus {
    let Some(f) = f else {
        return STAT_ERR_VALUE;
    };
    // Execução sequencial: cada índice roda uma vez, na ordem (válido pelo contrato).
    let n = n.clamp(1, 256);
    for i in 0..n {
        // SAFETY: função e argumento do próprio plug-in.
        unsafe { f(i, n, arg) };
    }
    STAT_OK
}

unsafe extern "C" fn mt_num_cpus(out: *mut c_uint) -> OfxStatus {
    if !out.is_null() {
        // SAFETY: `out` não nulo.
        unsafe { *out = 1 };
    }
    STAT_OK
}

unsafe extern "C" fn mt_index(out: *mut c_uint) -> OfxStatus {
    if !out.is_null() {
        // SAFETY: `out` não nulo.
        unsafe { *out = 0 };
    }
    STAT_OK
}

unsafe extern "C" fn mt_is_spawned() -> c_int {
    0
}

unsafe extern "C" fn mt_mutex_create(out: *mut *mut c_void, _count: c_int) -> OfxStatus {
    if out.is_null() {
        return STAT_ERR_VALUE;
    }
    // Um único thread: o mutex é só um marcador.
    // SAFETY: `out` não nulo.
    unsafe { *out = Box::into_raw(Box::new(0u8)).cast() };
    STAT_OK
}

unsafe extern "C" fn mt_mutex_destroy(m: *mut c_void) -> OfxStatus {
    if !m.is_null() {
        // SAFETY: veio de `mt_mutex_create`.
        drop(unsafe { Box::from_raw(m.cast::<u8>()) });
    }
    STAT_OK
}

unsafe extern "C" fn mt_mutex_noop(_m: *mut c_void) -> OfxStatus {
    STAT_OK
}

#[repr(C)]
struct MultiThreadSuiteV1 {
    multi_thread: unsafe extern "C" fn(Option<ThreadFn>, c_uint, *mut c_void) -> OfxStatus,
    num_cpus: unsafe extern "C" fn(*mut c_uint) -> OfxStatus,
    index: unsafe extern "C" fn(*mut c_uint) -> OfxStatus,
    is_spawned: unsafe extern "C" fn() -> c_int,
    mutex_create: unsafe extern "C" fn(*mut *mut c_void, c_int) -> OfxStatus,
    mutex_destroy: unsafe extern "C" fn(*mut c_void) -> OfxStatus,
    mutex_lock: unsafe extern "C" fn(*mut c_void) -> OfxStatus,
    mutex_unlock: unsafe extern "C" fn(*mut c_void) -> OfxStatus,
    mutex_try_lock: unsafe extern "C" fn(*mut c_void) -> OfxStatus,
}

static MULTI_THREAD_SUITE: MultiThreadSuiteV1 = MultiThreadSuiteV1 {
    multi_thread: mt_multi_thread,
    num_cpus: mt_num_cpus,
    index: mt_index,
    is_spawned: mt_is_spawned,
    mutex_create: mt_mutex_create,
    mutex_destroy: mt_mutex_destroy,
    mutex_lock: mt_mutex_noop,
    mutex_unlock: mt_mutex_noop,
    mutex_try_lock: mt_mutex_noop,
};

unsafe extern "C" fn msg_message(
    _h: *mut c_void,
    kind: *const c_char,
    _id: *const c_char,
    fmt: *const c_char,
) -> OfxStatus {
    // SAFETY: strings C do plug-in (os argumentos variádicos são ignorados).
    let (k, f) = unsafe { (key(kind), key(fmt)) };
    eprintln!("[ofx] {}: {}", k.unwrap_or_default(), f.unwrap_or_default());
    STAT_OK
}

unsafe extern "C" fn msg_clear(_h: *mut c_void) -> OfxStatus {
    STAT_OK
}

#[repr(C)]
struct MessageSuiteV2 {
    message:
        unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char, *const c_char) -> OfxStatus,
    set_persistent:
        unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char, *const c_char) -> OfxStatus,
    clear_persistent: unsafe extern "C" fn(*mut c_void) -> OfxStatus,
}

static MESSAGE_SUITE: MessageSuiteV2 = MessageSuiteV2 {
    message: msg_message,
    set_persistent: msg_message,
    clear_persistent: msg_clear,
};

unsafe extern "C" fn tl_get_time(_e: *mut c_void, out: *mut f64) -> OfxStatus {
    if !out.is_null() {
        // SAFETY: `out` não nulo.
        unsafe { *out = 0.0 };
    }
    STAT_OK
}

unsafe extern "C" fn tl_goto(_e: *mut c_void, _t: f64) -> OfxStatus {
    STAT_OK
}

unsafe extern "C" fn tl_bounds(_e: *mut c_void, a: *mut f64, b: *mut f64) -> OfxStatus {
    // SAFETY: saídas não nulas, se fornecidas.
    unsafe {
        if !a.is_null() {
            *a = 0.0;
        }
        if !b.is_null() {
            *b = 0.0;
        }
    }
    STAT_OK
}

#[repr(C)]
struct TimeLineSuiteV1 {
    get_time: unsafe extern "C" fn(*mut c_void, *mut f64) -> OfxStatus,
    goto_time: unsafe extern "C" fn(*mut c_void, f64) -> OfxStatus,
    get_time_bounds: unsafe extern "C" fn(*mut c_void, *mut f64, *mut f64) -> OfxStatus,
}

static TIME_LINE_SUITE: TimeLineSuiteV1 = TimeLineSuiteV1 {
    get_time: tl_get_time,
    goto_time: tl_goto,
    get_time_bounds: tl_bounds,
};

unsafe extern "C" fn pr_start(_e: *mut c_void, _label: *const c_char) -> OfxStatus {
    STAT_OK
}

unsafe extern "C" fn pr_update(_e: *mut c_void, _p: f64) -> OfxStatus {
    STAT_OK
}

unsafe extern "C" fn pr_end(_e: *mut c_void) -> OfxStatus {
    STAT_OK
}

#[repr(C)]
struct ProgressSuiteV1 {
    start: unsafe extern "C" fn(*mut c_void, *const c_char) -> OfxStatus,
    update: unsafe extern "C" fn(*mut c_void, f64) -> OfxStatus,
    end: unsafe extern "C" fn(*mut c_void) -> OfxStatus,
}

static PROGRESS_SUITE: ProgressSuiteV1 = ProgressSuiteV1 {
    start: pr_start,
    update: pr_update,
    end: pr_end,
};

unsafe extern "C" fn fetch_suite(
    _host: *mut PropSet,
    name: *const c_char,
    version: c_int,
) -> *const c_void {
    // SAFETY: nome da suite vindo do plug-in.
    let Some(n) = (unsafe { key(name) }) else {
        return std::ptr::null();
    };
    let p: *const c_void = match (n.as_str(), version) {
        ("OfxPropertySuite", 1) => std::ptr::from_ref(&PROPERTY_SUITE).cast(),
        ("OfxImageEffectSuite", 1) => std::ptr::from_ref(&IMAGE_EFFECT_SUITE).cast(),
        ("OfxParameterSuite", 1) => std::ptr::from_ref(&PARAMETER_SUITE).cast(),
        ("OfxMemorySuite", 1) => std::ptr::from_ref(&MEMORY_SUITE).cast(),
        ("OfxMultiThreadSuite", 1) => std::ptr::from_ref(&MULTI_THREAD_SUITE).cast(),
        ("OfxMessageSuite", 1 | 2) => std::ptr::from_ref(&MESSAGE_SUITE).cast(),
        ("OfxTimeLineSuite", 1) => std::ptr::from_ref(&TIME_LINE_SUITE).cast(),
        ("OfxProgressSuite", 1) => std::ptr::from_ref(&PROGRESS_SUITE).cast(),
        _ => std::ptr::null(),
    };
    p
}

/// `OfxHost` de `ofxCore.h`.
#[repr(C)]
struct OfxHostRaw {
    host: *mut PropSet,
    fetch_suite: unsafe extern "C" fn(*mut PropSet, *const c_char, c_int) -> *const c_void,
}

fn host_props() -> Box<PropSet> {
    let mut p = Box::<PropSet>::default();
    p.set_str("OfxPropName", &["org.neoveg.Neoveg"]);
    p.set_str("OfxPropLabel", &["Neoveg"]);
    p.set_int("OfxPropAPIVersion", &[1, 4]);
    let v_major: c_int = env!("CARGO_PKG_VERSION_MAJOR").parse().unwrap_or(0);
    let v_minor: c_int = env!("CARGO_PKG_VERSION_MINOR").parse().unwrap_or(0);
    let v_patch: c_int = env!("CARGO_PKG_VERSION_PATCH").parse().unwrap_or(0);
    p.set_int("OfxPropVersion", &[v_major, v_minor, v_patch]);
    p.set_str("OfxPropVersionLabel", &[env!("CARGO_PKG_VERSION")]);
    p.set_int("OfxImageEffectHostPropIsBackground", &[0]);
    p.set_int("OfxImageEffectPropSupportsOverlays", &[0]);
    p.set_int("OfxImageEffectPropSupportsMultiResolution", &[0]);
    p.set_int("OfxImageEffectPropSupportsTiles", &[1]);
    p.set_int("OfxImageEffectPropTemporalClipAccess", &[0]);
    p.set_str(
        "OfxImageEffectPropSupportedComponents",
        &["OfxImageComponentRGBA"],
    );
    p.set_str(
        "OfxImageEffectPropSupportedContexts",
        &[
            "OfxImageEffectContextGeneral",
            "OfxImageEffectContextFilter",
            "OfxImageEffectContextGenerator",
            "OfxImageEffectContextTransition",
        ],
    );
    p.set_str(
        "OfxImageEffectPropSupportedPixelDepths",
        &["OfxBitDepthFloat", "OfxBitDepthByte"],
    );
    p.set_int("OfxImageEffectPropSupportsMultipleClipDepths", &[0]);
    p.set_int("OfxImageEffectPropSupportsMultipleClipPARs", &[0]);
    p.set_int("OfxImageEffectPropSetableFrameRate", &[0]);
    p.set_int("OfxImageEffectPropSetableFielding", &[0]);
    p.set_int("OfxImageEffectInstancePropSequentialRender", &[0]);
    p.set_str("OfxImageEffectPropOpenGLRenderSupported", &["false"]);
    p.set_int("OfxImageEffectPropRenderQualityDraft", &[0]);
    p.set_int("OfxParamHostPropSupportsCustomInteract", &[0]);
    p.set_int("OfxParamHostPropSupportsStringAnimation", &[0]);
    p.set_int("OfxParamHostPropSupportsChoiceAnimation", &[0]);
    p.set_int("OfxParamHostPropSupportsBooleanAnimation", &[0]);
    p.set_int("OfxParamHostPropSupportsCustomAnimation", &[0]);
    p.set_int("OfxParamHostPropSupportsParametricAnimation", &[0]);
    p.set_int("OfxParamHostPropMaxParameters", &[-1]);
    p.set_int("OfxParamHostPropMaxPages", &[0]);
    p.set_int("OfxParamHostPropPageRowColumnCount", &[0, 0]);
    p
}

// ---------------------------------------------------------------------------
// Instância hospedada
// ---------------------------------------------------------------------------

/// Tamanho padrão dos tiles enviados em `OfxImageEffectPropRenderWindow` (em pixels).
pub(crate) const OFX_TILE_SIZE: i32 = 512;

/// Divide um quadro `w x h` em retângulos `[x1, y1, x2, y2]` para renderização
/// por tiles OFX. Quando `!supports_tiles` ou o quadro cabe em um tile, devolve
/// um único retângulo `[0, 0, w, h]`.
pub(crate) fn compute_render_tiles(
    w: i32,
    h: i32,
    tile_size: i32,
    supports_tiles: bool,
) -> Vec<[i32; 4]> {
    if w <= 0 || h <= 0 {
        return Vec::new();
    }
    let step = tile_size.max(64);
    if !supports_tiles || (w <= step && h <= step) {
        return vec![[0, 0, w, h]];
    }
    let mut tiles = Vec::new();
    let mut y = 0;
    while y < h {
        let y2 = y.saturating_add(step).min(h);
        let mut x = 0;
        while x < w {
            let x2 = x.saturating_add(step).min(w);
            tiles.push([x, y, x2, y2]);
            x = x2;
        }
        y = y2;
    }
    tiles
}

/// Converte um quadro RGBA `f32` de cima para baixo em buffer OFX (de baixo para cima).
fn encode_ofx_plane(pixels: &[f32], w: usize, h: usize, float: bool) -> Vec<u8> {
    let row = w.saturating_mul(4);
    let n = w.saturating_mul(h);
    if float {
        let mut dst = vec![0u8; n.saturating_mul(16)];
        for y in 0..h {
            let from = &pixels[(h - 1 - y) * row..][..row];
            for (i, v) in from.iter().enumerate() {
                dst[(y * row + i) * 4..][..4].copy_from_slice(&v.to_le_bytes());
            }
        }
        dst
    } else {
        let mut dst = vec![0u8; n.saturating_mul(4)];
        for y in 0..h {
            let from = &pixels[(h - 1 - y) * row..][..row];
            for (i, v) in from.iter().enumerate() {
                dst[y * row + i] = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        }
        dst
    }
}

fn select_supported_context(contexts: &[String]) -> Result<&'static str, String> {
    if contexts.iter().any(|c| c == "OfxImageEffectContextGeneral") {
        Ok("OfxImageEffectContextGeneral")
    } else if contexts.iter().any(|c| c == "OfxImageEffectContextFilter") {
        Ok("OfxImageEffectContextFilter")
    } else if contexts
        .iter()
        .any(|c| c == "OfxImageEffectContextGenerator")
    {
        Ok("OfxImageEffectContextGenerator")
    } else if contexts
        .iter()
        .any(|c| c == "OfxImageEffectContextTransition")
    {
        Ok("OfxImageEffectContextTransition")
    } else {
        Err("plug-in não suporta os contextos General, Filter, Generator nem Transition".into())
    }
}

fn has_required_context_clips(ctx: &str, clips: &[Box<Clip>]) -> bool {
    let has = |name: &str| clips.iter().any(|c| c.name == name);
    match ctx {
        "OfxImageEffectContextGenerator" => has("Output"),
        "OfxImageEffectContextTransition" => has("Output") && has("SourceFrom") && has("SourceTo"),
        _ => has("Output") && has("Source"),
    }
}

/// Plug-in OFX carregado, descrito e instanciado em *General*, *Filter*, *Generator* ou *Transition*.
pub(crate) struct HostedEffect {
    plugin: *const OfxPluginRaw,
    _host: Box<OfxHostRaw>,
    _host_props: Box<PropSet>,
    _desc: Box<Effect>,
    inst: Box<Effect>,
    float: bool,
    supports_tiles: bool,
    context: String,
    aux_clips: Vec<String>,
}

impl HostedEffect {
    fn call(
        &self,
        action: &str,
        handle: *mut c_void,
        in_args: *mut PropSet,
        out_args: *mut PropSet,
    ) -> OfxStatus {
        // SAFETY: `plugin` é o ponteiro devolvido por OfxGetPlugin, válido
        // enquanto a biblioteca estiver carregada (ela nunca é liberada).
        let Some(entry) = (unsafe { self.plugin.as_ref() }).and_then(|p| p.main_entry) else {
            return STAT_ERR_UNSUPPORTED;
        };
        let a = cstring(action);
        // SAFETY: assinatura `OfxPluginEntryPoint` de `ofxCore.h`.
        unsafe {
            entry(
                a.as_ptr(),
                handle.cast_const(),
                in_args.cast(),
                out_args.cast(),
            )
        }
    }

    /// Carrega o binário, acha `plugin_id`, descreve e cria a instância.
    pub(crate) fn open(binary: &std::path::Path, plugin_id: &str) -> Result<Self, String> {
        // SAFETY: carregar o binário executa código de terceiros; só no subprocesso.
        let lib = unsafe { libloading::Library::new(binary) }
            .map_err(|e| format!("não foi possível carregar {}: {e}", binary.display()))?;
        // SAFETY: assinaturas de `ofxCore.h`.
        let count: libloading::Symbol<super::GetCountFn> =
            unsafe { lib.get(b"OfxGetNumberOfPlugins\0") }
                .map_err(|_| "binário sem OfxGetNumberOfPlugins".to_string())?;
        // SAFETY: idem.
        let get: libloading::Symbol<super::GetPluginFn> = unsafe { lib.get(b"OfxGetPlugin\0") }
            .map_err(|_| "binário sem OfxGetPlugin".to_string())?;
        // SAFETY: função sem argumentos do plug-in.
        let n = unsafe { count() }.clamp(0, 4096);
        let mut plugin = std::ptr::null::<OfxPluginRaw>();
        for i in 0..n {
            // SAFETY: índice em 0..n.
            let p = unsafe { get(i) };
            // SAFETY: ponteiro não nulo do plug-in; string lida com limite.
            if let Some(r) = unsafe { p.as_ref() } {
                if unsafe { bounded_cstr(r.plugin_identifier) } == plugin_id {
                    plugin = p;
                    break;
                }
            }
        }
        // SAFETY: ponteiro do plug-in (pode ser nulo).
        let Some(raw) = (unsafe { plugin.as_ref() }) else {
            return Err(format!("plug-in '{plugin_id}' não encontrado no binário"));
        };
        // SAFETY: string do plug-in, lida com limite.
        if unsafe { bounded_cstr(raw.plugin_api) } != "OfxImageEffectPluginAPI" {
            return Err("plug-in não é um efeito de imagem OFX".into());
        }
        std::mem::forget(lib);

        let mut host_props = host_props();
        let mut host = Box::new(OfxHostRaw {
            host: &mut *host_props,
            fetch_suite,
        });
        if let Some(set_host) = raw.set_host {
            // SAFETY: `OfxHost` vive na caixa até o fim do processo.
            unsafe { set_host(std::ptr::from_mut(&mut *host).cast()) };
        }
        let mut me = HostedEffect {
            plugin,
            _host: host,
            _host_props: host_props,
            _desc: Effect::new(),
            inst: Effect::new(),
            float: true,
            supports_tiles: true,
            context: String::new(),
            aux_clips: Vec::new(),
        };
        let ok = |st: OfxStatus| st == STAT_OK || st == STAT_REPLY_DEFAULT;
        let st = me.call(
            "OfxActionLoad",
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        if !ok(st) {
            return Err(format!("OfxActionLoad falhou (status {st})"));
        }
        let desc_ptr: *mut Effect = &mut *me._desc;
        let st = me.call(
            "OfxActionDescribe",
            desc_ptr.cast(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        if !ok(st) {
            return Err(format!("OfxActionDescribe falhou (status {st})"));
        }
        let contexts = me._desc.props.strs("OfxImageEffectPropSupportedContexts");
        let ctx = select_supported_context(&contexts)?;
        me.context = ctx.to_string();
        // Pela especificação OFX 1.4, `OfxImageEffectPropSupportsTiles` tem padrão 1.
        me.supports_tiles = me
            ._desc
            .props
            .nums("OfxImageEffectPropSupportsTiles")
            .first()
            .copied()
            .map(|v| v != 0.0)
            .unwrap_or(true);
        let depths = me
            ._desc
            .props
            .strs("OfxImageEffectPropSupportedPixelDepths");
        me.float = depths.is_empty() || depths.iter().any(|d| d == "OfxBitDepthFloat");
        if !me.float && !depths.iter().any(|d| d == "OfxBitDepthByte") {
            return Err("plug-in não aceita float nem 8 bits".into());
        }
        let mut in_args = Box::<PropSet>::default();
        in_args.set_str("OfxImageEffectPropContext", &[ctx]);
        let st = me.call(
            "OfxImageEffectActionDescribeInContext",
            desc_ptr.cast(),
            &mut *in_args,
            std::ptr::null_mut(),
        );
        if !ok(st) {
            return Err(format!("DescribeInContext falhou (status {st})"));
        }
        let is_trans = ctx == "OfxImageEffectContextTransition";
        if !has_required_context_clips(ctx, &me._desc.clips) {
            return Err("plug-in não define os clipes obrigatórios do contexto".into());
        }
        me.aux_clips = me
            ._desc
            .clips
            .iter()
            .filter(|c| {
                c.name != "Output" && c.name != "Source" && !(is_trans && c.name == "SourceFrom")
            })
            .map(|c| c.name.clone())
            .collect();

        me.inst = Effect::instance_of(&me._desc);
        me.inst.props.set_str("OfxImageEffectPropContext", &[ctx]);
        me.inst.props.set_int("OfxPropIsInteractive", &[0]);
        me.inst
            .props
            .set_dbl("OfxImageEffectPropProjectPixelAspectRatio", &[1.0]);
        me.inst.props.set_ptr("OfxPropInstanceData", 0);
        let depth = if me.float {
            "OfxBitDepthFloat"
        } else {
            "OfxBitDepthByte"
        };
        for c in &mut me.inst.clips {
            let connected = if c.name == "Source"
                || c.name == "Output"
                || (is_trans && (c.name == "SourceFrom" || c.name == "SourceTo"))
            {
                1
            } else {
                0
            };
            c.props.set_int("OfxImageClipPropConnected", &[connected]);
            c.props
                .set_str("OfxImageEffectPropComponents", &["OfxImageComponentRGBA"]);
            c.props.set_str(
                "OfxImageClipPropUnmappedComponents",
                &["OfxImageComponentRGBA"],
            );
            c.props.set_str("OfxImageEffectPropPixelDepth", &[depth]);
            c.props
                .set_str("OfxImageClipPropUnmappedPixelDepth", &[depth]);
            c.props.set_str(
                "OfxImageEffectPropPreMultiplication",
                &["OfxImagePreMultiplied"],
            );
            c.props.set_dbl("OfxImagePropPixelAspectRatio", &[1.0]);
            c.props
                .set_str("OfxImageClipPropFieldOrder", &["OfxImageFieldNone"]);
            c.props.set_int("OfxImageClipPropContinuousSamples", &[0]);
        }
        me.set_frame_props(16, 16, 30.0);
        let inst_ptr: *mut Effect = &mut *me.inst;
        let st = me.call(
            "OfxActionCreateInstance",
            inst_ptr.cast(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        if !ok(st) {
            return Err(format!("OfxActionCreateInstance falhou (status {st})"));
        }
        Ok(me)
    }

    fn set_frame_props(&mut self, w: i32, h: i32, fps: f64) {
        let (wd, hd) = (f64::from(w), f64::from(h));
        let p = &mut self.inst.props;
        p.set_dbl("OfxImageEffectPropProjectSize", &[wd, hd]);
        p.set_dbl("OfxImageEffectPropProjectExtent", &[wd, hd]);
        p.set_dbl("OfxImageEffectPropProjectOffset", &[0.0, 0.0]);
        p.set_dbl("OfxImageEffectPropFrameRate", &[fps]);
        p.set_dbl("OfxImageEffectInstancePropEffectDuration", &[1.0e6]);
        for c in &mut self.inst.clips {
            c.props.set_dbl("OfxImageEffectPropFrameRate", &[fps]);
            c.props.set_dbl("OfxImageClipPropFrameRate", &[fps]);
            c.props
                .set_dbl("OfxImageEffectPropFrameRange", &[0.0, 1.0e6]);
            c.props
                .set_dbl("OfxImageEffectPropUnmappedFrameRange", &[0.0, 1.0e6]);
            c.props
                .set_dbl("OfxImageEffectPropUnmappedFrameRate", &[fps]);
        }
    }

    /// Descritores dos parâmetros (para a interface).
    pub(crate) fn params(&self) -> Vec<OfxParamInfo> {
        self.inst
            .params
            .params
            .iter()
            .filter(|p| param_shape(&p.kind).0 > 0)
            .map(|p| OfxParamInfo {
                name: p.props.str0("OfxPropName").unwrap_or_default(),
                label: p
                    .props
                    .str0("OfxPropLabel")
                    .or_else(|| p.props.str0("OfxPropName"))
                    .unwrap_or_default(),
                kind: p.kind.trim_start_matches("OfxParamType").to_string(),
                default: p.nums.clone(),
                min: p
                    .props
                    .nums("OfxParamPropDisplayMin")
                    .first()
                    .copied()
                    .or_else(|| p.props.nums("OfxParamPropMin").first().copied()),
                max: p
                    .props
                    .nums("OfxParamPropDisplayMax")
                    .first()
                    .copied()
                    .or_else(|| p.props.nums("OfxParamPropMax").first().copied()),
                choices: p.props.strs("OfxParamPropChoiceOption"),
            })
            .collect()
    }

    pub(crate) fn context(&self) -> &str {
        &self.context
    }

    pub(crate) fn aux_clips(&self) -> &[String] {
        &self.aux_clips
    }

    pub(crate) fn is_generator(&self) -> bool {
        self.context == "OfxImageEffectContextGenerator"
            || (self.context != "OfxImageEffectContextTransition"
                && !self.inst.clips.iter().any(|c| c.name == "Source"))
    }

    /// Renderiza um quadro RGBA premultiplicado (de cima para baixo) no lugar,
    /// com suporte a tiles (`OfxImageEffectPropSupportsTiles`) e entradas
    /// auxiliares no contexto *General*.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render_ex(
        &mut self,
        w: u32,
        h: u32,
        time: f64,
        fps: f64,
        values: &[OfxParamValue],
        pixels: &mut [f32],
        aux_inputs: &[(&str, &[f32])],
    ) -> Result<(), String> {
        let (wi, hi) = (
            i32::try_from(w).map_err(|_| "largura inválida")?,
            i32::try_from(h).map_err(|_| "altura inválida")?,
        );
        let n = (w as usize).saturating_mul(h as usize);
        if wi <= 0 || hi <= 0 || pixels.len() != n.saturating_mul(4) {
            return Err("quadro com tamanho inválido".into());
        }
        let fps = if fps.is_finite() && fps > 0.0 {
            fps
        } else {
            30.0
        };
        let time = if time.is_finite() { time } else { 0.0 };
        self.set_frame_props(wi, hi, fps);
        for v in values {
            if let Some(p) = self
                .inst
                .params
                .params
                .iter_mut()
                .find(|p| p.props.str0("OfxPropName").as_deref() == Some(v.name.as_str()))
            {
                let (k, is_int) = param_shape(&p.kind);
                if k > 0 {
                    p.integer = v.integer || is_int;
                    p.hold = v.hold || is_hold_kind(&p.kind);
                    p.nums = (0..k)
                        .map(|i| {
                            v.values
                                .get(i)
                                .copied()
                                .filter(|x| x.is_finite())
                                .unwrap_or(0.0)
                        })
                        .collect();
                    p.keys.clear();
                    for (kt, kvals) in v.keys.iter().take(MAX_OFX_KEYS) {
                        let t_frames = ticks_to_ofx_time(*kt, fps);
                        let fitted: Vec<f64> = (0..k)
                            .map(|i| {
                                kvals
                                    .get(i)
                                    .copied()
                                    .filter(|x| x.is_finite())
                                    .unwrap_or(0.0)
                            })
                            .collect();
                        p.set_key(t_frames, fitted);
                    }
                    if !p.keys.is_empty() {
                        p.nums = p.eval_at(time);
                    }
                }
            }
        }
        // monta os buffers (OFX: primeira linha = baixo)
        let row = w as usize * 4;
        let primary_plane = encode_ofx_plane(pixels, w as usize, h as usize, self.float);
        let mut aux_map: HashMap<String, Vec<u8>> = HashMap::new();
        for &(clip_name, aux_px) in aux_inputs.iter().take(4) {
            if aux_px.len() == pixels.len() {
                aux_map.insert(
                    clip_name.to_string(),
                    encode_ofx_plane(aux_px, w as usize, h as usize, self.float),
                );
            }
        }
        // Conecta entradas auxiliares declaradas no contexto General: se vier
        // um frame explícito (ex.: fundo ou máscara da timeline) conecta pelo
        // nome, ou liga o primeiro frame auxiliar disponível ao primeiro clipe
        // auxiliar que o plug-in declarou.
        let fallback_aux = aux_inputs
            .first()
            .filter(|(_, p)| p.len() == pixels.len())
            .map(|(_, p)| encode_ofx_plane(p, w as usize, h as usize, self.float));
        let is_trans = self.context == "OfxImageEffectContextTransition";
        for c in &mut self.inst.clips {
            if c.name == "Output" || c.name == "Source" || (is_trans && c.name == "SourceFrom") {
                c.props.set_int("OfxImageClipPropConnected", &[1]);
                continue;
            }
            if !aux_map.contains_key(&c.name) {
                let optional = c
                    .props
                    .nums("OfxImageClipPropOptional")
                    .first()
                    .copied()
                    .unwrap_or(0.0)
                    != 0.0;
                if let Some(fb) = &fallback_aux {
                    aux_map.insert(c.name.clone(), fb.clone());
                } else if !optional {
                    aux_map.insert(c.name.clone(), primary_plane.clone());
                }
            }
            let connected = i32::from(aux_map.contains_key(&c.name));
            c.props.set_int("OfxImageClipPropConnected", &[connected]);
        }

        let b = &mut self.inst.bufs;
        b.w = wi;
        b.h = hi;
        b.float = self.float;
        b.src = primary_plane;
        b.dst = vec![0u8; n * if self.float { 16 } else { 4 }];
        b.aux_srcs = aux_map;

        let inst_ptr: *mut Effect = &mut *self.inst;
        let mut seq = Box::<PropSet>::default();
        seq.set_dbl("OfxImageEffectPropFrameRange", &[time, time]);
        seq.set_dbl("OfxImageEffectPropFrameStep", &[1.0]);
        seq.set_int("OfxPropIsInteractive", &[0]);
        seq.set_dbl("OfxImageEffectPropRenderScale", &[1.0, 1.0]);
        seq.set_int("OfxImageEffectPropSequentialRenderStatus", &[0]);
        seq.set_int("OfxImageEffectPropInteractiveRenderStatus", &[0]);
        let _ = self.call(
            "OfxImageEffectActionBeginSequenceRender",
            inst_ptr.cast(),
            &mut *seq,
            std::ptr::null_mut(),
        );

        let tiles = compute_render_tiles(wi, hi, OFX_TILE_SIZE, self.supports_tiles);
        let mut st = STAT_OK;
        for tile in tiles {
            let mut args = Box::<PropSet>::default();
            args.set_dbl("OfxPropTime", &[time]);
            args.set_str("OfxImageEffectPropFieldToRender", &["OfxImageFieldNone"]);
            args.set_int("OfxImageEffectPropRenderWindow", &tile);
            args.set_dbl("OfxImageEffectPropRenderScale", &[1.0, 1.0]);
            args.set_int("OfxImageEffectPropSequentialRenderStatus", &[0]);
            args.set_int("OfxImageEffectPropInteractiveRenderStatus", &[0]);
            args.set_int("OfxImageEffectPropRenderQualityDraft", &[0]);
            st = self.call(
                "OfxImageEffectActionRender",
                inst_ptr.cast(),
                &mut *args,
                std::ptr::null_mut(),
            );
            if st != STAT_OK {
                break;
            }
        }
        let _ = self.call(
            "OfxImageEffectActionEndSequenceRender",
            inst_ptr.cast(),
            &mut *seq,
            std::ptr::null_mut(),
        );
        for c in &mut self.inst.clips {
            c.images.clear();
        }
        if st != STAT_OK {
            return Err(format!("OfxImageEffectActionRender falhou (status {st})"));
        }
        let b = &self.inst.bufs;
        for y in 0..h as usize {
            let to = &mut pixels[(h as usize - 1 - y) * row..][..row];
            for (i, v) in to.iter_mut().enumerate() {
                *v = if self.float {
                    let o = (y * row + i) * 4;
                    let x =
                        f32::from_le_bytes([b.dst[o], b.dst[o + 1], b.dst[o + 2], b.dst[o + 3]]);
                    if x.is_finite() {
                        x
                    } else {
                        0.0
                    }
                } else {
                    f32::from(b.dst[y * row + i]) / 255.0
                };
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn property_suite_round_trips_values() {
        let mut ps = Box::<PropSet>::default();
        let h: *mut PropSet = &mut *ps;
        let k = cstring("Teste");
        // SAFETY: handle e strings válidos e locais.
        unsafe {
            assert_eq!(prop_set_double(h, k.as_ptr(), 1, 2.5), STAT_OK);
            let mut d = 0.0;
            assert_eq!(prop_get_double(h, k.as_ptr(), 1, &mut d), STAT_OK);
            assert_eq!(d, 2.5);
            let mut dim = 0;
            assert_eq!(prop_get_dimension(h, k.as_ptr(), &mut dim), STAT_OK);
            assert_eq!(dim, 2);
            assert_eq!(
                prop_get_double(h, k.as_ptr(), 5, &mut d),
                STAT_ERR_BAD_INDEX
            );
            let outro = cstring("Nada");
            assert_eq!(
                prop_get_double(h, outro.as_ptr(), 0, &mut d),
                STAT_ERR_UNKNOWN
            );
            let s = cstring("valor");
            assert_eq!(prop_set_string(h, k.as_ptr(), 0, s.as_ptr()), STAT_OK);
            let mut out: *const c_char = std::ptr::null();
            assert_eq!(prop_get_string(h, k.as_ptr(), 0, &mut out), STAT_OK);
            assert_eq!(bounded_cstr(out), "valor");
            assert_eq!(
                prop_get_double(std::ptr::null_mut(), k.as_ptr(), 0, &mut d),
                STAT_ERR_BAD_HANDLE
            );
            assert_eq!(prop_set_int(h, k.as_ptr(), -1, 3), STAT_ERR_BAD_INDEX);
        }
    }

    #[test]
    fn param_values_follow_type_shape() {
        let mut props = Box::<PropSet>::default();
        props.set_dbl("OfxParamPropDefault", &[0.1, 0.2, 0.3, 1.0]);
        let mut p = Param::new_empty("OfxParamTypeRGBA".into(), props);
        p.reset_to_default();
        let mut out = [0f64; 4];
        let ptrs = out
            .each_mut()
            .map(|x| std::ptr::from_mut(x).cast::<c_void>());
        // SAFETY: quatro ponteiros para double locais.
        assert_eq!(unsafe { write_value(&p, ptrs) }, STAT_OK);
        assert_eq!(out, [0.1, 0.2, 0.3, 1.0]);
        // SAFETY: valores double como palavras de 64 bits.
        let st = unsafe {
            read_value(
                &mut p,
                [0.5f64.to_bits() as usize, 0, f64::NAN.to_bits() as usize, 0],
            )
        };
        assert_eq!(st, STAT_OK);
        assert_eq!(p.nums, vec![0.5, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn memory_suite_allocates_and_frees() {
        let mut p: *mut c_void = std::ptr::null_mut();
        // SAFETY: saída local.
        unsafe {
            assert_eq!(mem_alloc(std::ptr::null_mut(), 64, &mut p), STAT_OK);
            assert!(!p.is_null());
            p.cast::<u8>().write_bytes(7, 64);
            assert_eq!(mem_free(p), STAT_OK);
            assert_eq!(
                mem_alloc(std::ptr::null_mut(), MAX_ALLOC + 1, &mut p),
                STAT_ERR_MEMORY
            );
        }
    }

    #[test]
    fn param_keyframes_evaluate_derivative_integral_and_key_queries() {
        let mut p = Box::new(Param::new_empty(
            "OfxParamTypeDouble".into(),
            Box::default(),
        ));
        p.set_key(10.0, vec![0.0]);
        p.set_key(30.0, vec![10.0]);
        let ptr: *mut Param = &mut *p;
        // SAFETY: ponteiros locais válidos para c_uint / c_int / f64.
        unsafe {
            let mut count: c_uint = 0;
            assert_eq!(pa_get_num_keys(ptr, &mut count), STAT_OK);
            assert_eq!(count, 2);

            let mut kt = 0.0;
            assert_eq!(pa_get_key_time(ptr, 0, &mut kt), STAT_OK);
            assert_eq!(kt, 10.0);
            assert_eq!(pa_get_key_time(ptr, 1, &mut kt), STAT_OK);
            assert_eq!(kt, 30.0);
            assert_eq!(pa_get_key_time(ptr, 2, &mut kt), STAT_ERR_BAD_INDEX);

            let mut idx: c_int = -1;
            assert_eq!(pa_get_key_index(ptr, 10.0, 0, &mut idx), STAT_OK);
            assert_eq!(idx, 0);
            assert_eq!(pa_get_key_index(ptr, 15.0, 0, &mut idx), STAT_FAILED);
            assert_eq!(pa_get_key_index(ptr, 15.0, -1, &mut idx), STAT_OK);
            assert_eq!(idx, 0);
            assert_eq!(pa_get_key_index(ptr, 15.0, 1, &mut idx), STAT_OK);
            assert_eq!(idx, 1);

            let mut val = 0.0f64;
            assert_eq!(
                pa_get_value_at_time(
                    ptr,
                    20.0,
                    std::ptr::from_mut(&mut val).cast(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut()
                ),
                STAT_OK
            );
            assert!((val - 5.0).abs() < 1e-9, "{val}");

            let mut deriv = 0.0f64;
            assert_eq!(
                pa_get_derivative(
                    ptr,
                    20.0,
                    std::ptr::from_mut(&mut deriv).cast(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut()
                ),
                STAT_OK
            );
            assert!((deriv - 0.5).abs() < 1e-9, "{deriv}");

            let mut integ = 0.0f64;
            assert_eq!(
                pa_get_integral(
                    ptr,
                    10.0,
                    30.0,
                    std::ptr::from_mut(&mut integ).cast(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut()
                ),
                STAT_OK
            );
            assert!((integ - 100.0).abs() < 1e-9, "{integ}");

            // ordem reversa inverte sinal da integral
            assert_eq!(
                pa_get_integral(
                    ptr,
                    30.0,
                    10.0,
                    std::ptr::from_mut(&mut integ).cast(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut()
                ),
                STAT_OK
            );
            assert!((integ + 100.0).abs() < 1e-9, "{integ}");

            // tempo hostil (NaN) retorna erro sem pânico
            assert_eq!(
                pa_get_value_at_time(
                    ptr,
                    f64::NAN,
                    std::ptr::from_mut(&mut val).cast(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut()
                ),
                STAT_ERR_VALUE
            );

            assert_eq!(pa_delete_key(ptr, 10.0), STAT_OK);
            assert_eq!(pa_get_num_keys(ptr, &mut count), STAT_OK);
            assert_eq!(count, 1);
            assert_eq!(pa_delete_all_keys(ptr), STAT_OK);
            assert_eq!(pa_get_num_keys(ptr, &mut count), STAT_OK);
            assert_eq!(count, 0);
        }
    }

    #[test]
    fn ofx_tiles_cobrem_quadro_completo_e_aceitam_entradas_auxiliares() {
        let tiles = compute_render_tiles(1920, 1080, 512, true);
        assert_eq!(tiles.len(), 12); // 4 colunas x 3 linhas
        let area: i64 = tiles
            .iter()
            .map(|t| i64::from(t[2] - t[0]) * i64::from(t[3] - t[1]))
            .sum();
        assert_eq!(area, 1920 * 1080);
        assert_eq!(
            compute_render_tiles(1920, 1080, 512, false),
            vec![[0, 0, 1920, 1080]]
        );
        assert!(compute_render_tiles(0, 1080, 512, true).is_empty());
        assert!(compute_render_tiles(-4, 10, 512, true).is_empty());

        // Clipe auxiliar ("Mask") no contexto General resolve via ie_clip_get_image
        let mut eff = Effect::new();
        eff.bufs.w = 4;
        eff.bufs.h = 4;
        eff.bufs.float = true;
        eff.bufs.src = vec![1u8; 4 * 4 * 16];
        eff.bufs.dst = vec![0u8; 4 * 4 * 16];
        eff.bufs
            .aux_srcs
            .insert("Mask".to_string(), vec![2u8; 4 * 4 * 16]);
        let eff_ptr: *mut Effect = &mut *eff;
        let mut mask_clip = Box::new(Clip {
            name: "Mask".into(),
            props: Box::default(),
            images: vec![],
            owner: eff_ptr,
        });
        let mut missing_clip = Box::new(Clip {
            name: "Absent".into(),
            props: Box::default(),
            images: vec![],
            owner: eff_ptr,
        });
        let mut img_out: *mut PropSet = std::ptr::null_mut();
        // SAFETY: clipes e efeito locais válidos.
        unsafe {
            assert_eq!(
                ie_clip_get_image(&mut *mask_clip, 0.0, std::ptr::null(), &mut img_out),
                STAT_OK
            );
            assert!(!img_out.is_null());
            assert_eq!(
                ie_clip_get_image(&mut *missing_clip, 0.0, std::ptr::null(), &mut img_out),
                STAT_FAILED
            );
        }
    }

    #[test]
    fn ofx_transition_context_is_supported_and_maps_source_from_and_to() {
        let host = host_props();
        assert!(host
            .strs("OfxImageEffectPropSupportedContexts")
            .iter()
            .any(|c| c == "OfxImageEffectContextTransition"));
        assert_eq!(
            select_supported_context(&["OfxImageEffectContextTransition".into()]),
            Ok("OfxImageEffectContextTransition")
        );
        assert!(select_supported_context(&["OfxImageEffectContextUnknown".into()]).is_err());

        let mut eff = Effect::new();
        eff.bufs.w = 4;
        eff.bufs.h = 4;
        eff.bufs.float = true;
        eff.bufs.src = vec![11u8; 4 * 4 * 16];
        eff.bufs.dst = vec![0u8; 4 * 4 * 16];
        eff.bufs
            .aux_srcs
            .insert("SourceTo".to_string(), vec![22u8; 4 * 4 * 16]);
        let eff_ptr: *mut Effect = &mut *eff;
        let mk = |name: &str| {
            Box::new(Clip {
                name: name.into(),
                props: Box::default(),
                images: vec![],
                owner: eff_ptr,
            })
        };
        let incomplete = vec![mk("Output"), mk("SourceFrom")];
        assert!(!has_required_context_clips(
            "OfxImageEffectContextTransition",
            &incomplete
        ));
        let mut complete = vec![mk("Output"), mk("SourceFrom"), mk("SourceTo")];
        assert!(has_required_context_clips(
            "OfxImageEffectContextTransition",
            &complete
        ));

        let mut img_from: *mut PropSet = std::ptr::null_mut();
        let mut img_to: *mut PropSet = std::ptr::null_mut();
        // SAFETY: clipes e efeito locais válidos.
        unsafe {
            assert_eq!(
                ie_clip_get_image(&mut *complete[1], 0.0, std::ptr::null(), &mut img_from),
                STAT_OK
            );
            assert_eq!(
                ie_clip_get_image(&mut *complete[2], 0.0, std::ptr::null(), &mut img_to),
                STAT_OK
            );
            assert!(!img_from.is_null());
            assert!(!img_to.is_null());
        }
    }
}
