//! Motor de execução em segundo plano do **Neoveg Capture**:
//! - monitoramento ao vivo de fontes de vídeo (miniaturas de prévia ~6 fps em
//!   `320×180` para baixo uso de CPU/RAM, §3.6) e de fontes de áudio (medidores
//!   VU estéreo em dBFS via `cpal` e WASAPI loopback);
//! - gravação simultânea multi-fonte com relógio sincronizado, codificação em
//!   streaming (H.264/HEVC via Media Foundation no Windows com fallback MJPEG,
//!   AVI MJPEG OpenDML em Rust puro, MKV FFV1 sem perdas em Rust puro, WAV
//!   PCM 16/24-bit mono/estéreo e MP3/AAC via Media Foundation no Windows);
//! - proteção integral contra pânicos em threads (`catch_unwind`, §0.6).

use super::manifest::{save_manifest, CaptureProjectManifest, CapturedTrackEntry};
use super::{
    CaptureAudioCodec, CaptureQuality, CaptureSourceCard, CaptureSourceKind, CaptureVideoCodec,
};
use crate::media::avi::AviWriter;
use crate::media::capture_os::{normalize_frame_size, open_video_grabber, VideoGrabTarget};
use crate::media::ffv1::Ffv1Encoder;
use crate::media::mkv::{MkvVideoCodec, MkvWriter};
use crate::model::{FrameRate, Ticks};
use crate::render::{WavBits, WavWriter};
use anyhow::{anyhow, bail, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use image::RgbaImage;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Dimensão máxima das miniaturas ao vivo dos cartões de vídeo (§3.6).
const LIVE_PREVIEW_MAX_W: u32 = 320;
const LIVE_PREVIEW_MAX_H: u32 = 180;
/// Intervalo entre quadros na prévia ao vivo antes de gravar (~6 fps).
const LIVE_PREVIEW_POLL_MS: u64 = 160;

/// Estado operacional instantâneo de uma fonte no painel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SourceRuntimeStatus {
    #[default]
    Idle,
    Monitoring,
    Recording,
    Error,
}

/// Estado compartilhado entre a thread da fonte e a interface.
#[derive(Default)]
struct SharedSourceState {
    preview: Option<RgbaImage>,
    preview_gen: u64,
    peak_db: [f32; 2],
    frames_captured: u64,
    status: SourceRuntimeStatus,
    last_error: Option<String>,
}

/// Fotografia imutável lida pela UI a cada quadro sem segurar locks.
#[derive(Clone, Debug)]
pub struct SourceLiveSnapshot {
    pub preview: Option<RgbaImage>,
    pub preview_gen: u64,
    pub peak_db: [f32; 2],
    pub frames_captured: u64,
    pub status: SourceRuntimeStatus,
    pub last_error: Option<String>,
}

/// Resultado de uma sessão concluída de gravação no Neoveg Capture.
#[derive(Clone, Debug)]
pub struct CaptureSessionResult {
    pub session_dir: PathBuf,
    pub manifest_path: PathBuf,
    pub manifest: CaptureProjectManifest,
    pub warnings: Vec<String>,
}

type TrackWorkerHandle = JoinHandle<Result<CapturedTrackEntry, String>>;
type AudioWorkerStart = (TrackWorkerHandle, Option<cpal::Stream>);

struct ActiveWorker {
    name: String,
    handle: TrackWorkerHandle,
}

struct MonitorSlot {
    key: String,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    _audio_stream: Option<cpal::Stream>,
}

impl MonitorSlot {
    fn stop_and_join(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        drop(self._audio_stream.take());
        if let Some(h) = self.worker.take() {
            let _ = h.join();
        }
    }
}

struct ActiveRecording {
    session_dir: PathBuf,
    started_at: Instant,
    created_at_iso: String,
    stop: Arc<AtomicBool>,
    workers: Vec<ActiveWorker>,
    /// Fluxos `cpal` mantidos vivos até o encerramento da gravação.
    audio_streams: Vec<cpal::Stream>,
}

/// Orquestrador de prévia ao vivo e gravação multi-fonte do Neoveg Capture.
pub struct CaptureRuntime {
    monitors: HashMap<u64, MonitorSlot>,
    live_states: HashMap<u64, Arc<Mutex<SharedSourceState>>>,
    recording: Option<ActiveRecording>,
}

impl Default for CaptureRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl CaptureRuntime {
    pub fn new() -> Self {
        Self {
            monitors: HashMap::new(),
            live_states: HashMap::new(),
            recording: None,
        }
    }

    pub fn is_recording(&self) -> bool {
        self.recording.is_some()
    }

    pub fn elapsed_ticks(&self) -> Ticks {
        match &self.recording {
            Some(rec) => Ticks::from_millis_i64(rec.started_at.elapsed().as_millis() as i64),
            None => Ticks::ZERO,
        }
    }

    /// Retorna o estado instantâneo de prévia/VU de uma fonte.
    pub fn live_snapshot(&self, source_id: u64) -> Option<SourceLiveSnapshot> {
        let arc = self.live_states.get(&source_id)?;
        let st = crate::util::lock(arc);
        Some(SourceLiveSnapshot {
            preview: st.preview.clone(),
            preview_gen: st.preview_gen,
            peak_db: st.peak_db,
            frames_captured: st.frames_captured,
            status: st.status,
            last_error: st.last_error.clone(),
        })
    }

    /// Sincroniza os workers de monitoramento ao vivo (prévia de vídeo e VU de áudio)
    /// com a lista atual de cartões quando a janela está aberta e não está gravando.
    pub fn sync_monitors(&mut self, sources: &[CaptureSourceCard], window_open: bool) {
        if self.is_recording() || !window_open {
            if !self.monitors.is_empty() {
                for (_, slot) in self.monitors.drain() {
                    slot.stop_and_join();
                }
            }
            return;
        }

        let desired: HashMap<u64, &CaptureSourceCard> = sources.iter().map(|s| (s.id, s)).collect();
        let stale: Vec<u64> = self
            .monitors
            .iter()
            .filter_map(|(&id, slot)| {
                let keep = desired
                    .get(&id)
                    .is_some_and(|c| c.kind.unique_key() == slot.key && c.enabled);
                (!keep).then_some(id)
            })
            .collect();
        for id in stale {
            if let Some(slot) = self.monitors.remove(&id) {
                slot.stop_and_join();
            }
        }
        self.live_states.retain(|id, _| desired.contains_key(id));

        for card in sources {
            let state = self
                .live_states
                .entry(card.id)
                .or_insert_with(|| Arc::new(Mutex::new(SharedSourceState::default())))
                .clone();
            if !card.enabled {
                let mut st = crate::util::lock(&state);
                st.status = SourceRuntimeStatus::Idle;
                st.peak_db = [-96.0, -96.0];
                continue;
            }
            if self.monitors.contains_key(&card.id) {
                continue;
            }
            let slot = spawn_monitor_slot(card, state);
            self.monitors.insert(card.id, slot);
        }
    }

    /// Inicia a gravação simultânea de todas as fontes habilitadas (e não silenciadas
    /// no caso de áudio) em uma subpasta datada dentro de `recording_root`.
    pub fn start_recording(
        &mut self,
        sources: &[CaptureSourceCard],
        recording_root: &Path,
    ) -> Result<PathBuf> {
        if self.recording.is_some() {
            bail!("Uma gravação do Neoveg Capture já está em andamento.");
        }
        let active_sources: Vec<CaptureSourceCard> = sources
            .iter()
            .filter(|s| s.enabled && !(s.is_audio() && s.muted))
            .cloned()
            .collect();
        if active_sources.is_empty() {
            bail!("Ative ao menos uma fonte de vídeo ou áudio para iniciar a gravação.");
        }

        // Encerra o monitoramento ao vivo antes de abrir os dispositivos para gravação
        // (evita disputa exclusiva de câmera ou dispositivo de áudio).
        for (_, slot) in self.monitors.drain() {
            slot.stop_and_join();
        }

        let now = chrono::Local::now();
        let folder_name = format!("neoveg_capture_{}", now.format("%Y%m%d_%H%M%S"));
        let session_dir = unique_session_dir(recording_root, &folder_name);
        std::fs::create_dir_all(&session_dir)
            .with_context(|| format!("criando pasta de gravação {}", session_dir.display()))?;

        let stop = Arc::new(AtomicBool::new(false));
        let started_at = Instant::now();
        let mut workers = Vec::new();
        let mut audio_streams = Vec::new();
        let mut used_names: HashMap<String, usize> = HashMap::new();

        for card in active_sources {
            let state = self
                .live_states
                .entry(card.id)
                .or_insert_with(|| Arc::new(Mutex::new(SharedSourceState::default())))
                .clone();
            {
                let mut st = crate::util::lock(&state);
                st.status = SourceRuntimeStatus::Recording;
                st.frames_captured = 0;
                st.last_error = None;
            }
            let stem = unique_file_stem(&card.name, &mut used_names);
            if card.is_video() {
                let stop_c = stop.clone();
                let dir_c = session_dir.clone();
                let card_c = card.clone();
                let state_c = state.clone();
                let handle = std::thread::Builder::new()
                    .name(format!("nvcap-vid-{}", card.id))
                    .spawn(move || {
                        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            record_video_worker(&card_c, &dir_c, &stem, &stop_c, &state_c)
                        }));
                        match res {
                            Ok(Ok(entry)) => {
                                let mut st = crate::util::lock(&state_c);
                                st.status = SourceRuntimeStatus::Idle;
                                Ok(entry)
                            }
                            Ok(Err(e)) => {
                                let msg = format!("{e:#}");
                                let mut st = crate::util::lock(&state_c);
                                st.status = SourceRuntimeStatus::Error;
                                st.last_error = Some(msg.clone());
                                Err(msg)
                            }
                            Err(_) => {
                                let msg = "Falha interna isolada na thread de captura de vídeo."
                                    .to_string();
                                let mut st = crate::util::lock(&state_c);
                                st.status = SourceRuntimeStatus::Error;
                                st.last_error = Some(msg.clone());
                                Err(msg)
                            }
                        }
                    })
                    .map_err(|e| anyhow!("falha ao iniciar worker de vídeo: {e}"))?;
                workers.push(ActiveWorker {
                    name: card.name.clone(),
                    handle,
                });
            } else {
                match start_audio_worker(&card, &session_dir, &stem, stop.clone(), state.clone()) {
                    Ok((handle, opt_stream)) => {
                        if let Some(stream) = opt_stream {
                            audio_streams.push(stream);
                        }
                        workers.push(ActiveWorker {
                            name: card.name.clone(),
                            handle,
                        });
                    }
                    Err(e) => {
                        let msg = format!("{e:#}");
                        let mut st = crate::util::lock(&state);
                        st.status = SourceRuntimeStatus::Error;
                        st.last_error = Some(msg.clone());
                        // Mesmo se uma fonte falhar ao abrir, continua se houver outras,
                        // registrando o erro via worker imediato.
                        let handle = std::thread::spawn(move || Err(msg));
                        workers.push(ActiveWorker {
                            name: card.name.clone(),
                            handle,
                        });
                    }
                }
            }
        }

        self.recording = Some(ActiveRecording {
            session_dir: session_dir.clone(),
            started_at,
            created_at_iso: now.to_rfc3339(),
            stop,
            workers,
            audio_streams,
        });
        Ok(session_dir)
    }

    /// Encerra a gravação ativa, aguarda todos os codificadores finalizarem de
    /// forma limpa, grava `export.json` na pasta da sessão e retorna o manifesto.
    pub fn stop_recording(&mut self) -> Result<CaptureSessionResult> {
        let Some(mut rec) = self.recording.take() else {
            bail!("Nenhuma gravação em andamento.");
        };
        let elapsed_ticks =
            Ticks::from_millis_i64(rec.started_at.elapsed().as_millis().max(1) as i64);
        rec.stop.store(true, Ordering::SeqCst);
        // Fecha streams `cpal` primeiro para interromper imediatamente callbacks de áudio.
        rec.audio_streams.clear();

        let mut tracks = Vec::new();
        let mut warnings = Vec::new();
        for w in rec.workers {
            match w.handle.join() {
                Ok(Ok(entry)) => tracks.push(entry),
                Ok(Err(msg)) => warnings.push(format!("{}: {msg}", w.name)),
                Err(_) => warnings.push(format!(
                    "{}: thread de captura encerrada inesperadamente.",
                    w.name
                )),
            }
        }

        let max_track_ticks = tracks
            .iter()
            .map(|t| t.duration_ticks)
            .max()
            .unwrap_or(elapsed_ticks.0);
        let total_ticks = Ticks::from_raw(max_track_ticks.max(elapsed_ticks.0));
        let manifest = CaptureProjectManifest::new(rec.created_at_iso, total_ticks, tracks);
        let manifest_path = rec.session_dir.join("export.json");
        save_manifest(&manifest_path, &manifest)?;

        Ok(CaptureSessionResult {
            session_dir: rec.session_dir,
            manifest_path,
            manifest,
            warnings,
        })
    }

    /// Encerra todos os workers (chamado ao fechar a janela ou encerrar o app).
    pub fn shutdown(&mut self) {
        if self.recording.is_some() {
            let _ = self.stop_recording();
        }
        for (_, slot) in self.monitors.drain() {
            slot.stop_and_join();
        }
    }
}

impl Drop for CaptureRuntime {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn unique_session_dir(root: &Path, base: &str) -> PathBuf {
    let direct = root.join(base);
    if !direct.exists() {
        return direct;
    }
    for n in 2..=999u32 {
        let cand = root.join(format!("{base}_{n}"));
        if !cand.exists() {
            return cand;
        }
    }
    root.join(format!("{base}_{}", std::process::id()))
}

fn sanitize_filename_stem(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(48)
        .collect();
    let trimmed = cleaned.trim_matches('_');
    if trimmed.is_empty() {
        "fonte".to_string()
    } else {
        trimmed.to_string()
    }
}

fn unique_file_stem(name: &str, used: &mut HashMap<String, usize>) -> String {
    let base = sanitize_filename_stem(name);
    let count = used.entry(base.clone()).or_insert(0);
    *count += 1;
    if *count == 1 {
        base
    } else {
        format!("{base}_{}", *count)
    }
}

fn grab_target_for_kind(kind: &CaptureSourceKind) -> Option<VideoGrabTarget> {
    match kind {
        CaptureSourceKind::Screen {
            x,
            y,
            width,
            height,
            ..
        } => Some(VideoGrabTarget::Screen {
            x: *x,
            y: *y,
            width: *width,
            height: *height,
        }),
        CaptureSourceKind::Window { window_id, .. } => Some(VideoGrabTarget::Window {
            window_id: window_id.clone(),
        }),
        CaptureSourceKind::Camera { device_id, .. } => Some(VideoGrabTarget::Camera {
            device_id: device_id.clone(),
        }),
        CaptureSourceKind::SyntheticVideo {
            width,
            height,
            seed,
        } => Some(VideoGrabTarget::Synthetic {
            width: *width,
            height: *height,
            seed: *seed,
        }),
        _ => None,
    }
}

fn make_preview_thumb(img: &RgbaImage) -> RgbaImage {
    if img.width() <= LIVE_PREVIEW_MAX_W && img.height() <= LIVE_PREVIEW_MAX_H {
        return img.clone();
    }
    let sx = LIVE_PREVIEW_MAX_W as f32 / img.width().max(1) as f32;
    let sy = LIVE_PREVIEW_MAX_H as f32 / img.height().max(1) as f32;
    let s = sx.min(sy);
    let tw = ((img.width() as f32 * s).round() as u32).clamp(16, LIVE_PREVIEW_MAX_W);
    let th = ((img.height() as f32 * s).round() as u32).clamp(16, LIVE_PREVIEW_MAX_H);
    image::imageops::resize(img, tw, th, image::imageops::FilterType::Triangle)
}

fn spawn_monitor_slot(
    card: &CaptureSourceCard,
    state: Arc<Mutex<SharedSourceState>>,
) -> MonitorSlot {
    let stop = Arc::new(AtomicBool::new(false));
    let key = card.kind.unique_key();

    if let Some(target) = grab_target_for_kind(&card.kind) {
        let stop_c = stop.clone();
        let state_c = state.clone();
        let include_cursor = card.capture_cursor;
        let worker = std::thread::Builder::new()
            .name(format!("nvcap-prev-{}", card.id))
            .spawn(move || {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut grabber = match open_video_grabber(&target) {
                        Ok(g) => {
                            let mut st = crate::util::lock(&state_c);
                            st.status = SourceRuntimeStatus::Monitoring;
                            st.last_error = None;
                            g
                        }
                        Err(e) => {
                            let mut st = crate::util::lock(&state_c);
                            st.status = SourceRuntimeStatus::Error;
                            st.last_error = Some(format!("{e:#}"));
                            return;
                        }
                    };
                    while !stop_c.load(Ordering::Relaxed) {
                        match grabber.grab_frame(include_cursor) {
                            Ok(frame) => {
                                let thumb = make_preview_thumb(&frame);
                                let mut st = crate::util::lock(&state_c);
                                st.preview = Some(thumb);
                                st.preview_gen = st.preview_gen.saturating_add(1);
                                st.status = SourceRuntimeStatus::Monitoring;
                                st.last_error = None;
                            }
                            Err(e) => {
                                let mut st = crate::util::lock(&state_c);
                                st.status = SourceRuntimeStatus::Error;
                                st.last_error = Some(format!("{e:#}"));
                                std::thread::sleep(Duration::from_millis(500));
                                continue;
                            }
                        }
                        std::thread::sleep(Duration::from_millis(LIVE_PREVIEW_POLL_MS));
                    }
                }));
            })
            .ok();
        return MonitorSlot {
            key,
            stop,
            worker,
            _audio_stream: None,
        };
    }

    // Monitoramento de áudio (medidor VU estéreo ao vivo em dBFS).
    let stream = match &card.kind {
        CaptureSourceKind::AudioInput { device_name } => {
            open_audio_monitor_stream(device_name, false, state.clone())
        }
        CaptureSourceKind::AudioOutputLoopback { device_name } => {
            open_audio_monitor_stream(device_name, true, state.clone())
        }
        CaptureSourceKind::SyntheticAudio { .. } => {
            let mut st = crate::util::lock(&state);
            st.status = SourceRuntimeStatus::Monitoring;
            st.peak_db = [-18.0, -18.0];
            Ok(None)
        }
        _ => Ok(None),
    };
    let audio_stream = match stream {
        Ok(s) => {
            let mut st = crate::util::lock(&state);
            st.status = SourceRuntimeStatus::Monitoring;
            st.last_error = None;
            s
        }
        Err(e) => {
            let mut st = crate::util::lock(&state);
            st.status = SourceRuntimeStatus::Error;
            st.last_error = Some(e);
            None
        }
    };
    MonitorSlot {
        key,
        stop,
        worker: None,
        _audio_stream: audio_stream,
    }
}

fn peak_to_db(peak: f32) -> f32 {
    if peak <= 1e-5 || !peak.is_finite() {
        -96.0
    } else {
        (20.0 * peak.log10()).clamp(-96.0, 0.0)
    }
}

fn update_vu_peak(state: &Arc<Mutex<SharedSourceState>>, l: f32, r: f32) {
    let mut st = crate::util::lock(state);
    let db_l = peak_to_db(l);
    let db_r = peak_to_db(r);
    // Decaimento suave quando o sinal cai; ataque imediato quando sobe.
    st.peak_db[0] = if db_l >= st.peak_db[0] {
        db_l
    } else {
        (st.peak_db[0] * 0.85 + db_l * 0.15).max(-96.0)
    };
    st.peak_db[1] = if db_r >= st.peak_db[1] {
        db_r
    } else {
        (st.peak_db[1] * 0.85 + db_r * 0.15).max(-96.0)
    };
}

fn find_cpal_device(
    device_name: &str,
    loopback: bool,
) -> Result<(cpal::Device, cpal::SupportedStreamConfig), String> {
    crate::playback::ensure_audio_com_keeper();
    let host = cpal::default_host();
    if loopback {
        let dev = host
            .output_devices()
            .ok()
            .and_then(|mut it| it.find(|d| d.name().ok().as_deref() == Some(device_name)))
            .or_else(|| host.default_output_device())
            .ok_or_else(|| format!("Dispositivo de saída '{device_name}' não encontrado."))?;
        let cfg = dev
            .default_output_config()
            .map_err(|e| format!("Não foi possível consultar formato de loopback: {e}"))?;
        Ok((dev, cfg))
    } else {
        let dev = host
            .input_devices()
            .ok()
            .and_then(|mut it| it.find(|d| d.name().ok().as_deref() == Some(device_name)))
            .or_else(|| host.default_input_device())
            .ok_or_else(|| format!("Dispositivo de entrada '{device_name}' não encontrado."))?;
        let cfg = dev
            .default_input_config()
            .map_err(|e| format!("Não foi possível consultar formato da entrada: {e}"))?;
        Ok((dev, cfg))
    }
}

fn open_audio_monitor_stream(
    device_name: &str,
    loopback: bool,
    state: Arc<Mutex<SharedSourceState>>,
) -> Result<Option<cpal::Stream>, String> {
    let (device, supported) = find_cpal_device(device_name, loopback)?;
    let ch = supported.channels().max(1) as usize;
    let sample_format = supported.sample_format();
    let cfg = supported.config();

    let err_state = state.clone();
    let err_fn = move |e: cpal::StreamError| {
        let mut st = crate::util::lock(&err_state);
        st.status = SourceRuntimeStatus::Error;
        st.last_error = Some(format!("Erro de áudio: {e}"));
    };

    let cb_state = state.clone();
    let stream = match sample_format {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &cfg,
            move |data: &[f32], _| {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let (l, r) = measure_chunk_peak(data, ch, |x| x);
                    update_vu_peak(&cb_state, l, r);
                }));
            },
            err_fn,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_input_stream(
            &cfg,
            move |data: &[i16], _| {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let (l, r) = measure_chunk_peak(data, ch, |x| f32::from(x) / 32768.0);
                    update_vu_peak(&cb_state, l, r);
                }));
            },
            err_fn,
            None,
        ),
        cpal::SampleFormat::U16 => device.build_input_stream(
            &cfg,
            move |data: &[u16], _| {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let (l, r) =
                        measure_chunk_peak(data, ch, |x| (f32::from(x) - 32768.0) / 32768.0);
                    update_vu_peak(&cb_state, l, r);
                }));
            },
            err_fn,
            None,
        ),
        other => return Err(format!("Formato de amostra não suportado: {other:?}")),
    }
    .map_err(|e| format!("Falha ao abrir monitoramento de áudio: {e}"))?;

    stream
        .play()
        .map_err(|e| format!("Falha ao iniciar monitoramento de áudio: {e}"))?;
    Ok(Some(stream))
}

fn measure_chunk_peak<T: Copy>(data: &[T], ch: usize, to_f32: impl Fn(T) -> f32) -> (f32, f32) {
    let mut pl = 0.0f32;
    let mut pr = 0.0f32;
    let step = ch.max(1);
    for frame in data.chunks(step) {
        let l = frame.first().map(|&v| to_f32(v).abs()).unwrap_or(0.0);
        let r = if step > 1 {
            frame.get(1).map(|&v| to_f32(v).abs()).unwrap_or(l)
        } else {
            l
        };
        if l > pl {
            pl = l;
        }
        if r > pr {
            pr = r;
        }
    }
    (pl, pr)
}

// ---------------------------------------------------------------------------
// Codificador de vídeo em streaming por fonte
// ---------------------------------------------------------------------------
enum VideoStreamSink {
    #[cfg(windows)]
    Mf {
        enc: crate::media::mf::encode::MfSinkEncoder,
        w: u32,
        h: u32,
    },
    AviMjpeg {
        writer: AviWriter,
        jpeg_quality: u8,
    },
    MkvFfv1 {
        writer: MkvWriter,
        encoder: Ffv1Encoder,
    },
}

impl VideoStreamSink {
    fn open(
        codec: CaptureVideoCodec,
        quality: CaptureQuality,
        session_dir: &Path,
        stem: &str,
        w: u32,
        h: u32,
        fps: FrameRate,
    ) -> Result<(Self, String, String)> {
        #[cfg(windows)]
        if matches!(
            codec,
            CaptureVideoCodec::H264Mp4 | CaptureVideoCodec::HevcMp4
        ) {
            use crate::media::mf::encode::{MfEncodeConfig, MfOutputFormat, MfSinkEncoder};
            let mf_fmt = match codec {
                CaptureVideoCodec::HevcMp4 => MfOutputFormat::Mp4Hevc,
                _ => MfOutputFormat::Mp4H264,
            };
            let file_name = format!("{stem}.mp4");
            let out_path = session_dir.join(&file_name);
            let cfg = MfEncodeConfig {
                format: mf_fmt,
                path: out_path,
                width: w,
                height: h,
                fps,
                video_bitrate_bps: quality.video_bitrate_bps(w, h, fps),
                audio_sr: None,
                audio_bitrate_bps: 192_000,
            };
            if let Ok(enc) = MfSinkEncoder::create(cfg) {
                return Ok((
                    Self::Mf { enc, w, h },
                    file_name,
                    codec.short_name().to_string(),
                ));
            }
            // Fallback seguro para MJPEG AVI em hardware sem codificador MF (§3.6).
        }

        if codec == CaptureVideoCodec::Ffv1Mkv {
            let file_name = format!("{stem}.mkv");
            let out_path = session_dir.join(&file_name);
            let encoder = Ffv1Encoder::new(w, h, false)?;
            let writer = MkvWriter::create(
                &out_path,
                w,
                h,
                fps,
                MkvVideoCodec::Ffv1 {
                    codec_private: encoder.configuration_record().to_vec(),
                    has_alpha: false,
                },
                None,
            )?;
            return Ok((
                Self::MkvFfv1 { writer, encoder },
                file_name,
                CaptureVideoCodec::Ffv1Mkv.short_name().to_string(),
            ));
        }

        let file_name = format!("{stem}.avi");
        let out_path = session_dir.join(&file_name);
        let writer = AviWriter::create(&out_path, w, h, fps, None)?;
        Ok((
            Self::AviMjpeg {
                writer,
                jpeg_quality: quality.jpeg_quality(),
            },
            file_name,
            CaptureVideoCodec::MjpegAvi.short_name().to_string(),
        ))
    }

    fn write_frame(&mut self, frame_idx: u64, img: &RgbaImage) -> Result<()> {
        match self {
            #[cfg(windows)]
            Self::Mf { enc, w, h } => {
                let nv12 = crate::media::mf::encode::rgba_to_nv12(img.as_raw(), *w, *h);
                enc.write_video_nv12(frame_idx, &nv12)
            }
            Self::AviMjpeg {
                writer,
                jpeg_quality,
            } => {
                let rgb = image::DynamicImage::ImageRgba8(img.clone()).to_rgb8();
                let mut jpg = Vec::with_capacity((img.width() * img.height() / 4) as usize);
                image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg, *jpeg_quality)
                    .encode_image(&rgb)?;
                writer.write_video_jpeg(&jpg)
            }
            Self::MkvFfv1 { writer, encoder } => {
                let pkt = encoder.encode_frame(img.as_raw())?;
                let pts_ms = writer.frame_pts_ms(frame_idx);
                writer.write_video_packet(pts_ms, true, pkt)
            }
        }
    }

    fn finish(self) -> Result<()> {
        match self {
            #[cfg(windows)]
            Self::Mf { enc, .. } => enc.finish(),
            Self::AviMjpeg { writer, .. } => writer.finish(),
            Self::MkvFfv1 { writer, .. } => writer.finish(),
        }
    }
}

fn record_video_worker(
    card: &CaptureSourceCard,
    session_dir: &Path,
    stem: &str,
    stop: &Arc<AtomicBool>,
    state: &Arc<Mutex<SharedSourceState>>,
) -> Result<CapturedTrackEntry> {
    let target = grab_target_for_kind(&card.kind)
        .ok_or_else(|| anyhow!("fonte de vídeo inválida: {}", card.name))?;
    let mut grabber = open_video_grabber(&target)?;
    let (w, h) = card.target_dimensions();
    let fps = card.fps;
    let (mut sink, file_name, codec_used) = VideoStreamSink::open(
        card.video_codec,
        card.video_quality,
        session_dir,
        stem,
        w,
        h,
        fps,
    )?;

    let frame_dur = Duration::from_secs_f64(1.0 / fps.as_f64().clamp(1.0, 120.0));
    let started = Instant::now();
    let mut frame_idx: u64 = 0;
    let mut last_preview = Instant::now()
        .checked_sub(Duration::from_secs(1))
        .unwrap_or(started);
    let mut last_good_frame: Option<RgbaImage> = None;

    while !stop.load(Ordering::Relaxed) || frame_idx == 0 {
        let frame = match grabber.grab_frame(card.capture_cursor) {
            Ok(raw) => {
                let norm = normalize_frame_size(raw, w, h);
                last_good_frame = Some(norm.clone());
                norm
            }
            Err(e) => {
                if let Some(prev) = &last_good_frame {
                    prev.clone()
                } else if frame_idx == 0 {
                    return Err(e);
                } else {
                    break;
                }
            }
        };

        sink.write_frame(frame_idx, &frame)?;
        frame_idx = frame_idx.saturating_add(1);

        if last_preview.elapsed() >= Duration::from_millis(LIVE_PREVIEW_POLL_MS) {
            last_preview = Instant::now();
            let thumb = make_preview_thumb(&frame);
            let mut st = crate::util::lock(state);
            st.preview = Some(thumb);
            st.preview_gen = st.preview_gen.saturating_add(1);
            st.frames_captured = frame_idx;
        } else {
            let mut st = crate::util::lock(state);
            st.frames_captured = frame_idx;
        }

        if stop.load(Ordering::Relaxed) {
            break;
        }
        let next_deadline =
            started + frame_dur.saturating_mul(frame_idx.min(u32::MAX as u64) as u32);
        let now = Instant::now();
        if next_deadline > now {
            let mut rem = next_deadline - now;
            while rem > Duration::from_millis(15) && !stop.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(15));
                rem = next_deadline.saturating_duration_since(Instant::now());
            }
            if !stop.load(Ordering::Relaxed) && rem > Duration::ZERO {
                std::thread::sleep(rem);
            }
        }
    }

    sink.finish()?;
    let dur_ticks = fps.frame_to_ticks(frame_idx.min(i64::MAX as u64) as i64);
    Ok(CapturedTrackEntry {
        source_name: card.name.clone(),
        is_video: true,
        file: file_name,
        codec: codec_used,
        width: w,
        height: h,
        fps_num: fps.num,
        fps_den: fps.den,
        sample_rate: 0,
        channels: 0,
        duration_ticks: dur_ticks.0,
        frame_count: frame_idx,
    })
}

// ---------------------------------------------------------------------------
// Gravação de áudio (cpal microfone / WASAPI loopback / sintético)
// ---------------------------------------------------------------------------
struct AudioRecShared {
    writer: Option<WavWriter>,
    frames_written: u64,
    error: Option<String>,
}

fn start_audio_worker(
    card: &CaptureSourceCard,
    session_dir: &Path,
    stem: &str,
    stop: Arc<AtomicBool>,
    state: Arc<Mutex<SharedSourceState>>,
) -> Result<AudioWorkerStart> {
    let target_ch = card.audio_channels.clamp(1, 2);
    let wav_bits = match card.audio_codec {
        CaptureAudioCodec::WavPcm24 => WavBits::Pcm24,
        _ => WavBits::Pcm16,
    };

    // Caminho sintético determinístico para testes sem placa de som.
    if let CaptureSourceKind::SyntheticAudio {
        freq_hz,
        sample_rate,
    } = card.kind
    {
        let sr = sample_rate.clamp(8_000, 192_000);
        let wav_name = format!("{stem}.wav");
        let wav_path = session_dir.join(&wav_name);
        let card_c = card.clone();
        let dir_c = session_dir.to_path_buf();
        let stem_c = stem.to_string();
        let handle = std::thread::Builder::new()
            .name(format!("nvcap-syna-{}", card.id))
            .spawn(move || {
                let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                    || -> Result<CapturedTrackEntry> {
                        let mut writer =
                            WavWriter::create_with_channels(&wav_path, sr, wav_bits, target_ch)?;
                        let chunk_frames = (sr / 50).max(256) as usize;
                        let mut total_frames: u64 = 0;
                        let mut buf = vec![0.0f32; chunk_frames * target_ch as usize];
                        while !stop.load(Ordering::Relaxed) || total_frames == 0 {
                            for i in 0..chunk_frames {
                                let t = (total_frames + i as u64) as f64 / f64::from(sr);
                                let v =
                                    (2.0 * std::f64::consts::PI * f64::from(freq_hz.max(20)) * t)
                                        .sin() as f32
                                        * 0.35;
                                if target_ch == 1 {
                                    buf[i] = v;
                                } else {
                                    buf[i * 2] = v;
                                    buf[i * 2 + 1] = v;
                                }
                            }
                            writer.write(&buf)?;
                            total_frames += chunk_frames as u64;
                            update_vu_peak(&state, 0.35, 0.35);
                            {
                                let mut st = crate::util::lock(&state);
                                st.frames_captured = total_frames;
                            }
                            if stop.load(Ordering::Relaxed) {
                                break;
                            }
                            std::thread::sleep(Duration::from_millis(20));
                        }
                        writer.finish()?;
                        finalize_audio_track(
                            &card_c,
                            &dir_c,
                            &stem_c,
                            &wav_name,
                            sr,
                            target_ch,
                            total_frames,
                        )
                    },
                ));
                match res {
                    Ok(Ok(e)) => {
                        crate::util::lock(&state).status = SourceRuntimeStatus::Idle;
                        Ok(e)
                    }
                    Ok(Err(e)) => {
                        let msg = format!("{e:#}");
                        let mut st = crate::util::lock(&state);
                        st.status = SourceRuntimeStatus::Error;
                        st.last_error = Some(msg.clone());
                        Err(msg)
                    }
                    Err(_) => Err("Falha interna na gravação de áudio sintético.".into()),
                }
            })
            .map_err(|e| anyhow!("falha ao iniciar worker de áudio sintético: {e}"))?;
        return Ok((handle, None));
    }

    let (device_name, loopback) = match &card.kind {
        CaptureSourceKind::AudioInput { device_name } => (device_name.as_str(), false),
        CaptureSourceKind::AudioOutputLoopback { device_name } => (device_name.as_str(), true),
        _ => bail!("fonte não é de áudio"),
    };
    let (device, supported) = find_cpal_device(device_name, loopback).map_err(|e| anyhow!(e))?;
    let in_sr = supported.sample_rate().0.clamp(8_000, 192_000);
    let in_ch = supported.channels().max(1) as usize;
    let sample_format = supported.sample_format();
    let cfg = supported.config();

    let wav_name = format!("{stem}.wav");
    let wav_path = session_dir.join(&wav_name);
    let writer = WavWriter::create_with_channels(&wav_path, in_sr, wav_bits, target_ch)?;
    let rec_shared = Arc::new(Mutex::new(AudioRecShared {
        writer: Some(writer),
        frames_written: 0,
        error: None,
    }));
    let frames_atomic = Arc::new(AtomicU64::new(0));

    let err_rec = rec_shared.clone();
    let err_state = state.clone();
    let err_fn = move |e: cpal::StreamError| {
        let msg = format!("Erro no fluxo de gravação de áudio: {e}");
        crate::util::lock(&err_rec).error = Some(msg.clone());
        let mut st = crate::util::lock(&err_state);
        st.status = SourceRuntimeStatus::Error;
        st.last_error = Some(msg);
    };

    let rec_cb = rec_shared.clone();
    let state_cb = state.clone();
    let fa_cb = frames_atomic.clone();
    let stop_cb = stop.clone();

    let stream = match sample_format {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &cfg,
            move |data: &[f32], _| {
                if !stop_cb.load(Ordering::Relaxed) {
                    push_audio_samples(&rec_cb, &state_cb, &fa_cb, data, in_ch, target_ch, |x| x);
                }
            },
            err_fn,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_input_stream(
            &cfg,
            move |data: &[i16], _| {
                if !stop_cb.load(Ordering::Relaxed) {
                    push_audio_samples(&rec_cb, &state_cb, &fa_cb, data, in_ch, target_ch, |x| {
                        f32::from(x) / 32768.0
                    });
                }
            },
            err_fn,
            None,
        ),
        cpal::SampleFormat::U16 => device.build_input_stream(
            &cfg,
            move |data: &[u16], _| {
                if !stop_cb.load(Ordering::Relaxed) {
                    push_audio_samples(&rec_cb, &state_cb, &fa_cb, data, in_ch, target_ch, |x| {
                        (f32::from(x) - 32768.0) / 32768.0
                    });
                }
            },
            err_fn,
            None,
        ),
        other => bail!("Formato de amostra de áudio não suportado: {other:?}"),
    }
    .map_err(|e| anyhow!("Não foi possível abrir o fluxo de gravação de áudio: {e}"))?;

    stream
        .play()
        .map_err(|e| anyhow!("Não foi possível iniciar o fluxo de gravação de áudio: {e}"))?;

    let card_c = card.clone();
    let dir_c = session_dir.to_path_buf();
    let stem_c = stem.to_string();
    let handle = std::thread::Builder::new()
        .name(format!("nvcap-aud-{}", card.id))
        .spawn(move || {
            let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                || -> Result<CapturedTrackEntry> {
                    while !stop.load(Ordering::Relaxed) {
                        std::thread::sleep(Duration::from_millis(25));
                    }
                    // Breve pausa para último callback drenar antes de fechar o WAV.
                    std::thread::sleep(Duration::from_millis(40));
                    let (writer_opt, frames, err_opt) = {
                        let mut guard = crate::util::lock(&rec_shared);
                        (
                            guard.writer.take(),
                            guard.frames_written,
                            guard.error.take(),
                        )
                    };
                    if let Some(w) = writer_opt {
                        w.finish()?;
                    }
                    if let Some(err) = err_opt {
                        if frames == 0 {
                            bail!("{err}");
                        }
                    }
                    finalize_audio_track(
                        &card_c, &dir_c, &stem_c, &wav_name, in_sr, target_ch, frames,
                    )
                },
            ));
            match res {
                Ok(Ok(e)) => {
                    crate::util::lock(&state).status = SourceRuntimeStatus::Idle;
                    Ok(e)
                }
                Ok(Err(e)) => {
                    let msg = format!("{e:#}");
                    let mut st = crate::util::lock(&state);
                    st.status = SourceRuntimeStatus::Error;
                    st.last_error = Some(msg.clone());
                    Err(msg)
                }
                Err(_) => Err("Falha interna isolada na thread de gravação de áudio.".into()),
            }
        })
        .map_err(|e| anyhow!("falha ao iniciar worker de áudio: {e}"))?;

    Ok((handle, Some(stream)))
}

fn push_audio_samples<T: Copy>(
    rec_shared: &Arc<Mutex<AudioRecShared>>,
    state: &Arc<Mutex<SharedSourceState>>,
    frames_atomic: &Arc<AtomicU64>,
    data: &[T],
    in_ch: usize,
    out_ch: u16,
    to_f32: impl Fn(T) -> f32,
) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let step = in_ch.max(1);
        let nframes = data.len() / step;
        if nframes == 0 {
            return;
        }
        let mut pl = 0.0f32;
        let mut pr = 0.0f32;
        let mut out = Vec::with_capacity(nframes * out_ch as usize);
        for frame in data.chunks_exact(step) {
            let l = to_f32(frame[0]).clamp(-1.0, 1.0);
            let r = if step > 1 {
                to_f32(frame[1]).clamp(-1.0, 1.0)
            } else {
                l
            };
            pl = pl.max(l.abs());
            pr = pr.max(r.abs());
            if out_ch == 1 {
                out.push(0.5 * (l + r));
            } else {
                out.push(l);
                out.push(r);
            }
        }
        update_vu_peak(state, pl, pr);
        let mut rec = crate::util::lock(rec_shared);
        if let Some(w) = rec.writer.as_mut() {
            if let Err(e) = w.write(&out) {
                rec.error = Some(format!("Erro ao gravar áudio WAV: {e}"));
            } else {
                rec.frames_written = rec.frames_written.saturating_add(nframes as u64);
                let total = rec.frames_written;
                frames_atomic.store(total, Ordering::Relaxed);
                drop(rec);
                crate::util::lock(state).frames_captured = total;
            }
        }
    }));
}

fn finalize_audio_track(
    card: &CaptureSourceCard,
    session_dir: &Path,
    stem: &str,
    wav_name: &str,
    sample_rate: u32,
    channels: u16,
    frames: u64,
) -> Result<CapturedTrackEntry> {
    let dur_ticks = Ticks::from_samples(frames.min(i64::MAX as u64) as i64, sample_rate);
    #[cfg(windows)]
    if matches!(
        card.audio_codec,
        CaptureAudioCodec::Mp3 | CaptureAudioCodec::AacM4a
    ) && frames > 0
    {
        use crate::media::mf::encode::{MfEncodeConfig, MfOutputFormat, MfSinkEncoder};
        let wav_path = session_dir.join(wav_name);
        if let Ok(bytes) = std::fs::read(&wav_path) {
            if let Ok(pcm) = crate::media::wav::decode(&bytes, usize::MAX / 4) {
                let (fmt, ext) = match card.audio_codec {
                    CaptureAudioCodec::Mp3 => (MfOutputFormat::Mp3, "mp3"),
                    _ => (MfOutputFormat::M4aAac, "m4a"),
                };
                let target_name = format!("{stem}.{ext}");
                let target_path = session_dir.join(&target_name);
                let cfg = MfEncodeConfig {
                    format: fmt,
                    path: target_path,
                    width: 0,
                    height: 0,
                    fps: FrameRate::FPS_30,
                    video_bitrate_bps: 0,
                    audio_sr: Some(pcm.sample_rate),
                    audio_bitrate_bps: card.audio_quality.audio_bitrate_bps(),
                };
                if let Ok(mut enc) = MfSinkEncoder::create(cfg) {
                    let stereo = if pcm.channels == 1 {
                        let mut s = Vec::with_capacity(pcm.samples.len() * 2);
                        for &v in &pcm.samples {
                            s.push(v);
                            s.push(v);
                        }
                        s
                    } else {
                        pcm.samples
                    };
                    if enc.write_audio_f32(&stereo).is_ok() && enc.finish().is_ok() {
                        let _ = std::fs::remove_file(&wav_path);
                        return Ok(CapturedTrackEntry {
                            source_name: card.name.clone(),
                            is_video: false,
                            file: target_name,
                            codec: card.audio_codec.short_name().to_string(),
                            width: 0,
                            height: 0,
                            fps_num: 0,
                            fps_den: 0,
                            sample_rate,
                            channels: 2,
                            duration_ticks: dur_ticks.0,
                            frame_count: frames,
                        });
                    }
                }
            }
        }
    }

    let _ = (session_dir, stem);
    let codec_used = match card.audio_codec {
        CaptureAudioCodec::WavPcm24 => CaptureAudioCodec::WavPcm24.short_name().to_string(),
        _ => CaptureAudioCodec::WavPcm16.short_name().to_string(),
    };
    Ok(CapturedTrackEntry {
        source_name: card.name.clone(),
        is_video: false,
        file: wav_name.to_string(),
        codec: codec_used,
        width: 0,
        height: 0,
        fps_num: 0,
        fps_den: 0,
        sample_rate,
        channels,
        duration_ticks: dur_ticks.0,
        frame_count: frames,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::{apply_manifest_to_session, load_manifest, CaptureSettings};
    use crate::media::MediaCache;
    use crate::model::{Project, TrackKind};
    use crate::session::EditorSession;

    #[test]
    fn gravacao_multifonte_sintetica_gera_arquivos_manifesto_e_importa_em_trilhas_separadas() {
        let dir = std::env::temp_dir().join(format!("neoveg_cap_rt_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("criar pasta temporária");

        let defs = CaptureSettings::default();
        let mut cam = CaptureSourceCard::new(
            1,
            "Tela Principal".into(),
            CaptureSourceKind::SyntheticVideo {
                width: 128,
                height: 72,
                seed: 1,
            },
            &defs,
        );
        cam.video_codec = CaptureVideoCodec::MjpegAvi;
        cam.fps = FrameRate::FPS_25;

        let mut mic = CaptureSourceCard::new(
            2,
            "Microfone USB".into(),
            CaptureSourceKind::SyntheticAudio {
                freq_hz: 440,
                sample_rate: 48_000,
            },
            &defs,
        );
        mic.audio_codec = CaptureAudioCodec::WavPcm16;
        mic.audio_channels = 2;

        let sources = vec![cam, mic];
        let mut rt = CaptureRuntime::new();
        let sess_dir = rt
            .start_recording(&sources, &dir)
            .expect("iniciar gravação multi-fonte sintética");
        assert!(rt.is_recording());
        std::thread::sleep(Duration::from_millis(180));
        let res = rt.stop_recording().expect("parar gravação");
        assert!(!rt.is_recording());
        assert!(
            res.warnings.is_empty(),
            "avisos inesperados: {:?}",
            res.warnings
        );
        assert_eq!(res.session_dir, sess_dir);
        assert!(res.manifest_path.exists());
        assert_eq!(res.manifest.tracks.len(), 2);

        // Relê o manifesto do disco e importa para um projeto em trilhas dedicadas
        let reloaded = load_manifest(&res.manifest_path).expect("recarregar export.json");
        let mut session = EditorSession::new(Project::new_default());
        let tracks_before = session.project.tracks.len();
        let cache = MediaCache::default();
        let report = apply_manifest_to_session(
            &mut session,
            &cache,
            &reloaded,
            &res.session_dir,
            Ticks::from_secs_i64(2),
        )
        .expect("importar sessão na timeline");

        assert_eq!(report.imported_media_ids.len(), 2);
        assert_eq!(report.created_track_ids.len(), 2);
        assert_eq!(report.placed_event_ids.len(), 2);
        assert_eq!(session.project.tracks.len(), tracks_before + 2);
        assert!(session
            .project
            .tracks
            .iter()
            .any(|t| t.kind == TrackKind::Video
                && t.name == "Tela Principal"
                && t.events.len() == 1));
        assert!(session
            .project
            .tracks
            .iter()
            .any(|t| t.kind == TrackKind::Audio
                && t.name == "Microfone USB"
                && t.events.len() == 1));

        // Um único Ctrl+Z desfaz toda a importação da sessão
        assert!(session.undo());
        assert_eq!(session.project.tracks.len(), tracks_before);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn gravacao_ffv1_mkv_e_wav_mono_24bit_funcionam() {
        let dir = std::env::temp_dir().join(format!("neoveg_cap_ffv1_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("criar pasta");

        let defs = CaptureSettings::default();
        let mut v = CaptureSourceCard::new(
            10,
            "Janela App".into(),
            CaptureSourceKind::SyntheticVideo {
                width: 64,
                height: 48,
                seed: 9,
            },
            &defs,
        );
        v.video_codec = CaptureVideoCodec::Ffv1Mkv;

        let mut a = CaptureSourceCard::new(
            11,
            "Mic Mono".into(),
            CaptureSourceKind::SyntheticAudio {
                freq_hz: 330,
                sample_rate: 44_100,
            },
            &defs,
        );
        a.audio_codec = CaptureAudioCodec::WavPcm24;
        a.audio_channels = 1;

        let mut rt = CaptureRuntime::new();
        rt.start_recording(&[v, a], &dir).expect("iniciar");
        std::thread::sleep(Duration::from_millis(120));
        let res = rt.stop_recording().expect("parar");
        assert_eq!(res.manifest.tracks.len(), 2);
        assert!(res.manifest.tracks.iter().any(|t| t.file.ends_with(".mkv")));
        assert!(res
            .manifest
            .tracks
            .iter()
            .any(|t| t.file.ends_with(".wav") && t.channels == 1));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
