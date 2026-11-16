//! Conversão de mídia que o Neoveg não abre diretamente para um arquivo que
//! ele abre de forma nativa (AGENTS.md §0 e §2.3).
//!
//! A decodificação usa a API de mídia do sistema operacional (Windows Media
//! Foundation, pela fronteira FFI `media::mf`); o `ffmpeg` nunca é usado.
//! Vídeo vira AVI MJPEG com áudio PCM 16 bits estéreo; áudio sem vídeo vira
//! WAV PCM 24 bits estéreo. Os dois são lidos pelos decodificadores próprios
//! do Neoveg.
//!
//! Nada é inventado nem perdido em silêncio: duração desconhecida, quadro que
//! falha ao decodificar e áudio cortado pelo limite de memória são erros; um
//! stream que termina antes da duração anunciada é encurtado e relatado; a
//! qualidade JPEG escolhida para caber no limite do AVI OpenDML é relatada. O
//! arquivo é gravado como `.part` e só é renomeado quando termina sem erro.

// Fora do Windows só o erro de "não suportado" é compilado; o resto fica
// para quando houver um decodificador do sistema nessas plataformas.
#![cfg_attr(not(windows), allow(dead_code))]

use super::avi::AviWriter;
use crate::model::{FrameRate, Ticks, MAX_DIM};
use crate::render::{WavBits, WavWriter};
use anyhow::{anyhow, bail, Context, Result};
use image::RgbaImage;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

/// Qualidades JPEG tentadas, da melhor para a menor, até o vídeo caber no AVI.
const QUALITIES: [u8; 5] = [90, 80, 70, 60, 50];
/// Capacidade do AVI OpenDML gravado pelo `AviWriter` (256 segmentos de
/// 1 GB), com folga para índices e áudio.
const AVI_BUDGET_BYTES: u64 = 250 << 30;
/// Maior duração convertida (12 h): limita o laço de quadros (§0.5).
const MAX_CONVERT_SECS: i64 = 12 * 3600;
/// Tentativas de nome único no destino.
const MAX_NAME_TRIES: u32 = 999;
/// Quadros-estéreo de áudio gravados por bloco num WAV.
const WAV_BLOCK_FRAMES: usize = 48_000;
/// Sufixo dos arquivos convertidos.
pub const CONVERTED_SUFFIX: &str = "_compativel";

/// Progresso e cancelamento de uma conversão, lidos pela UI.
#[derive(Default)]
pub struct ConvertProgress {
    pub done: AtomicU64,
    pub total: AtomicU64,
    pub cancel: AtomicBool,
    /// Nome do arquivo em conversão.
    pub current: Mutex<String>,
}

impl ConvertProgress {
    pub fn fraction(&self) -> f32 {
        let t = self.total.load(Ordering::Relaxed).max(1);
        (self.done.load(Ordering::Relaxed) as f32 / t as f32).clamp(0.0, 1.0)
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    fn start(&self, name: &str, total: u64) {
        *self
            .current
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = name.to_string();
        self.done.store(0, Ordering::Relaxed);
        self.total.store(total.max(1), Ordering::Relaxed);
    }
}

/// Resultado de uma conversão bem-sucedida.
#[derive(Debug)]
pub struct Converted {
    pub path: PathBuf,
    /// Avisos para o usuário (qualidade reduzida, stream mais curto etc.).
    pub warnings: Vec<String>,
}

/// Converte `src` para um arquivo compatível dentro de `dest_dir`.
///
/// O nome é `<nome>_compativel.avi` (ou `.wav`), com `(2)`, `(3)`... se já
/// existir. Nunca sobrescreve um arquivo. Roda isolado por `catch_unwind`
/// (§0.6): uma falha interna vira erro, não derruba o chamador.
pub fn convert_to_compatible(
    src: &Path,
    dest_dir: &Path,
    progress: &ConvertProgress,
) -> Result<Converted> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        convert_inner(src, dest_dir, progress)
    }))
    .unwrap_or_else(|_| {
        Err(anyhow!(
            "falha interna isolada ao converter {}",
            src.display()
        ))
    })
}

fn convert_inner(src: &Path, dest_dir: &Path, progress: &ConvertProgress) -> Result<Converted> {
    std::fs::create_dir_all(dest_dir)
        .with_context(|| format!("não foi possível criar a pasta {}", dest_dir.display()))?;
    let name = src
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    progress.start(&name, 1);
    let loaded = super::system::open_media(src).map_err(|e| {
        anyhow!(
            "nenhum decodificador do sistema reconhece {}: {e:#}",
            src.display()
        )
    })?;
    if loaded.audio_truncated {
        bail!(
            "o áudio de {} passa do limite de memória da conversão (~2 h estéreo a 48 kHz); \
             nada foi gravado",
            src.display()
        );
    }
    let stem = output_stem(src);
    match loaded.video {
        Some(vr) => {
            let src_info = VideoSrc {
                width: vr.width(),
                height: vr.height(),
                fps: vr.fps(),
                duration: loaded.duration,
            };
            convert_video(
                &src_info,
                &|t| vr.decode_frame_at(t),
                loaded.audio.as_ref(),
                dest_dir,
                &stem,
                progress,
            )
        }
        None => {
            let audio = loaded
                .audio
                .ok_or_else(|| anyhow!("{} não tem áudio nem vídeo legível", src.display()))?;
            convert_audio(&audio, dest_dir, &stem, progress)
        }
    }
}

/// Descrição do vídeo de origem (separada do leitor para testar o laço).
struct VideoSrc {
    width: u32,
    height: u32,
    fps: FrameRate,
    duration: Option<Ticks>,
}

/// Arquivo `.part` apagado no `Drop` a menos que seja confirmado.
struct PartFile {
    path: PathBuf,
    keep: bool,
}

impl Drop for PartFile {
    fn drop(&mut self) {
        if !self.keep {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

fn part_path(out: &Path) -> PathBuf {
    let mut s = out.as_os_str().to_os_string();
    s.push(".part");
    PathBuf::from(s)
}

/// Promove o `.part` para o nome final sem sobrescrever nada.
fn commit_part(mut part: PartFile, out: &Path) -> Result<()> {
    if out.exists() {
        bail!("{} passou a existir durante a conversão", out.display());
    }
    std::fs::rename(&part.path, out)
        .with_context(|| format!("não foi possível gravar {}", out.display()))?;
    part.keep = true;
    Ok(())
}

fn output_stem(src: &Path) -> String {
    let stem = src
        .file_stem()
        .map(|s| s.to_string_lossy().trim().to_string())
        .unwrap_or_default();
    if stem.is_empty() {
        "midia".to_string()
    } else {
        stem
    }
}

/// Primeiro nome livre `<stem>_compativel[ (n)].<ext>` em `dir`.
pub fn unique_output(dir: &Path, stem: &str, ext: &str) -> Result<PathBuf> {
    for k in 1..=MAX_NAME_TRIES {
        let name = if k == 1 {
            format!("{stem}{CONVERTED_SUFFIX}.{ext}")
        } else {
            format!("{stem}{CONVERTED_SUFFIX} ({k}).{ext}")
        };
        let p = dir.join(name);
        if !p.exists() && !part_path(&p).exists() {
            return Ok(p);
        }
    }
    bail!(
        "não há nome livre para o arquivo convertido em {}",
        dir.display()
    )
}

/// Índice do primeiro quadro-estéreo de áudio do quadro de vídeo `frame`,
/// em aritmética inteira exata (§5): `frame * sr * den / num`.
fn audio_frame_boundary(frame: u64, sr: u32, fps: FrameRate) -> usize {
    let num = u128::from(fps.num.max(1));
    let v = u128::from(frame) * u128::from(sr) * u128::from(fps.den) / num;
    usize::try_from(v).unwrap_or(usize::MAX)
}

/// Codifica um quadro RGBA como JPEG (também usado pelo gerador de proxy).
pub(crate) fn encode_jpeg(img: &RgbaImage, quality: u8) -> Result<Vec<u8>> {
    let (w, h) = img.dimensions();
    let rgb = image::DynamicImage::ImageRgba8(img.clone()).into_rgb8();
    let mut jpg = Vec::with_capacity((w as usize * h as usize) / 4);
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg, quality).encode(
        &rgb,
        w,
        h,
        image::ExtendedColorType::Rgb8,
    )?;
    Ok(jpg)
}

fn fit_frame(img: RgbaImage, w: u32, h: u32) -> RgbaImage {
    if img.width() == w && img.height() == h {
        img
    } else {
        image::imageops::resize(&img, w, h, image::imageops::FilterType::Triangle)
    }
}

/// Escolhe a maior qualidade cujo tamanho estimado cabe no AVI, a partir de
/// quadros de amostra. `None` se nem a menor qualidade cabe.
fn pick_quality(samples: &[RgbaImage], n_frames: u64, audio_bytes: u64) -> Result<Option<u8>> {
    if samples.is_empty() {
        return Ok(QUALITIES.first().copied());
    }
    for &q in &QUALITIES {
        let mut sum = 0u64;
        for s in samples {
            sum = sum.saturating_add(encode_jpeg(s, q)?.len() as u64);
        }
        let avg = sum / samples.len() as u64;
        // 16 bytes de índice + 8 de cabeçalho de chunk por quadro, 10% de folga
        let per_frame = avg.saturating_add(24).saturating_mul(11) / 10;
        let estimate = per_frame
            .saturating_mul(n_frames)
            .saturating_add(audio_bytes);
        if estimate <= AVI_BUDGET_BYTES {
            return Ok(Some(q));
        }
    }
    Ok(None)
}

fn convert_video(
    src: &VideoSrc,
    decode: &dyn Fn(Ticks) -> Result<(RgbaImage, bool)>,
    audio: Option<&super::AudioData>,
    dest_dir: &Path,
    stem: &str,
    progress: &ConvertProgress,
) -> Result<Converted> {
    let (w, h, fps) = (src.width, src.height, src.fps);
    if w == 0 || h == 0 || w > MAX_DIM || h > MAX_DIM {
        bail!("dimensões de vídeo fora do limite: {w}x{h} (máximo {MAX_DIM})");
    }
    let dur = src.duration.ok_or_else(|| {
        anyhow!("o arquivo não informa a duração; a conversão não adivinha quantos quadros gravar")
    })?;
    if dur <= Ticks::ZERO || dur > Ticks::from_secs_i64(MAX_CONVERT_SECS) {
        bail!(
            "duração fora do limite da conversão (até {} h)",
            MAX_CONVERT_SECS / 3600
        );
    }
    let n = u64::try_from(fps.ticks_to_frame_floor(dur)).unwrap_or(0);
    if n == 0 {
        bail!("o vídeo tem menos de um quadro");
    }

    let audio_bytes = audio.map_or(0, |a| (a.data.len() as u64).saturating_mul(2));
    let mut samples = Vec::with_capacity(3);
    for pct in [10u64, 50, 90] {
        let fi = i64::try_from(n * pct / 100).unwrap_or(0);
        let (img, _) = decode(fps.frame_to_ticks(fi))
            .with_context(|| format!("falha ao decodificar o quadro de amostra {fi}"))?;
        samples.push(fit_frame(img, w, h));
    }
    let Some(quality) = pick_quality(&samples, n, audio_bytes)? else {
        bail!(
            "o vídeo é longo demais para o formato intermediário AVI (limite de ~250 GB) \
             mesmo na qualidade {}; nada foi gravado",
            QUALITIES.last().copied().unwrap_or(50)
        );
    };
    drop(samples);

    let out = unique_output(dest_dir, stem, "avi")?;
    let part = PartFile {
        path: part_path(&out),
        keep: false,
    };
    progress.total.store(n, Ordering::Relaxed);
    let sr = audio.map(|a| a.sample_rate.max(1));
    let mut wr = AviWriter::create(&part.path, w, h, fps, sr)?;
    let mut written = 0u64;
    let mut short_by = 0u64;
    let mut audio_pos = 0usize; // em quadros-estéreo
    for i in 0..n {
        if progress.cancelled() {
            bail!("conversão cancelada");
        }
        let fi = i64::try_from(i).unwrap_or(i64::MAX);
        let (img, fresh) = decode(fps.frame_to_ticks(fi))
            .with_context(|| format!("falha ao decodificar o quadro {i}"))?;
        if !fresh {
            // o stream acabou antes da duração anunciada: não grava repetições
            short_by = n - i;
            break;
        }
        wr.write_video_jpeg(&encode_jpeg(&fit_frame(img, w, h), quality)?)?;
        if let (Some(a), Some(sr)) = (audio, sr) {
            let end = audio_frame_boundary(i + 1, sr, fps);
            let chunk = a
                .data
                .get(audio_pos.saturating_mul(2)..end.saturating_mul(2).min(a.data.len()))
                .unwrap_or(&[]);
            wr.write_audio(&crate::render::to_i16(chunk))?;
            audio_pos = audio_pos.max(end);
        }
        written += 1;
        progress.done.store(written, Ordering::Relaxed);
    }
    if written == 0 {
        bail!("nenhum quadro de vídeo pôde ser lido");
    }
    // o áudio que passa do último quadro também é gravado (nada se perde)
    if let Some(a) = audio {
        if let Some(rest) = a.data.get(audio_pos.saturating_mul(2)..) {
            wr.write_audio(&crate::render::to_i16(rest))?;
        }
    }
    wr.finish()?;
    commit_part(part, &out)?;

    let mut warnings = vec![];
    if quality < QUALITIES.first().copied().unwrap_or(90) {
        warnings.push(format!(
            "qualidade JPEG reduzida para {quality} para caber no limite do AVI (~250 GB)"
        ));
    }
    if short_by > 0 {
        warnings.push(format!(
            "o arquivo terminou {short_by} quadro(s) antes da duração anunciada; \
             o convertido tem {written} quadro(s)"
        ));
    }
    Ok(Converted {
        path: out,
        warnings,
    })
}

fn convert_audio(
    audio: &super::AudioData,
    dest_dir: &Path,
    stem: &str,
    progress: &ConvertProgress,
) -> Result<Converted> {
    let frames = audio.data.len() / 2;
    if frames == 0 {
        bail!("o áudio está vazio");
    }
    let out = unique_output(dest_dir, stem, "wav")?;
    let part = PartFile {
        path: part_path(&out),
        keep: false,
    };
    progress.total.store(frames as u64, Ordering::Relaxed);
    let mut wr = WavWriter::create(&part.path, audio.sample_rate.max(1), WavBits::Pcm24)?;
    for (k, block) in audio.data.chunks(WAV_BLOCK_FRAMES * 2).enumerate() {
        if progress.cancelled() {
            bail!("conversão cancelada");
        }
        wr.write(block)?;
        let done = ((k + 1) * WAV_BLOCK_FRAMES).min(frames);
        progress.done.store(done as u64, Ordering::Relaxed);
    }
    wr.finish()?;
    commit_part(part, &out)?;
    Ok(Converted {
        path: out,
        warnings: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("neoveg_convert_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("cria pasta");
        d
    }

    #[test]
    fn audio_boundaries_are_exact_and_cover_everything() {
        let fps = FrameRate::FPS_29_97;
        // 30000 quadros a 30000/1001 = 1001 s exatos = 48_048_000 quadros a 48 kHz
        assert_eq!(audio_frame_boundary(30_000, 48_000, fps), 48_048_000);
        let mut prev = 0;
        for i in 1..=300 {
            let b = audio_frame_boundary(i, 48_000, fps);
            assert!(b >= prev);
            assert!(
                (1601..=1602).contains(&(b - prev)),
                "quadro {i}: {}",
                b - prev
            );
            prev = b;
        }
        assert_eq!(audio_frame_boundary(25, 48_000, FrameRate::FPS_25), 48_000);
    }

    #[test]
    fn unique_output_never_reuses_a_name() {
        let d = tmp_dir("nomes");
        let a = unique_output(&d, "clipe", "avi").expect("nome");
        assert_eq!(
            a.file_name().and_then(|s| s.to_str()),
            Some("clipe_compativel.avi")
        );
        std::fs::write(&a, b"x").expect("grava");
        let b = unique_output(&d, "clipe", "avi").expect("nome");
        assert_eq!(
            b.file_name().and_then(|s| s.to_str()),
            Some("clipe_compativel (2).avi")
        );
        // um .part em andamento também ocupa o nome
        std::fs::write(part_path(&b), b"x").expect("grava");
        let c = unique_output(&d, "clipe", "avi").expect("nome");
        assert_eq!(
            c.file_name().and_then(|s| s.to_str()),
            Some("clipe_compativel (3).avi")
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    fn solid(w: u32, h: u32, v: u8) -> RgbaImage {
        RgbaImage::from_pixel(w, h, image::Rgba([v, v / 2, 255 - v, 255]))
    }

    #[test]
    fn video_loop_writes_every_frame_and_audio() {
        let d = tmp_dir("video");
        let fps = FrameRate::FPS_25;
        let src = VideoSrc {
            width: 64,
            height: 36,
            fps,
            duration: Some(fps.frame_to_ticks(10)),
        };
        let audio = super::super::AudioData::from_stereo(48_000, vec![0.25; 2 * 48_000 * 10 / 25]);
        let decode = |t: Ticks| -> Result<(RgbaImage, bool)> {
            Ok((
                solid(64, 36, (fps.ticks_to_frame_floor(t) * 20) as u8),
                true,
            ))
        };
        let p = ConvertProgress::default();
        let out = convert_video(&src, &decode, Some(&audio), &d, "teste", &p).expect("converte");
        assert!(out.warnings.is_empty(), "{:?}", out.warnings);
        assert!(!part_path(&out.path).exists());
        let mut rd = super::super::avi::AviReader::open(&out.path).expect("reabre");
        assert_eq!(rd.frame_count(), 10);
        let (sr, ch, pcm) = rd.read_audio().expect("áudio").expect("tem áudio");
        assert_eq!((sr, ch), (48_000, 2));
        assert_eq!(pcm.len(), 2 * 48_000 * 10 / 25);
        assert_eq!(p.done.load(Ordering::Relaxed), 10);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn short_stream_is_reported_not_padded() {
        let d = tmp_dir("curto");
        let fps = FrameRate::FPS_25;
        let src = VideoSrc {
            width: 32,
            height: 32,
            fps,
            duration: Some(fps.frame_to_ticks(10)),
        };
        // só 6 quadros reais; depois o leitor repete o último
        let decode = |t: Ticks| -> Result<(RgbaImage, bool)> {
            Ok((solid(32, 32, 9), fps.ticks_to_frame_floor(t) < 6))
        };
        let out = convert_video(
            &src,
            &decode,
            None,
            &d,
            "curto",
            &ConvertProgress::default(),
        )
        .expect("converte");
        assert_eq!(out.warnings.len(), 1, "{:?}", out.warnings);
        let rd = super::super::avi::AviReader::open(&out.path).expect("reabre");
        assert_eq!(rd.frame_count(), 6);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn decode_error_and_cancel_leave_no_file() {
        let d = tmp_dir("falha");
        let fps = FrameRate::FPS_25;
        let src = VideoSrc {
            width: 32,
            height: 32,
            fps,
            duration: Some(fps.frame_to_ticks(20)),
        };
        let failing = |t: Ticks| -> Result<(RgbaImage, bool)> {
            if fps.ticks_to_frame_floor(t) == 12 {
                Err(anyhow!("quadro corrompido"))
            } else {
                Ok((solid(32, 32, 1), true))
            }
        };
        assert!(convert_video(&src, &failing, None, &d, "x", &ConvertProgress::default()).is_err());
        let p = ConvertProgress::default();
        p.cancel.store(true, Ordering::Relaxed);
        let ok = |_t: Ticks| -> Result<(RgbaImage, bool)> { Ok((solid(32, 32, 1), true)) };
        assert!(convert_video(&src, &ok, None, &d, "x", &p).is_err());
        let left: Vec<_> = std::fs::read_dir(&d).expect("lista").flatten().collect();
        assert!(left.is_empty(), "sobrou arquivo: {left:?}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn unknown_duration_and_bad_dimensions_are_errors() {
        let d = tmp_dir("hostil");
        let ok = |_t: Ticks| -> Result<(RgbaImage, bool)> { Ok((solid(8, 8, 1), true)) };
        let p = ConvertProgress::default();
        let mut src = VideoSrc {
            width: 8,
            height: 8,
            fps: FrameRate::FPS_25,
            duration: None,
        };
        assert!(convert_video(&src, &ok, None, &d, "x", &p).is_err());
        src.duration = Some(Ticks::from_secs_i64(1));
        src.width = MAX_DIM + 1;
        assert!(convert_video(&src, &ok, None, &d, "x", &p).is_err());
        src.width = 0;
        assert!(convert_video(&src, &ok, None, &d, "x", &p).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn audio_only_becomes_wav() {
        let d = tmp_dir("wav");
        let audio = super::super::AudioData::from_stereo(44_100, vec![0.1; 2 * 44_100]);
        let out = convert_audio(&audio, &d, "som", &ConvertProgress::default()).expect("converte");
        assert_eq!(out.path.extension().and_then(|e| e.to_str()), Some("wav"));
        let back = super::super::audio::decode_file(&out.path).expect("reabre");
        assert_eq!(back.sample_rate, 44_100);
        assert_eq!(back.frames(), 44_100);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn garbage_input_is_an_error_not_a_panic() {
        let d = tmp_dir("lixo");
        let f = d.join("lixo.mts");
        std::fs::write(&f, b"isto nao e video").expect("grava");
        let r = convert_to_compatible(&f, &d, &ConvertProgress::default());
        assert!(r.is_err());
        let left = std::fs::read_dir(&d).expect("lista").flatten().count();
        assert_eq!(left, 1, "só o arquivo de entrada deve existir");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Converte a fixture `mf_smoke.mp4` (`tools/gen_media_fixtures.py`) e
    /// confere com o `ffprobe` como oráculo externo (§2.3). Pula sem fixture.
    #[cfg(windows)]
    #[test]
    fn converts_mf_smoke_fixture() {
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("target")
            .join("fixtures")
            .join("mf_smoke.mp4");
        if !fixture.exists() {
            return;
        }
        let d = tmp_dir("fixture");
        let out =
            convert_to_compatible(&fixture, &d, &ConvertProgress::default()).expect("converte");
        let rd = super::super::avi::AviReader::open(&out.path).expect("reabre");
        assert_eq!(rd.frame_count(), 50, "2 s a 25 fps");
        let mut item = super::super::base_item(
            "conv".into(),
            crate::model::MediaSource::File {
                path: out.path.to_string_lossy().to_string(),
            },
        );
        let imp = super::super::load_entry(&mut item).expect("o Neoveg abre o convertido");
        assert!(imp.item.has_video && imp.item.has_audio);
        if let Ok(o) = std::process::Command::new("ffprobe")
            .args(["-v", "error", "-count_frames", "-select_streams", "v:0"])
            .args(["-show_entries", "stream=nb_read_frames", "-of", "csv=p=0"])
            .arg(&out.path)
            .output()
        {
            if o.status.success() {
                let n = String::from_utf8_lossy(&o.stdout).trim().to_string();
                assert_eq!(n, "50", "ffprobe conta os quadros do AVI convertido");
            }
        }
        let _ = std::fs::remove_dir_all(&d);
    }
}
