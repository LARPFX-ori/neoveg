//! Bridge FFI isolada para decodificação por hardware **VA-API** no Linux
//! (`AGENTS.md` §0.3 e §2.3: carregamento dinâmico via `libloading` de
//! `libva.so.2` e `libva-drm.so.2`, zero GStreamer/LGPL, zero dependência de
//! headers C em tempo de compilação).
//!
//! Especificação pública consultada: Intel *libva* (`va/va.h`, `va/va_drm.h`,
//! licença MIT). Toda função FFI é encapsulada em guardas RAII que liberam
//! contexto, superfícies, configuração, display VA e descritor DRM no `Drop`.

#![allow(unsafe_code)]

use crate::media::demux::{DemuxTrack, TrackKind};

/// Perfis VA-API (`VAProfile` em `va/va.h`) relevantes para decodificação VLD.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VaCodecProfile {
    H264High,
    H264Main,
    HevcMain,
    Vp9Profile0,
    Av1Profile0,
    JpegBaseline,
}

impl VaCodecProfile {
    /// Identificador numérico oficial de `VAProfile` em `va/va.h`.
    #[allow(dead_code)]
    pub const fn va_profile_id(self) -> i32 {
        match self {
            Self::H264Main => 6,
            Self::H264High => 7,
            Self::JpegBaseline => 14,
            Self::HevcMain => 17,
            Self::Vp9Profile0 => 19,
            Self::Av1Profile0 => 32,
        }
    }

    /// Mapeia o código de trilha do demuxer (`avc1`, `hvc1`, `vp09`, `av01`,
    /// `V_MPEG4/ISO/AVC`, `V_MPEGH/ISO/HEVC`, `V_VP9`, `V_AV1`, `jpeg`) para
    /// os perfis VA-API candidatos em ordem de preferência.
    pub fn candidates_for_codec(codec: &str) -> &'static [Self] {
        let cu = codec.trim();
        if cu.eq_ignore_ascii_case("avc1")
            || cu.eq_ignore_ascii_case("avc3")
            || cu.eq_ignore_ascii_case("V_MPEG4/ISO/AVC")
        {
            &[Self::H264High, Self::H264Main]
        } else if cu.eq_ignore_ascii_case("hvc1")
            || cu.eq_ignore_ascii_case("hev1")
            || cu.eq_ignore_ascii_case("V_MPEGH/ISO/HEVC")
        {
            &[Self::HevcMain]
        } else if cu.eq_ignore_ascii_case("vp09") || cu.eq_ignore_ascii_case("V_VP9") {
            &[Self::Vp9Profile0]
        } else if cu.eq_ignore_ascii_case("av01") || cu.eq_ignore_ascii_case("V_AV1") {
            &[Self::Av1Profile0]
        } else if cu.eq_ignore_ascii_case("jpeg")
            || cu.eq_ignore_ascii_case("mjpa")
            || cu.eq_ignore_ascii_case("V_MJPEG")
        {
            &[Self::JpegBaseline]
        } else {
            &[]
        }
    }
}

/// Seleciona a primeira trilha de vídeo compatível com VA-API no índice demuxado.
#[allow(dead_code)]
pub(crate) fn select_vaapi_video_track(
    tracks: &[DemuxTrack],
) -> Option<(usize, &'static [VaCodecProfile])> {
    for (idx, tr) in tracks.iter().enumerate() {
        if tr.kind != TrackKind::Video {
            continue;
        }
        let cands = VaCodecProfile::candidates_for_codec(&tr.codec);
        if !cands.is_empty() {
            return Some((idx, cands));
        }
    }
    None
}

#[cfg(target_os = "linux")]
mod linux_impl {
    use super::{select_vaapi_video_track, VaCodecProfile};
    use crate::media::demux::{self, DemuxSample};
    use crate::media::fallback::{
        bgra_to_rgba, nv12_to_rgba, MAX_FALLBACK_DIM, MAX_FALLBACK_PACKET,
    };
    use crate::media::system::{SystemLoaded, SystemVideoReader};
    use crate::model::{FrameRate, Ticks};
    use anyhow::{anyhow, bail, Context, Result};
    use image::RgbaImage;
    use libloading::Library;
    use std::ffi::{c_int, c_uint, c_void};
    use std::fs::{File, OpenOptions};
    use std::io::{Read, Seek, SeekFrom};
    use std::os::fd::AsRawFd;
    use std::path::{Path, PathBuf};
    use std::ptr;
    use std::sync::{Arc, Mutex, PoisonError};

    type VaDisplay = *mut c_void;
    type VaStatus = c_int;
    type VaConfigId = u32;
    type VaContextId = u32;
    type VaSurfaceId = u32;
    type VaBufferId = u32;
    type VaImageId = u32;

    const VA_STATUS_SUCCESS: VaStatus = 0;
    const VA_ENTRYPOINT_VLD: c_int = 1;
    const VA_RT_FORMAT_YUV420: c_uint = 0x0000_0001;
    const VA_PROGRESSIVE: c_int = 0x0000_0001;
    const VA_SLICE_DATA_BUFFER_TYPE: c_int = 5;
    const VA_FOURCC_NV12: u32 = u32::from_le_bytes(*b"NV12");
    const VA_FOURCC_BGRA: u32 = u32::from_le_bytes(*b"BGRA");
    const VA_FOURCC_RGBA: u32 = u32::from_le_bytes(*b"RGBA");

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct VaConfigAttrib {
        attrib_type: c_int,
        value: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct VaImageFormat {
        fourcc: u32,
        byte_order: u32,
        bits_per_pixel: u32,
        depth: u32,
        red_mask: u32,
        green_mask: u32,
        blue_mask: u32,
        alpha_mask: u32,
        va_reserved: [u32; 4],
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct VaImage {
        image_id: VaImageId,
        format: VaImageFormat,
        buf: VaBufferId,
        width: u16,
        height: u16,
        data_size: u32,
        num_planes: u32,
        pitches: [u32; 3],
        offsets: [u32; 3],
        num_palette_entries: c_int,
        entry_bytes: c_int,
        component_order: [i8; 4],
        va_reserved: [u32; 4],
    }

    type FnGetDisplayDrm = unsafe extern "C" fn(fd: c_int) -> VaDisplay;
    type FnInitialize =
        unsafe extern "C" fn(dpy: VaDisplay, major: *mut c_int, minor: *mut c_int) -> VaStatus;
    type FnTerminate = unsafe extern "C" fn(dpy: VaDisplay) -> VaStatus;
    type FnMaxNumProfiles = unsafe extern "C" fn(dpy: VaDisplay) -> c_int;
    type FnQueryConfigProfiles = unsafe extern "C" fn(
        dpy: VaDisplay,
        profile_list: *mut c_int,
        num_profiles: *mut c_int,
    ) -> VaStatus;
    type FnCreateConfig = unsafe extern "C" fn(
        dpy: VaDisplay,
        profile: c_int,
        entrypoint: c_int,
        attrib_list: *mut VaConfigAttrib,
        num_attribs: c_int,
        config_id: *mut VaConfigId,
    ) -> VaStatus;
    type FnDestroyConfig = unsafe extern "C" fn(dpy: VaDisplay, config_id: VaConfigId) -> VaStatus;
    type FnCreateSurfaces = unsafe extern "C" fn(
        dpy: VaDisplay,
        format: c_uint,
        width: c_uint,
        height: c_uint,
        surfaces: *mut VaSurfaceId,
        num_surfaces: c_uint,
        attrib_list: *mut c_void,
        num_attribs: c_uint,
    ) -> VaStatus;
    type FnDestroySurfaces = unsafe extern "C" fn(
        dpy: VaDisplay,
        surfaces: *mut VaSurfaceId,
        num_surfaces: c_int,
    ) -> VaStatus;
    type FnCreateContext = unsafe extern "C" fn(
        dpy: VaDisplay,
        config_id: VaConfigId,
        picture_width: c_int,
        picture_height: c_int,
        flag: c_int,
        render_targets: *mut VaSurfaceId,
        num_render_targets: c_int,
        context: *mut VaContextId,
    ) -> VaStatus;
    type FnDestroyContext = unsafe extern "C" fn(dpy: VaDisplay, context: VaContextId) -> VaStatus;
    type FnCreateBuffer = unsafe extern "C" fn(
        dpy: VaDisplay,
        context: VaContextId,
        buf_type: c_int,
        size: c_uint,
        num_elements: c_uint,
        data: *mut c_void,
        buf_id: *mut VaBufferId,
    ) -> VaStatus;
    type FnDestroyBuffer = unsafe extern "C" fn(dpy: VaDisplay, buf_id: VaBufferId) -> VaStatus;
    type FnBeginPicture = unsafe extern "C" fn(
        dpy: VaDisplay,
        context: VaContextId,
        render_target: VaSurfaceId,
    ) -> VaStatus;
    type FnRenderPicture = unsafe extern "C" fn(
        dpy: VaDisplay,
        context: VaContextId,
        buffers: *mut VaBufferId,
        num_buffers: c_int,
    ) -> VaStatus;
    type FnEndPicture = unsafe extern "C" fn(dpy: VaDisplay, context: VaContextId) -> VaStatus;
    type FnSyncSurface =
        unsafe extern "C" fn(dpy: VaDisplay, render_target: VaSurfaceId) -> VaStatus;
    type FnDeriveImage =
        unsafe extern "C" fn(dpy: VaDisplay, surface: VaSurfaceId, image: *mut VaImage) -> VaStatus;
    type FnDestroyImage = unsafe extern "C" fn(dpy: VaDisplay, image: VaImageId) -> VaStatus;
    type FnMapBuffer = unsafe extern "C" fn(
        dpy: VaDisplay,
        buf_id: VaBufferId,
        pbuf: *mut *mut c_void,
    ) -> VaStatus;
    type FnUnmapBuffer = unsafe extern "C" fn(dpy: VaDisplay, buf_id: VaBufferId) -> VaStatus;

    struct VaApi {
        _libva: Library,
        _libva_drm: Library,
        get_display_drm: FnGetDisplayDrm,
        initialize: FnInitialize,
        terminate: FnTerminate,
        max_num_profiles: FnMaxNumProfiles,
        query_config_profiles: FnQueryConfigProfiles,
        create_config: FnCreateConfig,
        destroy_config: FnDestroyConfig,
        create_surfaces: FnCreateSurfaces,
        destroy_surfaces: FnDestroySurfaces,
        create_context: FnCreateContext,
        destroy_context: FnDestroyContext,
        create_buffer: FnCreateBuffer,
        destroy_buffer: FnDestroyBuffer,
        begin_picture: FnBeginPicture,
        render_picture: FnRenderPicture,
        end_picture: FnEndPicture,
        sync_surface: FnSyncSurface,
        derive_image: FnDeriveImage,
        destroy_image: FnDestroyImage,
        map_buffer: FnMapBuffer,
        unmap_buffer: FnUnmapBuffer,
    }

    impl VaApi {
        fn load() -> Result<Self> {
            // SAFETY: carregamento dinâmico sob demanda das bibliotecas do SO
            // `libva.so.2` e `libva-drm.so.2` (licença MIT, sem linkagem estática).
            let libva = unsafe { Library::new("libva.so.2") }
                .map_err(|e| anyhow!("libva.so.2 indisponível: {e}"))?;
            // SAFETY: idem para `libva-drm.so.2`.
            let libva_drm = unsafe { Library::new("libva-drm.so.2") }
                .map_err(|e| anyhow!("libva-drm.so.2 indisponível: {e}"))?;

            macro_rules! sym {
                ($lib:expr, $name:literal, $ty:ty) => {{
                    // SAFETY: assinatura oficial em `va/va.h` / `va/va_drm.h`;
                    // a `Library` permanece viva dentro de `VaApi` durante toda a vida do ponteiro.
                    let s: libloading::Symbol<'_, $ty> = unsafe { $lib.get($name) }
                        .map_err(|e| anyhow!("símbolo VA-API ausente: {e}"))?;
                    *s
                }};
            }

            let get_display_drm = sym!(libva_drm, b"vaGetDisplayDRM\0", FnGetDisplayDrm);
            let initialize = sym!(libva, b"vaInitialize\0", FnInitialize);
            let terminate = sym!(libva, b"vaTerminate\0", FnTerminate);
            let max_num_profiles = sym!(libva, b"vaMaxNumProfiles\0", FnMaxNumProfiles);
            let query_config_profiles =
                sym!(libva, b"vaQueryConfigProfiles\0", FnQueryConfigProfiles);
            let create_config = sym!(libva, b"vaCreateConfig\0", FnCreateConfig);
            let destroy_config = sym!(libva, b"vaDestroyConfig\0", FnDestroyConfig);
            let create_surfaces = sym!(libva, b"vaCreateSurfaces\0", FnCreateSurfaces);
            let destroy_surfaces = sym!(libva, b"vaDestroySurfaces\0", FnDestroySurfaces);
            let create_context = sym!(libva, b"vaCreateContext\0", FnCreateContext);
            let destroy_context = sym!(libva, b"vaDestroyContext\0", FnDestroyContext);
            let create_buffer = sym!(libva, b"vaCreateBuffer\0", FnCreateBuffer);
            let destroy_buffer = sym!(libva, b"vaDestroyBuffer\0", FnDestroyBuffer);
            let begin_picture = sym!(libva, b"vaBeginPicture\0", FnBeginPicture);
            let render_picture = sym!(libva, b"vaRenderPicture\0", FnRenderPicture);
            let end_picture = sym!(libva, b"vaEndPicture\0", FnEndPicture);
            let sync_surface = sym!(libva, b"vaSyncSurface\0", FnSyncSurface);
            let derive_image = sym!(libva, b"vaDeriveImage\0", FnDeriveImage);
            let destroy_image = sym!(libva, b"vaDestroyImage\0", FnDestroyImage);
            let map_buffer = sym!(libva, b"vaMapBuffer\0", FnMapBuffer);
            let unmap_buffer = sym!(libva, b"vaUnmapBuffer\0", FnUnmapBuffer);

            Ok(Self {
                _libva: libva,
                _libva_drm: libva_drm,
                get_display_drm,
                initialize,
                terminate,
                max_num_profiles,
                query_config_profiles,
                create_config,
                destroy_config,
                create_surfaces,
                destroy_surfaces,
                create_context,
                destroy_context,
                create_buffer,
                destroy_buffer,
                begin_picture,
                render_picture,
                end_picture,
                sync_surface,
                derive_image,
                destroy_image,
                map_buffer,
                unmap_buffer,
            })
        }
    }

    struct VaSession {
        api: VaApi,
        _drm_file: File,
        dpy: VaDisplay,
        config_id: VaConfigId,
        surface_id: VaSurfaceId,
        context_id: VaContextId,
        profile: VaCodecProfile,
        width: u32,
        height: u32,
    }

    // SAFETY: `VaSession` só é acessada sob `Mutex` exclusivo dentro de `VaapiVideoReader`,
    // garantindo acesso serializado aos handles VA-API/DRM.
    unsafe impl Send for VaSession {}

    impl Drop for VaSession {
        fn drop(&mut self) {
            // SAFETY: handles criados por `vaInitialize`/`vaCreate*` nesta sessão,
            // destruídos na ordem inversa antes de fechar o descritor DRM.
            unsafe {
                if self.context_id != 0 {
                    let _ = (self.api.destroy_context)(self.dpy, self.context_id);
                }
                if self.surface_id != 0 {
                    let mut s = self.surface_id;
                    let _ = (self.api.destroy_surfaces)(self.dpy, &mut s, 1);
                }
                if self.config_id != 0 {
                    let _ = (self.api.destroy_config)(self.dpy, self.config_id);
                }
                if !self.dpy.is_null() {
                    let _ = (self.api.terminate)(self.dpy);
                }
            }
        }
    }

    impl VaSession {
        fn open(width: u32, height: u32, candidates: &[VaCodecProfile]) -> Result<Self> {
            if width == 0 || height == 0 || width > MAX_FALLBACK_DIM || height > MAX_FALLBACK_DIM {
                bail!("dimensões VA-API inválidas: {width}x{height}");
            }
            let api = VaApi::load()?;
            let mut drm_file: Option<File> = None;
            let mut dpy: VaDisplay = ptr::null_mut();
            for idx in 128..136 {
                let node = format!("/dev/dri/renderD{idx}");
                if let Ok(f) = OpenOptions::new().read(true).write(true).open(&node) {
                    // SAFETY: `f.as_raw_fd()` é um descritor DRM aberto e vivo.
                    let cand_dpy = unsafe { (api.get_display_drm)(f.as_raw_fd()) };
                    if !cand_dpy.is_null() {
                        let mut maj: c_int = 0;
                        let mut min: c_int = 0;
                        // SAFETY: `cand_dpy` não é nulo; ponteiros `maj`/`min` são válidos.
                        let st = unsafe { (api.initialize)(cand_dpy, &mut maj, &mut min) };
                        if st == VA_STATUS_SUCCESS {
                            drm_file = Some(f);
                            dpy = cand_dpy;
                            break;
                        }
                        // SAFETY: limpa display parcial que falhou em `vaInitialize`.
                        unsafe {
                            let _ = (api.terminate)(cand_dpy);
                        }
                    }
                }
            }
            let Some(drm_file) = drm_file else {
                bail!("nenhum nó DRM renderD* com driver VA-API ativo foi encontrado");
            };

            let mut session = Self {
                api,
                _drm_file: drm_file,
                dpy,
                config_id: 0,
                surface_id: 0,
                context_id: 0,
                profile: candidates[0],
                width,
                height,
            };

            // SAFETY: `session.dpy` foi inicializado com sucesso por `vaInitialize`.
            let max_prof = unsafe { (session.api.max_num_profiles)(session.dpy) };
            let cap = usize::try_from(max_prof).unwrap_or(0).clamp(1, 256);
            let mut supported = vec![0 as c_int; cap];
            let mut count: c_int = 0;
            // SAFETY: `supported` tem capacidade `cap` compatível com `vaMaxNumProfiles`.
            let st = unsafe {
                (session.api.query_config_profiles)(session.dpy, supported.as_mut_ptr(), &mut count)
            };
            if st != VA_STATUS_SUCCESS {
                bail!("vaQueryConfigProfiles falhou ({st})");
            }
            let n = usize::try_from(count).unwrap_or(0).min(supported.len());
            let chosen = candidates
                .iter()
                .copied()
                .find(|p| supported[..n].contains(&p.va_profile_id()))
                .ok_or_else(|| anyhow!("perfil VA-API não suportado pela GPU"))?;
            session.profile = chosen;

            let mut cfg_id: VaConfigId = 0;
            // SAFETY: `session.dpy` válido; `cfg_id` aponta para saída válida.
            let st = unsafe {
                (session.api.create_config)(
                    session.dpy,
                    chosen.va_profile_id(),
                    VA_ENTRYPOINT_VLD,
                    ptr::null_mut(),
                    0,
                    &mut cfg_id,
                )
            };
            if st != VA_STATUS_SUCCESS {
                bail!("vaCreateConfig falhou ({st})");
            }
            session.config_id = cfg_id;

            let mut surf: VaSurfaceId = 0;
            // SAFETY: aloca 1 superfície YUV420 no driver VA-API.
            let st = unsafe {
                (session.api.create_surfaces)(
                    session.dpy,
                    VA_RT_FORMAT_YUV420,
                    width,
                    height,
                    &mut surf,
                    1,
                    ptr::null_mut(),
                    0,
                )
            };
            if st != VA_STATUS_SUCCESS {
                bail!("vaCreateSurfaces falhou ({st})");
            }
            session.surface_id = surf;

            let mut ctx_id: VaContextId = 0;
            // SAFETY: cria contexto de decodificação ligado à superfície alocada.
            let st = unsafe {
                (session.api.create_context)(
                    session.dpy,
                    session.config_id,
                    width as c_int,
                    height as c_int,
                    VA_PROGRESSIVE,
                    &mut session.surface_id,
                    1,
                    &mut ctx_id,
                )
            };
            if st != VA_STATUS_SUCCESS {
                bail!("vaCreateContext falhou ({st})");
            }
            session.context_id = ctx_id;
            Ok(session)
        }

        fn decode_slice_buffer(&mut self, slice_bytes: &[u8]) -> Result<RgbaImage> {
            if slice_bytes.is_empty() || slice_bytes.len() > MAX_FALLBACK_PACKET {
                bail!("pacote VA-API com tamanho inválido");
            }
            let mut buf_id: VaBufferId = 0;
            // SAFETY: `slice_bytes` permanece válido durante `vaCreateBuffer`, que copia os dados.
            let st = unsafe {
                (self.api.create_buffer)(
                    self.dpy,
                    self.context_id,
                    VA_SLICE_DATA_BUFFER_TYPE,
                    slice_bytes.len() as c_uint,
                    1,
                    slice_bytes.as_ptr() as *mut c_void,
                    &mut buf_id,
                )
            };
            if st != VA_STATUS_SUCCESS {
                bail!("vaCreateBuffer falhou ({st})");
            }

            // SAFETY: inicia quadro, envia buffer e encerra picture na superfície da sessão.
            let render_res = unsafe {
                let s1 = (self.api.begin_picture)(self.dpy, self.context_id, self.surface_id);
                let s2 = if s1 == VA_STATUS_SUCCESS {
                    (self.api.render_picture)(self.dpy, self.context_id, &mut buf_id, 1)
                } else {
                    s1
                };
                let s3 = if s2 == VA_STATUS_SUCCESS {
                    (self.api.end_picture)(self.dpy, self.context_id)
                } else {
                    s2
                };
                let _ = (self.api.destroy_buffer)(self.dpy, buf_id);
                if s3 == VA_STATUS_SUCCESS {
                    (self.api.sync_surface)(self.dpy, self.surface_id)
                } else {
                    s3
                }
            };
            if render_res != VA_STATUS_SUCCESS {
                bail!("decodificação VA-API falhou (status {render_res})");
            }

            self.readback_surface()
        }

        fn readback_surface(&self) -> Result<RgbaImage> {
            let mut img = VaImage::default();
            // SAFETY: `self.surface_id` pertence a `self.dpy`; `img` é preenchido pelo driver.
            let st = unsafe { (self.api.derive_image)(self.dpy, self.surface_id, &mut img) };
            if st != VA_STATUS_SUCCESS {
                bail!("vaDeriveImage falhou ({st})");
            }
            let mut raw_ptr: *mut c_void = ptr::null_mut();
            // SAFETY: mapeia o buffer de imagem recém-derivado.
            let map_st = unsafe { (self.api.map_buffer)(self.dpy, img.buf, &mut raw_ptr) };
            if map_st != VA_STATUS_SUCCESS || raw_ptr.is_null() {
                // SAFETY: libera `img` antes de retornar erro.
                unsafe {
                    let _ = (self.api.destroy_image)(self.dpy, img.image_id);
                }
                bail!("vaMapBuffer falhou ({map_st})");
            }

            let data_size = usize::try_from(img.data_size)
                .unwrap_or(0)
                .min(256 * 1024 * 1024);
            // SAFETY: `raw_ptr` está mapeado pelo driver VA-API por `data_size` bytes até `vaUnmapBuffer`.
            let bytes = unsafe { std::slice::from_raw_parts(raw_ptr as *const u8, data_size) };
            let res = match img.format.fourcc {
                VA_FOURCC_NV12 => {
                    let y_off = img.offsets[0] as usize;
                    let uv_off = img.offsets[1] as usize;
                    let y_pitch = img.pitches[0] as usize;
                    let uv_pitch = img.pitches[1] as usize;
                    if y_off <= bytes.len() && uv_off <= bytes.len() {
                        nv12_to_rgba(
                            self.width,
                            self.height,
                            &bytes[y_off..],
                            y_pitch,
                            &bytes[uv_off..],
                            uv_pitch,
                        )
                    } else {
                        Err(anyhow!("offsets NV12 VA-API fora do buffer"))
                    }
                }
                VA_FOURCC_BGRA => {
                    let off = img.offsets[0] as usize;
                    let pitch = img.pitches[0] as usize;
                    bytes
                        .get(off..)
                        .ok_or_else(|| anyhow!("offset BGRA VA-API inválido"))
                        .and_then(|s| bgra_to_rgba(self.width, self.height, s, pitch))
                }
                VA_FOURCC_RGBA => {
                    let off = img.offsets[0] as usize;
                    let pitch = img.pitches[0] as usize;
                    let row_b = (self.width as usize).saturating_mul(4);
                    let mut out = vec![0u8; row_b.saturating_mul(self.height as usize)];
                    if let Some(src_all) = bytes.get(off..) {
                        for y in 0..self.height as usize {
                            if let Some(src) = src_all.get(y * pitch..y * pitch + row_b) {
                                out[y * row_b..(y + 1) * row_b].copy_from_slice(src);
                            }
                        }
                    }
                    RgbaImage::from_raw(self.width, self.height, out)
                        .ok_or_else(|| anyhow!("falha ao converter RGBA VA-API"))
                }
                other => Err(anyhow!("formato VAImage não suportado (fourcc {other:#x})")),
            };

            // SAFETY: desmapeia e destrói a imagem VA antes de retornar.
            unsafe {
                let _ = (self.api.unmap_buffer)(self.dpy, img.buf);
                let _ = (self.api.destroy_image)(self.dpy, img.image_id);
            }
            res
        }
    }

    pub struct VaapiVideoReader {
        path: PathBuf,
        file: Mutex<File>,
        session: Mutex<VaSession>,
        width: u32,
        height: u32,
        fps: FrameRate,
        samples: Vec<DemuxSample>,
    }

    impl SystemVideoReader for VaapiVideoReader {
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
                bail!("faixa VA-API sem amostras");
            }
            let idx = self
                .samples
                .partition_point(|s| s.pts <= t)
                .saturating_sub(1)
                .min(self.samples.len() - 1);
            let sample = &self.samples[idx];
            let sz = sample.size as usize;
            if sz == 0 || sz > MAX_FALLBACK_PACKET {
                bail!("pacote VA-API fora do limite ({sz} bytes)");
            }
            let mut pkt = vec![0u8; sz];
            {
                let mut f = self.file.lock().unwrap_or_else(PoisonError::into_inner);
                f.seek(SeekFrom::Start(sample.offset))
                    .with_context(|| format!("seek em {}", self.path.display()))?;
                f.read_exact(&mut pkt)?;
            }
            let mut sess = self.session.lock().unwrap_or_else(PoisonError::into_inner);
            // JPEG baseline pode ser decodificado diretamente pela GPU VA-API
            // com fallback imediato para software se o driver recusar o pacote.
            match sess.decode_slice_buffer(&pkt) {
                Ok(img) => Ok(img),
                Err(_) if sess.profile == VaCodecProfile::JpegBaseline => {
                    let img = image::load_from_memory_with_format(&pkt, image::ImageFormat::Jpeg)
                        .map_err(|e| anyhow!("fallback JPEG após VA-API falhou: {e}"))?;
                    Ok(img.to_rgba8())
                }
                Err(e) => Err(e),
            }
        }
    }

    pub fn open_media(path: &Path) -> Result<SystemLoaded> {
        let idx = demux::demux_file(path)?;
        let Some((ti, candidates)) = select_vaapi_video_track(&idx.tracks) else {
            bail!("contêiner sem trilha compatível com VA-API");
        };
        let tr = &idx.tracks[ti];
        let samples: Vec<DemuxSample> = idx.samples_for_track(ti).copied().collect();
        if samples.is_empty() {
            bail!("trilha de vídeo VA-API sem amostras");
        }
        let session = VaSession::open(tr.width, tr.height, candidates)?;
        let file = File::open(path).with_context(|| format!("abrindo {}", path.display()))?;
        let reader = VaapiVideoReader {
            path: path.to_path_buf(),
            file: Mutex::new(file),
            session: Mutex::new(session),
            width: tr.width,
            height: tr.height,
            fps: tr.fps.unwrap_or(FrameRate::FPS_30),
            samples,
        };
        // Áudio é aproveitado do decodificador fallback em Rust puro quando presente.
        let (audio, audio_truncated) = crate::media::fallback::open_media(path)
            .map(|fb| (fb.audio, fb.audio_truncated))
            .unwrap_or((None, false));
        Ok(SystemLoaded {
            backend: "Linux VA-API",
            duration: idx.duration,
            video: Some(Arc::new(reader)),
            audio,
            audio_truncated,
        })
    }
}

#[cfg(target_os = "linux")]
pub use linux_impl::open_media;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapeamento_perfis_vaapi_cobre_codecs_principais() {
        assert_eq!(
            VaCodecProfile::candidates_for_codec("avc1"),
            &[VaCodecProfile::H264High, VaCodecProfile::H264Main]
        );
        assert_eq!(
            VaCodecProfile::candidates_for_codec("V_MPEGH/ISO/HEVC"),
            &[VaCodecProfile::HevcMain]
        );
        assert_eq!(
            VaCodecProfile::candidates_for_codec("vp09"),
            &[VaCodecProfile::Vp9Profile0]
        );
        assert_eq!(
            VaCodecProfile::candidates_for_codec("V_AV1"),
            &[VaCodecProfile::Av1Profile0]
        );
        assert!(VaCodecProfile::candidates_for_codec("unknown").is_empty());
        assert_eq!(VaCodecProfile::H264High.va_profile_id(), 7);
        assert_eq!(VaCodecProfile::HevcMain.va_profile_id(), 17);
    }
}
