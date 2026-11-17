//! Fronteira FFI isolada para Windows Media Foundation (AGENTS.md §0.3 e §2.3).
//!
//! Decodifica contêineres como MP4/MOV/MKV/M4V/M4A/AAC via API nativa do
//! sistema operacional, em modo streaming sob demanda para respeitar limites de
//! hardware (§3.6) e protegido contra falhas e entradas hostis (§0.4 e §0.6).

#![allow(unsafe_code)]

pub(crate) mod encode;

use crate::media::audio::{AudioData, MAX_SAMPLES};
use crate::model::{FrameRate, Ticks, MAX_DIM, TICKS_PER_SEC};
use anyhow::{anyhow, bail, Result};
use image::RgbaImage;
use std::path::Path;
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::{Arc, Mutex};
use windows::core::{GUID, HSTRING, PCWSTR, PROPVARIANT};
use windows::Win32::Foundation::{RPC_E_CHANGED_MODE, S_FALSE, S_OK};
use windows::Win32::Media::MediaFoundation::{
    IMFMediaBuffer, IMFSample, IMFSourceReader, MFAudioFormat_Float, MFAudioFormat_PCM,
    MFCreateAttributes, MFCreateMediaType, MFCreateSourceReaderFromURL, MFMediaType_Audio,
    MFMediaType_Video, MFShutdown, MFStartup, MFVideoFormat_RGB32, MFSTARTUP_NOSOCKET,
    MF_MT_AUDIO_NUM_CHANNELS, MF_MT_AUDIO_SAMPLES_PER_SECOND, MF_MT_DEFAULT_STRIDE,
    MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_PD_DURATION,
    MF_SOURCE_READERF_ENDOFSTREAM, MF_SOURCE_READERF_ERROR,
    MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, MF_SOURCE_READER_FIRST_AUDIO_STREAM,
    MF_SOURCE_READER_FIRST_VIDEO_STREAM, MF_SOURCE_READER_MEDIASOURCE, MF_VERSION,
};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

pub(super) const HNS_PER_SEC: i128 = 10_000_000;

fn hns_to_ticks(hns: i64) -> Ticks {
    let clamped = hns.max(0) as i128;
    let t = (clamped * TICKS_PER_SEC as i128) / HNS_PER_SEC;
    Ticks::from_raw(t.min(i64::MAX as i128) as i64)
}

pub(super) fn ticks_to_hns(t: Ticks) -> i64 {
    let raw = t.0.max(0) as i128;
    ((raw * HNS_PER_SEC) / TICKS_PER_SEC as i128).min(i64::MAX as i128) as i64
}

/// Garante ciclo de vida RAII de COM MTA e Media Foundation na thread atual.
pub(super) struct MfGuard {
    uninit_com: bool,
    shutdown_mf: bool,
}

impl MfGuard {
    pub(super) fn enter() -> Result<Self> {
        // SAFETY: `CoInitializeEx` com `COINIT_MULTITHREADED` e `pvReserved = None`
        // segue a especificação Win32; se a thread já estiver inicializada com outro
        // apartamento (`RPC_E_CHANGED_MODE`), não devemos chamar `CoUninitialize`.
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let uninit_com = if hr == S_OK || hr == S_FALSE {
            true
        } else if hr == RPC_E_CHANGED_MODE {
            false
        } else {
            return Err(anyhow!("CoInitializeEx falhou: HRESULT {hr:?}"));
        };

        // SAFETY: `MFStartup` é pareado com `MFShutdown` no `Drop` desta guarda.
        let mf_res = unsafe { MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET) };
        if let Err(e) = mf_res {
            if uninit_com {
                // SAFETY: pareado com `CoInitializeEx` bem-sucedido logo acima.
                unsafe { CoUninitialize() };
            }
            return Err(anyhow!("MFStartup falhou: {e}"));
        }

        Ok(Self {
            uninit_com,
            shutdown_mf: true,
        })
    }
}

impl Drop for MfGuard {
    fn drop(&mut self) {
        if self.shutdown_mf {
            // SAFETY: pareado com `MFStartup` bem-sucedido na construção de `MfGuard`.
            let _ = unsafe { MFShutdown() };
        }
        if self.uninit_com {
            // SAFETY: pareado com `CoInitializeEx` que retornou `S_OK` ou `S_FALSE`.
            unsafe { CoUninitialize() };
        }
    }
}

/// Libera automaticamente o `IMFMediaBuffer` bloqueado por `IMFMediaBuffer::Lock`.
struct LockedBuffer<'a> {
    buf: &'a IMFMediaBuffer,
    bytes: &'a [u8],
}

impl<'a> LockedBuffer<'a> {
    fn lock(buf: &'a IMFMediaBuffer, max_expected: usize) -> Result<Self> {
        let mut ptr: *mut u8 = std::ptr::null_mut();
        let mut cur_len: u32 = 0;
        // SAFETY: `ptr` e `cur_len` são ponteiros válidos na stack e `Unlock`
        // é garantido pelo `Drop` de `LockedBuffer` antes de `buf` sair de escopo.
        unsafe { buf.Lock(&mut ptr, None, Some(&mut cur_len)) }
            .map_err(|e| anyhow!("IMFMediaBuffer::Lock falhou: {e}"))?;
        if ptr.is_null() {
            // SAFETY: `Lock` retornou Ok; desbloqueia antes de retornar erro.
            let _ = unsafe { buf.Unlock() };
            bail!("buffer nulo retornado pelo Media Foundation");
        }
        let len = (cur_len as usize).min(max_expected);
        // SAFETY: `ptr` permanece válido por pelo menos `cur_len` bytes enquanto
        // este `LockedBuffer` mantiver o bloqueio até `Unlock()` no `Drop`.
        let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
        Ok(Self { buf, bytes })
    }
}

impl Drop for LockedBuffer<'_> {
    fn drop(&mut self) {
        // SAFETY: `Lock` teve sucesso na criação de `LockedBuffer`.
        let _ = unsafe { self.buf.Unlock() };
    }
}

fn open_source_reader(path: &Path) -> Result<IMFSourceReader> {
    let hpath = HSTRING::from(path.as_os_str());
    let mut attrs = None;
    // SAFETY: ponteiro válido para `Option<IMFAttributes>`.
    unsafe { MFCreateAttributes(&mut attrs, 1) }
        .map_err(|e| anyhow!("MFCreateAttributes falhou: {e}"))?;
    if let Some(a) = &attrs {
        // SAFETY: `a` é interface COM válida retornada por `MFCreateAttributes`.
        unsafe { a.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1) }
            .map_err(|e| anyhow!("habilitar processamento de vídeo falhou: {e}"))?;
    }
    // SAFETY: `hpath` permanece vivo durante a chamada síncrona de abertura.
    let reader = unsafe { MFCreateSourceReaderFromURL(PCWSTR(hpath.as_ptr()), attrs.as_ref()) }
        .map_err(|e| {
            anyhow!(
                "Media Foundation não conseguiu abrir '{}': {e}",
                path.display()
            )
        })?;
    Ok(reader)
}

fn read_duration(reader: &IMFSourceReader) -> Option<Ticks> {
    // SAFETY: consulta somente leitura de `MF_PD_DURATION` no objeto raiz do leitor.
    let prop = unsafe {
        reader.GetPresentationAttribute(MF_SOURCE_READER_MEDIASOURCE.0 as u32, &MF_PD_DURATION)
    }
    .ok()?;
    let hns = i64::try_from(&prop).ok()?;
    (hns > 0).then(|| hns_to_ticks(hns))
}

struct MfVideoInfo {
    width: u32,
    height: u32,
    fps: FrameRate,
    stride: i32,
}

fn configure_video_stream(reader: &IMFSourceReader) -> Option<MfVideoInfo> {
    let stream = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
    // SAFETY: consulta o tipo nativo apenas para verificar se existe faixa de vídeo.
    let native = unsafe { reader.GetNativeMediaType(stream, 0) }.ok()?;
    // SAFETY: atributo u64 empacota `(width << 32) | height` conforme especificação MF.
    let size = unsafe { native.GetUINT64(&MF_MT_FRAME_SIZE) }.ok()?;
    let width = (size >> 32) as u32;
    let height = size as u32;
    if width == 0 || height == 0 || width > MAX_DIM || height > MAX_DIM {
        return None;
    }

    // SAFETY: cria media type novo para solicitar conversão para RGB32.
    let mt = unsafe { MFCreateMediaType() }.ok()?;
    // SAFETY: `mt` recém-criado recebe MAJOR_TYPE = Video e SUBTYPE = RGB32.
    unsafe {
        mt.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).ok()?;
        mt.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32).ok()?;
        reader.SetCurrentMediaType(stream, None, &mt).ok()?;
    }

    // SAFETY: obtém o tipo negociado atual para confirmar dimensões, FPS e stride.
    let cur = unsafe { reader.GetCurrentMediaType(stream) }.ok()?;
    // SAFETY: leitura de atributos em interface COM válida.
    let frame_rate = unsafe { cur.GetUINT64(&MF_MT_FRAME_RATE) }.unwrap_or(0);
    let num = (frame_rate >> 32) as u32;
    let den = frame_rate as u32;
    let fps = FrameRate::new(num, den)
        .filter(|f| (1.0..=240.0).contains(&f.as_f64()))
        .unwrap_or(FrameRate::FPS_30);

    // SAFETY: `MF_MT_DEFAULT_STRIDE` é `u32` interpretado como `i32` (pode ser negativo
    // em superfícies bottom-up); ausência cai no stride RGBA compacto.
    let stride = unsafe { cur.GetUINT32(&MF_MT_DEFAULT_STRIDE) }
        .map(|u| u as i32)
        .unwrap_or_else(|_| (width as i32).saturating_mul(4));

    if stride == 0 || stride.unsigned_abs() < width.saturating_mul(4) {
        return None;
    }

    Some(MfVideoInfo {
        width,
        height,
        fps,
        stride,
    })
}

fn decode_rgb32_sample(sample: &IMFSample, w: u32, h: u32, stride: i32) -> Result<RgbaImage> {
    // SAFETY: `sample` é uma amostra válida retornada por `ReadSample`.
    let buffer = unsafe { sample.ConvertToContiguousBuffer() }
        .map_err(|e| anyhow!("ConvertToContiguousBuffer falhou: {e}"))?;
    let abs_stride = stride.unsigned_abs() as usize;
    let needed = abs_stride
        .checked_mul(h as usize)
        .ok_or_else(|| anyhow!("dimensão de quadro inválida no Media Foundation"))?;
    // Limita tamanho de leitura contra buffers malformados (§0.4).
    let locked = LockedBuffer::lock(&buffer, needed.saturating_add(4096))?;
    if locked.bytes.len() < needed {
        bail!(
            "buffer de vídeo incompleto: {} bytes para {w}x{h} (esperado >= {needed})",
            locked.bytes.len()
        );
    }

    let mut rgba = vec![0u8; (w as usize).saturating_mul(h as usize).saturating_mul(4)];
    let bottom_up = stride < 0;
    for y in 0..h as usize {
        let src_y = if bottom_up { h as usize - 1 - y } else { y };
        let src_row_off = src_y * abs_stride;
        let src_row = locked
            .bytes
            .get(src_row_off..src_row_off + (w as usize) * 4)
            .ok_or_else(|| anyhow!("linha de quadro fora dos limites"))?;
        let dst_row_off = y * (w as usize) * 4;
        for x in 0..w as usize {
            let si = x * 4;
            let di = dst_row_off + x * 4;
            // MFVideoFormat_RGB32 tem ordem BGRA/BGRX na memória em little-endian.
            rgba[di] = src_row[si + 2];
            rgba[di + 1] = src_row[si + 1];
            rgba[di + 2] = src_row[si];
            rgba[di + 3] = 255;
        }
    }

    RgbaImage::from_raw(w, h, rgba).ok_or_else(|| anyhow!("falha ao montar RgbaImage"))
}

enum DecodeReq {
    FrameAt {
        hns: i64,
        /// `(quadro, novo)`: `novo` é falso quando o stream já terminou e o
        /// último quadro válido foi repetido.
        reply: SyncSender<Result<(RgbaImage, bool)>>,
    },
    Shutdown,
}

/// Leitor streaming de vídeo via Windows Media Foundation executado em thread
/// COM MTA dedicada para compatibilidade segura entre UI, prévia e renderização.
pub struct MfVideoReader {
    pub width: u32,
    pub height: u32,
    pub fps: FrameRate,
    tx: Mutex<Option<SyncSender<DecodeReq>>>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl MfVideoReader {
    fn spawn(path: &Path, info: MfVideoInfo) -> Result<Self> {
        let (tx, rx) = sync_channel::<DecodeReq>(8);
        let (init_tx, init_rx) = sync_channel::<Result<()>>(1);
        let path_owned = path.to_path_buf();
        let w = info.width;
        let h = info.height;
        let mut stride = info.stride;
        let frame_hns = (HNS_PER_SEC as f64 / info.fps.as_f64().max(1.0)).round() as i64;

        let handle = std::thread::Builder::new()
            .name("mf-video".into())
            .spawn(move || {
                let _guard = match MfGuard::enter() {
                    Ok(g) => g,
                    Err(e) => {
                        let _ = init_tx.send(Err(e));
                        return;
                    }
                };
                let reader = match open_source_reader(&path_owned) {
                    Ok(r) => r,
                    Err(e) => {
                        let _ = init_tx.send(Err(e));
                        return;
                    }
                };
                if configure_video_stream(&reader).is_none() {
                    let _ = init_tx.send(Err(anyhow!("fluxo de vídeo incompatível")));
                    return;
                }
                let _ = init_tx.send(Ok(()));

                let stream = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
                let mut cur_hns: Option<i64> = None;
                let mut last_img: Option<Arc<RgbaImage>> = None;

                while let Ok(req) = rx.recv() {
                    match req {
                        DecodeReq::Shutdown => break,
                        DecodeReq::FrameAt { hns, reply } => {
                            let target = hns.max(0);
                            if let (Some(ch), Some(img)) = (cur_hns, &last_img) {
                                if (target - ch).abs() <= frame_hns / 2 {
                                    let _ = reply.send(Ok(((**img).clone(), true)));
                                    continue;
                                }
                            }

                            let need_seek = match cur_hns {
                                None => true,
                                Some(ch) => target < ch || target.saturating_sub(ch) > 10_000_000,
                            };
                            if need_seek {
                                let pos = PROPVARIANT::from(target);
                                // SAFETY: busca com `GUID::zeroed()` (formato padrão em 100 ns)
                                // usando `PROPVARIANT(VT_I8)`.
                                if let Err(e) =
                                    unsafe { reader.SetCurrentPosition(&GUID::zeroed(), &pos) }
                                {
                                    let _ = reply.send(Err(anyhow!("seek MF falhou: {e}")));
                                    continue;
                                }
                            }

                            let mut decoded: Option<RgbaImage> = None;
                            let mut read_err: Option<anyhow::Error> = None;
                            // Limite defensivo contra arquivos malformados que não avançam timestamp (§0.4).
                            for _ in 0..512 {
                                let mut flags: u32 = 0;
                                let mut ts: i64 = 0;
                                let mut sample: Option<IMFSample> = None;
                                // SAFETY: ponteiros válidos na stack e `reader` pertence a esta thread MTA.
                                let rr = unsafe {
                                    reader.ReadSample(
                                        stream,
                                        0,
                                        None,
                                        Some(&mut flags),
                                        Some(&mut ts),
                                        Some(&mut sample),
                                    )
                                };
                                if let Err(e) = rr {
                                    read_err = Some(anyhow!("ReadSample falhou: {e}"));
                                    break;
                                }
                                if (flags & MF_SOURCE_READERF_ERROR.0 as u32) != 0 {
                                    read_err = Some(anyhow!("erro no fluxo Media Foundation"));
                                    break;
                                }
                                if (flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32) != 0 {
                                    break;
                                }
                                // SAFETY: atualiza stride caso o decodificador anuncie mudança dinâmica.
                                if let Ok(cur_mt) = unsafe { reader.GetCurrentMediaType(stream) } {
                                    // SAFETY: leitura de atributo simples na interface válida.
                                    if let Ok(ns) =
                                        unsafe { cur_mt.GetUINT32(&MF_MT_DEFAULT_STRIDE) }
                                    {
                                        if ns != 0 {
                                            stride = ns as i32;
                                        }
                                    }
                                }
                                if let Some(s) = sample {
                                    if ts + frame_hns / 2 >= target || decoded.is_none() {
                                        match decode_rgb32_sample(&s, w, h, stride) {
                                            Ok(img) => {
                                                cur_hns = Some(ts);
                                                decoded = Some(img);
                                            }
                                            Err(e) => {
                                                read_err = Some(e);
                                                break;
                                            }
                                        }
                                    }
                                    if ts + frame_hns / 2 >= target {
                                        break;
                                    }
                                }
                            }

                            let res = if let Some(img) = decoded {
                                last_img = Some(Arc::new(img.clone()));
                                Ok((img, true))
                            } else if let Some(img) = &last_img {
                                // Após o último quadro (EOF suave), devolve último quadro válido.
                                Ok(((**img).clone(), false))
                            } else {
                                Err(read_err
                                    .unwrap_or_else(|| anyhow!("nenhum quadro decodificado")))
                            };
                            let _ = reply.send(res);
                        }
                    }
                }
            })
            .map_err(|e| anyhow!("não foi possível iniciar thread Media Foundation: {e}"))?;

        match init_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                width: info.width,
                height: info.height,
                fps: info.fps,
                tx: Mutex::new(Some(tx)),
                worker: Some(handle),
            }),
            Ok(Err(e)) => {
                let _ = handle.join();
                Err(e)
            }
            Err(_) => {
                let _ = handle.join();
                Err(anyhow!("thread Media Foundation encerrou durante abertura"))
            }
        }
    }

    pub fn decode_at(&self, t: Ticks) -> Result<RgbaImage> {
        self.decode_frame_at(t).map(|(img, _)| img)
    }

    /// Como [`Self::decode_at`], mas informa se o quadro é novo. Falso
    /// significa que o stream terminou antes de `t` e o último quadro
    /// válido foi repetido (usado pela conversão para não gravar
    /// quadros duplicados sem avisar).
    pub fn decode_frame_at(&self, t: Ticks) -> Result<(RgbaImage, bool)> {
        let tx = {
            let guard = self
                .tx
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            guard
                .as_ref()
                .ok_or_else(|| anyhow!("leitor Media Foundation já encerrado"))?
                .clone()
        };
        let (reply_tx, reply_rx) = sync_channel(1);
        tx.send(DecodeReq::FrameAt {
            hns: ticks_to_hns(t),
            reply: reply_tx,
        })
        .map_err(|_| anyhow!("thread Media Foundation indisponível"))?;
        reply_rx
            .recv()
            .map_err(|_| anyhow!("sem resposta do decodificador Media Foundation"))?
    }
}

impl Drop for MfVideoReader {
    fn drop(&mut self) {
        if let Some(tx) = self
            .tx
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            let _ = tx.send(DecodeReq::Shutdown);
        }
        if let Some(h) = self.worker.take() {
            let _ = h.join();
        }
    }
}

/// Devolve o áudio decodificado e se ele foi cortado pelo limite de
/// amostras em memória (`MAX_SAMPLES`, §0.4).
fn read_audio_stream(reader: &IMFSourceReader) -> Option<(AudioData, bool)> {
    let stream = MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32;
    // SAFETY: verifica se o arquivo possui faixa de áudio.
    let _native = unsafe { reader.GetNativeMediaType(stream, 0) }.ok()?;

    // SAFETY: cria media type de saída PCM Float 32-bit; se não suportado pelo
    // transformador local, tenta PCM 16-bit inteiro.
    let mt_float = unsafe { MFCreateMediaType() }.ok()?;
    // SAFETY: configuração de GUIDs de formato no tipo recém-criado.
    let use_float = unsafe {
        mt_float
            .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)
            .is_ok()
            && mt_float
                .SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_Float)
                .is_ok()
            && reader.SetCurrentMediaType(stream, None, &mt_float).is_ok()
    };

    if !use_float {
        // SAFETY: fallback para PCM linear 16 bits.
        let mt_pcm = unsafe { MFCreateMediaType() }.ok()?;
        // SAFETY: configuração de GUIDs de formato no tipo recém-criado.
        let ok = unsafe {
            mt_pcm
                .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)
                .is_ok()
                && mt_pcm.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM).is_ok()
                && reader.SetCurrentMediaType(stream, None, &mt_pcm).is_ok()
        };
        if !ok {
            return None;
        }
    }

    // SAFETY: consulta tipo negociado para taxa e número de canais reais.
    let cur = unsafe { reader.GetCurrentMediaType(stream) }.ok()?;
    // SAFETY: leitura de atributos numéricos de áudio na interface válida.
    let sr = unsafe { cur.GetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND) }
        .ok()?
        .max(1);
    // SAFETY: leitura de canais na interface válida.
    let ch = unsafe { cur.GetUINT32(&MF_MT_AUDIO_NUM_CHANNELS) }
        .unwrap_or(2)
        .max(1) as usize;

    let mut samples: Vec<f32> = Vec::new();
    let max_in_samples = MAX_SAMPLES.saturating_mul(ch).min(48_000 * 60 * 120 * 2);

    loop {
        let mut flags: u32 = 0;
        let mut sample: Option<IMFSample> = None;
        // SAFETY: ponteiros válidos na stack para ler pacotes de áudio em sequência.
        if unsafe { reader.ReadSample(stream, 0, None, Some(&mut flags), None, Some(&mut sample)) }
            .is_err()
        {
            break;
        }
        if (flags & (MF_SOURCE_READERF_ERROR.0 as u32 | MF_SOURCE_READERF_ENDOFSTREAM.0 as u32))
            != 0
        {
            break;
        }
        let Some(s) = sample else {
            continue;
        };
        // SAFETY: `s` é amostra COM válida.
        let Ok(buf) = (unsafe { s.ConvertToContiguousBuffer() }) else {
            continue;
        };
        let Ok(locked) = LockedBuffer::lock(&buf, 4 * 1024 * 1024) else {
            break;
        };
        if use_float {
            for &chunk in locked.bytes.as_chunks::<4>().0 {
                if samples.len() >= max_in_samples {
                    return Some((AudioData::from_interleaved(sr, ch, &samples), true));
                }
                let v = f32::from_le_bytes(chunk);
                samples.push(if v.is_finite() { v } else { 0.0 });
            }
        } else {
            for &chunk in locked.bytes.as_chunks::<2>().0 {
                if samples.len() >= max_in_samples {
                    return Some((AudioData::from_interleaved(sr, ch, &samples), true));
                }
                let v = i16::from_le_bytes(chunk) as f32 / 32768.0;
                samples.push(v);
            }
        }
    }

    if samples.is_empty() {
        None
    } else {
        Some((AudioData::from_interleaved(sr, ch, &samples), false))
    }
}

/// Resultado da abertura de um contêiner via Windows Media Foundation.
pub struct MfLoaded {
    pub duration: Option<Ticks>,
    pub video: Option<MfVideoReader>,
    pub audio: Option<AudioData>,
    /// O áudio foi cortado pelo limite de memória (`MAX_SAMPLES`).
    pub audio_truncated: bool,
}

/// Abre um arquivo de mídia com Windows Media Foundation sob isolamento de
/// apartamento COM e limite defensivo de memória (§0.3, §0.4, §3.6).
pub fn open_media(path: &Path) -> Result<MfLoaded> {
    let _guard = MfGuard::enter()?;
    let reader = open_source_reader(path)?;
    let duration = read_duration(&reader);
    let video_info = configure_video_stream(&reader);
    let (audio, audio_truncated) = match read_audio_stream(&reader) {
        Some((a, cut)) => (Some(a), cut),
        None => (None, false),
    };
    drop(reader);

    let video = if let Some(info) = video_info {
        Some(MfVideoReader::spawn(path, info)?)
    } else {
        None
    };

    if video.is_none() && audio.is_none() {
        bail!("nenhum fluxo de áudio ou vídeo reconhecido pelo Media Foundation");
    }

    Ok(MfLoaded {
        duration,
        video,
        audio,
        audio_truncated,
    })
}
