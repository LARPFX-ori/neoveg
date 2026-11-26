//! Fronteira FFI isolada para captura nativa de vídeo do sistema operacional
//! (telas/monitores, janelas de aplicativos, câmeras/webcams e espaço livre em
//! disco) — `AGENTS.md` §0.3, §0.4 e §2.3.
//!
//! - **Windows:** GDI (`EnumDisplayMonitors`, `BitBlt`, `GetDIBits`, cursor via
//!   `GetCursorInfo`/`DrawIconEx`), janelas (`EnumWindows`, `PrintWindow`),
//!   câmeras via Media Foundation (`MFEnumDeviceSources`, `IMFSourceReader`) e
//!   espaço livre via `GetDiskFreeSpaceExW`.
//! - **Linux:** carregamento dinâmico de `libX11.so.6` via `libloading` para
//!   captura X11 (sem dependência de headers C nem LGPL), enumeração de câmeras
//!   em `/sys/class/video4linux` e espaço livre via `statvfs` dinâmico em
//!   `libc.so.6`. Em ambientes sem servidor gráfico (Wayland puro / CI headless)
//!   retorna listas vazias ou erros recuperáveis sem nunca entrar em pânico.
//! - **Demais plataformas:** stubs seguros sem pânico; o gerador sintético
//!   (`SyntheticVideoGrabber`) funciona em todas as plataformas para testes.

#![allow(unsafe_code)]

use anyhow::{anyhow, bail, Result};
use image::RgbaImage;
use std::path::Path;

/// Limite superior seguro de dimensões capturadas (§0.4).
pub const MAX_CAPTURE_DIM: u32 = 8192;
/// Limite de janelas enumeradas para evitar custos excessivos em sistemas lotados.
const MAX_ENUM_WINDOWS: usize = 96;

/// Informações de um monitor/tela detectado no sistema operacional.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OsMonitorInfo {
    pub id: String,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub is_primary: bool,
}

/// Informações de uma janela visível detectada no sistema operacional.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OsWindowInfo {
    pub id: String,
    pub title: String,
    pub width: u32,
    pub height: u32,
}

/// Informações de um dispositivo de câmera/webcam detectado no sistema operacional.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OsCameraInfo {
    pub id: String,
    pub name: String,
    pub default_width: u32,
    pub default_height: u32,
}

/// Origem concreta para abertura de um capturador de vídeo (`OsVideoGrabber`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VideoGrabTarget {
    Screen {
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    },
    Window {
        window_id: String,
    },
    Camera {
        device_id: String,
    },
    Synthetic {
        width: u32,
        height: u32,
        seed: u32,
    },
}

/// Contrato de captura de quadros RGBA em streaming, executado em thread de
/// trabalho fora da UI (§3.6).
pub trait VideoGrabber: Send {
    fn grab_frame(&mut self, include_cursor: bool) -> Result<RgbaImage>;
}

/// Capturador determinístico em Rust puro para testes automatizados e validação
/// sem hardware físico (barras de cor animadas com deslocamento por quadro).
pub struct SyntheticVideoGrabber {
    width: u32,
    height: u32,
    seed: u32,
    frame_idx: u64,
}

impl SyntheticVideoGrabber {
    pub fn new(width: u32, height: u32, seed: u32) -> Self {
        Self {
            width: width.clamp(16, MAX_CAPTURE_DIM),
            height: height.clamp(16, MAX_CAPTURE_DIM),
            seed,
            frame_idx: 0,
        }
    }
}

impl VideoGrabber for SyntheticVideoGrabber {
    fn grab_frame(&mut self, _include_cursor: bool) -> Result<RgbaImage> {
        let w = self.width;
        let h = self.height;
        let shift =
            ((self.frame_idx.wrapping_mul(7)) as u32).wrapping_add(self.seed.wrapping_mul(31));
        self.frame_idx = self.frame_idx.saturating_add(1);
        let mut rgba = vec![0u8; (w as usize).saturating_mul(h as usize).saturating_mul(4)];
        let palette: [[u8; 3]; 7] = [
            [210, 210, 210],
            [210, 210, 24],
            [24, 210, 210],
            [24, 200, 32],
            [200, 28, 200],
            [210, 28, 28],
            [28, 40, 210],
        ];
        for y in 0..h as usize {
            let row_off = y * (w as usize) * 4;
            let bar_band = y < (h as usize * 3) / 4;
            for x in 0..w as usize {
                let off = row_off + x * 4;
                let (r, g, b) = if bar_band {
                    let idx = ((x + (shift as usize % (w as usize).max(1))) * palette.len()
                        / (w as usize).max(1))
                        % palette.len();
                    let c = palette[idx];
                    (c[0], c[1], c[2])
                } else {
                    let v = (((x as u32).wrapping_add(shift)) & 0xFF) as u8;
                    (v, v / 2, 255u8.saturating_sub(v))
                };
                rgba[off] = r;
                rgba[off + 1] = g;
                rgba[off + 2] = b;
                rgba[off + 3] = 255;
            }
        }
        RgbaImage::from_raw(w, h, rgba).ok_or_else(|| anyhow!("falha ao gerar quadro sintético"))
    }
}

/// Converte buffer BGRA top-down em `RgbaImage` opaco (alfa = 255).
pub(crate) fn bgra_top_down_to_rgba(bgra: &[u8], w: u32, h: u32) -> Result<RgbaImage> {
    if w == 0 || h == 0 || w > MAX_CAPTURE_DIM || h > MAX_CAPTURE_DIM {
        bail!("dimensões de captura inválidas: {w}x{h}");
    }
    let expected = (w as usize)
        .checked_mul(h as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or_else(|| anyhow!("estouro de dimensões na captura {w}x{h}"))?;
    if bgra.len() < expected {
        bail!(
            "buffer BGRA incompleto: {} bytes para {w}x{h} (esperado {expected})",
            bgra.len()
        );
    }
    let mut rgba = vec![0u8; expected];
    for i in (0..expected).step_by(4) {
        rgba[i] = bgra[i + 2];
        rgba[i + 1] = bgra[i + 1];
        rgba[i + 2] = bgra[i];
        rgba[i + 3] = 255;
    }
    RgbaImage::from_raw(w, h, rgba).ok_or_else(|| anyhow!("falha ao montar RgbaImage"))
}

/// Redimensiona `img` para `(target_w, target_h)` se necessário, mantendo dimensões
/// pares (>= 2) para compatibilidade com codificadores YUV 4:2:0 (H.264/HEVC).
pub fn normalize_frame_size(img: RgbaImage, target_w: u32, target_h: u32) -> RgbaImage {
    let tw = ((target_w.clamp(16, MAX_CAPTURE_DIM)) / 2).max(1) * 2;
    let th = ((target_h.clamp(16, MAX_CAPTURE_DIM)) / 2).max(1) * 2;
    if img.width() == tw && img.height() == th {
        return img;
    }
    image::imageops::resize(&img, tw, th, image::imageops::FilterType::Triangle)
}

/// Abre um capturador para `target`.
pub fn open_video_grabber(target: &VideoGrabTarget) -> Result<Box<dyn VideoGrabber>> {
    match target {
        VideoGrabTarget::Synthetic {
            width,
            height,
            seed,
        } => Ok(Box::new(SyntheticVideoGrabber::new(*width, *height, *seed))),
        other => platform::open_grabber(other),
    }
}

/// Enumera telas/monitores disponíveis no sistema operacional.
pub fn list_monitors() -> Vec<OsMonitorInfo> {
    platform::list_monitors()
}

/// Enumera janelas visíveis de aplicativos disponíveis para captura.
pub fn list_windows() -> Vec<OsWindowInfo> {
    platform::list_windows()
}

/// Enumera câmeras/webcams disponíveis no sistema operacional.
pub fn list_cameras() -> Vec<OsCameraInfo> {
    platform::list_cameras()
}

/// Retorna o espaço livre em disco (em bytes) no volume que contém `dir`, ou
/// `None` se não puder ser consultado.
pub fn free_disk_bytes(dir: &Path) -> Option<u64> {
    // Caminha até o primeiro ancestral existente para suportar pastas ainda não criadas.
    let mut probe = dir;
    while !probe.exists() {
        match probe.parent() {
            Some(p) if p != probe => probe = p,
            _ => break,
        }
    }
    platform::free_disk_bytes(probe)
}

/// Formata bytes em texto legível (`1537,4 GB`, `420,5 MB`, etc.).
pub fn format_bytes_human(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.1} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.0} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

// ---------------------------------------------------------------------------
// Backend Windows (GDI + PrintWindow + Media Foundation + GetDiskFreeSpaceExW)
// ---------------------------------------------------------------------------
#[cfg(windows)]
mod platform {
    use super::{
        bgra_top_down_to_rgba, OsCameraInfo, OsMonitorInfo, OsWindowInfo, VideoGrabTarget,
        VideoGrabber, MAX_CAPTURE_DIM, MAX_ENUM_WINDOWS,
    };
    use crate::media::mf::{configure_video_stream, decode_rgb32_sample, MfGuard, MfVideoInfo};
    use anyhow::{anyhow, bail, Result};
    use image::RgbaImage;
    use std::ffi::c_void;
    use std::mem::size_of;
    use std::path::Path;
    use windows::core::{HSTRING, PCWSTR, PWSTR};
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM, RECT};
    use windows::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject,
        EnumDisplayMonitors, GetDC, GetDIBits, GetMonitorInfoW, ReleaseDC, SelectObject,
        BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, HBITMAP, HBRUSH, HDC, HGDIOBJ, HMONITOR,
        MONITORINFO, MONITORINFOEXW, SRCCOPY,
    };
    use windows::Win32::Media::MediaFoundation::{
        IMFActivate, IMFMediaSource, IMFSourceReader, MFCreateAttributes,
        MFCreateSourceReaderFromMediaSource, MFEnumDeviceSources,
        MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME, MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
        MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
        MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK, MF_SOURCE_READERF_ERROR,
        MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, MF_SOURCE_READER_FIRST_VIDEO_STREAM,
    };
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    use windows::Win32::Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS};
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::WindowsAndMessaging::{
        DrawIconEx, EnumWindows, GetCursorInfo, GetIconInfo, GetWindowRect, GetWindowTextW,
        IsIconic, IsWindow, IsWindowVisible, CURSORINFO, CURSOR_SHOWING, DI_NORMAL, HICON,
        ICONINFO,
    };

    const MONITORINFOF_PRIMARY_FLAG: u32 = 1;
    const PW_RENDERFULLCONTENT: PRINT_WINDOW_FLAGS = PRINT_WINDOW_FLAGS(2u32);

    struct DcReleaseGuard {
        hwnd: HWND,
        hdc: HDC,
    }
    impl Drop for DcReleaseGuard {
        fn drop(&mut self) {
            if !self.hdc.is_invalid() {
                // SAFETY: `hdc` foi obtido por `GetDC(self.hwnd)` e liberado uma única vez.
                unsafe {
                    let _ = ReleaseDC(self.hwnd, self.hdc);
                }
            }
        }
    }

    struct MemDcGuard(HDC);
    impl Drop for MemDcGuard {
        fn drop(&mut self) {
            if !self.0.is_invalid() {
                // SAFETY: `self.0` foi criado por `CreateCompatibleDC`.
                unsafe {
                    let _ = DeleteDC(self.0);
                }
            }
        }
    }

    struct BitmapGuard(HBITMAP);
    impl Drop for BitmapGuard {
        fn drop(&mut self) {
            if !self.0.is_invalid() {
                // SAFETY: `self.0` é um `HBITMAP` próprio não selecionado em nenhum DC.
                unsafe {
                    let _ = DeleteObject(HGDIOBJ(self.0 .0));
                }
            }
        }
    }

    struct SelectObjGuard {
        hdc: HDC,
        prev: HGDIOBJ,
    }
    impl Drop for SelectObjGuard {
        fn drop(&mut self) {
            if !self.hdc.is_invalid() && !self.prev.is_invalid() {
                // SAFETY: restaura o objeto GDI anterior antes da destruição do bitmap e do DC.
                unsafe {
                    let _ = SelectObject(self.hdc, self.prev);
                }
            }
        }
    }

    unsafe extern "system" fn enum_monitors_cb(
        hmon: HMONITOR,
        _hdc: HDC,
        _rect: *mut RECT,
        lparam: LPARAM,
    ) -> BOOL {
        if lparam.0 == 0 {
            return BOOL(0);
        }
        // SAFETY: `lparam` aponta para `Vec<OsMonitorInfo>` vivo durante `EnumDisplayMonitors`.
        let list = unsafe { &mut *(lparam.0 as *mut Vec<OsMonitorInfo>) };
        let mut info = MONITORINFOEXW {
            monitorInfo: MONITORINFO {
                cbSize: size_of::<MONITORINFOEXW>() as u32,
                ..Default::default()
            },
            ..Default::default()
        };
        // SAFETY: `info` tem `cbSize = size_of::<MONITORINFOEXW>()` conforme Win32.
        let ok =
            unsafe { GetMonitorInfoW(hmon, &mut info.monitorInfo as *mut MONITORINFO).as_bool() };
        if !ok {
            return BOOL(1);
        }
        let rc = info.monitorInfo.rcMonitor;
        let w = rc.right.saturating_sub(rc.left).max(0) as u32;
        let h = rc.bottom.saturating_sub(rc.top).max(0) as u32;
        if w == 0 || h == 0 || w > MAX_CAPTURE_DIM || h > MAX_CAPTURE_DIM {
            return BOOL(1);
        }
        let dev_len = info
            .szDevice
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(info.szDevice.len());
        let dev = String::from_utf16_lossy(&info.szDevice[..dev_len]);
        let idx = list.len() + 1;
        let is_primary = (info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY_FLAG) != 0;
        let id = if dev.is_empty() {
            format!("monitor:{idx}")
        } else {
            dev
        };
        let name = if is_primary {
            format!("Tela {idx} (Principal) — {w}×{h}")
        } else {
            format!("Tela {idx} — {w}×{h}")
        };
        list.push(OsMonitorInfo {
            id,
            name,
            x: rc.left,
            y: rc.top,
            width: w,
            height: h,
            is_primary,
        });
        BOOL(1)
    }

    pub fn list_monitors() -> Vec<OsMonitorInfo> {
        let mut out: Vec<OsMonitorInfo> = Vec::new();
        // SAFETY: ponteiro para `out` permanece válido até o retorno de `EnumDisplayMonitors`.
        unsafe {
            let _ = EnumDisplayMonitors(
                HDC::default(),
                None,
                Some(enum_monitors_cb),
                LPARAM(&mut out as *mut Vec<OsMonitorInfo> as isize),
            );
        }
        out
    }

    unsafe extern "system" fn enum_windows_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        if lparam.0 == 0 {
            return BOOL(0);
        }
        // SAFETY: `lparam` aponta para `Vec<OsWindowInfo>` vivo durante `EnumWindows`.
        let list = unsafe { &mut *(lparam.0 as *mut Vec<OsWindowInfo>) };
        if list.len() >= MAX_ENUM_WINDOWS {
            return BOOL(0);
        }
        // SAFETY: consultas somente leitura no `HWND` enumerado pelo sistema.
        let visible = unsafe { IsWindowVisible(hwnd).as_bool() };
        let iconic = unsafe { IsIconic(hwnd).as_bool() };
        if !visible || iconic {
            return BOOL(1);
        }
        let mut buf = [0u16; 256];
        // SAFETY: `buf` é um slice mutável válido na stack.
        let len = unsafe { GetWindowTextW(hwnd, &mut buf) };
        if len <= 0 {
            return BOOL(1);
        }
        let title = String::from_utf16_lossy(&buf[..(len as usize).min(buf.len())])
            .trim()
            .to_string();
        if title.is_empty() {
            return BOOL(1);
        }
        let mut rc = RECT::default();
        // SAFETY: `rc` é ponteiro válido para `RECT`.
        if unsafe { GetWindowRect(hwnd, &mut rc) }.is_err() {
            return BOOL(1);
        }
        let w = rc.right.saturating_sub(rc.left).max(0) as u32;
        let h = rc.bottom.saturating_sub(rc.top).max(0) as u32;
        if w < 64 || h < 64 || w > MAX_CAPTURE_DIM || h > MAX_CAPTURE_DIM {
            return BOOL(1);
        }
        list.push(OsWindowInfo {
            id: format!("hwnd:{}", hwnd.0 as usize),
            title,
            width: w,
            height: h,
        });
        BOOL(1)
    }

    pub fn list_windows() -> Vec<OsWindowInfo> {
        let mut out: Vec<OsWindowInfo> = Vec::new();
        // SAFETY: `out` vive durante toda a execução síncrona de `EnumWindows`.
        unsafe {
            let _ = EnumWindows(
                Some(enum_windows_cb),
                LPARAM(&mut out as *mut Vec<OsWindowInfo> as isize),
            );
        }
        out
    }

    fn read_mf_attr_string(act: &IMFActivate, key: &windows::core::GUID) -> Option<String> {
        let mut pwstr = PWSTR::null();
        let mut len: u32 = 0;
        // SAFETY: `GetAllocatedString` aloca UTF-16 com `CoTaskMemAlloc`; liberamos
        // sempre com `CoTaskMemFree` logo após copiar para `String`.
        unsafe {
            if act.GetAllocatedString(key, &mut pwstr, &mut len).is_err() || pwstr.is_null() {
                return None;
            }
            let slice = std::slice::from_raw_parts(pwstr.0, len as usize);
            let s = String::from_utf16_lossy(slice);
            CoTaskMemFree(Some(pwstr.0 as *const c_void));
            Some(s)
        }
    }

    /// Libera array de `IMFActivate` retornado por `MFEnumDeviceSources`.
    fn enum_mf_video_devices() -> Result<(MfGuard, Vec<IMFActivate>)> {
        let guard = MfGuard::enter()?;
        let mut attrs = None;
        // SAFETY: cria `IMFAttributes` para filtrar dispositivos de captura de vídeo.
        unsafe { MFCreateAttributes(&mut attrs, 1) }
            .map_err(|e| anyhow!("MFCreateAttributes (vidcap) falhou: {e}"))?;
        let attrs = attrs.ok_or_else(|| anyhow!("IMFAttributes nulo"))?;
        // SAFETY: define filtro `MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE = VIDCAP`.
        unsafe {
            attrs.SetGUID(
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
            )
        }
        .map_err(|e| anyhow!("SetGUID (vidcap) falhou: {e}"))?;

        let mut raw_ptr: *mut Option<IMFActivate> = std::ptr::null_mut();
        let mut count: u32 = 0;
        // SAFETY: `MFEnumDeviceSources` escreve o array alocado com `CoTaskMemAlloc`
        // em `raw_ptr` e a contagem em `count`.
        unsafe { MFEnumDeviceSources(&attrs, &mut raw_ptr, &mut count) }
            .map_err(|e| anyhow!("MFEnumDeviceSources falhou: {e}"))?;

        let mut list = Vec::new();
        if !raw_ptr.is_null() {
            // SAFETY: `raw_ptr` contém `count` ponteiros `Option<IMFActivate>` válidos.
            // Assumimos propriedade de cada interface e liberamos o array com `CoTaskMemFree`.
            unsafe {
                let slice = std::slice::from_raw_parts_mut(raw_ptr, count as usize);
                for slot in slice.iter_mut() {
                    if let Some(act) = slot.take() {
                        list.push(act);
                    }
                }
                CoTaskMemFree(Some(raw_ptr as *const c_void));
            }
        }
        Ok((guard, list))
    }

    pub fn list_cameras() -> Vec<OsCameraInfo> {
        let Ok((_guard, devices)) = enum_mf_video_devices() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for (i, act) in devices.iter().enumerate() {
            let name = read_mf_attr_string(act, &MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME)
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| format!("Câmera {}", i + 1));
            let sym = read_mf_attr_string(
                act,
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK,
            )
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| format!("cam:{i}"));
            out.push(OsCameraInfo {
                id: sym,
                name,
                default_width: 1280,
                default_height: 720,
            });
        }
        out
    }

    fn draw_cursor_on_dc(mem_dc: HDC, origin_x: i32, origin_y: i32, w: u32, h: u32) {
        let mut ci = CURSORINFO {
            cbSize: size_of::<CURSORINFO>() as u32,
            ..Default::default()
        };
        // SAFETY: `ci.cbSize` preenchido conforme especificação Win32.
        if unsafe { GetCursorInfo(&mut ci) }.is_err() {
            return;
        }
        if (ci.flags.0 & CURSOR_SHOWING.0) == 0 || ci.hCursor.is_invalid() {
            return;
        }
        let icon = HICON(ci.hCursor.0);
        let mut ii = ICONINFO::default();
        let (hx, hy) = {
            // SAFETY: consulta hotspot e bitmaps do ícone; destruímos os bitmaps retornados.
            if unsafe { GetIconInfo(icon, &mut ii) }.is_ok() {
                if !ii.hbmMask.is_invalid() {
                    unsafe {
                        let _ = DeleteObject(HGDIOBJ(ii.hbmMask.0));
                    }
                }
                if !ii.hbmColor.is_invalid() {
                    unsafe {
                        let _ = DeleteObject(HGDIOBJ(ii.hbmColor.0));
                    }
                }
                (ii.xHotspot as i32, ii.yHotspot as i32)
            } else {
                (0, 0)
            }
        };
        let rel_x = ci.ptScreenPos.x - origin_x - hx;
        let rel_y = ci.ptScreenPos.y - origin_y - hy;
        if rel_x > -(64) && rel_y > -(64) && rel_x < w as i32 + 64 && rel_y < h as i32 + 64 {
            // SAFETY: `mem_dc` é um DC de memória válido com o bitmap selecionado.
            unsafe {
                let _ = DrawIconEx(
                    mem_dc,
                    rel_x,
                    rel_y,
                    icon,
                    0,
                    0,
                    0,
                    HBRUSH::default(),
                    DI_NORMAL,
                );
            }
        }
    }

    fn read_bitmap_rgba(mem_dc: HDC, bmp: HBITMAP, w: u32, h: u32) -> Result<RgbaImage> {
        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w as i32,
                biHeight: -(h as i32), // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: 0, // BI_RGB
                ..Default::default()
            },
            ..Default::default()
        };
        let byte_len = (w as usize)
            .checked_mul(h as usize)
            .and_then(|n| n.checked_mul(4))
            .ok_or_else(|| anyhow!("dimensão GDI inválida: {w}x{h}"))?;
        let mut bgra = vec![0u8; byte_len];
        // SAFETY: `bgra` tem exatamente `w * h * 4` bytes para DIB 32-bit BI_RGB top-down.
        let lines = unsafe {
            GetDIBits(
                mem_dc,
                bmp,
                0,
                h,
                Some(bgra.as_mut_ptr() as *mut c_void),
                &mut bmi,
                DIB_RGB_COLORS,
            )
        };
        if lines == 0 {
            bail!("GetDIBits falhou ao extrair pixels da captura");
        }
        bgra_top_down_to_rgba(&bgra, w, h)
    }

    struct WinScreenGrabber {
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    }

    impl VideoGrabber for WinScreenGrabber {
        fn grab_frame(&mut self, include_cursor: bool) -> Result<RgbaImage> {
            let w = self.width.clamp(16, MAX_CAPTURE_DIM);
            let h = self.height.clamp(16, MAX_CAPTURE_DIM);
            // SAFETY: `GetDC(HWND::default())` retorna o DC da área de trabalho virtual;
            // guardado em RAII para liberação garantida.
            let screen_dc = unsafe { GetDC(HWND::default()) };
            if screen_dc.is_invalid() {
                bail!("GetDC da área de trabalho falhou");
            }
            let _screen_guard = DcReleaseGuard {
                hwnd: HWND::default(),
                hdc: screen_dc,
            };
            // SAFETY: `screen_dc` é válido.
            let mem_dc = unsafe { CreateCompatibleDC(screen_dc) };
            if mem_dc.is_invalid() {
                bail!("CreateCompatibleDC falhou");
            }
            let _mem_guard = MemDcGuard(mem_dc);
            // SAFETY: cria bitmap compatível com o DC da tela.
            let bmp = unsafe { CreateCompatibleBitmap(screen_dc, w as i32, h as i32) };
            if bmp.is_invalid() {
                bail!("CreateCompatibleBitmap ({w}x{h}) falhou");
            }
            let _bmp_guard = BitmapGuard(bmp);
            // SAFETY: seleciona `bmp` em `mem_dc`; `SelectObjGuard` restaura antes do `Drop` de `bmp`.
            let prev = unsafe { SelectObject(mem_dc, HGDIOBJ(bmp.0)) };
            let _sel_guard = SelectObjGuard { hdc: mem_dc, prev };
            // SAFETY: copia retângulo da tela virtual para `mem_dc`.
            unsafe {
                BitBlt(
                    mem_dc, 0, 0, w as i32, h as i32, screen_dc, self.x, self.y, SRCCOPY,
                )
            }
            .map_err(|e| anyhow!("BitBlt da tela falhou: {e}"))?;
            if include_cursor {
                draw_cursor_on_dc(mem_dc, self.x, self.y, w, h);
            }
            read_bitmap_rgba(mem_dc, bmp, w, h)
        }
    }

    struct WinWindowGrabber {
        hwnd_raw: usize,
    }

    impl VideoGrabber for WinWindowGrabber {
        fn grab_frame(&mut self, include_cursor: bool) -> Result<RgbaImage> {
            let hwnd = HWND(self.hwnd_raw as *mut c_void);
            // SAFETY: verifica se o `HWND` ainda existe antes de consultá-lo.
            if !unsafe { IsWindow(hwnd).as_bool() } {
                bail!("A janela selecionada foi fechada.");
            }
            let mut rc = RECT::default();
            // SAFETY: `rc` válido na stack.
            unsafe { GetWindowRect(hwnd, &mut rc) }
                .map_err(|e| anyhow!("GetWindowRect falhou: {e}"))?;
            let w = rc.right.saturating_sub(rc.left).max(0) as u32;
            let h = rc.bottom.saturating_sub(rc.top).max(0) as u32;
            if w < 16 || h < 16 || w > MAX_CAPTURE_DIM || h > MAX_CAPTURE_DIM {
                bail!("Dimensões de janela inválidas para captura ({w}x{h}).");
            }
            // SAFETY: obtém DC da tela para compatibilidade de formato de cor.
            let screen_dc = unsafe { GetDC(HWND::default()) };
            if screen_dc.is_invalid() {
                bail!("GetDC falhou");
            }
            let _screen_guard = DcReleaseGuard {
                hwnd: HWND::default(),
                hdc: screen_dc,
            };
            // SAFETY: cria DC de memória e bitmap compatíveis.
            let mem_dc = unsafe { CreateCompatibleDC(screen_dc) };
            if mem_dc.is_invalid() {
                bail!("CreateCompatibleDC falhou");
            }
            let _mem_guard = MemDcGuard(mem_dc);
            let bmp = unsafe { CreateCompatibleBitmap(screen_dc, w as i32, h as i32) };
            if bmp.is_invalid() {
                bail!("CreateCompatibleBitmap ({w}x{h}) falhou");
            }
            let _bmp_guard = BitmapGuard(bmp);
            let prev = unsafe { SelectObject(mem_dc, HGDIOBJ(bmp.0)) };
            let _sel_guard = SelectObjGuard { hdc: mem_dc, prev };

            // SAFETY: tenta `PrintWindow` com `PW_RENDERFULLCONTENT` (captura janelas GPU/DWM).
            let printed = unsafe { PrintWindow(hwnd, mem_dc, PW_RENDERFULLCONTENT).as_bool() };
            if !printed {
                // Fallback por região na tela virtual.
                unsafe {
                    BitBlt(
                        mem_dc, 0, 0, w as i32, h as i32, screen_dc, rc.left, rc.top, SRCCOPY,
                    )
                }
                .map_err(|e| anyhow!("Captura da janela falhou: {e}"))?;
            }
            if include_cursor {
                draw_cursor_on_dc(mem_dc, rc.left, rc.top, w, h);
            }
            read_bitmap_rgba(mem_dc, bmp, w, h)
        }
    }

    struct WinCameraGrabber {
        // `reader` deve ser descartado antes de `_guard` (que chama `MFShutdown`).
        reader: Option<IMFSourceReader>,
        info: MfVideoInfo,
        _guard: MfGuard,
    }

    // SAFETY: `WinCameraGrabber` é movido integralmente para uma única thread de
    // captura (nunca compartilhado concorrentemente), e o `MfGuard` inicializa
    // COM MTA (`COINIT_MULTITHREADED`) na thread que cria e lê o leitor.
    unsafe impl Send for WinCameraGrabber {}

    impl WinCameraGrabber {
        fn open(device_id: &str) -> Result<Self> {
            let (guard, devices) = enum_mf_video_devices()?;
            let mut chosen: Option<IMFActivate> = None;
            for (i, act) in devices.into_iter().enumerate() {
                let sym = read_mf_attr_string(
                    &act,
                    &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK,
                )
                .unwrap_or_else(|| format!("cam:{i}"));
                if sym == device_id || (chosen.is_none() && device_id.is_empty()) {
                    chosen = Some(act);
                    break;
                }
            }
            let act = chosen.ok_or_else(|| anyhow!("Câmera não encontrada ou desconectada."))?;
            // SAFETY: ativa a interface `IMFMediaSource` do dispositivo selecionado.
            let source: IMFMediaSource = unsafe { act.ActivateObject() }
                .map_err(|e| anyhow!("Não foi possível ativar a câmera: {e}"))?;
            let mut attrs = None;
            // SAFETY: habilita conversão de espaço de cor (NV12/YUY2/MJPG -> RGB32).
            unsafe { MFCreateAttributes(&mut attrs, 1) }
                .map_err(|e| anyhow!("MFCreateAttributes (câmera) falhou: {e}"))?;
            if let Some(a) = &attrs {
                unsafe { a.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1) }
                    .map_err(|e| anyhow!("Enable video processing na câmera falhou: {e}"))?;
            }
            // SAFETY: cria `IMFSourceReader` sobre `IMFMediaSource` válido.
            let reader = unsafe { MFCreateSourceReaderFromMediaSource(&source, attrs.as_ref()) }
                .map_err(|e| anyhow!("MFCreateSourceReaderFromMediaSource falhou: {e}"))?;
            let info = configure_video_stream(&reader)
                .ok_or_else(|| anyhow!("A câmera não forneceu um fluxo de vídeo compatível."))?;
            Ok(Self {
                reader: Some(reader),
                info,
                _guard: guard,
            })
        }
    }

    impl Drop for WinCameraGrabber {
        fn drop(&mut self) {
            drop(self.reader.take());
        }
    }

    impl VideoGrabber for WinCameraGrabber {
        fn grab_frame(&mut self, _include_cursor: bool) -> Result<RgbaImage> {
            let Some(reader) = self.reader.as_ref() else {
                bail!("Leitor da câmera encerrado");
            };
            let stream = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
            for _ in 0..8 {
                let mut flags = 0u32;
                let mut sample = None;
                // SAFETY: `reader` válido; ponteiros de saída válidos na stack.
                unsafe {
                    reader.ReadSample(stream, 0, None, Some(&mut flags), None, Some(&mut sample))
                }
                .map_err(|e| anyhow!("Leitura de quadro da câmera falhou: {e}"))?;
                if (flags & MF_SOURCE_READERF_ERROR.0 as u32) != 0 {
                    bail!("Erro reportado pelo dispositivo de câmera.");
                }
                if let Some(s) = sample {
                    return decode_rgb32_sample(
                        &s,
                        self.info.width,
                        self.info.height,
                        self.info.stride,
                    );
                }
            }
            bail!("Nenhum quadro recebido da câmera.")
        }
    }

    pub fn open_grabber(target: &VideoGrabTarget) -> Result<Box<dyn VideoGrabber>> {
        match target {
            VideoGrabTarget::Screen {
                x,
                y,
                width,
                height,
            } => Ok(Box::new(WinScreenGrabber {
                x: *x,
                y: *y,
                width: *width,
                height: *height,
            })),
            VideoGrabTarget::Window { window_id } => {
                let raw = window_id
                    .strip_prefix("hwnd:")
                    .and_then(|s| s.parse::<usize>().ok())
                    .ok_or_else(|| anyhow!("Identificador de janela inválido: {window_id}"))?;
                Ok(Box::new(WinWindowGrabber { hwnd_raw: raw }))
            }
            VideoGrabTarget::Camera { device_id } => {
                Ok(Box::new(WinCameraGrabber::open(device_id)?))
            }
            VideoGrabTarget::Synthetic {
                width,
                height,
                seed,
            } => Ok(Box::new(super::SyntheticVideoGrabber::new(
                *width, *height, *seed,
            ))),
        }
    }

    pub fn free_disk_bytes(dir: &Path) -> Option<u64> {
        let hpath = HSTRING::from(dir.as_os_str());
        let mut avail: u64 = 0;
        // SAFETY: `hpath` permanece vivo durante `GetDiskFreeSpaceExW`.
        unsafe { GetDiskFreeSpaceExW(PCWSTR(hpath.as_ptr()), Some(&mut avail), None, None) }
            .ok()?;
        Some(avail)
    }
}

// ---------------------------------------------------------------------------
// Backend Linux (libX11 dinâmica via libloading + sysfs video4linux + statvfs)
// ---------------------------------------------------------------------------
#[cfg(target_os = "linux")]
mod platform {
    use super::{
        OsCameraInfo, OsMonitorInfo, OsWindowInfo, VideoGrabTarget, VideoGrabber, MAX_CAPTURE_DIM,
        MAX_ENUM_WINDOWS,
    };
    use anyhow::{anyhow, bail, Result};
    use image::RgbaImage;
    use libloading::Library;
    use std::ffi::{c_char, c_int, c_uint, c_ulong, c_void, CStr, CString};
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    const ZPIXMAP: c_int = 2;
    const IS_VIEWABLE: c_int = 2;

    #[repr(C)]
    struct XImage {
        width: c_int,
        height: c_int,
        xoffset: c_int,
        format: c_int,
        data: *mut c_char,
        byte_order: c_int,
        bitmap_unit: c_int,
        bitmap_bit_order: c_int,
        bitmap_pad: c_int,
        depth: c_int,
        bytes_per_line: c_int,
        bits_per_pixel: c_int,
        red_mask: c_ulong,
        green_mask: c_ulong,
        blue_mask: c_ulong,
        obdata: *mut c_void,
        create_image: *mut c_void,
        destroy_image: Option<unsafe extern "C" fn(*mut XImage) -> c_int>,
    }

    #[repr(C)]
    struct XWindowAttributes {
        x: c_int,
        y: c_int,
        width: c_int,
        height: c_int,
        border_width: c_int,
        depth: c_int,
        visual: *mut c_void,
        root: c_ulong,
        class: c_int,
        bit_gravity: c_int,
        win_gravity: c_int,
        backing_store: c_int,
        backing_planes: c_ulong,
        backing_pixel: c_ulong,
        save_under: c_int,
        colormap: c_ulong,
        map_installed: c_int,
        map_state: c_int,
        all_event_masks: i64,
        your_event_mask: i64,
        do_not_propagate_mask: i64,
        override_redirect: c_int,
        screen: *mut c_void,
    }

    struct X11Lib {
        _lib: Library,
        open_display: unsafe extern "C" fn(*const c_char) -> *mut c_void,
        close_display: unsafe extern "C" fn(*mut c_void) -> c_int,
        default_screen: unsafe extern "C" fn(*mut c_void) -> c_int,
        root_window: unsafe extern "C" fn(*mut c_void, c_int) -> c_ulong,
        display_width: unsafe extern "C" fn(*mut c_void, c_int) -> c_int,
        display_height: unsafe extern "C" fn(*mut c_void, c_int) -> c_int,
        get_image: unsafe extern "C" fn(
            *mut c_void,
            c_ulong,
            c_int,
            c_int,
            c_uint,
            c_uint,
            c_ulong,
            c_int,
        ) -> *mut XImage,
        query_tree: unsafe extern "C" fn(
            *mut c_void,
            c_ulong,
            *mut c_ulong,
            *mut c_ulong,
            *mut *mut c_ulong,
            *mut c_uint,
        ) -> c_int,
        get_window_attributes:
            unsafe extern "C" fn(*mut c_void, c_ulong, *mut XWindowAttributes) -> c_int,
        fetch_name: unsafe extern "C" fn(*mut c_void, c_ulong, *mut *mut c_char) -> c_int,
        xfree: unsafe extern "C" fn(*mut c_void) -> c_int,
    }

    impl X11Lib {
        fn load() -> Result<Self> {
            // SAFETY: carregamento dinâmico da biblioteca do sistema `libX11.so.6`.
            let lib = unsafe { Library::new("libX11.so.6") }
                .map_err(|e| anyhow!("libX11.so.6 indisponível: {e}"))?;
            // SAFETY: assinaturas C estáveis da Xlib (X11R6).
            unsafe {
                let open_display = *lib.get(b"XOpenDisplay\0")?;
                let close_display = *lib.get(b"XCloseDisplay\0")?;
                let default_screen = *lib.get(b"XDefaultScreen\0")?;
                let root_window = *lib.get(b"XRootWindow\0")?;
                let display_width = *lib.get(b"XDisplayWidth\0")?;
                let display_height = *lib.get(b"XDisplayHeight\0")?;
                let get_image = *lib.get(b"XGetImage\0")?;
                let query_tree = *lib.get(b"XQueryTree\0")?;
                let get_window_attributes = *lib.get(b"XGetWindowAttributes\0")?;
                let fetch_name = *lib.get(b"XFetchName\0")?;
                let xfree = *lib.get(b"XFree\0")?;
                Ok(Self {
                    _lib: lib,
                    open_display,
                    close_display,
                    default_screen,
                    root_window,
                    display_width,
                    display_height,
                    get_image,
                    query_tree,
                    get_window_attributes,
                    fetch_name,
                    xfree,
                })
            }
        }
    }

    struct XDisplayGuard<'a> {
        lib: &'a X11Lib,
        dpy: *mut c_void,
    }

    impl Drop for XDisplayGuard<'_> {
        fn drop(&mut self) {
            if !self.dpy.is_null() {
                // SAFETY: `self.dpy` foi aberto por `XOpenDisplay` nesta mesma `lib`.
                unsafe {
                    (self.lib.close_display)(self.dpy);
                }
            }
        }
    }

    fn ximage_to_rgba(img: *mut XImage) -> Result<RgbaImage> {
        if img.is_null() {
            bail!("XGetImage retornou nulo");
        }
        // SAFETY: `img` é um `XImage` válido retornado por `XGetImage`; destruído no fim.
        let res = unsafe {
            let xim = &*img;
            let w = xim.width.max(0) as u32;
            let h = xim.height.max(0) as u32;
            if w == 0 || h == 0 || w > MAX_CAPTURE_DIM || h > MAX_CAPTURE_DIM || xim.data.is_null()
            {
                Err(anyhow!("XImage com dimensões inválidas: {w}x{h}"))
            } else if xim.bits_per_pixel != 32 && xim.bits_per_pixel != 24 {
                Err(anyhow!(
                    "Profundidade X11 não suportada: {} bpp",
                    xim.bits_per_pixel
                ))
            } else {
                let bpp = (xim.bits_per_pixel / 8) as usize;
                let stride = xim.bytes_per_line.max(0) as usize;
                let total = stride.saturating_mul(h as usize);
                let data = std::slice::from_raw_parts(xim.data as *const u8, total);
                let r_shift = xim.red_mask.trailing_zeros();
                let g_shift = xim.green_mask.trailing_zeros();
                let b_shift = xim.blue_mask.trailing_zeros();
                let mut rgba = vec![0u8; (w as usize) * (h as usize) * 4];
                for y in 0..h as usize {
                    let row = y * stride;
                    let dst_row = y * (w as usize) * 4;
                    for x in 0..w as usize {
                        let off = row + x * bpp;
                        let px = if bpp == 4 {
                            let b = data.get(off..off + 4).unwrap_or(&[0; 4]);
                            u32::from_ne_bytes([b[0], b[1], b[2], b[3]]) as c_ulong
                        } else {
                            let b = data.get(off..off + 3).unwrap_or(&[0; 3]);
                            u32::from_le_bytes([b[0], b[1], b[2], 0]) as c_ulong
                        };
                        let di = dst_row + x * 4;
                        rgba[di] = ((px & xim.red_mask) >> r_shift) as u8;
                        rgba[di + 1] = ((px & xim.green_mask) >> g_shift) as u8;
                        rgba[di + 2] = ((px & xim.blue_mask) >> b_shift) as u8;
                        rgba[di + 3] = 255;
                    }
                }
                RgbaImage::from_raw(w, h, rgba).ok_or_else(|| anyhow!("falha ao montar RgbaImage"))
            }
        };
        // SAFETY: destrói o `XImage` retornado por `XGetImage`.
        unsafe {
            if let Some(destroy) = (*img).destroy_image {
                destroy(img);
            }
        }
        res
    }

    pub fn list_monitors() -> Vec<OsMonitorInfo> {
        let Ok(lib) = X11Lib::load() else {
            return Vec::new();
        };
        // SAFETY: `XOpenDisplay(NULL)` lê `DISPLAY` do ambiente.
        let dpy = unsafe { (lib.open_display)(std::ptr::null()) };
        if dpy.is_null() {
            return Vec::new();
        }
        let _guard = XDisplayGuard { lib: &lib, dpy };
        // SAFETY: `dpy` válido.
        let screen = unsafe { (lib.default_screen)(dpy) };
        let w = unsafe { (lib.display_width)(dpy, screen) }.max(0) as u32;
        let h = unsafe { (lib.display_height)(dpy, screen) }.max(0) as u32;
        if w == 0 || h == 0 || w > MAX_CAPTURE_DIM || h > MAX_CAPTURE_DIM {
            return Vec::new();
        }
        vec![OsMonitorInfo {
            id: format!("x11:screen:{screen}"),
            name: format!("Tela 1 (X11) — {w}×{h}"),
            x: 0,
            y: 0,
            width: w,
            height: h,
            is_primary: true,
        }]
    }

    pub fn list_windows() -> Vec<OsWindowInfo> {
        let Ok(lib) = X11Lib::load() else {
            return Vec::new();
        };
        // SAFETY: `XOpenDisplay(NULL)` lê `DISPLAY`.
        let dpy = unsafe { (lib.open_display)(std::ptr::null()) };
        if dpy.is_null() {
            return Vec::new();
        }
        let _guard = XDisplayGuard { lib: &lib, dpy };
        let screen = unsafe { (lib.default_screen)(dpy) };
        let root = unsafe { (lib.root_window)(dpy, screen) };
        let mut r_ret: c_ulong = 0;
        let mut p_ret: c_ulong = 0;
        let mut children: *mut c_ulong = std::ptr::null_mut();
        let mut nchildren: c_uint = 0;
        // SAFETY: consulta lista de janelas filhas da janela raiz.
        let ok = unsafe {
            (lib.query_tree)(
                dpy,
                root,
                &mut r_ret,
                &mut p_ret,
                &mut children,
                &mut nchildren,
            )
        };
        let mut out = Vec::new();
        if ok != 0 && !children.is_null() {
            // SAFETY: `children` tem `nchildren` entradas; liberado por `XFree` ao final.
            unsafe {
                let slice = std::slice::from_raw_parts(children, nchildren as usize);
                for &win in slice.iter().take(MAX_ENUM_WINDOWS * 4) {
                    if out.len() >= MAX_ENUM_WINDOWS {
                        break;
                    }
                    let mut attrs: XWindowAttributes = std::mem::zeroed();
                    if (lib.get_window_attributes)(dpy, win, &mut attrs) == 0 {
                        continue;
                    }
                    if attrs.map_state != IS_VIEWABLE || attrs.width < 64 || attrs.height < 64 {
                        continue;
                    }
                    let mut name_ptr: *mut c_char = std::ptr::null_mut();
                    let got = (lib.fetch_name)(dpy, win, &mut name_ptr);
                    if got != 0 && !name_ptr.is_null() {
                        let title = CStr::from_ptr(name_ptr)
                            .to_string_lossy()
                            .trim()
                            .to_string();
                        (lib.xfree)(name_ptr as *mut c_void);
                        if !title.is_empty() {
                            out.push(OsWindowInfo {
                                id: format!("x11win:{win}"),
                                title,
                                width: attrs.width as u32,
                                height: attrs.height as u32,
                            });
                        }
                    }
                }
                (lib.xfree)(children as *mut c_void);
            }
        }
        out
    }

    pub fn list_cameras() -> Vec<OsCameraInfo> {
        let mut out = Vec::new();
        let Ok(entries) = std::fs::read_dir("/sys/class/video4linux") else {
            return out;
        };
        for entry in entries.flatten() {
            let dev = entry.file_name().to_string_lossy().to_string();
            if !dev.starts_with("video") {
                continue;
            }
            let name = std::fs::read_to_string(entry.path().join("name"))
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| format!("/dev/{dev}"));
            out.push(OsCameraInfo {
                id: format!("/dev/{dev}"),
                name,
                default_width: 1280,
                default_height: 720,
            });
        }
        out.sort_by(|a, b| a.id.cmp(&b.id));
        out
    }

    struct X11Grabber {
        lib: X11Lib,
        drawable_win: Option<c_ulong>,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    }

    // SAFETY: `X11Grabber` abre e fecha sua própria conexão X11 em cada captura
    // dentro da thread de trabalho dedicada (sem estado compartilhado entre threads).
    unsafe impl Send for X11Grabber {}

    impl VideoGrabber for X11Grabber {
        fn grab_frame(&mut self, _include_cursor: bool) -> Result<RgbaImage> {
            // SAFETY: conexão X11 isolada com guarda RAII.
            let dpy = unsafe { (self.lib.open_display)(std::ptr::null()) };
            if dpy.is_null() {
                bail!("Servidor gráfico X11 indisponível (DISPLAY não configurado).");
            }
            let _guard = XDisplayGuard {
                lib: &self.lib,
                dpy,
            };
            let screen = unsafe { (self.lib.default_screen)(dpy) };
            let drawable = self
                .drawable_win
                .unwrap_or_else(|| unsafe { (self.lib.root_window)(dpy, screen) });
            let (x, y, w, h) = if let Some(win) = self.drawable_win {
                let mut attrs: XWindowAttributes = unsafe { std::mem::zeroed() };
                if unsafe { (self.lib.get_window_attributes)(dpy, win, &mut attrs) } == 0
                    || attrs.width < 16
                    || attrs.height < 16
                {
                    bail!("A janela X11 selecionada não está mais visível.");
                }
                (0, 0, attrs.width as u32, attrs.height as u32)
            } else {
                (
                    self.x,
                    self.y,
                    self.width.clamp(16, MAX_CAPTURE_DIM),
                    self.height.clamp(16, MAX_CAPTURE_DIM),
                )
            };
            // SAFETY: solicita captura ZPixmap com todos os planos.
            let img =
                unsafe { (self.lib.get_image)(dpy, drawable, x, y, w, h, c_ulong::MAX, ZPIXMAP) };
            ximage_to_rgba(img)
        }
    }

    pub fn open_grabber(target: &VideoGrabTarget) -> Result<Box<dyn VideoGrabber>> {
        match target {
            VideoGrabTarget::Screen {
                x,
                y,
                width,
                height,
            } => Ok(Box::new(X11Grabber {
                lib: X11Lib::load()?,
                drawable_win: None,
                x: *x,
                y: *y,
                width: *width,
                height: *height,
            })),
            VideoGrabTarget::Window { window_id } => {
                let win = window_id
                    .strip_prefix("x11win:")
                    .and_then(|s| s.parse::<c_ulong>().ok())
                    .ok_or_else(|| anyhow!("Identificador de janela X11 inválido: {window_id}"))?;
                Ok(Box::new(X11Grabber {
                    lib: X11Lib::load()?,
                    drawable_win: Some(win),
                    x: 0,
                    y: 0,
                    width: 640,
                    height: 360,
                }))
            }
            VideoGrabTarget::Camera { device_id } => {
                bail!("Captura direta de câmera V4L2 ({device_id}) ainda não suportada no Linux.")
            }
            VideoGrabTarget::Synthetic {
                width,
                height,
                seed,
            } => Ok(Box::new(super::SyntheticVideoGrabber::new(
                *width, *height, *seed,
            ))),
        }
    }

    pub fn free_disk_bytes(dir: &Path) -> Option<u64> {
        // Usa `statvfs` de `libc.so.6` via carregamento dinâmico (sem crate `libc`).
        #[repr(C)]
        struct StatVfs64 {
            f_bsize: c_ulong,
            f_frsize: c_ulong,
            f_blocks: u64,
            f_bfree: u64,
            f_bavail: u64,
            _pad: [u64; 16],
        }
        let cpath = CString::new(dir.as_os_str().as_bytes()).ok()?;
        // SAFETY: consulta somente leitura via `statvfs64` / `statvfs` padrão POSIX da libc.
        unsafe {
            let lib = Library::new("libc.so.6").ok()?;
            let func: libloading::Symbol<
                unsafe extern "C" fn(*const c_char, *mut StatVfs64) -> c_int,
            > = lib
                .get(b"statvfs64\0")
                .or_else(|_| lib.get(b"statvfs\0"))
                .ok()?;
            let mut st: StatVfs64 = std::mem::zeroed();
            if func(cpath.as_ptr(), &mut st) == 0 {
                let unit = if st.f_frsize > 0 {
                    st.f_frsize as u64
                } else {
                    st.f_bsize as u64
                };
                Some(st.f_bavail.saturating_mul(unit))
            } else {
                None
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Demais plataformas (macOS, etc.)
// ---------------------------------------------------------------------------
#[cfg(not(any(windows, target_os = "linux")))]
mod platform {
    use super::{OsCameraInfo, OsMonitorInfo, OsWindowInfo, VideoGrabTarget, VideoGrabber};
    use anyhow::{bail, Result};
    use std::path::Path;

    pub fn list_monitors() -> Vec<OsMonitorInfo> {
        Vec::new()
    }

    pub fn list_windows() -> Vec<OsWindowInfo> {
        Vec::new()
    }

    pub fn list_cameras() -> Vec<OsCameraInfo> {
        Vec::new()
    }

    pub fn open_grabber(target: &VideoGrabTarget) -> Result<Box<dyn VideoGrabber>> {
        match target {
            VideoGrabTarget::Synthetic {
                width,
                height,
                seed,
            } => Ok(Box::new(super::SyntheticVideoGrabber::new(
                *width, *height, *seed,
            ))),
            _ => bail!("Captura nativa de vídeo ainda não suportada nesta plataforma."),
        }
    }

    pub fn free_disk_bytes(_dir: &Path) -> Option<u64> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sintatico_gera_quadros_validos_e_variados() {
        let mut g = SyntheticVideoGrabber::new(160, 90, 7);
        let a = g.grab_frame(false).expect("quadro 0");
        let b = g.grab_frame(true).expect("quadro 1");
        assert_eq!((a.width(), a.height()), (160, 90));
        assert_ne!(a.as_raw(), b.as_raw());
    }

    #[test]
    fn bgra_para_rgba_trata_entradas_hostis_sem_panico() {
        assert!(bgra_top_down_to_rgba(&[], 0, 0).is_err());
        assert!(bgra_top_down_to_rgba(&[0; 4], 2, 2).is_err());
        assert!(bgra_top_down_to_rgba(&[], MAX_CAPTURE_DIM + 1, 10).is_err());
        let ok = bgra_top_down_to_rgba(&[10, 20, 30, 0], 1, 1).expect("1x1");
        assert_eq!(ok.get_pixel(0, 0).0, [30, 20, 10, 255]);
    }

    #[test]
    fn normalize_frame_garante_dimensoes_pares() {
        let img = RgbaImage::from_pixel(101, 55, image::Rgba([1, 2, 3, 255]));
        let out = normalize_frame_size(img, 101, 55);
        assert_eq!(out.width() % 2, 0);
        assert_eq!(out.height() % 2, 0);
    }

    #[test]
    fn espaco_livre_e_formatacao_funcionam() {
        let tmp = std::env::temp_dir();
        let _ = free_disk_bytes(&tmp);
        assert_eq!(format_bytes_human(500), "500 B");
        assert_eq!(format_bytes_human(2048), "2 KB");
        assert_eq!(format_bytes_human(10 * 1024 * 1024), "10.0 MB");
        assert_eq!(format_bytes_human(2 * 1024 * 1024 * 1024), "2.0 GB");
    }
}
