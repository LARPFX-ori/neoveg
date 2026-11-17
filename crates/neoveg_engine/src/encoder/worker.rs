//! Motor de execução do Neoveg Encoder: preparação de origem, codificação paralela
//! de múltiplas saídas por item, miniatura ao vivo, pausa/interrupção, registros
//! persistentes (`neoveg_encoder.log` / `neoveg_encoder_errors.log`) e isolamento
//! com `catch_unwind` (`AGENTS.md` §0.6).

use super::effects::{
    apply_pre_encode_effects, crop_and_scale_frame, effective_output_dims, PreparedEffects,
};
use super::{
    encoder_log_paths, resolve_unique_output_path, EncoderPreferences, JobStatus, OutputJob,
    QueueItem, QueueSource, SourceRangeMode,
};
use crate::engine::Frame;
use crate::media::{self, MediaCache};
use crate::model::{Event, FrameRate, Project, Rate, Ticks};
use crate::render::{
    run_with_options, FramePreviewHook, FrameRenderHook, RenderFormat, RenderOptions,
    RenderProgress, RenderSettings,
};
use crate::util::{guard, lock, redact};
use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const STILL_IMAGE_DEFAULT_SECS: i64 = 5;

/// Miniatura ao vivo e estado compartilhado entre o worker em background e a UI.
#[derive(Clone, Default)]
pub struct LivePreviewFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct OutputLiveProgress {
    pub item_id: u64,
    pub output_id: u64,
    pub status: JobStatus,
    pub progress: f32,
    pub elapsed_secs: f64,
    pub eta_secs: Option<f64>,
    pub message: Option<String>,
    pub warnings: Vec<String>,
    pub final_output_path: Option<PathBuf>,
}

#[derive(Default)]
pub struct EncoderRuntime {
    pub running: AtomicBool,
    pub paused: Arc<AtomicBool>,
    pub stop_current_flag: AtomicBool,
    pub stop_queue_flag: AtomicBool,
    pub preview: Mutex<Option<LivePreviewFrame>>,
    pub updates: Mutex<Vec<OutputLiveProgress>>,
    pub active_progress: Mutex<HashMap<u64, Arc<RenderProgress>>>,
    pub started_at: Mutex<Option<Instant>>,
    pub finished_chime: AtomicBool,
}

impl EncoderRuntime {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    pub fn toggle_pause(&self) {
        let prev = self.paused.load(Ordering::Relaxed);
        self.paused.store(!prev, Ordering::Relaxed);
    }

    /// Cancela apenas o item atual em codificação; a fila segue para os próximos.
    pub fn stop_current(&self) {
        self.stop_current_flag.store(true, Ordering::Relaxed);
        self.paused.store(false, Ordering::Relaxed);
        let map = lock(&self.active_progress);
        for p in map.values() {
            p.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Cancela o item atual e interrompe toda a fila restante.
    pub fn stop_queue(&self) {
        self.stop_queue_flag.store(true, Ordering::Relaxed);
        self.stop_current();
    }

    pub fn push_update(&self, upd: OutputLiveProgress) {
        lock(&self.updates).push(upd);
    }

    pub fn drain_updates(&self) -> Vec<OutputLiveProgress> {
        let mut g = lock(&self.updates);
        std::mem::take(&mut *g)
    }

    pub fn take_preview(&self) -> Option<LivePreviewFrame> {
        lock(&self.preview).take()
    }
}

struct ResolvedSource {
    project: Project,
    media: Arc<MediaCache>,
    src_w: u32,
    src_h: u32,
    src_fps: FrameRate,
    in_point: Option<Ticks>,
    out_point: Option<Ticks>,
    source_name: String,
    warnings: Vec<String>,
}

fn resolve_source(
    src: &QueueSource,
    external_cache: Option<&Arc<MediaCache>>,
    undefined_fps: FrameRate,
) -> Result<ResolvedSource> {
    let mut warnings = Vec::new();
    match src {
        QueueSource::ProjectSnapshot {
            title,
            project,
            in_point,
            out_point,
            ..
        } => {
            let mut p = (**project).clone();
            p.sanitize();
            let cache = Arc::new(MediaCache::default());
            for m in &p.media {
                if let Some(ext_c) = external_cache {
                    if let Some(entry) = ext_c.clone_entry(m.id) {
                        cache.insert_arc(m.id, entry);
                        continue;
                    }
                }
                let mut m_copy = m.clone();
                match media::load_entry(&mut m_copy) {
                    Ok(imp) => cache.insert(m.id, imp.entry),
                    Err(e) => warnings.push(format!("Mídia '{}' indisponível: {e}", m.name)),
                }
            }
            let sw = p.settings.width.max(2);
            let sh = p.settings.height.max(2);
            let sfps = p.settings.fps;
            Ok(ResolvedSource {
                project: p,
                media: cache,
                src_w: sw,
                src_h: sh,
                src_fps: sfps,
                in_point: *in_point,
                out_point: *out_point,
                source_name: title.clone(),
                warnings,
            })
        }
        QueueSource::ProjectFile { path } => {
            let (p, fixes) = Project::load_file(path).map_err(anyhow::Error::msg)?;
            for f in fixes {
                warnings.push(format!("Projeto ajustado: {f}"));
            }
            let cache = Arc::new(MediaCache::default());
            for m in &p.media {
                let mut m_copy = m.clone();
                match media::load_entry(&mut m_copy) {
                    Ok(imp) => cache.insert(m.id, imp.entry),
                    Err(e) => warnings.push(format!("Mídia '{}' indisponível: {e}", m.name)),
                }
            }
            let sw = p.settings.width.max(2);
            let sh = p.settings.height.max(2);
            let sfps = p.settings.fps;
            Ok(ResolvedSource {
                project: p,
                media: cache,
                src_w: sw,
                src_h: sh,
                src_fps: sfps,
                in_point: None,
                out_point: None,
                source_name: path.display().to_string(),
                warnings,
            })
        }
        QueueSource::MediaFile {
            path,
            is_sequence,
            sequence_fps,
            interpret,
        } => {
            let fps_seq = sequence_fps
                .or(interpret.fps_override)
                .unwrap_or(undefined_fps);
            let imp = if *is_sequence {
                media::import_sequence(path, fps_seq)?
            } else {
                media::import_file(path)?
            };
            if let Some(w) = imp.warning {
                warnings.push(w);
            }
            let item = imp.item.clone();
            let mut p = Project::new_default();
            let sw = if item.width > 0 {
                item.width
            } else {
                p.settings.width
            }
            .max(2);
            let sh = if item.height > 0 {
                item.height
            } else {
                p.settings.height
            }
            .max(2);
            let sfps = interpret.fps_override.or(item.fps).unwrap_or(undefined_fps);
            p.settings.width = sw;
            p.settings.height = sh;
            p.settings.fps = sfps;
            if let Some(a) = &imp.entry.audio {
                p.settings.sample_rate = a.sample_rate.clamp(8000, 192_000);
            }
            let dur = item
                .duration
                .filter(|d| *d > Ticks::ZERO)
                .unwrap_or_else(|| Ticks::from_secs_i64(STILL_IMAGE_DEFAULT_SECS));
            p.media.push(item.clone());
            if item.has_video && !p.tracks.is_empty() {
                p.tracks[0].events.push(Event::new(&item, Ticks::ZERO, dur));
            }
            if item.has_audio && p.tracks.len() > 1 {
                p.tracks[1].events.push(Event::new(&item, Ticks::ZERO, dur));
            }
            let cache = Arc::new(MediaCache::default());
            cache.insert(item.id, imp.entry);
            Ok(ResolvedSource {
                project: p,
                media: cache,
                src_w: sw,
                src_h: sh,
                src_fps: sfps,
                in_point: None,
                out_point: None,
                source_name: path.display().to_string(),
                warnings,
            })
        }
    }
}

fn effective_range(
    src: &ResolvedSource,
    job: &OutputJob,
    warnings: &mut Vec<String>,
) -> Result<(Ticks, Ticks)> {
    let proj_dur = src.project.duration().max(src.src_fps.ticks_per_frame());
    let (start, end) = match job.range {
        SourceRangeMode::EntireClip => (Ticks::ZERO, proj_dur),
        SourceRangeMode::InOut => match (src.in_point, src.out_point) {
            (Some(a), Some(b)) if b > a => (a.max(Ticks::ZERO), b.min(proj_dur)),
            _ => {
                warnings.push(
                    "Intervalo Entrada/Saída indefinido na origem; usando duração total.".into(),
                );
                (Ticks::ZERO, proj_dur)
            }
        },
        SourceRangeMode::Custom => {
            let s = job.custom_in.clamp(Ticks::ZERO, proj_dur);
            let e = job.custom_out.clamp(Ticks::ZERO, proj_dur);
            if e > s {
                (s, e)
            } else {
                warnings.push("Intervalo personalizado vazio; usando duração total.".into());
                (Ticks::ZERO, proj_dur)
            }
        }
    };
    if end <= start {
        bail!("intervalo de origem vazio");
    }
    Ok((start, end))
}

fn apply_time_tuner_to_project(
    base: &Project,
    start: Ticks,
    end: Ticks,
    duration_pct: f32,
) -> (Project, Ticks, Ticks) {
    let pct_milli = (duration_pct.clamp(25.0, 400.0) * 10.0).round() as i128;
    if (pct_milli - 1000).abs() <= 1 {
        return (base.clone(), start, end);
    }
    let scale_tick = |t: Ticks| -> Ticks { Ticks(((t.0 as i128) * pct_milli / 1000) as i64) };
    let mut p = base.clone();
    let speed = Rate::from_ratio(1000, pct_milli.max(1) as u64).unwrap_or(Rate::NORMAL);
    for tr in &mut p.tracks {
        for ev in &mut tr.events {
            ev.start = scale_tick(ev.start);
            ev.length = scale_tick(ev.length).max(Ticks(1));
            ev.rate = Rate::from_ratio(
                ev.rate.num.saturating_mul(speed.num).max(1),
                ev.rate.den.saturating_mul(speed.den).max(1),
            )
            .unwrap_or(ev.rate);
        }
    }
    (p, scale_tick(start), scale_tick(end))
}

fn resolve_audio_only_fallback(fmt: RenderFormat) -> RenderFormat {
    if fmt.is_audio_only() {
        return fmt;
    }
    match fmt {
        RenderFormat::Mp4H264
        | RenderFormat::Mp4Hevc
        | RenderFormat::MovH264
        | RenderFormat::ThreeGp => {
            #[cfg(windows)]
            {
                RenderFormat::AacM4a
            }
            #[cfg(not(windows))]
            {
                RenderFormat::Wav
            }
        }
        RenderFormat::Wmv => {
            #[cfg(windows)]
            {
                RenderFormat::Wma
            }
            #[cfg(not(windows))]
            {
                RenderFormat::Wav
            }
        }
        _ => RenderFormat::Wav,
    }
}

/// Inicia a fila assíncrona do Neoveg Encoder em thread isolada protegida (§0.6).
pub fn spawn_queue(
    items: Vec<QueueItem>,
    prefs: EncoderPreferences,
    runtime: Arc<EncoderRuntime>,
    external_cache: Option<Arc<MediaCache>>,
) {
    if runtime.running.swap(true, Ordering::SeqCst) {
        return;
    }
    runtime.paused.store(false, Ordering::Relaxed);
    runtime.stop_current_flag.store(false, Ordering::Relaxed);
    runtime.stop_queue_flag.store(false, Ordering::Relaxed);
    *lock(&runtime.started_at) = Some(Instant::now());

    let rt = runtime.clone();
    let res = crate::util::spawn("neoveg-encoder-worker", move || {
        let rt_inner = rt.clone();
        let prefs_inner = prefs.clone();
        let _ = guard("neoveg-encoder-queue", move || {
            let mut reserved_paths = HashSet::new();
            for item in items {
                if rt_inner.stop_queue_flag.load(Ordering::Relaxed) {
                    break;
                }
                rt_inner.stop_current_flag.store(false, Ordering::Relaxed);
                encode_item_internal(
                    &item,
                    &prefs_inner,
                    &rt_inner,
                    external_cache.as_ref(),
                    &mut reserved_paths,
                );
            }
        });
        *lock(&rt.preview) = None;
        lock(&rt.active_progress).clear();
        if prefs.play_chime_on_finish {
            rt.finished_chime.store(true, Ordering::Relaxed);
        }
        rt.running.store(false, Ordering::SeqCst);
    });
    if let Err(e) = res {
        append_encoder_log(true, &format!("falha ao iniciar thread da fila: {e}"));
        runtime.running.store(false, Ordering::SeqCst);
    }
}

/// Executa sincronamente um item da fila (inclusive todas as saídas paralelas),
/// devolvendo os resultados finais por saída. Útil para CLI e testes determinísticos.
pub fn encode_item_sync(
    item: &QueueItem,
    prefs: &EncoderPreferences,
    external_cache: Option<&Arc<MediaCache>>,
) -> Vec<OutputLiveProgress> {
    let rt = EncoderRuntime::new();
    let mut reserved = HashSet::new();
    encode_item_internal(item, prefs, &rt, external_cache, &mut reserved);
    let mut final_by_id: HashMap<u64, OutputLiveProgress> = HashMap::new();
    for u in rt.drain_updates() {
        final_by_id.insert(u.output_id, u);
    }
    item.outputs
        .iter()
        .filter_map(|o| final_by_id.remove(&o.id))
        .collect()
}

fn encode_item_internal(
    item: &QueueItem,
    prefs: &EncoderPreferences,
    rt: &Arc<EncoderRuntime>,
    external_cache: Option<&Arc<MediaCache>>,
    reserved_paths: &mut HashSet<PathBuf>,
) {
    let ready_outputs: Vec<OutputJob> = item
        .outputs
        .iter()
        .filter(|o| o.status == JobStatus::Ready)
        .cloned()
        .collect();
    if ready_outputs.is_empty() {
        return;
    }

    let resolved = match resolve_source(&item.source, external_cache, prefs.undefined_media_fps) {
        Ok(r) => Arc::new(r),
        Err(e) => {
            let msg = format!("{e:#}");
            for o in &ready_outputs {
                append_encoder_log(true, &format!("[{}] {}", item.source.display_name(), msg));
                rt.push_update(OutputLiveProgress {
                    item_id: item.id,
                    output_id: o.id,
                    status: JobStatus::Failed,
                    progress: 0.0,
                    elapsed_secs: 0.0,
                    eta_secs: None,
                    message: Some(msg.clone()),
                    warnings: vec![],
                    final_output_path: None,
                });
            }
            return;
        }
    };

    // Pré-aloca caminhos únicos antes do disparo paralelo para evitar colisões entre saídas.
    let mut planned: Vec<(OutputJob, PathBuf)> = Vec::with_capacity(ready_outputs.len());
    for o in ready_outputs {
        let mut target_fmt = o.format;
        if !o.include_video && o.include_audio {
            target_fmt = resolve_audio_only_fallback(o.format);
        }
        let base_path = if target_fmt != o.format {
            o.output_path.with_extension(target_fmt.ext())
        } else {
            o.output_path.clone()
        };
        let unique =
            resolve_unique_output_path(&base_path, prefs.increment_existing_output, reserved_paths);
        planned.push((o, unique));
    }

    // Executa múltiplas saídas da mesma origem em paralelo (Codificação paralela).
    std::thread::scope(|scope| {
        for (idx, (job, out_path)) in planned.into_iter().enumerate() {
            let res_ref = resolved.clone();
            let rt_ref = rt.clone();
            let emit_preview = prefs.preview_during_encoding && idx == 0;
            scope.spawn(move || {
                let res = encode_single_output(
                    item.id,
                    &job,
                    out_path.clone(),
                    &res_ref,
                    &rt_ref,
                    emit_preview,
                );
                let upd = match res {
                    Ok(ok) => {
                        let line = format!(
                            "OK [{}] -> {} ({:.2}s, {} avisos)",
                            res_ref.source_name,
                            out_path.display(),
                            ok.elapsed,
                            ok.warnings.len()
                        );
                        append_encoder_log(false, &line);
                        OutputLiveProgress {
                            item_id: item.id,
                            output_id: job.id,
                            status: ok.status,
                            progress: 1.0,
                            elapsed_secs: ok.elapsed,
                            eta_secs: Some(0.0),
                            message: ok.warnings.first().cloned(),
                            warnings: ok.warnings,
                            final_output_path: Some(out_path),
                        }
                    }
                    Err(fail) => {
                        let line = format!(
                            "{} [{}] -> {}: {}",
                            fail.status.name(),
                            res_ref.source_name,
                            out_path.display(),
                            fail.message
                        );
                        append_encoder_log(fail.status == JobStatus::Failed, &line);
                        OutputLiveProgress {
                            item_id: item.id,
                            output_id: job.id,
                            status: fail.status,
                            progress: 0.0,
                            elapsed_secs: fail.elapsed,
                            eta_secs: None,
                            message: Some(fail.message),
                            warnings: fail.warnings,
                            final_output_path: None,
                        }
                    }
                };
                lock(&rt_ref.active_progress).remove(&job.id);
                rt_ref.push_update(upd);
            });
        }
    });
}

struct EncodeSuccess {
    status: JobStatus,
    warnings: Vec<String>,
    elapsed: f64,
}

struct EncodeFailure {
    status: JobStatus,
    message: String,
    warnings: Vec<String>,
    elapsed: f64,
}

fn encode_single_output(
    item_id: u64,
    job: &OutputJob,
    out_path: PathBuf,
    src: &ResolvedSource,
    rt: &Arc<EncoderRuntime>,
    emit_preview: bool,
) -> std::result::Result<EncodeSuccess, EncodeFailure> {
    let t0 = Instant::now();
    let mut warnings = src.warnings.clone();

    if !job.include_video && !job.include_audio {
        return Err(EncodeFailure {
            status: JobStatus::Failed,
            message: "Ative Exportar Vídeo ou Exportar Áudio nas configurações.".into(),
            warnings,
            elapsed: 0.0,
        });
    }

    let mut eff_format = job.format;
    if !job.include_video {
        eff_format = resolve_audio_only_fallback(job.format);
        if eff_format != job.format {
            warnings.push(format!(
                "Exportar Vídeo desativado: saída convertida para {}.",
                eff_format.name()
            ));
        }
    }

    let (raw_start, raw_end) = match effective_range(src, job, &mut warnings) {
        Ok(r) => r,
        Err(e) => {
            return Err(EncodeFailure {
                status: JobStatus::Failed,
                message: format!("{e:#}"),
                warnings,
                elapsed: 0.0,
            })
        }
    };
    let (eff_proj, start, end) = if job.effects.time_tuner.enabled {
        apply_time_tuner_to_project(
            &src.project,
            raw_start,
            raw_end,
            job.effects.time_tuner.duration_pct,
        )
    } else {
        (src.project.clone(), raw_start, raw_end)
    };

    let (out_w, out_h) = effective_output_dims(
        src.src_w,
        src.src_h,
        job.crop,
        job.scaling,
        job.width,
        job.height,
        job.match_source_dims,
    );
    let fps = job.fps_override.unwrap_or(src.src_fps);
    let prep = PreparedEffects::prepare(&job.effects, &src.source_name, &out_path, &mut warnings);
    let fx_cfg = job.effects.clone();
    let crop = job.crop;
    let scaling = job.scaling;
    let sr = eff_proj.settings.sample_rate;

    let frame_hook: FrameRenderHook = Arc::new(move |fi, out_ticks, frame| {
        let mut out = crop_and_scale_frame(&frame, crop, scaling, out_w as usize, out_h as usize);
        apply_pre_encode_effects(&mut out, &fx_cfg, &prep, fi, out_ticks, fps, sr);
        out
    });

    let rt_pv = rt.clone();
    let preview_hook: Option<FramePreviewHook> = if emit_preview {
        Some(Arc::new(move |_fi, frame| {
            let thumb = downsample_preview(frame, 200, 112);
            *lock(&rt_pv.preview) = Some(thumb);
        }))
    } else {
        None
    };

    let prog = Arc::new(RenderProgress::default());
    lock(&rt.active_progress).insert(job.id, prog.clone());
    rt.push_update(OutputLiveProgress {
        item_id,
        output_id: job.id,
        status: JobStatus::Encoding,
        progress: 0.0,
        elapsed_secs: 0.0,
        eta_secs: None,
        message: None,
        warnings: vec![],
        final_output_path: None,
    });

    let settings = RenderSettings {
        format: eff_format,
        path: out_path.clone(),
        width: out_w,
        height: out_h,
        start,
        end,
        jpeg_quality: job.jpeg_quality,
        include_audio: job.include_audio,
        wav_bits: job.wav_bits,
        video_bitrate_kbps: job.video_bitrate_kbps,
        audio_bitrate_kbps: job.audio_bitrate_kbps,
    };
    let opts = RenderOptions {
        fps_override: Some(fps),
        source_size: Some((src.src_w, src.src_h)),
        frame_hook: Some(frame_hook),
        preview_hook,
        time_map: None,
        pause: Some(rt.paused.clone()),
        include_video: job.include_video,
    };

    // Garante que pausa cooperativa usa o mesmo Arc<AtomicBool> sem custo extra:
    let outcome = guard("encode_output", || {
        run_with_options(&eff_proj, &src.media, &settings, &prog, &opts)
    });
    let elapsed = t0.elapsed().as_secs_f64();

    match outcome {
        Ok(Ok(())) => {
            let status = if warnings.is_empty() {
                JobStatus::Done
            } else {
                JobStatus::DoneWithWarnings
            };
            Ok(EncodeSuccess {
                status,
                warnings,
                elapsed,
            })
        }
        Ok(Err(e)) => {
            let msg = format!("{e:#}");
            let st = if prog.cancel.load(Ordering::Relaxed)
                || rt.stop_current_flag.load(Ordering::Relaxed)
                || rt.stop_queue_flag.load(Ordering::Relaxed)
            {
                JobStatus::Stopped
            } else {
                JobStatus::Failed
            };
            Err(EncodeFailure {
                status: st,
                message: msg,
                warnings,
                elapsed,
            })
        }
        Err(panic_msg) => Err(EncodeFailure {
            status: JobStatus::Failed,
            message: panic_msg,
            warnings,
            elapsed,
        }),
    }
}

fn downsample_preview(src: &Frame, max_w: usize, max_h: usize) -> LivePreviewFrame {
    if src.w == 0 || src.h == 0 {
        return LivePreviewFrame::default();
    }
    let scale = (max_w as f32 / src.w as f32)
        .min(max_h as f32 / src.h as f32)
        .min(1.0);
    let tw = ((src.w as f32 * scale).round() as usize).max(2);
    let th = ((src.h as f32 * scale).round() as usize).max(2);
    let mut rgba = Vec::with_capacity(tw * th * 4);
    for y in 0..th {
        let sy = (y * src.h) / th;
        for x in 0..tw {
            let sx = (x * src.w) / tw;
            let p = src
                .px
                .get(sy * src.w + sx)
                .copied()
                .unwrap_or([0.0, 0.0, 0.0, 1.0]);
            let a = p[3].clamp(0.0, 1.0);
            let inv = if a > 1e-6 { 1.0 / a } else { 0.0 };
            rgba.push(((p[0] * inv).clamp(0.0, 1.0) * 255.0) as u8);
            rgba.push(((p[1] * inv).clamp(0.0, 1.0) * 255.0) as u8);
            rgba.push(((p[2] * inv).clamp(0.0, 1.0) * 255.0) as u8);
            rgba.push(255);
        }
    }
    LivePreviewFrame {
        width: tw as u32,
        height: th as u32,
        rgba,
    }
}

/// Adiciona entrada segura (sem segredos/CII) em `logs/neoveg_encoder.log` ou `neoveg_encoder_errors.log`.
pub fn append_encoder_log(is_error: bool, message: &str) {
    let (ok_path, err_path) = encoder_log_paths();
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let clean = redact(message);
    let line = format!("[{ts}] {clean}\n");
    for target in if is_error {
        vec![ok_path, err_path]
    } else {
        vec![ok_path]
    } {
        if let Some(parent) = target.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&target) {
            let _ = f.write_all(line.as_bytes());
        }
    }
}

/// Constrói `RenderOptions` pronto para o diálogo de pré-visualização da UI
/// aplicando Corte + Escala + Grade + LUT `.cube` + Sobreposições em um quadro.
pub fn preview_transformed_frame(
    src: &Frame,
    src_ticks: Ticks,
    fps: FrameRate,
    sample_rate: u32,
    job: &OutputJob,
    source_name: &str,
) -> Frame {
    let (ow, oh) = effective_output_dims(
        src.w as u32,
        src.h as u32,
        job.crop,
        job.scaling,
        job.width,
        job.height,
        job.match_source_dims,
    );
    let mut warns = Vec::new();
    let prep = PreparedEffects::prepare(&job.effects, source_name, &job.output_path, &mut warns);
    let mut out = crop_and_scale_frame(src, job.crop, job.scaling, ow as usize, oh as usize);
    let fi = fps.ticks_to_frame_round(src_ticks).max(0) as u64;
    apply_pre_encode_effects(
        &mut out,
        &job.effects,
        &prep,
        fi,
        src_ticks,
        fps,
        sample_rate,
    );
    out
}

#[cfg(test)]
mod tests {
    use super::super::effects::*;
    use super::super::presets::*;
    use super::super::watch::*;
    use super::super::*;
    use super::*;
    use crate::model::GenSpec;

    #[test]
    fn cube_lut_3d_trilinear_and_hostiles() {
        let lut_txt = "\
TITLE \"InvertTest\"
LUT_3D_SIZE 2
0.0 0.0 0.0
1.0 0.0 0.0
0.0 1.0 0.0
1.0 1.0 0.0
0.0 0.0 1.0
1.0 0.0 1.0
0.0 1.0 1.0
1.0 1.0 1.0
";
        let lut = CubeLut::parse(lut_txt).unwrap();
        let mid = lut.sample([0.25, 0.5, 0.75]);
        assert!((mid[0] - 0.25).abs() < 1e-5);
        assert!((mid[1] - 0.5).abs() < 1e-5);
        assert!((mid[2] - 0.75).abs() < 1e-5);

        assert!(CubeLut::parse("LUT_3D_SIZE 999999").is_err());
        assert!(CubeLut::parse("garbage").is_err());
        assert!(CubeLut::parse("LUT_3D_SIZE 2\n0 0 nan").is_err());
    }

    #[test]
    fn preset_catalog_groups_aliases_and_neovegpreset_roundtrip() {
        let dir = std::env::temp_dir().join(format!("nv_enc_pres_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let pfile = dir.join("meu.neovegpreset");
        let store = dir.join("presets.json");

        let mut cat = PresetCatalog::default();
        let mut custom = system_presets()[0].clone();
        custom.name = "Meu H.264 Social".into();
        custom.video_bitrate_kbps = 9500;
        let cid = cat.save_custom(custom.clone());
        let gid = cat.create_group("Entrega Dual", vec![cid.clone()]);
        let aid = cat
            .create_alias("Apelido Social", &cid, Some(&gid))
            .unwrap();
        assert_eq!(cat.resolve(&aid).unwrap().name, "Meu H.264 Social");

        export_preset_file(cat.resolve(&cid).unwrap(), &pfile).unwrap();
        let imported = import_preset_file(&pfile).unwrap();
        assert_eq!(imported.video_bitrate_kbps, 9500);

        cat.save(&store).unwrap();
        let loaded = PresetCatalog::load(&store, &dir);
        assert!(loaded.resolve(&aid).is_some());
        assert_eq!(loaded.resolve_group_or_preset(&gid).len(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn crop_scaling_and_overlays_render_without_panics() {
        let mut f = Frame::filled(160, 120, [0.8, 0.2, 0.1, 1.0]);
        let crop = CropRect {
            enabled: true,
            left_px: 20,
            top_px: 10,
            right_px: 20,
            bottom_px: 10,
            aspect: CropAspect::Ratio16x9,
        };
        for mode in SourceScalingMode::ALL {
            let scaled = crop_and_scale_frame(&f, crop, mode, 128, 72);
            if mode == SourceScalingMode::MatchSource {
                assert!(scaled.w >= 2 && scaled.h >= 2);
            } else {
                assert_eq!((scaled.w, scaled.h), (128, 72));
            }
        }
        let mut fx = PreEncodeEffects {
            grade: GradePreset::CinematicWarm,
            ..Default::default()
        };
        fx.name_overlay.enabled = true;
        fx.name_overlay.format = NameFormat::SourceStem;
        fx.timecode_overlay.enabled = true;
        let prep = PreparedEffects::prepare(&fx, "clipe.avi", Path::new("out.mp4"), &mut vec![]);
        apply_pre_encode_effects(
            &mut f,
            &fx,
            &prep,
            10,
            Ticks::from_secs_i64(2),
            FrameRate::FPS_25,
            48_000,
        );
        assert_eq!(f.w, 160);
    }

    #[test]
    fn parallel_multi_output_and_watch_folder_encode_end_to_end() {
        let dir = std::env::temp_dir().join(format!("nv_enc_e2e_{}", std::process::id()));
        let watch_in = dir.join("watch");
        let watch_out = dir.join("out");
        let _ = std::fs::create_dir_all(&watch_in);
        let _ = std::fs::create_dir_all(&watch_out);

        // Cria um clipe sintético via projeto PNG/AVI para testar Watch Folder + fila paralela
        let sample_png = watch_in.join("clipe.png");
        let img = image::RgbaImage::from_pixel(32, 24, image::Rgba([40, 180, 220, 255]));
        img.save(&sample_png).unwrap();

        let cat = PresetCatalog::default();
        let mut state = EncoderState::default();
        let mut wf = WatchFolder::new(watch_in.clone(), "sys.bc_avi_mjpeg");
        wf.output_dir = Some(watch_out.clone());
        wf.preset_ids.push("sys.mob_gif".into());
        state.watch_folders.push(wf);

        let (added, auto) = scan_watch_folders(&mut state, &cat);
        assert_eq!(added, 1);
        assert!(auto);
        assert_eq!(state.items[0].outputs.len(), 2);

        // Também adiciona um snapshot de projeto com TimeTuner + overlay
        let mut p = Project::new_default();
        p.settings.width = 48;
        p.settings.height = 36;
        p.settings.fps = FrameRate::FPS_25;
        let g = media::import_generated(GenSpec::ColorBars);
        p.media.push(g.item.clone());
        let dur = p.settings.fps.frame_to_ticks(4);
        p.tracks[0]
            .events
            .push(Event::new(&g.item, Ticks::ZERO, dur));
        let pid = state.add_project_snapshot("Timeline".into(), None, p, None, &cat);
        // Troca predefinição do snapshot para AVI RGB24 sem compressão + Sequência PNG em paralelo
        let _ = state.apply_preset_or_group(pid, &cat, "sys.bc_avi_rgb24", false);
        let _ = state.add_output(pid, cat.resolve("sys.seq_png").unwrap());
        if let Some(o) = state.items.last_mut().and_then(|it| it.outputs.first_mut()) {
            o.output_path = dir.join("timeline_snap.avi");
            o.effects.timecode_overlay.enabled = true;
            o.effects.time_tuner = TimeTuner {
                enabled: true,
                duration_pct: 50.0,
            };
        }
        if let Some(o2) = state.items.last_mut().and_then(|it| it.outputs.get_mut(1)) {
            o2.output_path = dir.join("timeline_seq.png");
        }

        // Executa sincronamente o item do Watch Folder (saídas paralelas AVI + GIF)
        let res_watch = encode_item_sync(&state.items[0], &state.prefs, None);
        assert_eq!(res_watch.len(), 2);
        for r in &res_watch {
            assert!(
                matches!(r.status, JobStatus::Done | JobStatus::DoneWithWarnings),
                "{:?}: {:?}",
                r.status,
                r.message
            );
            assert!(r.final_output_path.as_ref().unwrap().exists());
        }

        // Segunda varredura na mesma pasta não deve duplicar item já processado
        let (added_again, _) = scan_watch_folders(&mut state, &cat);
        assert_eq!(added_again, 0);

        // Executa sincronamente o snapshot da timeline (AVI RGB24 + sequência PNG paralelos)
        let res_proj = encode_item_sync(&state.items[1], &state.prefs, None);
        assert_eq!(res_proj.len(), 2);
        assert!(dir.join("timeline_snap.avi").exists());
        assert!(dir.join("timeline_seq_000000.png").exists());

        // Testar incremento anti-sobrescrita
        let mut resv = HashSet::new();
        let next_p = resolve_unique_output_path(&dir.join("timeline_snap.avi"), true, &mut resv);
        assert!(next_p
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("timeline_snap_1"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
