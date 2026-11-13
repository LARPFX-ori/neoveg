//! Reprodução em tempo real: saída de áudio (cpal/WASAPI) que também serve de
//! relógio mestre, e um worker de prévia de vídeo em segundo plano.

use crate::engine::audio::{Meters, Mixer};
use crate::engine::video::render_frame;
use crate::media::{AudioData, MediaCache};
use crate::model::{Id, Project};
use crate::scopes;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use eframe::egui;
use std::collections::HashMap;
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
    pub meters: Meters,
    /// Velocidade de reprodução (J/K/L). Áudio só é tocado em 1x.
    pub speed: f64,
    pub frac: f64,
    pub dim: bool,
}

pub struct AudioOut {
    _stream: Option<cpal::Stream>,
    pub shared: Arc<Mutex<Shared>>,
    pub sr: u32,
    pub device_name: String,
    pub error: Option<String>,
}

impl AudioOut {
    pub fn new(project: Arc<Project>) -> Self {
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
            meters: Meters::default(),
            speed: 1.0,
            frac: 0.0,
            dim: false,
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

    /// Avança o relógio quando não há dispositivo de áudio.
    pub fn tick_without_device(&self, dt: f64) {
        if self.has_device() {
            return;
        }
        let mut s = self.shared.lock().unwrap();
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

fn build_stream(dev: &cpal::Device, cfg: cpal::SupportedStreamConfig, shared: Arc<Mutex<Shared>>) -> anyhow::Result<cpal::Stream> {
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
            move |data: &mut [i16], _| fill(data, channels, &shared, |v| (v.clamp(-1.0, 1.0) * 32767.0) as i16),
            err_fn,
            None,
        )?,
        cpal::SampleFormat::U16 => dev.build_output_stream(
            &config,
            move |data: &mut [u16], _| fill(data, channels, &shared, |v| ((v.clamp(-1.0, 1.0) * 0.5 + 0.5) * 65535.0) as u16),
            err_fn,
            None,
        )?,
        other => anyhow::bail!("formato de amostra não suportado: {other:?}"),
    };
    Ok(stream)
}

fn fill<T: Copy>(data: &mut [T], channels: usize, shared: &Arc<Mutex<Shared>>, conv: impl Fn(f32) -> T) {
    let frames = data.len() / channels.max(1);
    let mut buf = vec![0f32; frames * 2];
    if let Ok(mut s) = shared.lock() {
        if s.playing {
            let sr = s.mixer.sr as f64;
            if (s.speed - 1.0).abs() < 1e-9 {
                // mixa em partes para respeitar o loop
                let mut done = 0usize;
                while done < frames {
                    let mut n = frames - done;
                    if let Some((a, b)) = s.loop_range {
                        if s.pos >= b {
                            s.pos = a;
                            s.mixer.reset();
                        }
                        n = n.min((b - s.pos).max(1) as usize);
                    }
                    let project = s.project.clone();
                    let pos = s.pos;
                    let sh = &mut *s;
                    sh.mixer.process(&project, &sh.audio, pos, &mut buf[done * 2..(done + n) * 2]);
                    s.pos += n as i64;
                    done += n;
                }
                s.meters = s.mixer.meters.clone();
                if s.dim {
                    buf.iter_mut().for_each(|v| *v *= 0.1);
                }
            } else {
                let adv = frames as f64 * s.speed + s.frac;
                s.pos += adv.trunc() as i64;
                s.frac = adv.fract();
                if s.pos < 0 {
                    s.pos = 0;
                    s.playing = false;
                }
                let _ = sr;
            }
        } else {
            s.meters.master = [0.0, 0.0];
            for v in s.meters.tracks.values_mut() {
                *v = [0.0, 0.0];
            }
        }
    }
    for f in 0..frames {
        let (l, r) = (buf[f * 2], buf[f * 2 + 1]);
        for c in 0..channels {
            data[f * channels + c] = conv(match (channels, c) {
                (1, _) => (l + r) * 0.5,
                (_, 0) => l,
                (_, 1) => r,
                _ => 0.0,
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Prévia de vídeo
// ---------------------------------------------------------------------------

pub struct PreviewRequest {
    pub project: Arc<Project>,
    pub t: f64,
    pub w: usize,
    pub h: usize,
    pub scopes: bool,
    pub overlay_safe: bool,
}

pub struct PreviewResult {
    pub image: egui::ColorImage,
    pub t: f64,
    pub scopes: Option<scopes::ScopeImages>,
    pub render_ms: f64,
}

pub struct Preview {
    tx: Sender<PreviewRequest>,
    pub rx: Receiver<PreviewResult>,
}

impl Preview {
    pub fn new(media: Arc<MediaCache>, ctx: egui::Context) -> Self {
        let (tx, rx_req) = channel::<PreviewRequest>();
        let (tx_res, rx) = channel::<PreviewResult>();
        std::thread::Builder::new()
            .name("preview".into())
            .spawn(move || {
                while let Ok(mut req) = rx_req.recv() {
                    while let Ok(r) = rx_req.try_recv() {
                        req = r;
                    }
                    let t0 = std::time::Instant::now();
                    let f = render_frame(&req.project, &media, req.t, req.w, req.h);
                    let sc = if req.scopes { Some(scopes::compute(&f)) } else { None };
                    let rgba = f.to_rgba8();
                    let image = egui::ColorImage::from_rgba_unmultiplied([req.w, req.h], &rgba);
                    let _ = req.overlay_safe;
                    if tx_res
                        .send(PreviewResult {
                            image,
                            t: req.t,
                            scopes: sc,
                            render_ms: t0.elapsed().as_secs_f64() * 1000.0,
                        })
                        .is_err()
                    {
                        break;
                    }
                    ctx.request_repaint();
                }
            })
            .expect("thread de prévia");
        Preview { tx, rx }
    }

    pub fn request(&self, r: PreviewRequest) {
        let _ = self.tx.send(r);
    }
}
