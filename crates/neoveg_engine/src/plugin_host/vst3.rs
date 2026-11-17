//! Host VST 3 em processo, a partir da API pública do VST 3 SDK 3.8
//! (licença MIT), pelos bindings do crate `vst3` (`MIT OR Apache-2.0`).
//!
//! **Só roda no subprocesso `neoveg --plugin-host serve-vst3`** (ou
//! `probe-vst3`): o plug-in é código de terceiros e pode travar ou derrubar o
//! processo.
//!
//! Escopo: classes `Audio Module Class` usadas como efeito, barramento de
//! áudio principal (entrada e saída) em estéreo ou mono, amostras `f32`,
//! blocos de até [`MAX_BLOCK`] quadros, parâmetros por valor normalizado
//! aplicados no início do bloco, sem editor gráfico (`IPlugView`), sem
//! eventos MIDI e sem compensação de latência.
//!
//! Os objetos do lado do host (`IHostApplication`, `IMessage`,
//! `IAttributeList`, `IBStream`, `IComponentHandler`, `IParameterChanges` e
//! `IParamValueQueue`) são implementados aqui com estado em `Mutex`, porque
//! o plug-in pode chamá-los de qualquer thread.

#![allow(unsafe_code)]

use super::{Vst3ClassInfo, Vst3ParamInfo};
use crate::model::Vst3ParamValue;
use std::collections::HashMap;
use std::ffi::{c_char, c_void, CString};
use std::mem::ManuallyDrop;
use std::path::Path;
use std::ptr;
use std::sync::{Mutex, PoisonError};
use vst3::Steinberg::Vst::IAttributeList_::AttrID;
use vst3::Steinberg::Vst::*;
use vst3::Steinberg::*;
use vst3::{Class, ComPtr, ComWrapper, Interface};

/// Categoria das classes de processamento de áudio (`ivstaudioprocessor.h`).
const AUDIO_MODULE_CLASS: &str = "Audio Module Class";
/// Máximo de classes aceitas de uma fábrica (entrada hostil, §0.4).
const MAX_CLASSES: i32 = 4096;
/// Máximo de parâmetros lidos de um controlador.
const MAX_PARAMS: i32 = 16384;
/// Maior bloco de áudio por chamada de `process`.
pub const MAX_BLOCK: usize = 8192;
/// Máximo de barramentos de áudio por direção.
const MAX_BUSES: i32 = 16;
/// Máximo de canais por barramento.
const MAX_CHANNELS: i32 = 32;
/// Maior fluxo de estado aceito (`IBStream`).
const MAX_STREAM_BYTES: usize = 64 * 1024 * 1024;
/// Máximo de atributos numa mensagem e de pontos numa fila de parâmetro.
const MAX_ATTRS: usize = 4096;
const MAX_POINTS: usize = 4096;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Lê um texto C de tamanho fixo até o primeiro NUL.
fn fixed_cstr(b: &[c_char]) -> String {
    let bytes: Vec<u8> = b
        .iter()
        .take_while(|c| **c != 0)
        .map(|c| *c as u8)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Lê um `String128` (UTF-16) até o primeiro NUL.
fn str128(s: &String128) -> String {
    let n = s.iter().position(|c| *c == 0).unwrap_or(s.len());
    String::from_utf16_lossy(s.get(..n).unwrap_or(&[]))
}

/// Escreve `src` num `String128`, truncando e terminando com NUL.
fn write_str128(src: &str, dst: &mut String128) {
    let mut n = 0;
    for (d, c) in dst.iter_mut().zip(src.encode_utf16()) {
        *d = c;
        n += 1;
    }
    let end = n.min(dst.len() - 1);
    if let Some(d) = dst.get_mut(end) {
        *d = 0;
    }
}

/// `TUID` em 32 dígitos hexadecimais (bytes na ordem da memória).
pub fn cid_to_hex(cid: &TUID) -> String {
    cid.iter().map(|b| format!("{:02X}", *b as u8)).collect()
}

/// Lê um `TUID` de 32 dígitos hexadecimais. Entrada inválida vira `None`.
pub fn hex_to_cid(s: &str) -> Option<TUID> {
    let s = s.trim();
    if s.len() != 32 || !s.is_ascii() {
        return None;
    }
    let mut out: TUID = [0; 16];
    for (i, o) in out.iter_mut().enumerate() {
        let byte = u8::from_str_radix(s.get(i * 2..i * 2 + 2)?, 16).ok()?;
        *o = byte as c_char;
    }
    Some(out)
}

fn guid_eq(a: *const TUID, b: &vst3::com_scrape_types::Guid) -> bool {
    if a.is_null() {
        return false;
    }
    // SAFETY: `a` não é nulo e aponta para 16 bytes (contrato de `TUID*`).
    let a = unsafe { &*a };
    a.iter().zip(b.iter()).all(|(x, y)| *x as u8 == *y)
}

// ---------------------------------------------------------------------------
// Módulo (biblioteca + fábrica)
// ---------------------------------------------------------------------------

type GetFactoryFn = unsafe extern "system" fn() -> *mut IPluginFactory;
type InitDllFn = unsafe extern "system" fn() -> bool;

struct Module {
    factory: ComPtr<IPluginFactory>,
    // Nunca descarregado: muitos plug-ins não suportam `FreeLibrary`, e o
    // subprocesso termina logo depois.
    _lib: ManuallyDrop<libloading::Library>,
}

impl Module {
    fn load(path: &Path) -> Result<Self, String> {
        // SAFETY: carregar a biblioteca executa a inicialização dela; o risco
        // é aceito porque este módulo só roda no subprocesso isolado.
        let lib = unsafe { libloading::Library::new(path) }
            .map_err(|e| format!("não foi possível carregar {}: {e}", path.display()))?;
        // SAFETY: `InitDll` é opcional no Windows e não recebe argumentos.
        if let Ok(init) = unsafe { lib.get::<InitDllFn>(b"InitDll\0") } {
            // SAFETY: assinatura `bool InitDll()` definida pelo SDK.
            if !unsafe { init() } {
                return Err("InitDll do plug-in falhou".into());
            }
        }
        // SAFETY: assinatura `IPluginFactory* GetPluginFactory()` do SDK.
        let get: libloading::Symbol<GetFactoryFn> = unsafe { lib.get(b"GetPluginFactory\0") }
            .map_err(|_| "binário sem GetPluginFactory (não é um plug-in VST3)".to_string())?;
        // SAFETY: função sem argumentos exportada pelo plug-in.
        let raw = unsafe { get() };
        // SAFETY: a fábrica devolvida já vem com uma referência para o host.
        let factory = unsafe { ComPtr::from_raw(raw) }.ok_or("fábrica VST3 nula")?;
        Ok(Module {
            factory,
            _lib: ManuallyDrop::new(lib),
        })
    }

    fn create<I: Interface>(&self, cid: &TUID) -> Option<ComPtr<I>> {
        let mut obj: *mut c_void = ptr::null_mut();
        // SAFETY: `cid` e o IID têm 16 bytes; `obj` recebe um ponteiro com
        // uma referência para o host, ou continua nulo.
        let r = unsafe {
            self.factory
                .createInstance(cid.as_ptr(), I::IID.as_ptr() as FIDString, &mut obj)
        };
        if r != kResultOk {
            return None;
        }
        // SAFETY: o plug-in devolveu um objeto da interface pedida.
        unsafe { ComPtr::from_raw(obj as *mut I) }
    }
}

/// Lista as classes de efeito de áudio do módulo. Só no subprocesso.
pub fn probe(path: &Path) -> Result<Vec<Vst3ClassInfo>, String> {
    let m = Module::load(path)?;
    // SAFETY: chamada sem argumentos à fábrica válida.
    let n = unsafe { m.factory.countClasses() };
    if !(0..=MAX_CLASSES).contains(&n) {
        return Err(format!("contagem de classes VST3 inválida: {n}"));
    }
    let f2 = m.factory.cast::<IPluginFactory2>();
    let mut out = Vec::new();
    for i in 0..n {
        // SAFETY: `PClassInfo` é POD; zeros são um valor válido.
        let mut info: PClassInfo = unsafe { std::mem::zeroed() };
        // SAFETY: `i` em `0..n` e `info` é gravável.
        if unsafe { m.factory.getClassInfo(i, &mut info) } != kResultOk {
            continue;
        }
        if fixed_cstr(&info.category) != AUDIO_MODULE_CLASS {
            continue;
        }
        let mut c = Vst3ClassInfo {
            class_id: cid_to_hex(&info.cid),
            name: fixed_cstr(&info.name),
            sub_categories: String::new(),
            vendor: String::new(),
            version: String::new(),
        };
        if let Some(f2) = &f2 {
            // SAFETY: idem, para `PClassInfo2`.
            let mut i2: PClassInfo2 = unsafe { std::mem::zeroed() };
            // SAFETY: `i` em `0..n` e `i2` é gravável.
            if unsafe { f2.getClassInfo2(i, &mut i2) } == kResultOk {
                c.sub_categories = fixed_cstr(&i2.subCategories);
                c.vendor = fixed_cstr(&i2.vendor);
                c.version = fixed_cstr(&i2.version);
            }
        }
        out.push(c);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Objetos do host
// ---------------------------------------------------------------------------

/// `IHostApplication`: nome do host e criação de `IMessage`/`IAttributeList`.
pub(crate) struct HostApp;

impl Class for HostApp {
    type Interfaces = (IHostApplication,);
}

impl IHostApplicationTrait for HostApp {
    unsafe fn getName(&self, name: *mut String128) -> tresult {
        if name.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: `name` não é nulo e aponta para um `String128` gravável.
        write_str128("Neoveg", unsafe { &mut *name });
        kResultOk
    }

    unsafe fn createInstance(
        &self,
        cid: *mut TUID,
        _iid: *mut TUID,
        obj: *mut *mut c_void,
    ) -> tresult {
        if obj.is_null() {
            return kInvalidArgument;
        }
        let made: *mut c_void = if guid_eq(cid, &IMessage::IID) {
            ComWrapper::new(HostMessage::default())
                .to_com_ptr::<IMessage>()
                .map(|p| p.into_raw() as *mut c_void)
                .unwrap_or(ptr::null_mut())
        } else if guid_eq(cid, &IAttributeList::IID) {
            ComWrapper::new(HostAttrs::default())
                .to_com_ptr::<IAttributeList>()
                .map(|p| p.into_raw() as *mut c_void)
                .unwrap_or(ptr::null_mut())
        } else {
            ptr::null_mut()
        };
        // SAFETY: `obj` não é nulo (verificado acima).
        unsafe { *obj = made };
        if made.is_null() {
            kResultFalse
        } else {
            kResultOk
        }
    }
}

#[derive(Clone, Debug)]
enum Attr {
    Int(i64),
    Float(f64),
    Str(Vec<u16>),
    Bin(Vec<u8>),
}

/// `IAttributeList` em memória.
#[derive(Default)]
pub(crate) struct HostAttrs {
    map: Mutex<HashMap<Vec<u8>, Attr>>,
}

impl Class for HostAttrs {
    type Interfaces = (IAttributeList,);
}

/// Lê a chave `AttrID` (string C) com limite.
fn attr_key(id: AttrID) -> Option<Vec<u8>> {
    if id.is_null() {
        return None;
    }
    let mut out = Vec::new();
    for i in 0..256 {
        // SAFETY: `id` é uma string C não nula; a leitura para no NUL ou em
        // 256 bytes.
        let b = unsafe { id.add(i).read() } as u8;
        if b == 0 {
            return Some(out);
        }
        out.push(b);
    }
    Some(out)
}

impl HostAttrs {
    fn set(&self, id: AttrID, v: Attr) -> tresult {
        let Some(k) = attr_key(id) else {
            return kInvalidArgument;
        };
        let mut m = lock(&self.map);
        if m.len() >= MAX_ATTRS && !m.contains_key(&k) {
            return kResultFalse;
        }
        m.insert(k, v);
        kResultOk
    }

    fn get(&self, id: AttrID) -> Option<Attr> {
        let k = attr_key(id)?;
        lock(&self.map).get(&k).cloned()
    }
}

impl IAttributeListTrait for HostAttrs {
    unsafe fn setInt(&self, id: AttrID, value: i64) -> tresult {
        self.set(id, Attr::Int(value))
    }

    unsafe fn getInt(&self, id: AttrID, value: *mut i64) -> tresult {
        match (self.get(id), value.is_null()) {
            (Some(Attr::Int(v)), false) => {
                // SAFETY: `value` não é nulo.
                unsafe { *value = v };
                kResultOk
            }
            _ => kResultFalse,
        }
    }

    unsafe fn setFloat(&self, id: AttrID, value: f64) -> tresult {
        self.set(id, Attr::Float(value))
    }

    unsafe fn getFloat(&self, id: AttrID, value: *mut f64) -> tresult {
        match (self.get(id), value.is_null()) {
            (Some(Attr::Float(v)), false) => {
                // SAFETY: `value` não é nulo.
                unsafe { *value = v };
                kResultOk
            }
            _ => kResultFalse,
        }
    }

    unsafe fn setString(&self, id: AttrID, string: *const TChar) -> tresult {
        if string.is_null() {
            return kInvalidArgument;
        }
        let mut v = Vec::new();
        for i in 0..MAX_ATTRS * 16 {
            // SAFETY: `string` é UTF-16 terminado em NUL; a leitura para no
            // NUL ou no limite.
            let c = unsafe { string.add(i).read() };
            if c == 0 {
                break;
            }
            v.push(c);
        }
        self.set(id, Attr::Str(v))
    }

    unsafe fn getString(&self, id: AttrID, string: *mut TChar, size_in_bytes: u32) -> tresult {
        let cap = (size_in_bytes / 2) as usize;
        if string.is_null() || cap == 0 {
            return kInvalidArgument;
        }
        let Some(Attr::Str(v)) = self.get(id) else {
            return kResultFalse;
        };
        let n = v.len().min(cap - 1);
        for (i, c) in v.iter().take(n).enumerate() {
            // SAFETY: `i` < `cap`, o espaço que o plug-in declarou.
            unsafe { string.add(i).write(*c) };
        }
        // SAFETY: `n` < `cap`.
        unsafe { string.add(n).write(0) };
        kResultOk
    }

    unsafe fn setBinary(&self, id: AttrID, data: *const c_void, size_in_bytes: u32) -> tresult {
        let n = size_in_bytes as usize;
        if n > MAX_STREAM_BYTES || (data.is_null() && n > 0) {
            return kInvalidArgument;
        }
        let v = if n == 0 {
            vec![]
        } else {
            // SAFETY: `data` não é nulo e aponta para `n` bytes legíveis.
            unsafe { std::slice::from_raw_parts(data as *const u8, n) }.to_vec()
        };
        self.set(id, Attr::Bin(v))
    }

    unsafe fn getBinary(
        &self,
        id: AttrID,
        data: *mut *const c_void,
        size_in_bytes: *mut u32,
    ) -> tresult {
        if data.is_null() || size_in_bytes.is_null() {
            return kInvalidArgument;
        }
        let Some(k) = attr_key(id) else {
            return kInvalidArgument;
        };
        let m = lock(&self.map);
        let Some(Attr::Bin(v)) = m.get(&k) else {
            return kResultFalse;
        };
        // O ponteiro aponta para dentro do mapa e vale até o atributo mudar,
        // como no SDK.
        // SAFETY: `data` e `size_in_bytes` não são nulos.
        unsafe {
            *data = v.as_ptr() as *const c_void;
            *size_in_bytes = v.len() as u32;
        }
        kResultOk
    }
}

/// `IMessage` com id e lista de atributos.
pub(crate) struct HostMessage {
    id: Mutex<CString>,
    attrs: ComWrapper<HostAttrs>,
}

impl Default for HostMessage {
    fn default() -> Self {
        HostMessage {
            id: Mutex::new(CString::default()),
            attrs: ComWrapper::new(HostAttrs::default()),
        }
    }
}

impl Class for HostMessage {
    type Interfaces = (IMessage,);
}

impl IMessageTrait for HostMessage {
    unsafe fn getMessageID(&self) -> FIDString {
        // O texto vive no `CString` da mensagem até o próximo `setMessageID`.
        lock(&self.id).as_ptr()
    }

    unsafe fn setMessageID(&self, id: FIDString) {
        let s = attr_key(id).unwrap_or_default();
        *lock(&self.id) = CString::new(s).unwrap_or_default();
    }

    unsafe fn getAttributes(&self) -> *mut IAttributeList {
        // Sem `addRef`, como no SDK: a lista vive enquanto a mensagem viver.
        self.attrs
            .as_com_ref::<IAttributeList>()
            .map(|r| r.as_ptr())
            .unwrap_or(ptr::null_mut())
    }
}

/// `IBStream` em memória, com limite de tamanho.
#[derive(Default)]
pub(crate) struct MemStream {
    inner: Mutex<(Vec<u8>, usize)>,
}

impl Class for MemStream {
    type Interfaces = (IBStream,);
}

impl MemStream {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let len = bytes.len().min(MAX_STREAM_BYTES);
        Self {
            inner: Mutex::new((bytes[..len].to_vec(), 0)),
        }
    }

    pub fn bytes(&self) -> Vec<u8> {
        lock(&self.inner).0.clone()
    }
}

impl IBStreamTrait for MemStream {
    unsafe fn read(
        &self,
        buffer: *mut c_void,
        num_bytes: i32,
        num_bytes_read: *mut i32,
    ) -> tresult {
        if num_bytes < 0 || (buffer.is_null() && num_bytes > 0) {
            return kInvalidArgument;
        }
        let mut g = lock(&self.inner);
        let (data, pos) = &mut *g;
        let start = (*pos).min(data.len());
        let n = (num_bytes as usize).min(data.len() - start);
        if n > 0 {
            // SAFETY: `buffer` tem `num_bytes` >= `n` bytes graváveis e a fonte
            // tem `n` bytes a partir de `start`.
            unsafe {
                ptr::copy_nonoverlapping(data.as_ptr().add(start), buffer as *mut u8, n);
            }
        }
        *pos = start + n;
        if !num_bytes_read.is_null() {
            // SAFETY: ponteiro não nulo fornecido pelo plug-in.
            unsafe { *num_bytes_read = n as i32 };
        }
        kResultOk
    }

    unsafe fn write(
        &self,
        buffer: *mut c_void,
        num_bytes: i32,
        num_bytes_written: *mut i32,
    ) -> tresult {
        if num_bytes < 0 || (buffer.is_null() && num_bytes > 0) {
            return kInvalidArgument;
        }
        let mut g = lock(&self.inner);
        let (data, pos) = &mut *g;
        let n = num_bytes as usize;
        let Some(end) = pos.checked_add(n).filter(|e| *e <= MAX_STREAM_BYTES) else {
            return kResultFalse;
        };
        if data.len() < end {
            data.resize(end, 0);
        }
        if n > 0 {
            // SAFETY: `buffer` tem `n` bytes legíveis e o destino foi
            // redimensionado para caber `end` bytes.
            unsafe {
                ptr::copy_nonoverlapping(buffer as *const u8, data.as_mut_ptr().add(*pos), n);
            }
        }
        *pos = end;
        if !num_bytes_written.is_null() {
            // SAFETY: ponteiro não nulo fornecido pelo plug-in.
            unsafe { *num_bytes_written = num_bytes };
        }
        kResultOk
    }

    unsafe fn seek(&self, pos: i64, mode: i32, result: *mut i64) -> tresult {
        let mut g = lock(&self.inner);
        let (data, cur) = &mut *g;
        let base: i64 = match mode {
            IBStream_::IStreamSeekMode_::kIBSeekSet => 0,
            IBStream_::IStreamSeekMode_::kIBSeekCur => *cur as i64,
            IBStream_::IStreamSeekMode_::kIBSeekEnd => data.len() as i64,
            _ => return kInvalidArgument,
        };
        let Some(np) = base
            .checked_add(pos)
            .filter(|p| (0..=MAX_STREAM_BYTES as i64).contains(p))
        else {
            return kInvalidArgument;
        };
        *cur = np as usize;
        if !result.is_null() {
            // SAFETY: ponteiro não nulo fornecido pelo plug-in.
            unsafe { *result = np };
        }
        kResultOk
    }

    unsafe fn tell(&self, pos: *mut i64) -> tresult {
        if pos.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: ponteiro não nulo fornecido pelo plug-in.
        unsafe { *pos = lock(&self.inner).1 as i64 };
        kResultOk
    }
}

/// `IComponentHandler`: sem editor gráfico, as edições do plug-in são aceitas
/// e ignoradas.
pub(crate) struct Handler;

impl Class for Handler {
    type Interfaces = (IComponentHandler,);
}

impl IComponentHandlerTrait for Handler {
    unsafe fn beginEdit(&self, _id: ParamID) -> tresult {
        kResultOk
    }
    unsafe fn performEdit(&self, _id: ParamID, _value: ParamValue) -> tresult {
        kResultOk
    }
    unsafe fn endEdit(&self, _id: ParamID) -> tresult {
        kResultOk
    }
    unsafe fn restartComponent(&self, _flags: i32) -> tresult {
        kNotImplemented
    }
}

/// `IParamValueQueue`: pontos (deslocamento, valor) de um parâmetro.
pub(crate) struct ParamQueue {
    id: ParamID,
    points: Mutex<Vec<(i32, f64)>>,
}

impl Class for ParamQueue {
    type Interfaces = (IParamValueQueue,);
}

impl IParamValueQueueTrait for ParamQueue {
    unsafe fn getParameterId(&self) -> ParamID {
        self.id
    }

    unsafe fn getPointCount(&self) -> i32 {
        lock(&self.points).len() as i32
    }

    unsafe fn getPoint(&self, index: i32, sample_offset: *mut i32, value: *mut f64) -> tresult {
        if sample_offset.is_null() || value.is_null() {
            return kInvalidArgument;
        }
        let pts = lock(&self.points);
        let Some((o, v)) = usize::try_from(index).ok().and_then(|i| pts.get(i)) else {
            return kInvalidArgument;
        };
        // SAFETY: ponteiros não nulos fornecidos pelo plug-in.
        unsafe {
            *sample_offset = *o;
            *value = *v;
        }
        kResultOk
    }

    unsafe fn addPoint(&self, sample_offset: i32, value: f64, index: *mut i32) -> tresult {
        let mut pts = lock(&self.points);
        if pts.len() >= MAX_POINTS {
            return kResultFalse;
        }
        let v = if value.is_finite() {
            value.clamp(0.0, 1.0)
        } else {
            0.0
        };
        pts.push((sample_offset.max(0), v));
        if !index.is_null() {
            // SAFETY: ponteiro não nulo fornecido pelo plug-in.
            unsafe { *index = (pts.len() - 1) as i32 };
        }
        kResultOk
    }
}

/// `IParameterChanges`: uma fila por parâmetro alterado no bloco.
#[derive(Default)]
pub(crate) struct ParamChanges {
    queues: Mutex<Vec<ComWrapper<ParamQueue>>>,
}

impl Class for ParamChanges {
    type Interfaces = (IParameterChanges,);
}

impl ParamChanges {
    fn with_values(values: &[(ParamID, f64)]) -> Self {
        let queues = values
            .iter()
            .map(|(id, v)| {
                ComWrapper::new(ParamQueue {
                    id: *id,
                    points: Mutex::new(vec![(0, *v)]),
                })
            })
            .collect();
        ParamChanges {
            queues: Mutex::new(queues),
        }
    }
}

impl IParameterChangesTrait for ParamChanges {
    unsafe fn getParameterCount(&self) -> i32 {
        lock(&self.queues).len() as i32
    }

    unsafe fn getParameterData(&self, index: i32) -> *mut IParamValueQueue {
        // Sem `addRef`, como no SDK: a fila vive enquanto a lista viver.
        let q = lock(&self.queues);
        usize::try_from(index)
            .ok()
            .and_then(|i| q.get(i))
            .and_then(|w| w.as_com_ref::<IParamValueQueue>())
            .map(|r| r.as_ptr())
            .unwrap_or(ptr::null_mut())
    }

    unsafe fn addParameterData(
        &self,
        id: *const ParamID,
        index: *mut i32,
    ) -> *mut IParamValueQueue {
        if id.is_null() {
            return ptr::null_mut();
        }
        // SAFETY: `id` não é nulo.
        let id = unsafe { *id };
        let mut q = lock(&self.queues);
        let pos = match q.iter().position(|w| w.id == id) {
            Some(p) => p,
            None if q.len() < MAX_PARAMS as usize => {
                q.push(ComWrapper::new(ParamQueue {
                    id,
                    points: Mutex::new(vec![]),
                }));
                q.len() - 1
            }
            None => return ptr::null_mut(),
        };
        if !index.is_null() {
            // SAFETY: ponteiro não nulo fornecido pelo plug-in.
            unsafe { *index = pos as i32 };
        }
        q.get(pos)
            .and_then(|w| w.as_com_ref::<IParamValueQueue>())
            .map(|r| r.as_ptr())
            .unwrap_or(ptr::null_mut())
    }
}

// ---------------------------------------------------------------------------
// Efeito hospedado
// ---------------------------------------------------------------------------

/// Buffers de um barramento: um vetor por canal e os ponteiros para eles.
struct BusBufs {
    chans: Vec<Vec<f32>>,
    ptrs: Vec<*mut f32>,
}

impl BusBufs {
    fn new(channels: usize) -> Self {
        BusBufs {
            chans: vec![vec![0.0; MAX_BLOCK]; channels],
            ptrs: vec![],
        }
    }

    fn refresh_ptrs(&mut self) {
        self.ptrs = self.chans.iter_mut().map(|c| c.as_mut_ptr()).collect();
    }

    /// `AudioBusBuffers` apontando para os canais (vale até o próximo
    /// `refresh_ptrs`).
    fn raw(&mut self) -> AudioBusBuffers {
        AudioBusBuffers {
            numChannels: self.ptrs.len() as i32,
            silenceFlags: 0,
            __field0: AudioBusBuffers__type0 {
                channelBuffers32: if self.ptrs.is_empty() {
                    ptr::null_mut()
                } else {
                    self.ptrs.as_mut_ptr()
                },
            },
        }
    }
}

fn bus_channels(component: &ComPtr<IComponent>, dir: BusDirection, index: i32) -> usize {
    // SAFETY: `BusInfo` é POD; zeros são um valor válido.
    let mut info: BusInfo = unsafe { std::mem::zeroed() };
    // SAFETY: componente válido; `info` é gravável.
    let r =
        unsafe { component.getBusInfo(MediaTypes_::kAudio as MediaType, dir, index, &mut info) };
    if r == kResultOk {
        info.channelCount.clamp(0, MAX_CHANNELS) as usize
    } else {
        2
    }
}

/// Parâmetros a enviar no próximo bloco: os de `values` que mudaram desde o
/// último envio (`sent`) e, para os que saíram da lista (ex.: "Valores
/// padrão"), o valor que o plug-in tinha ao abrir. Atualiza `sent`.
fn param_changes(
    sent: &mut HashMap<ParamID, f64>,
    values: &[Vst3ParamValue],
    params: &[Vst3ParamInfo],
) -> Vec<(ParamID, f64)> {
    let mut changed: Vec<(ParamID, f64)> = Vec::new();
    for p in values.iter().take(MAX_PARAMS as usize) {
        if !p.value.is_finite() {
            continue;
        }
        let v = p.value.clamp(0.0, 1.0);
        if sent.get(&p.id) != Some(&v) {
            sent.insert(p.id, v);
            changed.push((p.id, v));
        }
    }
    let removed: Vec<ParamID> = sent
        .keys()
        .filter(|id| !values.iter().any(|p| p.id == **id))
        .copied()
        .collect();
    for id in removed {
        sent.remove(&id);
        if let Some(info) = params.iter().find(|p| p.id == id) {
            changed.push((id, info.current));
        }
    }
    changed
}

/// Efeito VST3 aberto, pronto para processar blocos estéreo intercalados.
pub struct HostedVst3 {
    component: ComPtr<IComponent>,
    processor: ComPtr<IAudioProcessor>,
    controller: Option<ComPtr<IEditController>>,
    separate_controller: bool,
    connection: Option<(ComPtr<IConnectionPoint>, ComPtr<IConnectionPoint>)>,
    /// O plug-in guarda o ponteiro do host recebido em `initialize`; o
    /// objeto precisa viver tanto quanto o componente.
    _host: ComWrapper<HostApp>,
    handler: ComWrapper<Handler>,
    params: Vec<Vst3ParamInfo>,
    sent: HashMap<ParamID, f64>,
    inputs: Vec<BusBufs>,
    outputs: Vec<BusBufs>,
    sample_rate: f64,
    pos: i64,
    latency: u32,
    active: bool,
    // por último: a biblioteca só some depois dos objetos acima
    module: Module,
}

impl HostedVst3 {
    /// Abre a classe `class_id` (hex) do módulo `path` em `sample_rate`.
    pub fn open(path: &Path, class_id: &str, sample_rate: u32) -> Result<Self, String> {
        if !(8000..=768_000).contains(&sample_rate) {
            return Err(format!("taxa de amostragem inválida: {sample_rate}"));
        }
        let cid = hex_to_cid(class_id).ok_or("identificador de classe VST3 inválido")?;
        let module = Module::load(path)?;
        let component: ComPtr<IComponent> = module
            .create(&cid)
            .ok_or("o plug-in não criou o componente pedido")?;
        let host = ComWrapper::new(HostApp);
        let handler = ComWrapper::new(Handler);
        let host_ptr = host
            .as_com_ref::<IHostApplication>()
            .map(|r| r.as_ptr() as *mut FUnknown)
            .ok_or("host VST3 indisponível")?;
        // SAFETY: componente recém-criado; `host_ptr` vive enquanto `host`.
        if unsafe { component.initialize(host_ptr) } != kResultOk {
            return Err("o componente VST3 não inicializou".into());
        }
        let mut fx = HostedVst3 {
            processor: match component.cast::<IAudioProcessor>() {
                Some(p) => p,
                None => {
                    // SAFETY: componente inicializado acima.
                    unsafe { component.terminate() };
                    return Err("a classe não é um processador de áudio".into());
                }
            },
            component,
            controller: None,
            separate_controller: false,
            connection: None,
            _host: host,
            handler,
            params: vec![],
            sent: HashMap::new(),
            inputs: vec![],
            outputs: vec![],
            sample_rate: f64::from(sample_rate),
            pos: 0,
            latency: 0,
            active: false,
            module,
        };
        fx.open_controller(host_ptr);
        fx.setup_buses()?;
        fx.activate()?;
        fx.params = fx.read_params();
        Ok(fx)
    }

    /// Cria e conecta o controlador de edição, se houver. Sem controlador o
    /// efeito processa, mas não expõe parâmetros.
    fn open_controller(&mut self, host_ptr: *mut FUnknown) {
        if let Some(c) = self.component.cast::<IEditController>() {
            self.controller = Some(c);
        } else {
            let mut ccid: TUID = [0; 16];
            // SAFETY: componente válido; `ccid` tem 16 bytes graváveis.
            if unsafe { self.component.getControllerClassId(&mut ccid) } != kResultOk {
                return;
            }
            let Some(c) = self.module.create::<IEditController>(&ccid) else {
                return;
            };
            // SAFETY: controlador recém-criado; `host_ptr` vive com `self.host`.
            if unsafe { c.initialize(host_ptr) } != kResultOk {
                return;
            }
            self.separate_controller = true;
            if let (Some(a), Some(b)) = (
                self.component.cast::<IConnectionPoint>(),
                c.cast::<IConnectionPoint>(),
            ) {
                // SAFETY: os dois pontos de conexão são válidos.
                unsafe {
                    a.connect(b.as_ptr());
                    b.connect(a.as_ptr());
                }
                self.connection = Some((a, b));
            }
            self.controller = Some(c);
        }
        let Some(c) = &self.controller else {
            return;
        };
        if let Some(h) = self.handler.to_com_ptr::<IComponentHandler>() {
            // SAFETY: o controlador guarda a referência; `h` é um objeto COM
            // válido com contagem própria.
            unsafe { c.setComponentHandler(h.as_ptr()) };
        }
        // Sincroniza o controlador com o estado do processador.
        let stream = ComWrapper::new(MemStream::default());
        if let Some(s) = stream.to_com_ptr::<IBStream>() {
            // SAFETY: `s` é um fluxo válido durante as duas chamadas.
            unsafe {
                if self.component.getState(s.as_ptr()) == kResultOk {
                    s.seek(0, IBStream_::IStreamSeekMode_::kIBSeekSet, ptr::null_mut());
                    c.setComponentState(s.as_ptr());
                }
            }
        }
    }

    fn setup_buses(&mut self) -> Result<(), String> {
        let audio = MediaTypes_::kAudio as MediaType;
        let (din, dout) = (
            BusDirections_::kInput as BusDirection,
            BusDirections_::kOutput as BusDirection,
        );
        // SAFETY: componente válido.
        let (n_in, n_out) = unsafe {
            (
                self.component.getBusCount(audio, din).clamp(0, MAX_BUSES),
                self.component.getBusCount(audio, dout).clamp(0, MAX_BUSES),
            )
        };
        if n_out < 1 {
            return Err("o plug-in não tem saída de áudio".into());
        }
        let mut ins = vec![SpeakerArr::kStereo; n_in as usize];
        let mut outs = vec![SpeakerArr::kStereo; n_out as usize];
        // SAFETY: os vetores têm `n_in`/`n_out` elementos graváveis. Falha
        // aqui não é fatal: o plug-in fica no arranjo que ele preferir.
        unsafe {
            self.processor
                .setBusArrangements(ins.as_mut_ptr(), n_in, outs.as_mut_ptr(), n_out);
            for i in 0..n_in {
                self.component.activateBus(audio, din, i, u8::from(i == 0));
            }
            for i in 0..n_out {
                self.component.activateBus(audio, dout, i, u8::from(i == 0));
            }
        }
        self.inputs = (0..n_in)
            .map(|i| BusBufs::new(bus_channels(&self.component, din, i)))
            .collect();
        self.outputs = (0..n_out)
            .map(|i| BusBufs::new(bus_channels(&self.component, dout, i)))
            .collect();
        // SAFETY: processador válido.
        if unsafe {
            self.processor
                .canProcessSampleSize(SymbolicSampleSizes_::kSample32)
        } != kResultOk
        {
            return Err("o plug-in não processa amostras de 32 bits".into());
        }
        let mut setup = ProcessSetup {
            processMode: ProcessModes_::kRealtime,
            symbolicSampleSize: SymbolicSampleSizes_::kSample32,
            maxSamplesPerBlock: MAX_BLOCK as i32,
            sampleRate: self.sample_rate,
        };
        // SAFETY: `setup` é válido durante a chamada.
        if unsafe { self.processor.setupProcessing(&mut setup) } != kResultOk {
            return Err("o plug-in recusou a configuração de processamento".into());
        }
        Ok(())
    }

    fn activate(&mut self) -> Result<(), String> {
        // SAFETY: componente configurado em `setup_buses`.
        if unsafe { self.component.setActive(1) } != kResultOk {
            return Err("o plug-in não ativou".into());
        }
        // SAFETY: processador ativo. Vários plug-ins devolvem
        // `kNotImplemented` aqui, o que é aceito.
        unsafe {
            self.processor.setProcessing(1);
            self.latency = self.processor.getLatencySamples();
        }
        self.active = true;
        Ok(())
    }

    fn deactivate(&mut self) {
        if self.active {
            // SAFETY: componente ativo.
            unsafe {
                self.processor.setProcessing(0);
                self.component.setActive(0);
            }
            self.active = false;
        }
    }

    /// Zera o estado interno (caudas de reverb e delay) após um salto.
    pub fn reset(&mut self) -> Result<(), String> {
        self.deactivate();
        self.pos = 0;
        self.activate()
    }

    fn read_params(&self) -> Vec<Vst3ParamInfo> {
        let Some(c) = &self.controller else {
            return vec![];
        };
        // SAFETY: controlador válido.
        let n = unsafe { c.getParameterCount() }.clamp(0, MAX_PARAMS);
        let mut out = Vec::new();
        for i in 0..n {
            // SAFETY: `ParameterInfo` é POD; zeros são um valor válido.
            let mut info: ParameterInfo = unsafe { std::mem::zeroed() };
            // SAFETY: `i` em `0..n` e `info` é gravável.
            if unsafe { c.getParameterInfo(i, &mut info) } != kResultOk {
                continue;
            }
            let flags = info.flags;
            let has = |f: i32| flags & f != 0;
            if has(ParameterInfo_::ParameterFlags_::kIsHidden)
                || has(ParameterInfo_::ParameterFlags_::kIsReadOnly)
            {
                continue;
            }
            // SAFETY: id devolvido pelo próprio controlador.
            let current = unsafe { c.getParamNormalized(info.id) };
            let norm = |v: f64| {
                if v.is_finite() {
                    v.clamp(0.0, 1.0)
                } else {
                    0.0
                }
            };
            out.push(Vst3ParamInfo {
                id: info.id,
                title: str128(&info.title),
                units: str128(&info.units),
                step_count: info.stepCount.max(0),
                default: norm(info.defaultNormalizedValue),
                current: norm(current),
                bypass: has(ParameterInfo_::ParameterFlags_::kIsBypass),
            });
        }
        out
    }

    pub fn params(&self) -> &[Vst3ParamInfo] {
        &self.params
    }

    pub fn latency(&self) -> u32 {
        self.latency
    }

    /// Serializa o estado completo do componente (`IComponent::getState`).
    pub fn state_bytes(&self) -> Result<Vec<u8>, String> {
        let stream = ComWrapper::new(MemStream::default());
        let Some(s) = stream.to_com_ptr::<IBStream>() else {
            return Err("não foi possível alocar stream de estado VST3".into());
        };
        // SAFETY: `component` e `s` válidos durante a chamada.
        let st = unsafe { self.component.getState(s.as_ptr()) };
        if st != kResultOk {
            return Err(format!(
                "plug-in não expõe estado serializável (status {st})"
            ));
        }
        Ok(stream.bytes())
    }

    /// Restaura o estado completo do componente (`IComponent::setState`) e sincroniza o controlador.
    pub fn set_state_bytes(&mut self, bytes: &[u8]) -> Result<(), String> {
        if bytes.len() > MAX_STREAM_BYTES {
            return Err("estado VST3 excede o tamanho máximo".into());
        }
        let was_active = self.active;
        if was_active {
            self.deactivate();
        }
        let stream = ComWrapper::new(MemStream::from_bytes(bytes));
        let Some(s) = stream.to_com_ptr::<IBStream>() else {
            if was_active {
                let _ = self.activate();
            }
            return Err("não foi possível preparar stream de estado VST3".into());
        };
        // SAFETY: `component` e `s` são válidos durante a restauração.
        let st = unsafe { self.component.setState(s.as_ptr()) };
        if st != kResultOk && st != kNotImplemented {
            if was_active {
                let _ = self.activate();
            }
            return Err(format!(
                "plug-in recusou restauração de estado (status {st})"
            ));
        }
        if let Some(c) = &self.controller {
            // SAFETY: reposiciona o stream e sincroniza a UI/controlador.
            unsafe {
                s.seek(0, IBStream_::IStreamSeekMode_::kIBSeekSet, ptr::null_mut());
                c.setComponentState(s.as_ptr());
            }
        }
        self.sent.clear();
        if was_active {
            self.activate()?;
        }
        self.params = self.read_params();
        Ok(())
    }

    /// Texto do plug-in para o valor normalizado de um parâmetro (ex.: "-6.0 dB").
    pub fn param_text(&self, id: u32, value: f64) -> Option<String> {
        let c = self.controller.as_ref()?;
        let mut s: String128 = [0; 128];
        let v = if value.is_finite() {
            value.clamp(0.0, 1.0)
        } else {
            0.0
        };
        // SAFETY: `s` é um `String128` gravável.
        if unsafe { c.getParamStringByValue(id, v, &mut s) } != kResultOk {
            return None;
        }
        Some(str128(&s))
    }

    /// Processa `buf` (estéreo intercalado, até [`MAX_BLOCK`] quadros) com os
    /// valores `values`. Valores não finitos na saída viram zero.
    pub fn process(&mut self, buf: &mut [f32], values: &[Vst3ParamValue]) -> Result<(), String> {
        let frames = buf.len() / 2;
        if frames == 0 {
            return Ok(());
        }
        if frames > MAX_BLOCK || !buf.len().is_multiple_of(2) {
            return Err("bloco de áudio inválido".into());
        }
        if !self.active {
            return Err("plug-in inativo".into());
        }
        let changed = param_changes(&mut self.sent, values, &self.params);
        for (id, v) in &changed {
            if let Some(c) = &self.controller {
                // SAFETY: controlador válido; mantém a UI do plug-in em dia.
                unsafe { c.setParamNormalized(*id, *v) };
            }
        }
        let in_changes = ComWrapper::new(ParamChanges::with_values(&changed));
        let out_changes = ComWrapper::new(ParamChanges::default());

        // Entrada: L/R no barramento principal (mono = média); o resto em zero.
        for (bi, bus) in self.inputs.iter_mut().enumerate() {
            let nch = bus.chans.len();
            for (ci, ch) in bus.chans.iter_mut().enumerate() {
                let Some(dst) = ch.get_mut(..frames) else {
                    return Err("buffer interno curto".into());
                };
                for (i, d) in dst.iter_mut().enumerate() {
                    let (l, r) = (buf[i * 2], buf[i * 2 + 1]);
                    *d = match (bi, nch, ci) {
                        (0, 1, 0) => 0.5 * (l + r),
                        (0, _, 0) => l,
                        (0, _, 1) => r,
                        _ => 0.0,
                    };
                }
            }
            bus.refresh_ptrs();
        }
        for bus in self.outputs.iter_mut() {
            for ch in bus.chans.iter_mut() {
                if let Some(c) = ch.get_mut(..frames) {
                    c.fill(0.0);
                }
            }
            bus.refresh_ptrs();
        }
        let mut in_bufs: Vec<AudioBusBuffers> = self.inputs.iter_mut().map(BusBufs::raw).collect();
        let mut out_bufs: Vec<AudioBusBuffers> =
            self.outputs.iter_mut().map(BusBufs::raw).collect();

        // SAFETY: `ProcessContext` é POD; zeros são um valor válido.
        let mut ctx: ProcessContext = unsafe { std::mem::zeroed() };
        ctx.state = (ProcessContext_::StatesAndFlags_::kPlaying
            | ProcessContext_::StatesAndFlags_::kTempoValid
            | ProcessContext_::StatesAndFlags_::kTimeSigValid) as u32;
        ctx.sampleRate = self.sample_rate;
        ctx.projectTimeSamples = self.pos;
        ctx.continousTimeSamples = self.pos;
        ctx.tempo = 120.0;
        ctx.timeSigNumerator = 4;
        ctx.timeSigDenominator = 4;

        let in_ptr = in_changes
            .as_com_ref::<IParameterChanges>()
            .map(|r| r.as_ptr())
            .unwrap_or(ptr::null_mut());
        let out_ptr = out_changes
            .as_com_ref::<IParameterChanges>()
            .map(|r| r.as_ptr())
            .unwrap_or(ptr::null_mut());
        let mut data = ProcessData {
            processMode: ProcessModes_::kRealtime,
            symbolicSampleSize: SymbolicSampleSizes_::kSample32,
            numSamples: frames as i32,
            numInputs: in_bufs.len() as i32,
            numOutputs: out_bufs.len() as i32,
            inputs: if in_bufs.is_empty() {
                ptr::null_mut()
            } else {
                in_bufs.as_mut_ptr()
            },
            outputs: out_bufs.as_mut_ptr(),
            inputParameterChanges: in_ptr,
            outputParameterChanges: out_ptr,
            inputEvents: ptr::null_mut(),
            outputEvents: ptr::null_mut(),
            processContext: &mut ctx,
        };
        // SAFETY: todos os ponteiros de `data` apontam para buffers com
        // `frames` <= MAX_BLOCK amostras por canal, vivos até o fim da chamada.
        let r = unsafe { self.processor.process(&mut data) };
        if r != kResultOk {
            return Err(format!("o plug-in recusou o bloco (código {r})"));
        }
        self.pos = self.pos.saturating_add(frames as i64);

        let Some(main) = self.outputs.first() else {
            return Err("sem barramento de saída".into());
        };
        let clean = |v: f32| if v.is_finite() { v } else { 0.0 };
        match main.chans.as_slice() {
            [] => buf.fill(0.0),
            [m] => {
                for (i, s) in m.iter().take(frames).enumerate() {
                    buf[i * 2] = clean(*s);
                    buf[i * 2 + 1] = clean(*s);
                }
            }
            [l, r, ..] => {
                for i in 0..frames {
                    buf[i * 2] = clean(l[i]);
                    buf[i * 2 + 1] = clean(r[i]);
                }
            }
        }
        Ok(())
    }
}

impl Drop for HostedVst3 {
    fn drop(&mut self) {
        self.deactivate();
        if let Some((a, b)) = self.connection.take() {
            // SAFETY: os dois pontos foram conectados em `open_controller`.
            unsafe {
                a.disconnect(b.as_ptr());
                b.disconnect(a.as_ptr());
            }
        }
        if let Some(c) = self.controller.take() {
            if self.separate_controller {
                // SAFETY: controlador inicializado por este host.
                unsafe { c.terminate() };
            }
        }
        // SAFETY: componente inicializado por este host.
        unsafe { self.component.terminate() };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vst3::ComRef;

    #[test]
    fn param_changes_send_only_deltas_and_revert_removed_params() {
        let info = |id: u32, current: f64| Vst3ParamInfo {
            id,
            title: String::new(),
            units: String::new(),
            step_count: 0,
            default: current,
            current,
            bypass: false,
        };
        let params = [info(1, 0.2), info(2, 0.7)];
        let v = |id: u32, value: f64| Vst3ParamValue { id, value };
        let mut sent = HashMap::new();
        // primeiro bloco: tudo é novo; valores hostis saneados ou ignorados
        let c = param_changes(&mut sent, &[v(1, 0.5), v(2, 9.0), v(3, f64::NAN)], &params);
        assert_eq!(c, vec![(1, 0.5), (2, 1.0)]);
        // nada mudou: nada a enviar
        assert!(param_changes(&mut sent, &[v(1, 0.5), v(2, 1.0)], &params).is_empty());
        // "Valores padrão": a lista esvazia e os dois voltam ao valor de abertura
        let mut c = param_changes(&mut sent, &[], &params);
        c.sort_by_key(|x| x.0);
        assert_eq!(c, vec![(1, 0.2), (2, 0.7)]);
        // e não são reenviados nos blocos seguintes
        assert!(param_changes(&mut sent, &[], &params).is_empty());
        assert!(sent.is_empty());
    }

    #[test]
    fn cid_hex_roundtrip_and_hostile_input() {
        let cid: TUID = [0x12, 0x34, 0x56, 0x78, -1, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let h = cid_to_hex(&cid);
        assert_eq!(h.len(), 32);
        assert_eq!(hex_to_cid(&h), Some(cid));
        assert_eq!(hex_to_cid(""), None);
        assert_eq!(hex_to_cid("zz"), None);
        assert_eq!(hex_to_cid(&"Z".repeat(32)), None);
        assert_eq!(hex_to_cid(&"é".repeat(16)), None);
    }

    #[test]
    fn mem_stream_reads_writes_and_bounds_seek() {
        let w = ComWrapper::new(MemStream::default());
        let s = w.to_com_ptr::<IBStream>().expect("IBStream");
        let mut data = *b"abcdef";
        let mut n = 0;
        unsafe {
            assert_eq!(
                s.write(data.as_mut_ptr() as *mut c_void, 6, &mut n),
                kResultOk
            );
            assert_eq!(n, 6);
            let mut p = 0;
            assert_eq!(
                s.seek(2, IBStream_::IStreamSeekMode_::kIBSeekSet, &mut p),
                kResultOk
            );
            assert_eq!(p, 2);
            let mut out = [0u8; 10];
            assert_eq!(
                s.read(out.as_mut_ptr() as *mut c_void, 10, &mut n),
                kResultOk
            );
            assert_eq!(n, 4);
            assert_eq!(&out[..4], b"cdef");
            // além do fim: lê zero bytes sem estourar
            assert_eq!(
                s.read(out.as_mut_ptr() as *mut c_void, 10, &mut n),
                kResultOk
            );
            assert_eq!(n, 0);
            // posições hostis
            assert_ne!(
                s.seek(-1, IBStream_::IStreamSeekMode_::kIBSeekSet, &mut p),
                kResultOk
            );
            assert_ne!(
                s.seek(i64::MAX, IBStream_::IStreamSeekMode_::kIBSeekCur, &mut p),
                kResultOk
            );
            assert_ne!(s.seek(0, 99, &mut p), kResultOk);
            assert_ne!(s.read(ptr::null_mut(), 4, &mut n), kResultOk);
            assert_ne!(
                s.write(data.as_mut_ptr() as *mut c_void, -1, &mut n),
                kResultOk
            );
        }
        assert_eq!(w.bytes(), b"abcdef");

        let restored = ComWrapper::new(MemStream::from_bytes(b"xyz"));
        let rs = restored.to_com_ptr::<IBStream>().expect("IBStream");
        let mut rbuf = [0u8; 4];
        let mut rn = 0;
        unsafe {
            assert_eq!(
                rs.read(rbuf.as_mut_ptr() as *mut c_void, 4, &mut rn),
                kResultOk
            );
        }
        assert_eq!(rn, 3);
        assert_eq!(&rbuf[..3], b"xyz");
    }

    #[test]
    fn param_changes_expose_points_and_reject_bad_indices() {
        let w = ComWrapper::new(ParamChanges::with_values(&[(7, 0.25), (9, 2.0)]));
        let pc = w
            .to_com_ptr::<IParameterChanges>()
            .expect("IParameterChanges");
        unsafe {
            assert_eq!(pc.getParameterCount(), 2);
            assert!(pc.getParameterData(-1).is_null());
            assert!(pc.getParameterData(2).is_null());
            let q = ComRef::from_raw(pc.getParameterData(0)).expect("fila 0");
            assert_eq!(q.getParameterId(), 7);
            let (mut o, mut v) = (0, 0.0);
            assert_eq!(q.getPoint(0, &mut o, &mut v), kResultOk);
            assert_eq!((o, v), (0, 0.25));
            assert_ne!(q.getPoint(1, &mut o, &mut v), kResultOk);
            // saída: o plug-in adiciona filas e pontos
            let id: ParamID = 3;
            let mut idx = -1;
            let q2 = ComRef::from_raw(pc.addParameterData(&id, &mut idx)).expect("nova fila");
            assert_eq!(idx, 2);
            assert_eq!(q2.addPoint(5, f64::NAN, &mut idx), kResultOk);
            assert_eq!(q2.getPoint(0, &mut o, &mut v), kResultOk);
            assert_eq!((o, v), (5, 0.0));
            assert!(pc.addParameterData(ptr::null(), &mut idx).is_null());
        }
    }

    #[test]
    fn host_creates_messages_with_attribute_lists() {
        let host = ComWrapper::new(HostApp);
        let h = host.to_com_ptr::<IHostApplication>().expect("host");
        unsafe {
            let mut name: String128 = [1; 128];
            assert_eq!(h.getName(&mut name), kResultOk);
            assert_eq!(str128(&name), "Neoveg");

            let mut cid: TUID = [0; 16];
            for (d, s) in cid.iter_mut().zip(IMessage::IID.iter()) {
                *d = *s as c_char;
            }
            let mut obj: *mut c_void = ptr::null_mut();
            assert_eq!(h.createInstance(&mut cid, &mut cid, &mut obj), kResultOk);
            let msg = ComPtr::from_raw(obj as *mut IMessage).expect("IMessage");
            msg.setMessageID(c"Teste".as_ptr());
            assert_eq!(
                fixed_cstr(std::slice::from_raw_parts(msg.getMessageID(), 6)),
                "Teste"
            );
            let attrs = ComRef::from_raw(msg.getAttributes()).expect("atributos");
            assert_eq!(attrs.setInt(c"n".as_ptr(), 42), kResultOk);
            let mut n = 0;
            assert_eq!(attrs.getInt(c"n".as_ptr(), &mut n), kResultOk);
            assert_eq!(n, 42);
            // string truncada ao espaço do plug-in, sempre com NUL
            let txt: Vec<u16> = "abcdef".encode_utf16().chain([0]).collect();
            assert_eq!(attrs.setString(c"s".as_ptr(), txt.as_ptr()), kResultOk);
            let mut small = [0xFFFFu16; 3];
            assert_eq!(
                attrs.getString(c"s".as_ptr(), small.as_mut_ptr(), 6),
                kResultOk
            );
            assert_eq!(small, [b'a' as u16, b'b' as u16, 0]);
            // tipo errado ou ausente
            let mut f = 0.0;
            assert_ne!(attrs.getFloat(c"n".as_ptr(), &mut f), kResultOk);
            assert_ne!(attrs.getInt(c"falta".as_ptr(), &mut n), kResultOk);
            assert_ne!(attrs.setInt(ptr::null(), 1), kResultOk);
            // classe desconhecida: nada criado
            let mut other: TUID = [7; 16];
            assert_eq!(
                h.createInstance(&mut other, &mut other, &mut obj),
                kResultFalse
            );
            assert!(obj.is_null());
        }
    }

    #[test]
    fn opening_a_missing_or_foreign_module_is_an_error() {
        assert!(probe(Path::new("nao-existe.vst3")).is_err());
        assert!(HostedVst3::open(Path::new("nao-existe.vst3"), &"0".repeat(32), 48000).is_err());
        assert!(HostedVst3::open(Path::new("x.vst3"), "ruim", 48000).is_err());
        assert!(HostedVst3::open(Path::new("x.vst3"), &"0".repeat(32), 0).is_err());
        // uma DLL do sistema que não exporta GetPluginFactory
        let err = probe(Path::new("kernel32.dll")).expect_err("não é VST3");
        assert!(err.contains("GetPluginFactory"), "{err}");
    }
}
