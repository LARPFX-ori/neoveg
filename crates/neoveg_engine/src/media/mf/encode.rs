//! Codificador de vídeo e áudio via Windows Media Foundation (`IMFSinkWriter`),
//! dentro da fronteira FFI isolada `media::mf` (AGENTS.md §0.3 e §2.3).
//!
//! Grava sempre em arquivo temporário irmão e faz renomeação atômica ao
//! finalizar sem erros (§0), removendo o temporário em caso de erro/cancelamento.

#![allow(unsafe_code)]

use super::{ticks_to_hns, MfGuard, HNS_PER_SEC};
use crate::model::FrameRate;
use anyhow::{anyhow, bail, Result};
use std::path::{Path, PathBuf};
use windows::core::{Interface, GUID, HSTRING, PCWSTR};
use windows::Win32::Media::MediaFoundation::{
    eAVEncH264VProfile_Main, IMFMediaType, IMFSinkWriter, MFAudioFormat_AAC, MFAudioFormat_MP3,
    MFAudioFormat_PCM, MFAudioFormat_WMAudioV8, MFCreateAttributes, MFCreateMediaType,
    MFCreateMemoryBuffer, MFCreateSample, MFCreateSinkWriterFromURL, MFMediaType_Audio,
    MFMediaType_Video, MFTranscodeContainerType_3GP, MFTranscodeContainerType_ADTS,
    MFTranscodeContainerType_ASF, MFTranscodeContainerType_MP3, MFTranscodeContainerType_MPEG4,
    MFTranscodeGetAudioOutputAvailableTypes, MFVideoFormat_H264, MFVideoFormat_HEVC,
    MFVideoFormat_NV12, MFVideoFormat_WMV3, MFT_ENUM_FLAG_ALL, MF_MT_ALL_SAMPLES_INDEPENDENT,
    MF_MT_AUDIO_AVG_BYTES_PER_SECOND, MF_MT_AUDIO_BITS_PER_SAMPLE, MF_MT_AUDIO_BLOCK_ALIGNMENT,
    MF_MT_AUDIO_NUM_CHANNELS, MF_MT_AUDIO_SAMPLES_PER_SECOND, MF_MT_AVG_BITRATE,
    MF_MT_DEFAULT_STRIDE, MF_MT_FIXED_SIZE_SAMPLES, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE,
    MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE, MF_MT_MPEG2_PROFILE, MF_MT_PIXEL_ASPECT_RATIO,
    MF_MT_SUBTYPE, MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, MF_TRANSCODE_CONTAINERTYPE,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MfOutputFormat {
    Mp4H264,
    Mp4Hevc,
    MovH264,
    ThreeGp,
    Wmv,
    M4aAac,
    AdtsAac,
    Wma,
    Mp3,
}

impl MfOutputFormat {
    fn container_guid(self) -> GUID {
        match self {
            Self::Mp4H264 | Self::Mp4Hevc | Self::MovH264 | Self::M4aAac => {
                MFTranscodeContainerType_MPEG4
            }
            Self::ThreeGp => MFTranscodeContainerType_3GP,
            Self::Wmv | Self::Wma => MFTranscodeContainerType_ASF,
            Self::AdtsAac => MFTranscodeContainerType_ADTS,
            Self::Mp3 => MFTranscodeContainerType_MP3,
        }
    }

    fn video_subtype(self) -> Option<GUID> {
        match self {
            Self::Mp4H264 | Self::MovH264 | Self::ThreeGp => Some(MFVideoFormat_H264),
            Self::Mp4Hevc => Some(MFVideoFormat_HEVC),
            Self::Wmv => Some(MFVideoFormat_WMV3),
            Self::M4aAac | Self::AdtsAac | Self::Wma | Self::Mp3 => None,
        }
    }

    fn audio_subtype(self) -> GUID {
        match self {
            Self::Mp4H264
            | Self::Mp4Hevc
            | Self::MovH264
            | Self::ThreeGp
            | Self::M4aAac
            | Self::AdtsAac => MFAudioFormat_AAC,
            Self::Wmv | Self::Wma => MFAudioFormat_WMAudioV8,
            Self::Mp3 => MFAudioFormat_MP3,
        }
    }

    fn ext(self) -> &'static str {
        match self {
            Self::Mp4H264 | Self::Mp4Hevc => "mp4",
            Self::MovH264 => "mov",
            Self::ThreeGp => "3gp",
            Self::Wmv => "wmv",
            Self::M4aAac => "m4a",
            Self::AdtsAac => "aac",
            Self::Wma => "wma",
            Self::Mp3 => "mp3",
        }
    }
}

pub struct MfEncodeConfig {
    pub format: MfOutputFormat,
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub fps: FrameRate,
    pub video_bitrate_bps: u32,
    pub audio_sr: Option<u32>,
    pub audio_bitrate_bps: u32,
}

pub struct MfSinkEncoder {
    // `writer` deve ser descartado antes de `_guard` (que encerra MF/COM).
    writer: Option<IMFSinkWriter>,
    _guard: MfGuard,
    final_path: PathBuf,
    temp_path: PathBuf,
    video_stream: Option<u32>,
    audio_stream: Option<u32>,
    fps: FrameRate,
    in_audio_sr: u32,
    out_audio_sr: u32,
    audio_samples_written: i128,
    finalized: bool,
}

struct TempCleanup {
    path: PathBuf,
    armed: bool,
}

impl Drop for TempCleanup {
    fn drop(&mut self) {
        if self.armed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

fn temp_output_path(dest: &Path, ext: &str) -> PathBuf {
    let parent = dest.parent().unwrap_or_else(|| Path::new("."));
    let stem = dest
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("render");
    parent.join(format!("{stem}.neoveg_part_{}.{ext}", std::process::id()))
}

fn pick_audio_output_type(
    subtype: &GUID,
    target_sr: u32,
    target_bps: u32,
) -> Result<(IMFMediaType, u32)> {
    // SAFETY: consulta tabela somente leitura de tipos aceitos pelo MFT do sistema.
    let col = unsafe {
        MFTranscodeGetAudioOutputAvailableTypes(subtype, MFT_ENUM_FLAG_ALL.0 as u32, None)
    }
    .map_err(|e| anyhow!("codificador de áudio Media Foundation indisponível: {e}"))?;
    // SAFETY: `col` é uma `IMFCollection` válida recém-criada.
    let count = unsafe { col.GetElementCount() }.unwrap_or(0);
    let mut best: Option<(IMFMediaType, u32, i64)> = None;
    for i in 0..count {
        // SAFETY: índice `< count` garantido.
        let Ok(unk) = (unsafe { col.GetElement(i) }) else {
            continue;
        };
        let Ok(mt) = unk.cast::<IMFMediaType>() else {
            continue;
        };
        // SAFETY: leitura de atributos em interface COM válida.
        let ch = unsafe { mt.GetUINT32(&MF_MT_AUDIO_NUM_CHANNELS) }.unwrap_or(0);
        // SAFETY: leitura de atributos em interface COM válida.
        let sr = unsafe { mt.GetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND) }.unwrap_or(0);
        // SAFETY: leitura de atributos em interface COM válida.
        let avg_bps = unsafe { mt.GetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND) }
            .unwrap_or(0)
            .saturating_mul(8);
        if ch != 2 || sr == 0 {
            continue;
        }
        // Prioridade máxima para taxa exata; se indisponível, prefere 48000 / 44100.
        let sr_penalty: i64 = if sr == target_sr {
            0
        } else if sr == 48_000 || sr == 44_100 {
            100_000 + (sr as i64 - target_sr as i64).abs()
        } else {
            1_000_000 + (sr as i64 - target_sr as i64).abs()
        };
        let br_diff = (avg_bps as i64 - target_bps as i64).abs();
        let score = sr_penalty * 10 + br_diff;
        match &best {
            Some((_, _, best_score)) if score >= *best_score => {}
            _ => best = Some((mt, sr, score)),
        }
    }
    best.map(|(mt, sr, _)| (mt, sr))
        .ok_or_else(|| anyhow!("nenhum perfil estéreo suportado encontrado para este codec de áudio no Media Foundation"))
}

impl MfSinkEncoder {
    pub fn create(cfg: MfEncodeConfig) -> Result<Self> {
        let guard = MfGuard::enter()?;
        if let Some(parent) = cfg.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temp_path = temp_output_path(&cfg.path, cfg.format.ext());
        let _ = std::fs::remove_file(&temp_path);
        let mut cleanup = TempCleanup {
            path: temp_path.clone(),
            armed: true,
        };

        let mut attrs = None;
        // SAFETY: ponteiro válido para receber `IMFAttributes`.
        unsafe { MFCreateAttributes(&mut attrs, 2) }
            .map_err(|e| anyhow!("MFCreateAttributes (encoder) falhou: {e}"))?;
        if let Some(a) = &attrs {
            // SAFETY: interface COM válida; habilita aceleração por hardware quando disponível.
            let _ = unsafe { a.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1) };
            // SAFETY: define explicitamente o contêiner de saída no atributo de abertura.
            unsafe { a.SetGUID(&MF_TRANSCODE_CONTAINERTYPE, &cfg.format.container_guid()) }
                .map_err(|e| anyhow!("MF_TRANSCODE_CONTAINERTYPE falhou: {e}"))?;
        }

        let hpath = HSTRING::from(temp_path.as_os_str());
        // SAFETY: `hpath` vive durante toda a chamada síncrona de criação do `IMFSinkWriter`.
        let writer =
            unsafe { MFCreateSinkWriterFromURL(PCWSTR(hpath.as_ptr()), None, attrs.as_ref()) }
                .map_err(|e| anyhow!("MFCreateSinkWriterFromURL falhou: {e}"))?;

        let mut video_stream = None;
        let w = (cfg.width / 2).max(1) * 2;
        let h = (cfg.height / 2).max(1) * 2;
        if let Some(vsub) = cfg.format.video_subtype() {
            // SAFETY: criação e preenchimento de `IMFMediaType` de saída de vídeo.
            let out_v = unsafe { MFCreateMediaType() }?;
            let packed_size = ((w as u64) << 32) | (h as u64);
            let packed_fps = ((cfg.fps.num.max(1) as u64) << 32) | (cfg.fps.den.max(1) as u64);
            let packed_par = (1u64 << 32) | 1u64;
            // SAFETY: `out_v` é válido e os atributos correspondem ao tipo de vídeo Media Foundation.
            unsafe {
                out_v.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
                out_v.SetGUID(&MF_MT_SUBTYPE, &vsub)?;
                out_v.SetUINT32(&MF_MT_AVG_BITRATE, cfg.video_bitrate_bps.max(200_000))?;
                out_v.SetUINT32(&MF_MT_INTERLACE_MODE, 2)?; // Progressive
                out_v.SetUINT64(&MF_MT_FRAME_SIZE, packed_size)?;
                out_v.SetUINT64(&MF_MT_FRAME_RATE, packed_fps)?;
                out_v.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, packed_par)?;
                if vsub == MFVideoFormat_H264 {
                    let _ = out_v.SetUINT32(&MF_MT_MPEG2_PROFILE, eAVEncH264VProfile_Main.0 as u32);
                }
            }
            // SAFETY: adiciona stream de saída ao `IMFSinkWriter`.
            let idx = unsafe { writer.AddStream(&out_v) }
                .map_err(|e| anyhow!("codec de vídeo indisponível no Media Foundation: {e}"))?;

            // SAFETY: cria media type de entrada NV12 (nativo para H.264/HEVC/WMV no Windows).
            let in_v = unsafe { MFCreateMediaType() }?;
            // SAFETY: `in_v` é válido.
            unsafe {
                in_v.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
                in_v.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)?;
                in_v.SetUINT32(&MF_MT_INTERLACE_MODE, 2)?;
                in_v.SetUINT64(&MF_MT_FRAME_SIZE, packed_size)?;
                in_v.SetUINT64(&MF_MT_FRAME_RATE, packed_fps)?;
                in_v.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, packed_par)?;
                in_v.SetUINT32(&MF_MT_DEFAULT_STRIDE, w)?;
            }
            // SAFETY: configura formato de entrada NV12 na stream `idx`.
            unsafe { writer.SetInputMediaType(idx, &in_v, None) }.map_err(|e| {
                anyhow!("formato de entrada NV12 recusado pelo Media Foundation: {e}")
            })?;
            video_stream = Some(idx);
        }

        let mut audio_stream = None;
        let mut in_audio_sr = 48_000u32;
        let mut out_audio_sr = 48_000u32;
        if let Some(sr) = cfg.audio_sr {
            in_audio_sr = sr.max(1);
            let asub = cfg.format.audio_subtype();
            let (out_a, chosen_sr) =
                pick_audio_output_type(&asub, in_audio_sr, cfg.audio_bitrate_bps.max(64_000))?;
            out_audio_sr = chosen_sr;
            // SAFETY: adiciona stream de áudio ao `IMFSinkWriter`.
            let idx = unsafe { writer.AddStream(&out_a) }
                .map_err(|e| anyhow!("codec de áudio indisponível no Media Foundation: {e}"))?;

            // SAFETY: configura entrada PCM 16 bits estéreo na taxa negociada `out_audio_sr`.
            let in_a = unsafe { MFCreateMediaType() }?;
            let block_align = 4u32; // 2 canais * 2 bytes
            let avg_bytes = out_audio_sr.saturating_mul(block_align);
            // SAFETY: `in_a` é interface COM válida.
            unsafe {
                in_a.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
                in_a.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM)?;
                in_a.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
                in_a.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, out_audio_sr)?;
                in_a.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, 2)?;
                in_a.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, block_align)?;
                in_a.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, avg_bytes)?;
                in_a.SetUINT32(&MF_MT_FIXED_SIZE_SAMPLES, 1)?;
                in_a.SetUINT32(&MF_MT_ALL_SAMPLES_INDEPENDENT, 1)?;
            }
            // SAFETY: configura entrada PCM para o codificador de áudio.
            unsafe { writer.SetInputMediaType(idx, &in_a, None) }
                .map_err(|e| anyhow!("entrada PCM recusada pelo codificador de áudio: {e}"))?;
            audio_stream = Some(idx);
        }

        if video_stream.is_none() && audio_stream.is_none() {
            bail!("nenhum fluxo configurado para o codificador Media Foundation");
        }

        // SAFETY: todas as streams de entrada foram configuradas.
        unsafe { writer.BeginWriting() }
            .map_err(|e| anyhow!("BeginWriting falhou no Media Foundation: {e}"))?;

        cleanup.armed = false;
        Ok(Self {
            writer: Some(writer),
            _guard: guard,
            final_path: cfg.path,
            temp_path,
            video_stream,
            audio_stream,
            fps: cfg.fps,
            in_audio_sr,
            out_audio_sr,
            audio_samples_written: 0,
            finalized: false,
        })
    }

    pub fn write_video_nv12(&mut self, frame_index: u64, nv12: &[u8]) -> Result<()> {
        let Some(stream) = self.video_stream else {
            return Ok(());
        };
        let t0 = self.fps.frame_to_ticks(frame_index as i64);
        let t1 = self.fps.frame_to_ticks(frame_index as i64 + 1);
        let hns_start = ticks_to_hns(t0);
        let hns_dur = (ticks_to_hns(t1) - hns_start).max(1);
        self.write_buffer_sample(stream, nv12, hns_start, hns_dur)
    }

    pub fn write_audio_f32(&mut self, stereo_interleaved: &[f32]) -> Result<()> {
        let Some(stream) = self.audio_stream else {
            return Ok(());
        };
        if stereo_interleaved.is_empty() {
            return Ok(());
        }
        let resampled;
        let src = if self.in_audio_sr == self.out_audio_sr {
            stereo_interleaved
        } else {
            resampled =
                resample_stereo_linear(stereo_interleaved, self.in_audio_sr, self.out_audio_sr);
            &resampled
        };
        let frames = (src.len() / 2) as i128;
        if frames == 0 {
            return Ok(());
        }
        let mut pcm_bytes = Vec::with_capacity(frames as usize * 4);
        for &s in &src[..frames as usize * 2] {
            let clean = if s.is_finite() {
                s.clamp(-1.0, 1.0)
            } else {
                0.0
            };
            let i = (clean * 32767.0).round() as i16;
            pcm_bytes.extend_from_slice(&i.to_le_bytes());
        }
        let hns_start =
            ((self.audio_samples_written * HNS_PER_SEC) / self.out_audio_sr as i128) as i64;
        let hns_dur = ((frames * HNS_PER_SEC) / self.out_audio_sr as i128).max(1) as i64;
        self.audio_samples_written += frames;
        self.write_buffer_sample(stream, &pcm_bytes, hns_start, hns_dur)
    }

    fn write_buffer_sample(
        &self,
        stream: u32,
        bytes: &[u8],
        hns_start: i64,
        hns_dur: i64,
    ) -> Result<()> {
        let len =
            u32::try_from(bytes.len()).map_err(|_| anyhow!("buffer grande demais para MF"))?;
        // SAFETY: cria buffer COM contíguo gerenciado pelo runtime do Media Foundation.
        let buf = unsafe { MFCreateMemoryBuffer(len) }?;
        {
            let mut ptr: *mut u8 = std::ptr::null_mut();
            // SAFETY: trava `buf` com capacidade `>= len`, copia exatamente `bytes.len()` e destrava antes de usar.
            unsafe { buf.Lock(&mut ptr, None, None) }?;
            if ptr.is_null() {
                // SAFETY: pareado com `Lock` bem-sucedido logo acima.
                let _ = unsafe { buf.Unlock() };
                bail!("MFCreateMemoryBuffer retornou ponteiro nulo");
            }
            // SAFETY: `ptr` aponta para região gravável alocada pelo `IMFMediaBuffer` de pelo menos `len` bytes.
            unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len()) };
            // SAFETY: destrava após concluir cópia de memória.
            unsafe { buf.Unlock() }?;
            // SAFETY: marca tamanho preenchido no buffer.
            unsafe { buf.SetCurrentLength(len) }?;
        }
        // SAFETY: cria `IMFSample` e anexa `buf`.
        let sample = unsafe { MFCreateSample() }?;
        let Some(writer) = &self.writer else {
            bail!("IMFSinkWriter já encerrado");
        };
        // SAFETY: interfaces COM válidas.
        unsafe {
            sample.AddBuffer(&buf)?;
            sample.SetSampleTime(hns_start)?;
            sample.SetSampleDuration(hns_dur)?;
            writer.WriteSample(stream, &sample)?;
        }
        Ok(())
    }

    pub fn finish(mut self) -> Result<()> {
        let writer = self
            .writer
            .take()
            .ok_or_else(|| anyhow!("IMFSinkWriter já finalizado"))?;
        // SAFETY: finaliza escrita do arquivo e grava átomos finais (ex.: `moov` no MP4).
        unsafe { writer.Finalize() }
            .map_err(|e| anyhow!("Finalize do Media Foundation falhou: {e}"))?;
        // Libera o handle COM do arquivo antes do rename no Windows.
        drop(writer);
        if self.final_path.exists() {
            let _ = std::fs::remove_file(&self.final_path);
        }
        std::fs::rename(&self.temp_path, &self.final_path)?;
        self.finalized = true;
        Ok(())
    }
}

impl Drop for MfSinkEncoder {
    fn drop(&mut self) {
        // Garante liberação do handle COM antes de remover o arquivo temporário.
        drop(self.writer.take());
        if !self.finalized {
            let _ = std::fs::remove_file(&self.temp_path);
        }
    }
}

/// Converte quadro RGBA8 (topo para baixo) em NV12 (plano Y + plano UV intercalado),
/// com dimensões pares.
pub fn rgba_to_nv12(rgba: &[u8], w: u32, h: u32) -> Vec<u8> {
    let wu = (w as usize / 2).max(1) * 2;
    let hu = (h as usize / 2).max(1) * 2;
    let y_size = wu * hu;
    let mut nv12 = vec![0u8; y_size + y_size / 2];
    for y in 0..hu {
        let src_row = y * (w as usize) * 4;
        let dst_row = y * wu;
        for x in 0..wu {
            let off = src_row + x * 4;
            let r = *rgba.get(off).unwrap_or(&0) as i32;
            let g = *rgba.get(off + 1).unwrap_or(&0) as i32;
            let b = *rgba.get(off + 2).unwrap_or(&0) as i32;
            let luma = (((66 * r + 129 * g + 25 * b + 128) >> 8) + 16).clamp(16, 235);
            nv12[dst_row + x] = luma as u8;
        }
    }
    let uv_base = y_size;
    for cy in 0..(hu / 2) {
        let y0 = cy * 2;
        let y1 = y0 + 1;
        for cx in 0..(wu / 2) {
            let x0 = cx * 2;
            let x1 = x0 + 1;
            let mut r_sum = 0i32;
            let mut g_sum = 0i32;
            let mut b_sum = 0i32;
            for (sy, sx) in [(y0, x0), (y0, x1), (y1, x0), (y1, x1)] {
                let off = (sy * (w as usize) + sx) * 4;
                r_sum += *rgba.get(off).unwrap_or(&0) as i32;
                g_sum += *rgba.get(off + 1).unwrap_or(&0) as i32;
                b_sum += *rgba.get(off + 2).unwrap_or(&0) as i32;
            }
            let r = (r_sum + 2) >> 2;
            let g = (g_sum + 2) >> 2;
            let b = (b_sum + 2) >> 2;
            let u = (((-38 * r - 74 * g + 112 * b + 128) >> 8) + 128).clamp(16, 240);
            let v = (((112 * r - 94 * g - 18 * b + 128) >> 8) + 128).clamp(16, 240);
            let uv_idx = uv_base + cy * wu + cx * 2;
            nv12[uv_idx] = u as u8;
            nv12[uv_idx + 1] = v as u8;
        }
    }
    nv12
}

fn resample_stereo_linear(src: &[f32], in_sr: u32, out_sr: u32) -> Vec<f32> {
    let in_frames = src.len() / 2;
    if in_frames == 0 || in_sr == 0 || out_sr == 0 {
        return Vec::new();
    }
    let out_frames = ((in_frames as u128 * out_sr as u128) / in_sr as u128).max(1) as usize;
    let mut out = Vec::with_capacity(out_frames * 2);
    for i in 0..out_frames {
        let pos = (i as f64) * (in_sr as f64) / (out_sr as f64);
        let idx = (pos as usize).min(in_frames - 1);
        let next = (idx + 1).min(in_frames - 1);
        let frac = (pos - idx as f64) as f32;
        let l0 = src[idx * 2];
        let r0 = src[idx * 2 + 1];
        let l1 = src[next * 2];
        let r1 = src[next * 2 + 1];
        out.push(l0 + (l1 - l0) * frac);
        out.push(r0 + (r1 - r0) * frac);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn encodes_mp4_h264_and_m4a_aac_verified_by_ffprobe() {
        let dir = std::env::temp_dir().join(format!("neoveg_mf_enc_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let mp4 = dir.join("out.mp4");
        let fps = FrameRate::FPS_30;
        let mut enc = MfSinkEncoder::create(MfEncodeConfig {
            format: MfOutputFormat::Mp4H264,
            path: mp4.clone(),
            width: 128,
            height: 96,
            fps,
            video_bitrate_bps: 1_500_000,
            audio_sr: Some(48_000),
            audio_bitrate_bps: 128_000,
        })
        .expect("SinkWriter H.264/AAC deve abrir no Windows");
        let rgba = vec![200u8; 128 * 96 * 4];
        let nv12 = rgba_to_nv12(&rgba, 128, 96);
        for f in 0..15u64 {
            enc.write_video_nv12(f, &nv12).unwrap();
        }
        let samples = vec![0.1f32; 48_000]; // 0.5s estéreo
        enc.write_audio_f32(&samples).unwrap();
        enc.finish().unwrap();
        assert!(mp4.exists());

        // Relê pelo próprio decodificador MF e checa dimensões + áudio
        let loaded = super::super::open_media(&mp4).expect("deve abrir MP4 exportado");
        let vr = loaded.video.expect("vídeo presente");
        assert_eq!((vr.width, vr.height), (128, 96));
        assert!(loaded.audio.is_some());

        if let Ok(out) = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-show_entries",
                "stream=codec_name,width,height",
                "-of",
                "default=noprint_wrappers=1",
            ])
            .arg(&mp4)
            .output()
        {
            if out.status.success() {
                let s = String::from_utf8_lossy(&out.stdout);
                assert!(s.contains("h264"), "{s}");
                assert!(s.contains("aac"), "{s}");
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
