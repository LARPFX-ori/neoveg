//! Fluxos de trabalho em Sala Limpa entre o Neoveg, os aplicativos ArtCraft e o Blender.
//!
//! Inclui gravação WAV/BWF (chunk `bext` segundo EBU Tech 3285), geração limpa
//! de placas SVG para o VectorCraft, disparo via RPC loopback ou processo
//! externo e observador leve de arquivos editados fora do processo (`Add As Takes`).

use super::craft_rpc::CraftClient;
use super::discovery::AppDiscoveryStatus;
use super::ExternalApp;
use crate::engine::audio::Mixer;
use crate::media::{audio::AudioData, MediaCache};
use crate::model::{Event, GenSpec, Id, MediaItem, MediaSource, Project, Ticks};
use crate::render::{self, WavBits};
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

/// Limite máximo de tempo em chamadas curtas de CLI auxiliar (`anicraft-cli`, etc.).
const MAX_CLI_TIMEOUT: Duration = Duration::from_secs(120);

/// Forma como o arquivo foi entregue ao aplicativo externo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeliveryMethod {
    LiveControlRpc { port: u16 },
    SpawnedDesktop { exe: PathBuf },
}

/// Entrada monitorada de edição externa (round-trip via Takes do VEGAS Pro).
#[derive(Clone, Debug)]
pub struct WatchedExternalEdit {
    pub app: ExternalApp,
    pub media_id: Id,
    pub event_id: Option<Id>,
    pub path: PathBuf,
    pub last_modified: Option<SystemTime>,
    pub last_size: u64,
    pub paused: bool,
    pub reload_count: u32,
}

impl WatchedExternalEdit {
    pub fn new(app: ExternalApp, media_id: Id, event_id: Option<Id>, path: PathBuf) -> Self {
        let meta = std::fs::metadata(&path).ok();
        let last_modified = meta.as_ref().and_then(|m| m.modified().ok());
        let last_size = meta.map(|m| m.len()).unwrap_or(0);
        Self {
            app,
            media_id,
            event_id,
            path,
            last_modified,
            last_size,
            paused: false,
            reload_count: 0,
        }
    }

    /// Verifica se o arquivo foi salvo novamente no disco desde a última leitura.
    pub fn poll_modified(&mut self) -> bool {
        if self.paused {
            return false;
        }
        let Ok(meta) = std::fs::metadata(&self.path) else {
            return false;
        };
        let mtime = meta.modified().ok();
        let size = meta.len();
        if size == 0 {
            return false;
        }
        let changed = match (self.last_modified, mtime) {
            (Some(prev), Some(curr)) => curr > prev || size != self.last_size,
            (None, Some(_)) => true,
            _ => size != self.last_size,
        };
        if changed {
            self.last_modified = mtime;
            self.last_size = size;
            self.reload_count = self.reload_count.saturating_add(1);
        }
        changed
    }
}

/// Gera caminho único para cópia editável ao lado do arquivo original (ou em pasta de trabalho).
pub fn sibling_copy_path(base_dir: &Path, stem_hint: &str, app: ExternalApp, ext: &str) -> PathBuf {
    let clean_stem: String = stem_hint
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .take(64)
        .collect::<String>()
        .trim()
        .to_string();
    let stem = if clean_stem.is_empty() {
        "midia".to_string()
    } else {
        clean_stem
    };
    let clean_ext = ext.trim_start_matches('.');
    for idx in 1u32..=999 {
        let name = format!("{stem} - {} Take {idx}.{clean_ext}", app.name());
        let candidate = base_dir.join(name);
        if !candidate.exists() {
            return candidate;
        }
    }
    base_dir.join(format!("{stem} - {}.{clean_ext}", app.name()))
}

/// Pasta de intercâmbio do projeto (subpasta ao lado do `.neoveg` ou temporária).
pub fn interchange_work_dir(project_path: Option<&Path>, subdir: &str) -> Result<PathBuf> {
    let base = project_path
        .and_then(|p| p.parent())
        .map(|d| d.join("Neoveg_Integracoes"))
        .unwrap_or_else(|| neoveg_common::util::base_dir().join("integracoes"));
    let dir = base.join(subdir);
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("não foi possível criar pasta de trabalho {}", dir.display()))?;
    Ok(dir)
}

/// Grava um arquivo WAV em padrão Broadcast Wave Format (EBU Tech 3285 `bext`),
/// gravando o deslocamento exato de amostras da timeline no campo `TimeReference`.
pub fn write_bwf_wav(
    path: &Path,
    sr: u32,
    channels: u16,
    bits: WavBits,
    samples: &[f32],
    time_reference_samples: u64,
    description: &str,
) -> Result<()> {
    if channels == 0 || channels > 6 {
        bail!("quantidade de canais inválida para WAV/BWF: {channels}");
    }
    if sr == 0 {
        bail!("taxa de amostragem inválida (0 Hz)");
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let bps: u16 = match bits {
        WavBits::Pcm16 => 16,
        WavBits::Pcm24 => 24,
        WavBits::Float32 => 32,
    };
    let fmt_tag: u16 = match bits {
        WavBits::Pcm16 | WavBits::Pcm24 => 1,
        WavBits::Float32 => 3,
    };
    let block_align = channels.saturating_mul(bps / 8);
    let byte_rate = sr.saturating_mul(u32::from(block_align));
    let sample_bytes = u64::from(bps / 8);
    let data_bytes_u64 = (samples.len() as u64).saturating_mul(sample_bytes);

    // Payload fixo do chunk BWF `bext` v1 = 602 bytes:
    // Description[256] + Originator[32] + OriginatorRef[32] + Date[10] + Time[8]
    // + TimeReferenceLow(4) + TimeReferenceHigh(4) + Version(2) + UMID(64)
    // + Loudness(10) + Reserved(180) = 602 bytes.
    let mut bext = [0u8; 602];
    copy_ascii_padded(&mut bext[0..256], description);
    copy_ascii_padded(&mut bext[256..288], "Neoveg");
    copy_ascii_padded(
        &mut bext[288..320],
        &format!("NVG-{:08X}", time_reference_samples as u32),
    );
    let (date_str, time_str) = neoveg_common::util::local_date_time_strings();
    copy_ascii_padded(&mut bext[320..330], &date_str);
    copy_ascii_padded(&mut bext[330..338], &time_str);
    let tref_low = (time_reference_samples & 0xFFFF_FFFF) as u32;
    let tref_high = (time_reference_samples >> 32) as u32;
    bext[338..342].copy_from_slice(&tref_low.to_le_bytes());
    bext[342..346].copy_from_slice(&tref_high.to_le_bytes());
    bext[346..348].copy_from_slice(&1u16.to_le_bytes()); // BWF Version 1

    // RIFF total = 4 ("WAVE") + (8 + 602 bext) + (8 + 16 fmt) + (8 + data_bytes)
    let total_riff = 4u64 + (8 + 602) + (8 + 16) + (8 + data_bytes_u64);
    let riff_size = u32::try_from(total_riff)
        .map_err(|_| anyhow!("arquivo WAV/BWF excede o limite RIFF de 4 GiB"))?;
    let data_size = u32::try_from(data_bytes_u64)
        .map_err(|_| anyhow!("bloco data excede o limite RIFF de 4 GiB"))?;

    let f = File::create(path).with_context(|| format!("criando BWF {}", path.display()))?;
    let mut w = BufWriter::new(f);
    w.write_all(b"RIFF")?;
    w.write_all(&riff_size.to_le_bytes())?;
    w.write_all(b"WAVE")?;

    w.write_all(b"bext")?;
    w.write_all(&(bext.len() as u32).to_le_bytes())?;
    w.write_all(&bext)?;

    w.write_all(b"fmt ")?;
    w.write_all(&16u32.to_le_bytes())?;
    w.write_all(&fmt_tag.to_le_bytes())?;
    w.write_all(&channels.to_le_bytes())?;
    w.write_all(&sr.to_le_bytes())?;
    w.write_all(&byte_rate.to_le_bytes())?;
    w.write_all(&block_align.to_le_bytes())?;
    w.write_all(&bps.to_le_bytes())?;

    w.write_all(b"data")?;
    w.write_all(&data_size.to_le_bytes())?;
    for &s in samples {
        let s = if s.is_finite() { s } else { 0.0 };
        match bits {
            WavBits::Pcm16 => {
                let v = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
                w.write_all(&v.to_le_bytes())?;
            }
            WavBits::Pcm24 => {
                let v = (s.clamp(-1.0, 1.0) * 8_388_607.0).round() as i32;
                w.write_all(&v.to_le_bytes()[..3])?;
            }
            WavBits::Float32 => {
                w.write_all(&s.to_le_bytes())?;
            }
        }
    }
    w.flush()?;
    let mut inner = w.into_inner().map_err(|e| anyhow!("{e}"))?;
    inner.seek(SeekFrom::Start(0))?;
    Ok(())
}

fn copy_ascii_padded(dst: &mut [u8], text: &str) {
    let bytes = text.as_bytes();
    let n = bytes.len().min(dst.len());
    dst[..n].copy_from_slice(&bytes[..n]);
}

/// Exporta um evento de áudio selecionado (ou trecho da timeline) em WAV BWF para o SoundCraft.
pub fn export_event_audio_bwf(
    project: &Project,
    media: &MediaCache,
    event: &Event,
    out_path: &Path,
) -> Result<()> {
    let item = project
        .media(event.media_id)
        .ok_or_else(|| anyhow!("mídia do evento não encontrada"))?;
    let sr = project.settings.sample_rate.max(8000);
    let time_ref = event.start.to_samples_round(sr).max(0) as u64;

    // Se já temos áudio carregado no MediaCache (`AudioData` guarda pares L/R em f32),
    // extraímos exatamente o recorte do evento!
    if let Some(entry_audio) = media_audio_for_item(media, item) {
        let out_ch: u16 = if entry_audio.mono_source { 1 } else { 2 };
        let start_sample = event
            .offset
            .to_samples_round(entry_audio.sample_rate)
            .max(0);
        let len_samples = event
            .length
            .to_samples_round(entry_audio.sample_rate)
            .max(1) as usize;
        let max_samples = 48_000 * 600; // cap 10 min por evento para exportação rápida
        let count = len_samples.min(max_samples);

        let mut pcm = Vec::with_capacity(count * out_ch as usize);
        for i in 0..count {
            let s_idx = (start_sample as usize).saturating_add(i);
            let frame_off = s_idx.saturating_mul(2);
            if frame_off + 1 < entry_audio.data.len() {
                if out_ch == 1 {
                    pcm.push(entry_audio.data[frame_off]);
                } else {
                    pcm.push(entry_audio.data[frame_off]);
                    pcm.push(entry_audio.data[frame_off + 1]);
                }
            } else if out_ch == 1 {
                pcm.push(0.0);
            } else {
                pcm.push(0.0);
                pcm.push(0.0);
            }
        }
        return write_bwf_wav(
            out_path,
            entry_audio.sample_rate,
            out_ch,
            WavBits::Float32,
            &pcm,
            event.start.to_samples_round(entry_audio.sample_rate).max(0) as u64,
            &format!("Neoveg Event: {}", event.name),
        );
    }

    // Fallback: mixdown offline do intervalo [event.start, event.end()] pelo Mixer.
    export_timeline_range_bwf(project, media, event.start, event.end(), out_path, time_ref)
}

/// Mixa um intervalo da timeline inteira em WAV BWF (estéreo) com `bext` TimeReference.
pub fn export_timeline_range_bwf(
    project: &Project,
    media: &MediaCache,
    start: Ticks,
    end: Ticks,
    out_path: &Path,
    time_reference_samples: u64,
) -> Result<()> {
    if end <= start {
        bail!("intervalo de áudio vazio");
    }
    let sr = project.settings.sample_rate.max(8000);
    let s0 = start.to_samples_round(sr);
    let s1 = end.to_samples_round(sr).max(s0 + 1);
    let total_frames = (s1 - s0).min(48_000 * 600) as usize;
    let ch = 2usize;

    let mut mixer = Mixer::new_offline(sr);
    let am = media.audio_map();
    let mut pcm = Vec::with_capacity(total_frames * ch);
    let mut block = vec![0f32; 4096 * ch];
    let mut pos = s0;
    let mut rem = total_frames;
    while rem > 0 {
        let take = rem.min(4096);
        mixer.process(project, &am, pos, &mut block[..take * ch]);
        pcm.extend_from_slice(&block[..take * ch]);
        pos += take as i64;
        rem -= take;
    }

    write_bwf_wav(
        out_path,
        sr,
        2,
        WavBits::Float32,
        &pcm,
        time_reference_samples,
        &format!(
            "Neoveg Mix ({}x{} @ {})",
            project.settings.width,
            project.settings.height,
            project.settings.fps.label()
        ),
    )
}

fn media_audio_for_item(media: &MediaCache, item: &MediaItem) -> Option<std::sync::Arc<AudioData>> {
    let am = media.audio_map();
    am.get(&item.id).cloned()
}

fn rgba_f32_to_hex(c: [f32; 4]) -> (String, f32) {
    let ch = |v: f32| -> u8 {
        if !v.is_finite() {
            0
        } else {
            (v.clamp(0.0, 1.0) * 255.0).round() as u8
        }
    };
    let a = if c[3].is_finite() {
        c[3].clamp(0.0, 1.0)
    } else {
        1.0
    };
    (
        format!("#{:02x}{:02x}{:02x}", ch(c[0]), ch(c[1]), ch(c[2])),
        a,
    )
}

/// Gera arquivo SVG 1.1 editável em Sala Limpa para abrir no VectorCraft.
pub fn export_clean_svg_for_vectorcraft(
    project: &Project,
    event: Option<&Event>,
    media_item: Option<&MediaItem>,
    out_svg: &Path,
) -> Result<()> {
    let w = project.settings.width.max(16);
    let h = project.settings.height.max(16);
    let mut body = String::new();

    if let Some(item) = media_item {
        match &item.source {
            MediaSource::Generated(GenSpec::SolidColor { color }) => {
                let (hex, opacity) = rgba_f32_to_hex(*color);
                body.push_str(&format!(
                    "  <rect id=\"solid-bg\" width=\"{w}\" height=\"{h}\" fill=\"{hex}\" fill-opacity=\"{opacity:.3}\" />\n"
                ));
            }
            MediaSource::Generated(GenSpec::Text(spec)) => {
                let (bg_hex, bg_op) = rgba_f32_to_hex(spec.background);
                if bg_op > 0.01 {
                    body.push_str(&format!(
                        "  <rect id=\"title-bg\" width=\"{w}\" height=\"{h}\" fill=\"{bg_hex}\" fill-opacity=\"{bg_op:.3}\" />\n"
                    ));
                }
                let (fg_hex, fg_op) = rgba_f32_to_hex(spec.color);
                let escaped = xml_escape_text(&spec.text);
                let cx = w / 2;
                let cy = h / 2;
                let size_px = ((spec.size.clamp(1.0, 80.0) / 100.0) * h as f32).max(12.0);
                body.push_str(&format!(
                    "  <text id=\"title-text\" x=\"{cx}\" y=\"{cy}\" fill=\"{fg_hex}\" fill-opacity=\"{fg_op:.3}\" font-family=\"sans-serif\" font-size=\"{size_px:.1}\" text-anchor=\"middle\" dominant-baseline=\"central\">{escaped}</text>\n"
                ));
            }
            _ => {
                let title = event.map(|e| e.name.as_str()).unwrap_or(&item.name);
                let escaped = xml_escape_text(title);
                body.push_str(&format!(
                    "  <rect id=\"safe-guide\" x=\"80\" y=\"60\" width=\"{}\" height=\"{}\" fill=\"none\" stroke=\"#00d4ff\" stroke-width=\"2\" stroke-dasharray=\"8 6\" />\n",
                    w.saturating_sub(160).max(16),
                    h.saturating_sub(120).max(16),
                ));
                body.push_str(&format!(
                    "  <text id=\"layer-label\" x=\"{}\" y=\"{}\" fill=\"#ffffff\" font-family=\"sans-serif\" font-size=\"48\" text-anchor=\"middle\">{escaped}</text>\n",
                    w / 2,
                    h / 2
                ));
            }
        }
    } else {
        body.push_str(&format!(
            "  <rect id=\"artboard-frame\" x=\"64\" y=\"64\" width=\"{}\" height=\"{}\" rx=\"16\" fill=\"#1e222b\" stroke=\"#5aa0ff\" stroke-width=\"3\" />\n",
            w.saturating_sub(128).max(16),
            h.saturating_sub(128).max(16),
        ));
    }

    let svg = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\">\n\
         {body}</svg>\n"
    );
    if let Some(parent) = out_svg.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(out_svg, svg.as_bytes())?;
    Ok(())
}

fn xml_escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ if c.is_control() && c != '\n' && c != '\t' => {}
            _ => out.push(c),
        }
    }
    out
}

/// Prepara uma imagem PNG editável para o PhotoCraft (copiando imagem ou renderizando o quadro atual).
pub fn export_frame_png_for_photocraft(
    project: &Project,
    media: &MediaCache,
    at: Ticks,
    source_image: Option<&Path>,
    out_png: &Path,
) -> Result<()> {
    if let Some(parent) = out_png.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if let Some(src) = source_image {
        if src.is_file() {
            let ext = src
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if ext == "png" {
                std::fs::copy(src, out_png)?;
                return Ok(());
            }
            let img =
                image::open(src).with_context(|| format!("abrindo imagem {}", src.display()))?;
            img.save(out_png)?;
            return Ok(());
        }
    }
    render::save_snapshot(project, media, at, out_png)
}

/// Entrega um arquivo a um app descoberto via canal RPC ao vivo (quando ativo) ou iniciando o executável.
pub fn send_file_to_app(
    status: &AppDiscoveryStatus,
    file_path: &Path,
    timeout_secs: u64,
) -> Result<DeliveryMethod> {
    if status.live_reachable && status.app.supports_loopback_rpc() {
        if let Some(port) = status.live_port {
            let mut client = CraftClient::connect_discovered(
                status,
                None,
                Duration::from_secs(timeout_secs.clamp(5, 300)),
            )?;
            let _ = client.open_file(file_path)?;
            return Ok(DeliveryMethod::LiveControlRpc { port });
        }
    }

    let exe = status.desktop_exe.as_ref().ok_or_else(|| {
        anyhow!(
            "{} não foi encontrado. Configure o caminho em Opções > Preferências > Aplicativos externos.",
            status.name
        )
    })?;

    Command::new(exe)
        .arg(file_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("não foi possível iniciar {}", exe.display()))?;

    Ok(DeliveryMethod::SpawnedDesktop { exe: exe.clone() })
}

/// Metadados básicos ao criar ou renderizar cena no AniCraft (`anicraft-cli`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AniCraftSequenceResult {
    pub out_dir: PathBuf,
    pub frames: Vec<PathBuf>,
}

/// Cria um novo projeto `.anicraft` alinhado ao FPS e resolução do projeto via `anicraft-cli new`.
pub fn create_anicraft_project(
    anicraft_cli: &Path,
    out_anicraft: &Path,
    width: u32,
    height: u32,
    fps_num: u32,
    fps_den: u32,
) -> Result<()> {
    if let Some(parent) = out_anicraft.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let fps = ((fps_num as f64) / (fps_den.max(1) as f64))
        .round()
        .clamp(1.0, 240.0) as u32;
    let output = Command::new(anicraft_cli)
        .arg("new")
        .arg("--out")
        .arg(out_anicraft)
        .arg("--width")
        .arg(width.max(16).to_string())
        .arg("--height")
        .arg(height.max(16).to_string())
        .arg("--fps")
        .arg(fps.to_string())
        .output()
        .with_context(|| format!("falha ao executar {}", anicraft_cli.display()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("anicraft-cli new falhou: {}", stderr.trim());
    }
    Ok(())
}

/// Renderiza um arquivo `.anicraft` em sequência PNG usando `anicraft-cli render-seq`.
pub fn render_anicraft_sequence(
    anicraft_cli: &Path,
    anicraft_file: &Path,
    out_dir: &Path,
) -> Result<AniCraftSequenceResult> {
    if !anicraft_file.is_file() {
        bail!(
            "arquivo .anicraft não encontrado: {}",
            anicraft_file.display()
        );
    }
    std::fs::create_dir_all(out_dir)?;

    let output = Command::new(anicraft_cli)
        .arg("render-seq")
        .arg(anicraft_file)
        .arg("--out-dir")
        .arg(out_dir)
        .output()
        .with_context(|| format!("falha ao executar {}", anicraft_cli.display()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("anicraft-cli render-seq falhou: {}", stderr.trim());
    }
    let frames = super::blender::collect_png_sequence(out_dir)?;
    if frames.is_empty() {
        bail!(
            "anicraft-cli não gerou quadros PNG em {}",
            out_dir.display()
        );
    }
    Ok(AniCraftSequenceResult {
        out_dir: out_dir.to_path_buf(),
        frames,
    })
}

/// Renderiza um projeto `.ecproj` do EffectCraft para sequência PNG usando `effectcraft-cli render`.
pub fn render_effectcraft_sequence(
    effectcraft_cli: &Path,
    ecproj_file: &Path,
    comp_name: Option<&str>,
    out_dir: &Path,
) -> Result<Vec<PathBuf>> {
    if !ecproj_file.is_file() {
        bail!(
            "projeto EffectCraft não encontrado: {}",
            ecproj_file.display()
        );
    }
    std::fs::create_dir_all(out_dir)?;
    let out_pattern = out_dir.join("frame.png");
    let mut cmd = Command::new(effectcraft_cli);
    cmd.arg("render");
    if let Some(c) = comp_name {
        if !c.trim().is_empty() {
            cmd.arg("--comp").arg(c.trim());
        }
    }
    cmd.arg("--out").arg(&out_pattern).arg(ecproj_file);

    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("falha ao executar {}", effectcraft_cli.display()))?;

    let start = std::time::Instant::now();
    loop {
        if start.elapsed() > MAX_CLI_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            bail!("tempo limite esgotado no effectcraft-cli render");
        }
        if let Some(st) = child.try_wait()? {
            if !st.success() {
                let err = child
                    .stderr
                    .take()
                    .and_then(|mut s| {
                        let mut buf = String::new();
                        std::io::Read::read_to_string(&mut s, &mut buf)
                            .ok()
                            .map(|_| buf)
                    })
                    .unwrap_or_default();
                bail!("effectcraft-cli render falhou: {}", err.trim());
            }
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    let frames = super::blender::collect_png_sequence(out_dir)?;
    if frames.is_empty() {
        bail!(
            "effectcraft-cli render não produziu quadros PNG em {}",
            out_dir.display()
        );
    }
    Ok(frames)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::wav;

    fn tmp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("neoveg_int_wf_{}_{name}", std::process::id()))
    }

    #[test]
    fn gravacao_bwf_wav_preserva_bext_time_reference_e_decodifica() {
        let p = tmp("soundcraft_bwf.wav");
        let samples = vec![0.25f32, -0.25f32, 0.5f32, -0.5f32];
        let time_ref = 48_000 * 12; // 12 segundos na linha do tempo a 48 kHz
        write_bwf_wav(
            &p,
            48000,
            2,
            WavBits::Float32,
            &samples,
            time_ref,
            "Trilha Dialogo - Cena 3",
        )
        .expect("escreve BWF");

        let bytes = std::fs::read(&p).expect("lê BWF");
        let _ = std::fs::remove_file(&p);

        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(&bytes[12..16], b"bext");
        let bext_size = u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        assert_eq!(bext_size, 602);
        // Originator em bext offset 256 -> byte 20 + 256 = 276
        assert!(bytes[276..282].starts_with(b"Neoveg"));
        // TimeReferenceLow em bext offset 338 -> byte 20 + 338 = 358
        let t_lo = u32::from_le_bytes([bytes[358], bytes[359], bytes[360], bytes[361]]);
        let t_hi = u32::from_le_bytes([bytes[362], bytes[363], bytes[364], bytes[365]]);
        let t_read = u64::from(t_lo) | (u64::from(t_hi) << 32);
        assert_eq!(t_read, time_ref);

        // Nosso próprio decodificador WAV pula `bext` e lê todas as amostras intactas
        let decoded = wav::decode(&bytes, 1024).expect("decodifica BWF");
        assert_eq!(decoded.sample_rate, 48000);
        assert_eq!(decoded.channels, 2);
        assert_eq!(decoded.samples.len(), 4);
        assert!((decoded.samples[0] - 0.25).abs() < 1e-5);
    }

    #[test]
    fn geracao_svg_limpa_escapa_xml_hostil() {
        let mut proj = Project::default();
        proj.settings.width = 1280;
        proj.settings.height = 720;
        let item = MediaItem {
            id: 1,
            name: "Título".into(),
            source: MediaSource::Generated(GenSpec::Text(crate::model::TextSpec {
                text: "Cena <1> & \"Final\"".into(),
                ..Default::default()
            })),
            duration: None,
            has_video: true,
            has_audio: false,
            width: 1280,
            height: 720,
            fps: None,
            info: String::new(),
            proxy_path: None,
        };
        let svg_path = tmp("title.svg");
        export_clean_svg_for_vectorcraft(&proj, None, Some(&item), &svg_path).expect("svg");
        let text = std::fs::read_to_string(&svg_path).expect("lê svg");
        let _ = std::fs::remove_file(&svg_path);
        assert!(text.contains("viewBox=\"0 0 1280 720\""));
        assert!(text.contains("Cena &lt;1&gt; &amp; &quot;Final&quot;"));
    }

    #[test]
    fn observador_de_edicao_externa_detecta_modificacao_sem_panico() {
        let p = tmp("watch.txt");
        std::fs::write(&p, b"v1").expect("v1");
        let mut w = WatchedExternalEdit::new(ExternalApp::PhotoCraft, 10, Some(20), p.clone());
        assert!(!w.poll_modified());

        std::fs::write(&p, b"v2-maior").expect("v2");
        assert!(w.poll_modified());
        assert_eq!(w.reload_count, 1);
        assert!(!w.poll_modified());

        w.paused = true;
        std::fs::write(&p, b"v3-pausado-nao-dispara").expect("v3");
        assert!(!w.poll_modified());
        let _ = std::fs::remove_file(&p);
    }
}
