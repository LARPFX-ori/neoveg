//! Reprodução em tempo real: saída de áudio (cpal/WASAPI) que também serve de
//! relógio mestre, e um worker de prévia de vídeo em segundo plano.

use crate::engine::audio::{Meters, Mixer, TransportVarispeed};
use crate::engine::loudness::{LoudnessAnalyzer, LoudnessSnapshot};
use crate::engine::video::{render_event_still, render_frame_with_opts, VideoRenderOpts};
use crate::engine::Frame;
use crate::media::{AudioData, MediaCache};
use crate::model::{FrameRate, Id, Project, Ticks};
use crate::render::{WavBits, WavWriter};
use crate::scopes;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

pub struct Shared {
    pub project: Arc<Project>,
    pub audio: HashMap<Id, Arc<AudioData>>,
    pub playing: bool,
    /// Posição em amostras na taxa do dispositivo.
    pub pos: i64,
    pub loop_range: Option<(i64, i64)>,
    pub mixer: Mixer,
    pub varispeed: TransportVarispeed,
    pub meters: Meters,
    pub loudness: LoudnessAnalyzer,
    pub loudness_snapshot: LoudnessSnapshot,
    /// Velocidade de reprodução (`Playback Rate` contínuo / shuttle J-K-L).
    pub speed: f64,
    pub frac: f64,
    pub dim: bool,
    pub mono_downmix: bool,
    /// Silencia apenas a monitoração de áudio da prévia sem afetar as trilhas/render.
    pub preview_muted: bool,
    /// Erro ocorrido na thread de áudio, exibido pela interface.
    pub error: Option<String>,
}

impl Shared {
    pub fn reset_dsp(&mut self) {
        self.mixer.reset();
        self.varispeed.reset();
    }
}

pub struct AudioOut {
    _stream: Option<cpal::Stream>,
    pub shared: Arc<Mutex<Shared>>,
    pub sr: u32,
    pub device_name: String,
    pub error: Option<String>,
}

/// Mantém viva a thread que fez a primeira consulta de dispositivos de áudio.
///
/// Por quê: no Windows o `cpal` (WASAPI) cria o enumerador de dispositivos COM
/// uma única vez, num `static`, na primeira thread que consulta dispositivos, e
/// cada thread desinicializa o COM ao terminar. Se essa primeira thread morre
/// (um teste, um worker), o enumerador fica sem apartamento e a próxima
/// consulta derruba o processo com `STATUS_ACCESS_VIOLATION` — visto no runner
/// da CI, que não tem placa de som (com placa, a thread do stream mantém o COM
/// vivo e esconde o problema). Esta thread faz a primeira consulta e nunca
/// termina. Não remova sem resolver o enumerador do `cpal` de outro jeito.
#[cfg(windows)]
fn ensure_audio_com_keeper() {
    use std::sync::OnceLock;
    use std::time::Duration;
    static KEEPER: OnceLock<()> = OnceLock::new();
    KEEPER.get_or_init(|| {
        let (tx, rx) = channel::<()>();
        let spawned = std::thread::Builder::new()
            .name("audio-com".into())
            .spawn(move || {
                let _ = cpal::default_host().default_output_device();
                let _ = tx.send(());
                loop {
                    std::thread::park();
                }
            });
        if spawned.is_ok() {
            let _ = rx.recv_timeout(Duration::from_secs(10));
        }
    });
}

#[cfg(not(windows))]
fn ensure_audio_com_keeper() {}

impl AudioOut {
    pub fn new(project: Arc<Project>) -> Self {
        ensure_audio_com_keeper();
        let mut sr = 48000;
        let mut device_name = "(sem dispositivo)".to_string();
        let mut error = None;
        let host = cpal::default_host();
        let device = host.default_output_device();
        let config = device.as_ref().and_then(|d| d.default_output_config().ok());
        if let Some(c) = &config {
            sr = c.sample_rate().0;
        }
        let shared = Arc::new(Mutex::new(Shared {
            project,
            audio: HashMap::new(),
            playing: false,
            pos: 0,
            loop_range: None,
            mixer: Mixer::new(sr),
            varispeed: TransportVarispeed::new(),
            meters: Meters::default(),
            loudness: LoudnessAnalyzer::new(sr),
            loudness_snapshot: LoudnessSnapshot::default(),
            speed: 1.0,
            frac: 0.0,
            dim: false,
            mono_downmix: false,
            preview_muted: false,
            error: None,
        }));
        let mut stream = None;
        if let (Some(dev), Some(cfg)) = (device, config) {
            device_name = dev.name().unwrap_or_default();
            match build_stream(&dev, cfg, shared.clone()) {
                Ok(s) => {
                    if let Err(e) = s.play() {
                        error = Some(format!("falha ao iniciar áudio: {e}"));
                    }
                    stream = Some(s);
                }
                Err(e) => error = Some(format!("falha ao abrir dispositivo de áudio: {e}")),
            }
        } else {
            error = Some("nenhum dispositivo de saída de áudio".into());
        }
        AudioOut {
            _stream: stream,
            shared,
            sr,
            device_name,
            error,
        }
    }

    pub fn has_device(&self) -> bool {
        self._stream.is_some()
    }

    pub fn loudness_snapshot(&self) -> LoudnessSnapshot {
        crate::util::lock(&self.shared).loudness_snapshot.clone()
    }

    pub fn reset_loudness(&self) {
        let mut s = crate::util::lock(&self.shared);
        s.loudness.reset();
        s.loudness_snapshot = s.loudness.snapshot();
    }

    /// Ignora ou reativa todos os efeitos de áudio (eventos, trilhas, barramentos e master).
    pub fn set_bypass_audio_fx(&self, bypass: bool) {
        let mut s = crate::util::lock(&self.shared);
        s.mixer.bypass_fx = bypass;
    }

    /// Avança o relógio quando não há dispositivo de áudio.
    pub fn tick_without_device(&self, dt: f64) {
        if self.has_device() {
            return;
        }
        let mut s = crate::util::lock(&self.shared);
        if s.playing {
            let adv = dt * self.sr as f64 * s.speed + s.frac;
            s.pos += adv.trunc() as i64;
            s.frac = adv.fract();
            if let Some((a, b)) = s.loop_range {
                if s.pos >= b {
                    s.pos = a;
                }
            }
            if s.pos < 0 {
                s.pos = 0;
                s.playing = false;
            }
        }
    }
}

fn build_stream(
    dev: &cpal::Device,
    cfg: cpal::SupportedStreamConfig,
    shared: Arc<Mutex<Shared>>,
) -> anyhow::Result<cpal::Stream> {
    let fmt = cfg.sample_format();
    let config: cpal::StreamConfig = cfg.into();
    let channels = config.channels as usize;
    let err_fn = |e| eprintln!("erro no fluxo de áudio: {e}");
    let stream = match fmt {
        cpal::SampleFormat::F32 => dev.build_output_stream(
            &config,
            move |data: &mut [f32], _| fill(data, channels, &shared, |v| v),
            err_fn,
            None,
        )?,
        cpal::SampleFormat::I16 => dev.build_output_stream(
            &config,
            move |data: &mut [i16], _| {
                fill(data, channels, &shared, |v| {
                    (v.clamp(-1.0, 1.0) * 32767.0) as i16
                })
            },
            err_fn,
            None,
        )?,
        cpal::SampleFormat::U16 => dev.build_output_stream(
            &config,
            move |data: &mut [u16], _| {
                fill(data, channels, &shared, |v| {
                    ((v.clamp(-1.0, 1.0) * 0.5 + 0.5) * 65535.0) as u16
                })
            },
            err_fn,
            None,
        )?,
        other => anyhow::bail!("formato de amostra não suportado: {other:?}"),
    };
    Ok(stream)
}

fn fill<T: Copy>(
    data: &mut [T],
    channels: usize,
    shared: &Arc<Mutex<Shared>>,
    conv: impl Fn(f32) -> T,
) {
    let frames = data.len() / channels.max(1);
    let mut buf = vec![0f32; frames * 2];
    // §0.6: um pânico na mixagem não pode derrubar a thread de áudio;
    // em caso de falha, toca silêncio e interrompe a reprodução.
    let ok = crate::util::guard("mixagem de áudio", || mix_into(&mut buf, frames, shared));
    if ok.is_err() {
        buf.iter_mut().for_each(|v| *v = 0.0);
        let mut s = crate::util::lock(shared);
        s.playing = false;
        s.reset_dsp();
        s.error = Some("falha interna na mixagem de áudio; reprodução interrompida".into());
    }
    for f in 0..frames {
        let l = buf.get(f * 2).copied().unwrap_or(0.0);
        let r = buf.get(f * 2 + 1).copied().unwrap_or(0.0);
        for c in 0..channels {
            if let Some(d) = data.get_mut(f * channels + c) {
                *d = conv(match (channels, c) {
                    (1, _) => (l + r) * 0.5,
                    (_, 0) => l,
                    (_, 1) => r,
                    _ => 0.0,
                });
            }
        }
    }
}

fn mix_into(buf: &mut [f32], frames: usize, shared: &Arc<Mutex<Shared>>) {
    {
        let mut s = crate::util::lock(shared);
        if s.playing {
            let project = s.project.clone();
            let speed = s.speed;
            let loop_range = s.loop_range;
            let sh = &mut *s;
            let mut pos = sh.pos;
            let mut frac = sh.frac;
            let hit_start = sh.varispeed.process(
                speed,
                &mut pos,
                &mut frac,
                loop_range,
                &mut buf[..frames * 2],
                |sample_pos, chunk| {
                    sh.mixer.process(&project, &sh.audio, sample_pos, chunk);
                },
            );
            sh.pos = pos;
            sh.frac = frac;
            if hit_start {
                sh.playing = false;
                sh.reset_dsp();
            }
            s.meters = s.mixer.meters.clone();
            s.loudness.push_interleaved(&buf[..frames * 2]);
            s.loudness_snapshot = s.loudness.snapshot();
            if s.mono_downmix {
                let mut mono_peak = 0.0_f32;
                for pair in buf.as_chunks_mut::<2>().0.iter_mut().take(frames) {
                    let m = 0.5 * (pair[0] + pair[1]);
                    pair[0] = m;
                    pair[1] = m;
                    mono_peak = mono_peak.max(m.abs());
                }
                s.meters.master = [mono_peak, mono_peak];
            }
            if s.dim {
                buf.iter_mut().take(frames * 2).for_each(|v| *v *= 0.1);
            }
            if s.preview_muted {
                buf.iter_mut().take(frames * 2).for_each(|v| *v = 0.0);
                s.meters.master = [0.0, 0.0];
            }
        } else {
            s.meters.master = [0.0, 0.0];
            for v in s.meters.tracks.values_mut() {
                *v = [0.0, 0.0];
            }
            for v in s.meters.buses.values_mut() {
                *v = [0.0, 0.0];
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Prévia de vídeo
// ---------------------------------------------------------------------------

/// Modo de comparação da Prévia de Vídeo em tela dividida (manual do VEGAS Pro 21, p. 203).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PreviewSplitMode {
    #[default]
    Off,
    FxBypass,
    Clipboard,
}

impl PreviewSplitMode {
    pub const ALL: [PreviewSplitMode; 3] = [
        PreviewSplitMode::Off,
        PreviewSplitMode::FxBypass,
        PreviewSplitMode::Clipboard,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PreviewSplitMode::Off => "Normal",
            PreviewSplitMode::FxBypass => "Tela dividida: FX ignorados",
            PreviewSplitMode::Clipboard => "Tela dividida: Área de transferência",
        }
    }
}

/// Cache de quadros pré-renderizados em memória para Prévia Dinâmica em RAM (`Shift+B`, p. 207).
#[derive(Clone, Debug)]
pub struct RamPreviewCache {
    pub generation: u64,
    pub width: usize,
    pub height: usize,
    pub start: Ticks,
    pub end: Ticks,
    pub fps: FrameRate,
    pub frames: HashMap<i64, image::RgbaImage>,
    pub bytes_used: usize,
    pub max_bytes: usize,
}

impl Default for RamPreviewCache {
    fn default() -> Self {
        Self::new(256)
    }
}

impl RamPreviewCache {
    pub fn new(max_mb: usize) -> Self {
        let cap_mb = max_mb.clamp(32, 4096);
        Self {
            generation: 0,
            width: 0,
            height: 0,
            start: Ticks::ZERO,
            end: Ticks::ZERO,
            fps: FrameRate::FPS_29_97,
            frames: HashMap::new(),
            bytes_used: 0,
            max_bytes: cap_mb.saturating_mul(1024 * 1024),
        }
    }

    pub fn reset(
        &mut self,
        generation: u64,
        (width, height): (usize, usize),
        (start, end): (Ticks, Ticks),
        fps: FrameRate,
        max_mb: usize,
    ) {
        self.generation = generation;
        self.width = width;
        self.height = height;
        self.start = start;
        self.end = end;
        self.fps = fps;
        self.frames.clear();
        self.bytes_used = 0;
        self.max_bytes = max_mb.clamp(32, 4096).saturating_mul(1024 * 1024);
    }

    pub fn clear(&mut self) {
        self.frames.clear();
        self.bytes_used = 0;
        self.generation = 0;
    }

    pub fn insert_frame(&mut self, frame_idx: i64, img: image::RgbaImage) -> bool {
        let sz = img.as_raw().len();
        if self.bytes_used.saturating_add(sz) > self.max_bytes {
            return false;
        }
        if let Some(old) = self.frames.insert(frame_idx, img) {
            self.bytes_used = self.bytes_used.saturating_sub(old.as_raw().len());
        }
        self.bytes_used = self.bytes_used.saturating_add(sz);
        true
    }

    pub fn get(
        &self,
        generation: u64,
        frame_idx: i64,
        w: usize,
        h: usize,
    ) -> Option<&image::RgbaImage> {
        if self.generation != generation || self.width != w || self.height != h {
            return None;
        }
        self.frames.get(&frame_idx)
    }

    pub fn mb_used(&self) -> f32 {
        self.bytes_used as f32 / (1024.0 * 1024.0)
    }
}

pub struct PreviewRequest {
    pub project: Arc<Project>,
    pub t: Ticks,
    pub w: usize,
    pub h: usize,
    pub scopes: bool,
    pub prefer_proxy: bool,
    pub split_mode: PreviewSplitMode,
    pub mute_video: bool,
    /// Modo de edição expandido (prévia dupla A/B lado a lado): `(esq_id, esq_t, dir_id, dir_t)`.
    pub expanded_ab: Option<(Option<Id>, Ticks, Option<Id>, Ticks)>,
}

pub struct PreviewResult {
    pub image: image::RgbaImage,
    pub split_alt: Option<image::RgbaImage>,
    pub scopes: Option<scopes::ScopeImages>,
    pub render_ms: f64,
    pub error: Option<String>,
    pub expanded_ab: Option<(image::RgbaImage, image::RgbaImage)>,
}

pub struct Preview {
    tx: Sender<PreviewRequest>,
    pub rx: Receiver<PreviewResult>,
    pub error: Option<String>,
}

fn error_image(w: usize, h: usize) -> image::RgbaImage {
    image::RgbaImage::from_pixel(
        w.max(1) as u32,
        h.max(1) as u32,
        image::Rgba([90, 0, 0, 255]),
    )
}

impl Preview {
    pub fn new(media: Arc<MediaCache>, notify: Box<dyn Fn() + Send + Sync>) -> Self {
        let (tx, rx_req) = channel::<PreviewRequest>();
        let (tx_res, rx) = channel::<PreviewResult>();
        let spawned = std::thread::Builder::new()
            .name("preview".into())
            .spawn(move || {
                while let Ok(mut req) = rx_req.recv() {
                    while let Ok(r) = rx_req.try_recv() {
                        req = r;
                    }
                    let t0 = std::time::Instant::now();
                    // §0.6: uma falha ao renderizar um quadro vira um quadro de erro,
                    // e o worker continua vivo para os próximos pedidos.
                    let rendered = crate::util::guard("prévia de vídeo", || {
                        let f = if req.mute_video {
                            Frame::filled(req.w, req.h, [0.04, 0.04, 0.05, 1.0])
                        } else {
                            render_frame_with_opts(
                                &req.project,
                                &media,
                                req.t,
                                req.w,
                                req.h,
                                VideoRenderOpts {
                                    prefer_proxy: req.prefer_proxy,
                                    bypass_fx: false,
                                },
                            )
                        };
                        let split_alt =
                            if !req.mute_video && req.split_mode == PreviewSplitMode::FxBypass {
                                let raw_f = render_frame_with_opts(
                                    &req.project,
                                    &media,
                                    req.t,
                                    req.w,
                                    req.h,
                                    VideoRenderOpts {
                                        prefer_proxy: req.prefer_proxy,
                                        bypass_fx: true,
                                    },
                                );
                                Some(
                                    image::RgbaImage::from_raw(
                                        req.w as u32,
                                        req.h as u32,
                                        raw_f.to_rgba8(),
                                    )
                                    .unwrap_or_else(|| error_image(req.w, req.h)),
                                )
                            } else {
                                None
                            };
                        let sc = if req.scopes {
                            Some(scopes::compute(&f))
                        } else {
                            None
                        };
                        let ab = req.expanded_ab.map(|(lid, lt, rid, rt)| {
                            let hw = (req.w / 2).max(1);
                            let hh = req.h.max(1);
                            let render_side = |ev_id: Option<Id>, st: Ticks| {
                                if let Some(id) = ev_id {
                                    let fr = render_event_still(
                                        &req.project,
                                        &media,
                                        id,
                                        st,
                                        hw,
                                        hh,
                                        req.prefer_proxy,
                                    );
                                    image::RgbaImage::from_raw(hw as u32, hh as u32, fr.to_rgba8())
                                        .unwrap_or_else(|| error_image(hw, hh))
                                } else {
                                    image::RgbaImage::from_pixel(
                                        hw as u32,
                                        hh as u32,
                                        image::Rgba([14, 16, 20, 255]),
                                    )
                                }
                            };
                            (render_side(lid, lt), render_side(rid, rt))
                        });
                        let rgba = f.to_rgba8();
                        let img = image::RgbaImage::from_raw(req.w as u32, req.h as u32, rgba)
                            .unwrap_or_else(|| error_image(req.w, req.h));
                        (img, split_alt, sc, ab)
                    });
                    let (image, split_alt, sc, ab, error) = match rendered {
                        Ok((img, split_alt, sc, ab)) => (img, split_alt, sc, ab, None),
                        Err(e) => (error_image(req.w, req.h), None, None, None, Some(e)),
                    };
                    if tx_res
                        .send(PreviewResult {
                            image,
                            split_alt,
                            scopes: sc,
                            render_ms: t0.elapsed().as_secs_f64() * 1000.0,
                            error,
                            expanded_ab: ab,
                        })
                        .is_err()
                    {
                        break;
                    }
                    notify();
                }
            });
        let error = spawned
            .err()
            .map(|e| format!("não foi possível iniciar a prévia: {e}"));
        Preview { tx, rx, error }
    }

    pub fn request(&self, r: PreviewRequest) {
        let _ = self.tx.send(r);
    }
}

// ---------------------------------------------------------------------------
// Gravação de áudio nas trilhas armadas (manual do VEGAS Pro 21, p. 821)
// ---------------------------------------------------------------------------

struct RecorderInner {
    writer: Option<WavWriter>,
    path: PathBuf,
    target_tracks: Vec<Id>,
    start_cursor: Ticks,
    sample_rate: u32,
    frames_written: u64,
    peak: [f32; 2],
    error: Option<String>,
}

/// Resultado final de uma tomada de gravação em arquivo WAV.
#[derive(Clone, Debug)]
pub struct RecordedClip {
    pub path: PathBuf,
    pub start_cursor: Ticks,
    pub sample_rate: u32,
    pub frames_written: u64,
    pub target_tracks: Vec<Id>,
}

/// Gravador de áudio via `cpal` (entrada padrão do SO) gravando em WAV estéreo
/// PCM 16 bits sob proteção contra pânicos em callbacks (§0.6).
pub struct AudioRecorder {
    _stream: Option<cpal::Stream>,
    inner: Arc<Mutex<RecorderInner>>,
    pub sample_rate: u32,
    pub channels: u16,
    pub device_name: String,
}

impl AudioRecorder {
    /// Inicia captura do dispositivo de entrada padrão gravando em `out_path`.
    pub fn start(
        target_tracks: Vec<Id>,
        start_cursor: Ticks,
        out_path: PathBuf,
    ) -> Result<Self, String> {
        if target_tracks.is_empty() {
            return Err("Nenhuma faixa de áudio armada para gravação.".into());
        }
        ensure_audio_com_keeper();
        let host = cpal::default_host();
        let device = host.default_input_device().ok_or_else(|| {
            "Nenhum dispositivo de entrada de áudio (microfone) encontrado.".to_string()
        })?;
        let device_name = device.name().unwrap_or_else(|_| "Entrada de áudio".into());
        let supported = device.default_input_config().map_err(|e| {
            format!("Não foi possível consultar o formato da entrada de áudio: {e}")
        })?;
        let sr = supported.sample_rate().0.clamp(8_000, 192_000);
        let ch = supported.channels().max(1);
        let sample_format = supported.sample_format();
        let cfg = supported.config();

        if let Some(parent) = out_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let writer = WavWriter::create(&out_path, sr, WavBits::Pcm16)
            .map_err(|e| format!("Não foi possível criar WAV de gravação: {e}"))?;
        let inner = Arc::new(Mutex::new(RecorderInner {
            writer: Some(writer),
            path: out_path,
            target_tracks,
            start_cursor,
            sample_rate: sr,
            frames_written: 0,
            peak: [0.0, 0.0],
            error: None,
        }));

        let err_state = inner.clone();
        let err_fn = move |e: cpal::StreamError| {
            let mut st = crate::util::lock(&err_state);
            st.error = Some(format!("Erro de gravação de áudio: {e}"));
        };
        let state = inner.clone();
        let stream = match sample_format {
            cpal::SampleFormat::F32 => device.build_input_stream(
                &cfg,
                move |data: &[f32], _| Self::push_typed(&state, data, ch as usize, |x| x),
                err_fn,
                None,
            ),
            cpal::SampleFormat::I16 => device.build_input_stream(
                &cfg,
                move |data: &[i16], _| {
                    Self::push_typed(&state, data, ch as usize, |x| f32::from(x) / 32768.0)
                },
                err_fn,
                None,
            ),
            cpal::SampleFormat::U16 => device.build_input_stream(
                &cfg,
                move |data: &[u16], _| {
                    Self::push_typed(&state, data, ch as usize, |x| {
                        (f32::from(x) - 32768.0) / 32768.0
                    })
                },
                err_fn,
                None,
            ),
            other => {
                return Err(format!(
                    "Formato de amostra de entrada não suportado: {other:?}"
                ));
            }
        }
        .map_err(|e| format!("Não foi possível abrir o fluxo de gravação: {e}"))?;

        stream
            .play()
            .map_err(|e| format!("Não foi possível iniciar a captura de áudio: {e}"))?;

        Ok(Self {
            _stream: Some(stream),
            inner,
            sample_rate: sr,
            channels: ch,
            device_name,
        })
    }

    /// Inicia um gravador sintético sem dispositivo físico (para testes determinísticos em CI).
    pub fn start_synthetic(
        target_tracks: Vec<Id>,
        start_cursor: Ticks,
        out_path: PathBuf,
        sample_rate: u32,
    ) -> Result<Self, String> {
        if target_tracks.is_empty() {
            return Err("Nenhuma faixa de áudio armada para gravação.".into());
        }
        let sr = sample_rate.clamp(8_000, 192_000);
        if let Some(parent) = out_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let writer = WavWriter::create(&out_path, sr, WavBits::Pcm16)
            .map_err(|e| format!("Não foi possível criar WAV sintético: {e}"))?;
        let inner = Arc::new(Mutex::new(RecorderInner {
            writer: Some(writer),
            path: out_path,
            target_tracks,
            start_cursor,
            sample_rate: sr,
            frames_written: 0,
            peak: [0.0, 0.0],
            error: None,
        }));
        Ok(Self {
            _stream: None,
            inner,
            sample_rate: sr,
            channels: 2,
            device_name: "Entrada sintética".into(),
        })
    }

    fn push_typed<T: Copy>(
        inner: &Arc<Mutex<RecorderInner>>,
        data: &[T],
        channels: usize,
        conv: impl Fn(T) -> f32,
    ) {
        let ch = channels.max(1);
        let frames = data.len() / ch;
        if frames == 0 {
            return;
        }
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut stereo = Vec::with_capacity(frames * 2);
            let mut pk = [0.0f32; 2];
            for f in 0..frames {
                let l_raw = conv(data[f * ch]);
                let r_raw = if ch >= 2 {
                    conv(data[f * ch + 1])
                } else {
                    l_raw
                };
                let l = if l_raw.is_finite() {
                    l_raw.clamp(-1.0, 1.0)
                } else {
                    0.0
                };
                let r = if r_raw.is_finite() {
                    r_raw.clamp(-1.0, 1.0)
                } else {
                    0.0
                };
                pk[0] = pk[0].max(l.abs());
                pk[1] = pk[1].max(r.abs());
                stereo.push(l);
                stereo.push(r);
            }
            let mut st = crate::util::lock(inner);
            st.peak = [st.peak[0].max(pk[0]), st.peak[1].max(pk[1])];
            if let Some(w) = st.writer.as_mut() {
                if let Err(e) = w.write(&stereo) {
                    st.error = Some(format!("Falha ao gravar WAV: {e}"));
                } else {
                    st.frames_written = st.frames_written.saturating_add(frames as u64);
                }
            }
        }));
    }

    /// Alimenta quadros estéreo diretamente (para captura sintética ou ponte externa).
    pub fn push_stereo(&self, stereo: &[f32]) {
        Self::push_typed(&self.inner, stereo, 2, |x| x);
    }

    /// Lê e atenua suavemente os picos de entrada `[L, R]` (`0.0..=1.0`).
    pub fn take_peaks(&self) -> [f32; 2] {
        let mut st = crate::util::lock(&self.inner);
        let p = st.peak;
        st.peak = [p[0] * 0.5, p[1] * 0.5];
        p
    }

    /// Quantos quadros de áudio já foram gravados e qual a duração aproximada.
    pub fn frames_written(&self) -> u64 {
        crate::util::lock(&self.inner).frames_written
    }

    pub fn last_error(&self) -> Option<String> {
        crate::util::lock(&self.inner).error.clone()
    }

    /// Encerra a captura, finaliza o cabeçalho RIFF/WAV e retorna os metadados do clipe.
    pub fn stop(mut self) -> Result<RecordedClip, String> {
        self._stream = None;
        let mut st = crate::util::lock(&self.inner);
        if let Some(w) = st.writer.take() {
            w.finish()
                .map_err(|e| format!("Falha ao finalizar arquivo WAV gravado: {e}"))?;
        }
        if let Some(e) = &st.error {
            return Err(e.clone());
        }
        Ok(RecordedClip {
            path: st.path.clone(),
            start_cursor: st.start_cursor,
            sample_rate: st.sample_rate,
            frames_written: st.frames_written,
            target_tracks: st.target_tracks.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regressão: criar a saída de áudio em threads que terminam não pode
    /// derrubar o processo. Sem a thread guardiã, num Windows sem placa de som
    /// (runner da CI), a segunda criação caía em `STATUS_ACCESS_VIOLATION`.
    #[test]
    fn audio_out_survives_creator_threads_exiting() {
        for i in 0..4 {
            let h = std::thread::spawn(|| {
                let a = AudioOut::new(Arc::new(Project::new_default()));
                a.has_device()
            });
            assert!(h.join().is_ok(), "criação {i} falhou");
        }
    }

    #[test]
    fn gravador_sintetico_gera_wav_pcm16_legivel_e_sem_faixas_falha_limpo() {
        let empty = AudioRecorder::start_synthetic(
            vec![],
            Ticks::ZERO,
            std::env::temp_dir().join("neoveg_rec_empty.wav"),
            48000,
        );
        assert!(empty.is_err());

        let out_wav = std::env::temp_dir().join(format!("neoveg_rec_{}.wav", std::process::id()));
        let rec = AudioRecorder::start_synthetic(
            vec![42],
            Ticks::from_secs_i64(2),
            out_wav.clone(),
            48000,
        )
        .expect("inicia gravador sintético");
        let mut stereo = Vec::with_capacity(4800 * 2);
        for i in 0..4800usize {
            let v = (2.0 * std::f32::consts::PI * 440.0 * (i as f32 / 48000.0)).sin() * 0.5;
            stereo.push(v);
            stereo.push(-v);
        }
        rec.push_stereo(&stereo);
        rec.push_stereo(&[f32::NAN, f32::INFINITY, -f32::INFINITY, 2.0]);
        assert!(rec.frames_written() >= 4802);
        let pk = rec.take_peaks();
        assert!(pk[0] > 0.4 && pk[0] <= 1.0);
        let clip = rec.stop().expect("fecha wav");
        assert_eq!(clip.start_cursor, Ticks::from_secs_i64(2));
        let bytes = std::fs::read(&clip.path).expect("lê wav");
        let decoded = crate::media::wav::decode(&bytes, 1_000_000).expect("decodifica wav gravado");
        assert_eq!(decoded.sample_rate, 48000);
        assert_eq!(decoded.samples.len() / 2, clip.frames_written as usize);
        assert!(decoded.samples.iter().all(|s| s.is_finite()));
        let _ = std::fs::remove_file(&clip.path);
    }

    #[test]
    fn ram_preview_cache_respeita_orcamento_e_geracao() {
        let mut cache = RamPreviewCache::new(32);
        cache.reset(
            7,
            (64, 64),
            (Ticks::ZERO, Ticks::from_secs_i64(2)),
            FrameRate::FPS_30,
            32,
        );
        cache.max_bytes = 32 * 1024; // 32 KiB para teste determinístico (§0.4)
        let f = image::RgbaImage::from_pixel(64, 64, image::Rgba([10, 20, 30, 255]));
        // 64*64*4 = 16 KiB: cabem 2 quadros e o 3º é recusado sem pânico
        assert!(cache.insert_frame(0, f.clone()));
        assert!(cache.insert_frame(1, f.clone()));
        assert!(!cache.insert_frame(2, f));
        assert!(cache.get(7, 0, 64, 64).is_some());
        assert!(cache.get(8, 0, 64, 64).is_none());
        assert!(cache.get(7, 0, 32, 32).is_none());
    }
}
