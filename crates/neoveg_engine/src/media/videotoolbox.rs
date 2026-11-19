//! Bridge FFI isolada para decodificação por hardware **VideoToolbox** e áudio
//! **AudioToolbox** (`ExtAudioFile`) no macOS (`AGENTS.md` §0.3 e §2.3:
//! carregamento dinâmico dos frameworks do sistema via `libloading`, sem
//! dependências proprietárias ou LGPL/GPL).
//!
//! Especificação pública consultada: Apple *VideoToolbox*, *CoreMedia*,
//! *CoreVideo*, *CoreFoundation* e *AudioToolbox* C APIs. Todas as referências
//! `CFTypeRef`/`VTDecompressionSessionRef`/`ExtAudioFileRef` são liberadas
//! deterministicamente por guardas RAII no `Drop`.

#![allow(unsafe_code)]

use crate::media::demux::{DemuxTrack, TrackKind};

/// Codec de vídeo suportado pela bridge VideoToolbox.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VtVideoCodec {
    H264,
    Hevc,
}

impl VtVideoCodec {
    pub fn from_codec_id(codec: &str) -> Option<Self> {
        let c = codec.trim();
        if c.eq_ignore_ascii_case("avc1")
            || c.eq_ignore_ascii_case("avc3")
            || c.eq_ignore_ascii_case("V_MPEG4/ISO/AVC")
        {
            Some(Self::H264)
        } else if c.eq_ignore_ascii_case("hvc1")
            || c.eq_ignore_ascii_case("hev1")
            || c.eq_ignore_ascii_case("V_MPEGH/ISO/HEVC")
        {
            Some(Self::Hevc)
        } else {
            None
        }
    }
}

/// Seleciona a primeira trilha de vídeo compatível com VideoToolbox no índice demuxado.
#[allow(dead_code)]
pub(crate) fn select_vt_video_track(tracks: &[DemuxTrack]) -> Option<(usize, VtVideoCodec)> {
    for (idx, tr) in tracks.iter().enumerate() {
        if tr.kind != TrackKind::Video {
            continue;
        }
        if let Some(codec) = VtVideoCodec::from_codec_id(&tr.codec) {
            return Some((idx, codec));
        }
    }
    None
}

#[cfg(target_os = "macos")]
mod macos_impl {
    use super::{select_vt_video_track, VtVideoCodec};
    use crate::media::audio::{AudioData, MAX_SAMPLES};
    use crate::media::demux::{self, DemuxSample};
    use crate::media::fallback::{
        bgra_to_rgba, extract_avcc_parameter_sets, extract_hvcc_parameter_sets, MAX_FALLBACK_DIM,
        MAX_FALLBACK_PACKET,
    };
    use crate::media::system::{SystemLoaded, SystemVideoReader};
    use crate::model::{FrameRate, Ticks};
    use anyhow::{anyhow, bail, Context, Result};
    use image::RgbaImage;
    use libloading::Library;
    use std::ffi::{c_int, c_uint, c_void};
    use std::fs::File;
    use std::io::{Read, Seek, SeekFrom};
    use std::os::unix::ffi::OsStrExt;
    use std::path::{Path, PathBuf};
    use std::ptr;
    use std::sync::{Arc, Mutex, PoisonError};

    type OsStatus = i32;
    type CfTypeRef = *const c_void;
    type CfAllocatorRef = *const c_void;
    type CfDictionaryRef = *const c_void;
    type CfUrlRef = *const c_void;
    type CfIndex = isize;
    type CvPixelBufferRef = *mut c_void;
    type CmFormatDescRef = *const c_void;
    type CmBlockBufferRef = *mut c_void;
    type CmSampleBufferRef = *mut c_void;
    type VtSessionRef = *mut c_void;
    type ExtAudioFileRef = *mut c_void;

    const K_CF_NUMBER_SINT32_TYPE: CfIndex = 3;
    const K_CV_PIXEL_FORMAT_32BGRA: i32 = 0x4247_5241; // 'BGRA'
    const K_CV_LOCK_READ_ONLY: u64 = 1;
    const K_AUDIO_FORMAT_LINEAR_PCM: u32 = u32::from_be_bytes(*b"lpcm");
    const K_AUDIO_FORMAT_FLAG_IS_FLOAT: u32 = 1 << 0;
    const K_AUDIO_FORMAT_FLAG_IS_PACKED: u32 = 1 << 3;
    const K_EXT_AUDIO_PROP_FILE_DATA_FORMAT: u32 = u32::from_be_bytes(*b"ffmt");
    const K_EXT_AUDIO_PROP_CLIENT_DATA_FORMAT: u32 = u32::from_be_bytes(*b"cfmt");

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CmTime {
        value: i64,
        timescale: i32,
        flags: u32,
        epoch: i64,
    }

    const CM_TIME_INVALID: CmTime = CmTime {
        value: 0,
        timescale: 0,
        flags: 0,
        epoch: 0,
    };

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CmSampleTimingInfo {
        duration: CmTime,
        pts: CmTime,
        dts: CmTime,
    }

    type VtOutputCallback = unsafe extern "C" fn(
        decompression_output_ref_con: *mut c_void,
        source_frame_ref_con: *mut c_void,
        status: OsStatus,
        info_flags: u32,
        image_buffer: CvPixelBufferRef,
        pts: CmTime,
        duration: CmTime,
    );

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct VtOutputCallbackRecord {
        callback: VtOutputCallback,
        ref_con: *mut c_void,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct AudioStreamBasicDescription {
        sample_rate: f64,
        format_id: u32,
        format_flags: u32,
        bytes_per_packet: u32,
        frames_per_packet: u32,
        bytes_per_frame: u32,
        channels_per_frame: u32,
        bits_per_channel: u32,
        reserved: u32,
    }

    #[repr(C)]
    struct AudioBuffer {
        number_channels: u32,
        data_byte_size: u32,
        data: *mut c_void,
    }

    #[repr(C)]
    struct AudioBufferList1 {
        number_buffers: u32,
        buffers: [AudioBuffer; 1],
    }

    type FnCfRelease = unsafe extern "C" fn(cf: CfTypeRef);
    type FnCfNumberCreate =
        unsafe extern "C" fn(alloc: CfAllocatorRef, ty: CfIndex, val: *const c_void) -> CfTypeRef;
    type FnCfDictCreate = unsafe extern "C" fn(
        alloc: CfAllocatorRef,
        keys: *const CfTypeRef,
        vals: *const CfTypeRef,
        num: CfIndex,
        key_cb: *const c_void,
        val_cb: *const c_void,
    ) -> CfDictionaryRef;
    type FnCfUrlFromPath = unsafe extern "C" fn(
        alloc: CfAllocatorRef,
        buf: *const u8,
        buf_len: CfIndex,
        is_dir: u8,
    ) -> CfUrlRef;

    type FnCvLock = unsafe extern "C" fn(buf: CvPixelBufferRef, flags: u64) -> i32;
    type FnCvUnlock = unsafe extern "C" fn(buf: CvPixelBufferRef, flags: u64) -> i32;
    type FnCvBaseAddr = unsafe extern "C" fn(buf: CvPixelBufferRef) -> *const c_void;
    type FnCvBytesPerRow = unsafe extern "C" fn(buf: CvPixelBufferRef) -> usize;
    type FnCvWidth = unsafe extern "C" fn(buf: CvPixelBufferRef) -> usize;
    type FnCvHeight = unsafe extern "C" fn(buf: CvPixelBufferRef) -> usize;
    type FnCvRetain = unsafe extern "C" fn(buf: CvPixelBufferRef) -> CvPixelBufferRef;
    type FnCvRelease = unsafe extern "C" fn(buf: CvPixelBufferRef);

    type FnCmH264Desc = unsafe extern "C" fn(
        alloc: CfAllocatorRef,
        count: usize,
        ptrs: *const *const u8,
        sizes: *const usize,
        nal_len_size: c_int,
        out_desc: *mut CmFormatDescRef,
    ) -> OsStatus;
    type FnCmHevcDesc = unsafe extern "C" fn(
        alloc: CfAllocatorRef,
        count: usize,
        ptrs: *const *const u8,
        sizes: *const usize,
        nal_len_size: c_int,
        extensions: CfDictionaryRef,
        out_desc: *mut CmFormatDescRef,
    ) -> OsStatus;
    type FnCmBlockBuf = unsafe extern "C" fn(
        alloc: CfAllocatorRef,
        mem_block: *mut c_void,
        block_len: usize,
        block_alloc: CfAllocatorRef,
        custom_source: *const c_void,
        offset: usize,
        data_len: usize,
        flags: u32,
        out_buf: *mut CmBlockBufferRef,
    ) -> OsStatus;
    type FnCmSampleBuf = unsafe extern "C" fn(
        alloc: CfAllocatorRef,
        data_buf: CmBlockBufferRef,
        fmt_desc: CmFormatDescRef,
        num_samples: CfIndex,
        num_timing: CfIndex,
        timing: *const CmSampleTimingInfo,
        num_sizes: CfIndex,
        sizes: *const usize,
        out_buf: *mut CmSampleBufferRef,
    ) -> OsStatus;

    type FnVtCreate = unsafe extern "C" fn(
        alloc: CfAllocatorRef,
        fmt_desc: CmFormatDescRef,
        decoder_spec: CfDictionaryRef,
        dst_attrs: CfDictionaryRef,
        cb: *const VtOutputCallbackRecord,
        out_sess: *mut VtSessionRef,
    ) -> OsStatus;
    type FnVtDecode = unsafe extern "C" fn(
        sess: VtSessionRef,
        sample_buf: CmSampleBufferRef,
        flags: u32,
        source_frame_ref_con: *mut c_void,
        info_flags_out: *mut u32,
    ) -> OsStatus;
    type FnVtWait = unsafe extern "C" fn(sess: VtSessionRef) -> OsStatus;
    type FnVtInvalidate = unsafe extern "C" fn(sess: VtSessionRef);

    type FnExtAudioOpen =
        unsafe extern "C" fn(url: CfUrlRef, out_ref: *mut ExtAudioFileRef) -> OsStatus;
    type FnExtAudioGetProp = unsafe extern "C" fn(
        af: ExtAudioFileRef,
        prop_id: u32,
        size: *mut u32,
        data: *mut c_void,
    ) -> OsStatus;
    type FnExtAudioSetProp = unsafe extern "C" fn(
        af: ExtAudioFileRef,
        prop_id: u32,
        size: u32,
        data: *const c_void,
    ) -> OsStatus;
    type FnExtAudioRead = unsafe extern "C" fn(
        af: ExtAudioFileRef,
        num_frames: *mut u32,
        io_data: *mut AudioBufferList1,
    ) -> OsStatus;
    type FnExtAudioDispose = unsafe extern "C" fn(af: ExtAudioFileRef) -> OsStatus;

    struct AppleMediaApi {
        _cf: Library,
        _cv: Library,
        _cm: Library,
        _vt: Library,
        _at: Library,
        cf_release: FnCfRelease,
        cf_number_create: FnCfNumberCreate,
        cf_dict_create: FnCfDictCreate,
        cf_url_from_path: FnCfUrlFromPath,
        cv_pix_fmt_key: CfTypeRef,
        cv_lock: FnCvLock,
        cv_unlock: FnCvUnlock,
        cv_base_addr: FnCvBaseAddr,
        cv_bytes_per_row: FnCvBytesPerRow,
        cv_width: FnCvWidth,
        cv_height: FnCvHeight,
        cv_retain: FnCvRetain,
        cv_release: FnCvRelease,
        cm_h264_desc: FnCmH264Desc,
        cm_hevc_desc: FnCmHevcDesc,
        cm_block_buf: FnCmBlockBuf,
        cm_sample_buf: FnCmSampleBuf,
        vt_create: FnVtCreate,
        vt_decode: FnVtDecode,
        vt_wait: FnVtWait,
        vt_invalidate: FnVtInvalidate,
        ext_audio_open: FnExtAudioOpen,
        ext_audio_get_prop: FnExtAudioGetProp,
        ext_audio_set_prop: FnExtAudioSetProp,
        ext_audio_read: FnExtAudioRead,
        ext_audio_dispose: FnExtAudioDispose,
    }

    // SAFETY: ponteiros de função e chaves constantes dos frameworks do macOS são imutáveis.
    unsafe impl Send for AppleMediaApi {}
    // SAFETY: idem.
    unsafe impl Sync for AppleMediaApi {}

    impl AppleMediaApi {
        fn load() -> Result<Self> {
            // SAFETY: frameworks padrão do sistema operacional macOS carregados via `libloading`.
            let cf = unsafe {
                Library::new("/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation")
            }
            .map_err(|e| anyhow!("CoreFoundation indisponível: {e}"))?;
            // SAFETY: idem para CoreVideo.
            let cv =
                unsafe { Library::new("/System/Library/Frameworks/CoreVideo.framework/CoreVideo") }
                    .map_err(|e| anyhow!("CoreVideo indisponível: {e}"))?;
            // SAFETY: idem para CoreMedia.
            let cm =
                unsafe { Library::new("/System/Library/Frameworks/CoreMedia.framework/CoreMedia") }
                    .map_err(|e| anyhow!("CoreMedia indisponível: {e}"))?;
            // SAFETY: idem para VideoToolbox.
            let vt = unsafe {
                Library::new("/System/Library/Frameworks/VideoToolbox.framework/VideoToolbox")
            }
            .map_err(|e| anyhow!("VideoToolbox indisponível: {e}"))?;
            // SAFETY: idem para AudioToolbox.
            let at = unsafe {
                Library::new("/System/Library/Frameworks/AudioToolbox.framework/AudioToolbox")
            }
            .map_err(|e| anyhow!("AudioToolbox indisponível: {e}"))?;

            macro_rules! sym {
                ($lib:expr, $name:literal, $ty:ty) => {{
                    // SAFETY: assinatura pública dos frameworks Apple; a `Library` vive em `AppleMediaApi`.
                    let s: libloading::Symbol<'_, $ty> = unsafe { $lib.get($name) }
                        .map_err(|e| anyhow!("símbolo Apple Media ausente: {e}"))?;
                    *s
                }};
            }

            // SAFETY: `kCVPixelBufferPixelFormatTypeKey` é um símbolo `CFStringRef` constante exportado por CoreVideo.
            let cv_pix_fmt_key_ptr: *const CfTypeRef =
                sym!(cv, b"kCVPixelBufferPixelFormatTypeKey\0", *const CfTypeRef);
            // SAFETY: ponteiro de símbolo estático não nulo exportado pelo framework.
            let cv_pix_fmt_key = if cv_pix_fmt_key_ptr.is_null() {
                bail!("kCVPixelBufferPixelFormatTypeKey nulo");
            } else {
                unsafe { *cv_pix_fmt_key_ptr }
            };

            Ok(Self {
                cf_release: sym!(cf, b"CFRelease\0", FnCfRelease),
                cf_number_create: sym!(cf, b"CFNumberCreate\0", FnCfNumberCreate),
                cf_dict_create: sym!(cf, b"CFDictionaryCreate\0", FnCfDictCreate),
                cf_url_from_path: sym!(
                    cf,
                    b"CFURLCreateFromFileSystemRepresentation\0",
                    FnCfUrlFromPath
                ),
                cv_pix_fmt_key,
                cv_lock: sym!(cv, b"CVPixelBufferLockBaseAddress\0", FnCvLock),
                cv_unlock: sym!(cv, b"CVPixelBufferUnlockBaseAddress\0", FnCvUnlock),
                cv_base_addr: sym!(cv, b"CVPixelBufferGetBaseAddress\0", FnCvBaseAddr),
                cv_bytes_per_row: sym!(cv, b"CVPixelBufferGetBytesPerRow\0", FnCvBytesPerRow),
                cv_width: sym!(cv, b"CVPixelBufferGetWidth\0", FnCvWidth),
                cv_height: sym!(cv, b"CVPixelBufferGetHeight\0", FnCvHeight),
                cv_retain: sym!(cv, b"CVBufferRetain\0", FnCvRetain),
                cv_release: sym!(cv, b"CVBufferRelease\0", FnCvRelease),
                cm_h264_desc: sym!(
                    cm,
                    b"CMVideoFormatDescriptionCreateFromH264ParameterSets\0",
                    FnCmH264Desc
                ),
                cm_hevc_desc: sym!(
                    cm,
                    b"CMVideoFormatDescriptionCreateFromHEVCParameterSets\0",
                    FnCmHevcDesc
                ),
                cm_block_buf: sym!(cm, b"CMBlockBufferCreateWithMemoryBlock\0", FnCmBlockBuf),
                cm_sample_buf: sym!(cm, b"CMSampleBufferCreateReady\0", FnCmSampleBuf),
                vt_create: sym!(vt, b"VTDecompressionSessionCreate\0", FnVtCreate),
                vt_decode: sym!(vt, b"VTDecompressionSessionDecodeFrame\0", FnVtDecode),
                vt_wait: sym!(
                    vt,
                    b"VTDecompressionSessionWaitForAsynchronousFrames\0",
                    FnVtWait
                ),
                vt_invalidate: sym!(vt, b"VTDecompressionSessionInvalidate\0", FnVtInvalidate),
                ext_audio_open: sym!(at, b"ExtAudioFileOpenURL\0", FnExtAudioOpen),
                ext_audio_get_prop: sym!(at, b"ExtAudioFileGetProperty\0", FnExtAudioGetProp),
                ext_audio_set_prop: sym!(at, b"ExtAudioFileSetProperty\0", FnExtAudioSetProp),
                ext_audio_read: sym!(at, b"ExtAudioFileRead\0", FnExtAudioRead),
                ext_audio_dispose: sym!(at, b"ExtAudioFileDispose\0", FnExtAudioDispose),
                _cf: cf,
                _cv: cv,
                _cm: cm,
                _vt: vt,
                _at: at,
            })
        }
    }

    struct CallbackSlot {
        api_retain: FnCvRetain,
        api_release: FnCvRelease,
        last_buf: CvPixelBufferRef,
        last_status: OsStatus,
    }

    unsafe extern "C" fn vt_output_cb(
        _decompression_ref_con: *mut c_void,
        source_frame_ref_con: *mut c_void,
        status: OsStatus,
        _info_flags: u32,
        image_buffer: CvPixelBufferRef,
        _pts: CmTime,
        _duration: CmTime,
    ) {
        if source_frame_ref_con.is_null() {
            return;
        }
        // SAFETY: `source_frame_ref_con` aponta para `CallbackSlot` vivo na pilha durante `vt_decode`/`vt_wait`.
        let slot = unsafe { &mut *(source_frame_ref_con as *mut CallbackSlot) };
        slot.last_status = status;
        if status == 0 && !image_buffer.is_null() {
            if !slot.last_buf.is_null() {
                // SAFETY: libera o pixel buffer anterior retido por este slot.
                unsafe { (slot.api_release)(slot.last_buf) };
            }
            // SAFETY: retém `image_buffer` para sobreviver além do callback até a cópia para `RgbaImage`.
            slot.last_buf = unsafe { (slot.api_retain)(image_buffer) };
        }
    }

    struct VtState {
        api: Arc<AppleMediaApi>,
        fmt_desc: CmFormatDescRef,
        session: VtSessionRef,
        file: File,
        last_decoded_idx: Option<usize>,
    }

    // SAFETY: `VtState` é protegido exclusivamente por `Mutex` em `VideoToolboxReader`.
    unsafe impl Send for VtState {}

    impl Drop for VtState {
        fn drop(&mut self) {
            // SAFETY: invalida e libera a sessão VideoToolbox e o `CMFormatDescription` na ordem correta.
            unsafe {
                if !self.session.is_null() {
                    (self.api.vt_invalidate)(self.session);
                    (self.api.cf_release)(self.session as CfTypeRef);
                }
                if !self.fmt_desc.is_null() {
                    (self.api.cf_release)(self.fmt_desc);
                }
            }
        }
    }

    pub struct VideoToolboxReader {
        path: PathBuf,
        width: u32,
        height: u32,
        fps: FrameRate,
        samples: Vec<DemuxSample>,
        state: Mutex<VtState>,
    }

    impl VideoToolboxReader {
        fn open(
            path: &Path,
            api: Arc<AppleMediaApi>,
            codec: VtVideoCodec,
            width: u32,
            height: u32,
            fps: FrameRate,
            extra: &[u8],
            samples: Vec<DemuxSample>,
        ) -> Result<Self> {
            if width == 0 || height == 0 || width > MAX_FALLBACK_DIM || height > MAX_FALLBACK_DIM {
                bail!("dimensões VideoToolbox inválidas: {width}x{height}");
            }
            let (nal_len_size, param_sets) = match codec {
                VtVideoCodec::H264 => extract_avcc_parameter_sets(extra)?,
                VtVideoCodec::Hevc => extract_hvcc_parameter_sets(extra)?,
            };
            let ptrs: Vec<*const u8> = param_sets.iter().map(|v| v.as_ptr()).collect();
            let sizes: Vec<usize> = param_sets.iter().map(Vec::len).collect();

            let mut fmt_desc: CmFormatDescRef = ptr::null();
            // SAFETY: `ptrs` e `sizes` têm o mesmo comprimento `param_sets.len()` e apontam para buffers válidos.
            let st = unsafe {
                match codec {
                    VtVideoCodec::H264 => (api.cm_h264_desc)(
                        ptr::null(),
                        param_sets.len(),
                        ptrs.as_ptr(),
                        sizes.as_ptr(),
                        nal_len_size as c_int,
                        &mut fmt_desc,
                    ),
                    VtVideoCodec::Hevc => (api.cm_hevc_desc)(
                        ptr::null(),
                        param_sets.len(),
                        ptrs.as_ptr(),
                        sizes.as_ptr(),
                        nal_len_size as c_int,
                        ptr::null(),
                        &mut fmt_desc,
                    ),
                }
            };
            if st != 0 || fmt_desc.is_null() {
                bail!("CMVideoFormatDescriptionCreate falhou ({st})");
            }

            let pix_fmt = K_CV_PIXEL_FORMAT_32BGRA;
            // SAFETY: cria `CFNumber` temporário para especificar saída BGRA32.
            let num_ref = unsafe {
                (api.cf_number_create)(
                    ptr::null(),
                    K_CF_NUMBER_SINT32_TYPE,
                    (&pix_fmt as *const i32) as *const c_void,
                )
            };
            let keys = [api.cv_pix_fmt_key];
            let vals = [num_ref];
            // SAFETY: dicionário temporário com 1 par válido (chave/valor CoreFoundation).
            let dst_attrs = unsafe {
                (api.cf_dict_create)(
                    ptr::null(),
                    keys.as_ptr(),
                    vals.as_ptr(),
                    1,
                    ptr::null(),
                    ptr::null(),
                )
            };

            let cb_rec = VtOutputCallbackRecord {
                callback: vt_output_cb,
                ref_con: ptr::null_mut(),
            };
            let mut session: VtSessionRef = ptr::null_mut();
            // SAFETY: cria sessão síncrona VideoToolbox; libera dicionário e número temporários logo após.
            let vt_st = unsafe {
                let r = (api.vt_create)(
                    ptr::null(),
                    fmt_desc,
                    ptr::null(),
                    dst_attrs,
                    &cb_rec,
                    &mut session,
                );
                if !dst_attrs.is_null() {
                    (api.cf_release)(dst_attrs);
                }
                if !num_ref.is_null() {
                    (api.cf_release)(num_ref);
                }
                r
            };
            if vt_st != 0 || session.is_null() {
                // SAFETY: libera `fmt_desc` se `VTDecompressionSessionCreate` falhou.
                unsafe { (api.cf_release)(fmt_desc) };
                bail!("VTDecompressionSessionCreate falhou ({vt_st})");
            }

            let file = File::open(path).with_context(|| format!("abrindo {}", path.display()))?;
            Ok(Self {
                path: path.to_path_buf(),
                width,
                height,
                fps,
                samples,
                state: Mutex::new(VtState {
                    api,
                    fmt_desc,
                    session,
                    file,
                    last_decoded_idx: None,
                }),
            })
        }
    }

    impl SystemVideoReader for VideoToolboxReader {
        fn width(&self) -> u32 {
            self.width
        }

        fn height(&self) -> u32 {
            self.height
        }

        fn fps(&self) -> FrameRate {
            self.fps
        }

        fn decode_at(&self, t: Ticks) -> Result<RgbaImage> {
            if self.samples.is_empty() {
                bail!("trilha VideoToolbox sem amostras");
            }
            let target = self
                .samples
                .partition_point(|s| s.pts <= t)
                .saturating_sub(1)
                .min(self.samples.len() - 1);

            // Retrocede ao quadro-chave (IDR) anterior quando necessário para decodificação inter-quadro.
            let mut st = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            let start = match st.last_decoded_idx {
                Some(prev) if target > prev && target - prev <= 16 => prev + 1,
                _ => {
                    let mut kf = target;
                    while kf > 0 && !self.samples[kf].is_keyframe {
                        kf -= 1;
                    }
                    kf
                }
            };

            let mut slot = CallbackSlot {
                api_retain: st.api.cv_retain,
                api_release: st.api.cv_release,
                last_buf: ptr::null_mut(),
                last_status: -1,
            };

            let mut pkt = Vec::new();
            for idx in start..=target {
                let sample = &self.samples[idx];
                let sz = sample.size as usize;
                if sz == 0 || sz > MAX_FALLBACK_PACKET {
                    continue;
                }
                pkt.resize(sz, 0);
                st.file
                    .seek(SeekFrom::Start(sample.offset))
                    .with_context(|| format!("seek em {}", self.path.display()))?;
                st.file.read_exact(&mut pkt)?;

                let mut block_buf: CmBlockBufferRef = ptr::null_mut();
                // SAFETY: `pkt` permanece vivo até o fim da chamada síncrona `vt_wait`;
                // `kCFAllocatorNull` (passando bloco copiado via `block_alloc = null`? Não:
                // criamos bloco com `block_alloc = null` copiando com `CMBlockBufferCreateWithMemoryBlock`
                // ou mantemos `pkt` vivo até liberar `sample_buf` e `block_buf`).
                let b_st = unsafe {
                    (st.api.cm_block_buf)(
                        ptr::null(),
                        pkt.as_mut_ptr() as *mut c_void,
                        pkt.len(),
                        ptr::null(),
                        ptr::null(),
                        0,
                        pkt.len(),
                        0,
                        &mut block_buf,
                    )
                };
                if b_st != 0 || block_buf.is_null() {
                    continue;
                }

                let timing = CmSampleTimingInfo {
                    duration: CM_TIME_INVALID,
                    pts: CmTime {
                        value: sample.pts.0,
                        timescale: crate::time::TICKS_PER_SEC as i32,
                        flags: 1,
                        epoch: 0,
                    },
                    dts: CM_TIME_INVALID,
                };
                let sample_sz = pkt.len();
                let mut sample_buf: CmSampleBufferRef = ptr::null_mut();
                // SAFETY: `block_buf` e `st.fmt_desc` válidos; `sample_buf` é liberado no fim da iteração.
                let s_st = unsafe {
                    (st.api.cm_sample_buf)(
                        ptr::null(),
                        block_buf,
                        st.fmt_desc,
                        1,
                        1,
                        &timing,
                        1,
                        &sample_sz,
                        &mut sample_buf,
                    )
                };
                if s_st == 0 && !sample_buf.is_null() {
                    let mut flags_out: u32 = 0;
                    // SAFETY: decodificação síncrona (`flags = 0`) + `WaitForAsynchronousFrames`;
                    // `slot` permanece vivo na pilha durante toda a chamada.
                    unsafe {
                        let _ = (st.api.vt_decode)(
                            st.session,
                            sample_buf,
                            0,
                            (&mut slot as *mut CallbackSlot) as *mut c_void,
                            &mut flags_out,
                        );
                        let _ = (st.api.vt_wait)(st.session);
                        (st.api.cf_release)(sample_buf as CfTypeRef);
                    }
                }
                // SAFETY: libera `block_buf` antes de reutilizar `pkt`.
                unsafe {
                    (st.api.cf_release)(block_buf as CfTypeRef);
                }
            }
            st.last_decoded_idx = Some(target);

            if slot.last_buf.is_null() {
                bail!(
                    "VideoToolbox não produziu quadro (status {})",
                    slot.last_status
                );
            }

            // SAFETY: trava o `CVPixelBuffer` para leitura somente-leitura, copia BGRA → RGBA e destrava/libera.
            let img_res = unsafe {
                let lock_st = (st.api.cv_lock)(slot.last_buf, K_CV_LOCK_READ_ONLY);
                let out = if lock_st == 0 {
                    let base = (st.api.cv_base_addr)(slot.last_buf) as *const u8;
                    let stride = (st.api.cv_bytes_per_row)(slot.last_buf);
                    let w = (st.api.cv_width)(slot.last_buf) as u32;
                    let h = (st.api.cv_height)(slot.last_buf) as u32;
                    let total = stride.saturating_mul(h as usize);
                    if !base.is_null() && total > 0 && w > 0 && h > 0 {
                        let slice = std::slice::from_raw_parts(base, total);
                        bgra_to_rgba(w, h, slice, stride)
                    } else {
                        Err(anyhow!("CVPixelBuffer vazio"))
                    }
                } else {
                    Err(anyhow!("CVPixelBufferLockBaseAddress falhou ({lock_st})"))
                };
                if lock_st == 0 {
                    let _ = (st.api.cv_unlock)(slot.last_buf, K_CV_LOCK_READ_ONLY);
                }
                (st.api.cv_release)(slot.last_buf);
                out
            };
            img_res
        }
    }

    /// Decodifica áudio nativo (AAC, ALAC, MP3, PCM em MP4/MOV/M4A) via `ExtAudioFile` (AudioToolbox).
    fn decode_ext_audio(api: &AppleMediaApi, path: &Path) -> Result<(AudioData, bool)> {
        let bytes = path.as_os_str().as_bytes();
        // SAFETY: cria `CFURLRef` a partir do caminho POSIX; liberado antes de sair.
        let url = unsafe {
            (api.cf_url_from_path)(ptr::null(), bytes.as_ptr(), bytes.len() as CfIndex, 0)
        };
        if url.is_null() {
            bail!("CFURLCreateFromFileSystemRepresentation retornou nulo");
        }
        let mut af: ExtAudioFileRef = ptr::null_mut();
        // SAFETY: abre `ExtAudioFile` para leitura; `url` é liberado imediatamente após.
        let st = unsafe {
            let r = (api.ext_audio_open)(url, &mut af);
            (api.cf_release)(url);
            r
        };
        if st != 0 || af.is_null() {
            bail!("ExtAudioFileOpenURL falhou ({st})");
        }

        let mut file_fmt = AudioStreamBasicDescription::default();
        let mut prop_sz = std::mem::size_of::<AudioStreamBasicDescription>() as u32;
        // SAFETY: consulta formato original de áudio do arquivo.
        let g_st = unsafe {
            (api.ext_audio_get_prop)(
                af,
                K_EXT_AUDIO_PROP_FILE_DATA_FORMAT,
                &mut prop_sz,
                (&mut file_fmt as *mut AudioStreamBasicDescription) as *mut c_void,
            )
        };
        if g_st != 0 {
            // SAFETY: fecha `ExtAudioFile` em caso de erro.
            unsafe {
                let _ = (api.ext_audio_dispose)(af);
            }
            bail!("ExtAudioFileGetProperty falhou ({g_st})");
        }

        let sr = (file_fmt.sample_rate.round() as u32).clamp(8_000, 192_000);
        let ch = file_fmt.channels_per_frame.clamp(1, 2);
        let client_fmt = AudioStreamBasicDescription {
            sample_rate: f64::from(sr),
            format_id: K_AUDIO_FORMAT_LINEAR_PCM,
            format_flags: K_AUDIO_FORMAT_FLAG_IS_FLOAT | K_AUDIO_FORMAT_FLAG_IS_PACKED,
            bytes_per_packet: 4 * ch,
            frames_per_packet: 1,
            bytes_per_frame: 4 * ch,
            channels_per_frame: ch,
            bits_per_channel: 32,
            reserved: 0,
        };
        // SAFETY: configura saída Float32 intercalada para `ExtAudioFileRead`.
        let s_st = unsafe {
            (api.ext_audio_set_prop)(
                af,
                K_EXT_AUDIO_PROP_CLIENT_DATA_FORMAT,
                std::mem::size_of::<AudioStreamBasicDescription>() as u32,
                (&client_fmt as *const AudioStreamBasicDescription) as *const c_void,
            )
        };
        if s_st != 0 {
            // SAFETY: fecha `ExtAudioFile`.
            unsafe {
                let _ = (api.ext_audio_dispose)(af);
            }
            bail!("ExtAudioFileSetProperty falhou ({s_st})");
        }

        let chunk_frames: u32 = 4096;
        let mut chunk = vec![0f32; (chunk_frames as usize) * (ch as usize)];
        let mut out = Vec::new();
        let mut truncated = false;
        loop {
            let mut n_frames = chunk_frames;
            let mut abl = AudioBufferList1 {
                number_buffers: 1,
                buffers: [AudioBuffer {
                    number_channels: ch,
                    data_byte_size: (chunk.len() * 4) as u32,
                    data: chunk.as_mut_ptr() as *mut c_void,
                }],
            };
            // SAFETY: `chunk` tem capacidade exata para `n_frames * ch` amostras `f32`.
            let r_st = unsafe { (api.ext_audio_read)(af, &mut n_frames, &mut abl) };
            if r_st != 0 || n_frames == 0 {
                break;
            }
            let got = (n_frames as usize) * (ch as usize);
            for &v in chunk.iter().take(got) {
                out.push(if v.is_finite() {
                    v.clamp(-4.0, 4.0)
                } else {
                    0.0
                });
                if out.len() >= MAX_SAMPLES {
                    truncated = true;
                    break;
                }
            }
            if truncated {
                break;
            }
        }
        // SAFETY: libera o handle `ExtAudioFileRef`.
        unsafe {
            let _ = (api.ext_audio_dispose)(af);
        }
        if out.is_empty() {
            bail!("ExtAudioFile não retornou amostras");
        }
        Ok((
            AudioData::from_interleaved(sr, ch as usize, &out),
            truncated,
        ))
    }

    pub fn open_media(path: &Path) -> Result<SystemLoaded> {
        let api = Arc::new(AppleMediaApi::load()?);
        let idx = demux::demux_file(path).ok();
        let video: Option<Arc<dyn SystemVideoReader>> = if let Some(ref idx) = idx {
            if let Some((ti, codec)) = select_vt_video_track(&idx.tracks) {
                let tr = &idx.tracks[ti];
                let samples: Vec<DemuxSample> = idx.samples_for_track(ti).copied().collect();
                if !samples.is_empty() {
                    VideoToolboxReader::open(
                        path,
                        Arc::clone(&api),
                        codec,
                        tr.width,
                        tr.height,
                        tr.fps.unwrap_or(FrameRate::FPS_30),
                        &tr.extra,
                        samples,
                    )
                    .ok()
                    .map(|r| Arc::new(r) as Arc<dyn SystemVideoReader>)
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let (audio, audio_truncated) = match decode_ext_audio(&api, path) {
            Ok((a, tr)) => (Some(a), tr),
            Err(_) => (None, false),
        };

        if video.is_none() && audio.is_none() {
            bail!(
                "VideoToolbox/AudioToolbox não pôde decodificar {}",
                path.display()
            );
        }
        let duration = idx
            .and_then(|i| i.duration)
            .or_else(|| audio.as_ref().map(AudioData::duration));
        Ok(SystemLoaded {
            backend: "macOS VideoToolbox",
            duration,
            video,
            audio,
            audio_truncated,
        })
    }
}

#[cfg(target_os = "macos")]
pub use macos_impl::open_media;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapeamento_codecs_videotoolbox() {
        assert_eq!(
            VtVideoCodec::from_codec_id("avc1"),
            Some(VtVideoCodec::H264)
        );
        assert_eq!(
            VtVideoCodec::from_codec_id("V_MPEG4/ISO/AVC"),
            Some(VtVideoCodec::H264)
        );
        assert_eq!(
            VtVideoCodec::from_codec_id("hvc1"),
            Some(VtVideoCodec::Hevc)
        );
        assert_eq!(
            VtVideoCodec::from_codec_id("V_MPEGH/ISO/HEVC"),
            Some(VtVideoCodec::Hevc)
        );
        assert_eq!(VtVideoCodec::from_codec_id("vp09"), None);
    }
}
