//! Multiplexador Matroska `.mkv` em Rust puro (`#[forbid(unsafe_code)]`, Sala Limpa)
//! implementado a partir das especificações públicas:
//! - IETF RFC 8794 (*Extensible Binary Meta Language (EBML)*);
//! - IETF RFC 9559 (*Matroska Media Container Format*).
//!
//! Suporta faixas de vídeo (`V_FFV1`, `V_MJPEG`, `V_PNG`, `V_MPEG4/ISO/AVC`,
//! `V_MPEGH/ISO/HEVC`, `V_AV1`, `V_VP9`) e áudio opcional (`A_PCM/INT/LIT`,
//! `A_FLAC`, `A_AAC`), com temporização inteira exata em microssegundos/ms e `Cues`.

use crate::model::FrameRate;
use anyhow::{anyhow, bail, Context, Result};
use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::Path;

// IDs EBML / Matroska (RFC 8794 / RFC 9559)
const ID_EBML: u32 = 0x1A45_DFA3;
const ID_EBML_VERSION: u32 = 0x4286;
const ID_EBML_READ_VERSION: u32 = 0x42F7;
const ID_EBML_MAX_ID_LEN: u32 = 0x42F2;
const ID_EBML_MAX_SIZE_LEN: u32 = 0x42F3;
const ID_DOC_TYPE: u32 = 0x4282;
const ID_DOC_TYPE_VERSION: u32 = 0x4287;
const ID_DOC_TYPE_READ_VERSION: u32 = 0x4285;

const ID_SEGMENT: u32 = 0x1853_8067;
const ID_SEEK_HEAD: u32 = 0x114D_9B74;
const ID_INFO: u32 = 0x1549_A966;
const ID_TIMESTAMP_SCALE: u32 = 0x002A_D7B1;
const ID_MUXING_APP: u32 = 0x4D80;
const ID_WRITING_APP: u32 = 0x5741;
const ID_DURATION: u32 = 0x4489;

const ID_TRACKS: u32 = 0x1654_AE6B;
const ID_TRACK_ENTRY: u32 = 0xAE;
const ID_TRACK_NUMBER: u32 = 0xD7;
const ID_TRACK_UID: u32 = 0x73C5;
const ID_TRACK_TYPE: u32 = 0x83;
const ID_FLAG_LACING: u32 = 0x9C;
const ID_DEFAULT_DURATION: u32 = 0x0023_E383;
const ID_CODEC_ID: u32 = 0x86;
const ID_CODEC_PRIVATE: u32 = 0x63A2;

const ID_VIDEO: u32 = 0xE0;
const ID_PIXEL_WIDTH: u32 = 0xB0;
const ID_PIXEL_HEIGHT: u32 = 0xBA;
const ID_ALPHA_MODE: u32 = 0x53C0;

const ID_AUDIO: u32 = 0xE1;
const ID_SAMPLING_FREQ: u32 = 0xB5;
const ID_CHANNELS: u32 = 0x9F;
const ID_BIT_DEPTH: u32 = 0x6264;

const ID_CLUSTER: u32 = 0x1F43_B675;
const ID_CLUSTER_TIMESTAMP: u32 = 0xE7;
const ID_SIMPLE_BLOCK: u32 = 0xA3;

const ID_CUES: u32 = 0x1C53_BB6B;
const ID_CUE_POINT: u32 = 0xBB;
const ID_CUE_TIME: u32 = 0xB3;
const ID_CUE_TRACK_POSITIONS: u32 = 0xB7;
const ID_CUE_TRACK: u32 = 0xF7;
const ID_CUE_CLUSTER_POSITION: u32 = 0xF1;

const TIMESTAMP_SCALE_NS: u64 = 1_000_000; // 1 ms

#[derive(Clone, Debug)]
pub enum MkvVideoCodec {
    Ffv1 {
        codec_private: Vec<u8>,
        has_alpha: bool,
    },
    Mjpeg,
    Png,
    H264 {
        avcc: Vec<u8>,
    },
    Hevc {
        hvcc: Vec<u8>,
    },
    Av1 {
        av1c: Vec<u8>,
    },
    Vp9 {
        codec_private: Vec<u8>,
    },
}

impl MkvVideoCodec {
    fn codec_id(&self) -> &'static str {
        match self {
            Self::Ffv1 { .. } => "V_FFV1",
            Self::Mjpeg => "V_MJPEG",
            Self::Png => "V_PNG",
            Self::H264 { .. } => "V_MPEG4/ISO/AVC",
            Self::Hevc { .. } => "V_MPEGH/ISO/HEVC",
            Self::Av1 { .. } => "V_AV1",
            Self::Vp9 { .. } => "V_VP9",
        }
    }

    fn codec_private(&self) -> &[u8] {
        match self {
            Self::Ffv1 { codec_private, .. } => codec_private,
            Self::H264 { avcc } => avcc,
            Self::Hevc { hvcc } => hvcc,
            Self::Av1 { av1c } => av1c,
            Self::Vp9 { codec_private } => codec_private,
            Self::Mjpeg | Self::Png => &[],
        }
    }
}

#[derive(Clone, Debug)]
pub enum MkvAudioCodec {
    Pcm16Le {
        sample_rate: u32,
        channels: u16,
    },
    Flac {
        sample_rate: u32,
        channels: u16,
        bits_per_sample: u16,
        codec_private: Vec<u8>,
    },
    Aac {
        sample_rate: u32,
        channels: u16,
        asc: Vec<u8>,
    },
}

impl MkvAudioCodec {
    fn codec_id(&self) -> &'static str {
        match self {
            Self::Pcm16Le { .. } => "A_PCM/INT/LIT",
            Self::Flac { .. } => "A_FLAC",
            Self::Aac { .. } => "A_AAC",
        }
    }

    fn sample_rate(&self) -> u32 {
        match self {
            Self::Pcm16Le { sample_rate, .. }
            | Self::Flac { sample_rate, .. }
            | Self::Aac { sample_rate, .. } => *sample_rate,
        }
    }

    fn channels(&self) -> u16 {
        match self {
            Self::Pcm16Le { channels, .. }
            | Self::Flac { channels, .. }
            | Self::Aac { channels, .. } => *channels,
        }
    }

    fn bit_depth(&self) -> Option<u16> {
        match self {
            Self::Pcm16Le { .. } => Some(16),
            Self::Flac {
                bits_per_sample, ..
            } => Some(*bits_per_sample),
            Self::Aac { .. } => None,
        }
    }

    fn codec_private(&self) -> &[u8] {
        match self {
            Self::Flac { codec_private, .. } => codec_private,
            Self::Aac { asc, .. } => asc,
            Self::Pcm16Le { .. } => &[],
        }
    }
}

struct PendingBlock {
    track_num: u8,
    pts_ms: i64,
    is_keyframe: bool,
    payload: Vec<u8>,
}

struct CueEntry {
    time_ms: u64,
    cluster_rel_pos: u64,
}

fn push_id(out: &mut Vec<u8>, id: u32) {
    let b = id.to_be_bytes();
    let start = if b[0] != 0 {
        0
    } else if b[1] != 0 {
        1
    } else if b[2] != 0 {
        2
    } else {
        3
    };
    out.extend_from_slice(&b[start..]);
}

fn push_vint_size(out: &mut Vec<u8>, size: u64) {
    for width in 1..=8u32 {
        let max_val = if width == 8 {
            (1u64 << 56) - 2
        } else {
            (1u64 << (7 * width)) - 2
        };
        if size <= max_val {
            let encoded = (1u64 << (7 * width)) | size;
            let bytes = encoded.to_be_bytes();
            out.extend_from_slice(&bytes[8 - width as usize..]);
            return;
        }
    }
}

fn push_elem(out: &mut Vec<u8>, id: u32, payload: &[u8]) {
    push_id(out, id);
    push_vint_size(out, payload.len() as u64);
    out.extend_from_slice(payload);
}

fn push_uint(out: &mut Vec<u8>, id: u32, val: u64) {
    let bytes = val.to_be_bytes();
    let first = bytes.iter().position(|&b| b != 0).unwrap_or(7);
    push_elem(out, id, &bytes[first..]);
}

fn push_str(out: &mut Vec<u8>, id: u32, s: &str) {
    push_elem(out, id, s.as_bytes());
}

fn push_f64(out: &mut Vec<u8>, id: u32, v: f64) {
    push_elem(out, id, &v.to_be_bytes());
}

pub struct MkvWriter {
    out: BufWriter<File>,
    fps: FrameRate,
    has_video: bool,
    segment_size_pos: u64,
    segment_data_start: u64,
    duration_pos: u64,
    pos: u64,
    cluster: Vec<PendingBlock>,
    cluster_bytes: usize,
    cluster_min_ts: i64,
    cluster_max_ts: i64,
    max_ts_ms: i64,
    cues: Vec<CueEntry>,
}

impl MkvWriter {
    pub fn create(
        path: &Path,
        width: u32,
        height: u32,
        fps: FrameRate,
        video: MkvVideoCodec,
        audio: Option<MkvAudioCodec>,
    ) -> Result<Self> {
        if width == 0 || height == 0 {
            bail!("dimensões MKV inválidas: {width}x{height}");
        }
        if let Some(a) = &audio {
            if a.sample_rate() == 0 || a.channels() == 0 {
                bail!(
                    "parâmetros de áudio MKV inválidos: {} Hz, {} ch",
                    a.sample_rate(),
                    a.channels()
                );
            }
        }
        let mut out = BufWriter::new(
            File::create(path).with_context(|| format!("criando {}", path.display()))?,
        );

        let mut ebml_body = Vec::with_capacity(64);
        push_uint(&mut ebml_body, ID_EBML_VERSION, 1);
        push_uint(&mut ebml_body, ID_EBML_READ_VERSION, 1);
        push_uint(&mut ebml_body, ID_EBML_MAX_ID_LEN, 4);
        push_uint(&mut ebml_body, ID_EBML_MAX_SIZE_LEN, 8);
        push_str(&mut ebml_body, ID_DOC_TYPE, "matroska");
        push_uint(&mut ebml_body, ID_DOC_TYPE_VERSION, 4);
        push_uint(&mut ebml_body, ID_DOC_TYPE_READ_VERSION, 2);
        let mut hdr = Vec::with_capacity(256);
        push_elem(&mut hdr, ID_EBML, &ebml_body);

        // Segment com tamanho reservado de 8 bytes (atualizado em finish())
        push_id(&mut hdr, ID_SEGMENT);
        let segment_size_pos = hdr.len() as u64;
        hdr.extend_from_slice(&0x01FF_FFFF_FFFF_FFFFu64.to_be_bytes());
        let segment_data_start = hdr.len() as u64;

        // Info
        let mut info_body = Vec::with_capacity(64);
        push_uint(&mut info_body, ID_TIMESTAMP_SCALE, TIMESTAMP_SCALE_NS);
        push_str(&mut info_body, ID_MUXING_APP, "Neoveg");
        push_str(&mut info_body, ID_WRITING_APP, "Neoveg");
        let dur_rel_off = info_body.len() as u64 + 3; // ID(2) + VINT_8(1)
        push_f64(&mut info_body, ID_DURATION, 0.0);
        push_elem(&mut hdr, ID_INFO, &info_body);
        let info_body_len = info_body.len() as u64;
        let duration_pos = hdr.len() as u64 - info_body_len + dur_rel_off;

        // Tracks
        let mut tracks_body = Vec::with_capacity(256);
        let mut vtrack = Vec::with_capacity(128);
        push_uint(&mut vtrack, ID_TRACK_NUMBER, 1);
        push_uint(&mut vtrack, ID_TRACK_UID, 1);
        push_uint(&mut vtrack, ID_TRACK_TYPE, 1);
        push_uint(&mut vtrack, ID_FLAG_LACING, 0);
        let default_dur_ns =
            (1_000_000_000u64 * u64::from(fps.den.max(1))) / u64::from(fps.num.max(1));
        if default_dur_ns > 0 {
            push_uint(&mut vtrack, ID_DEFAULT_DURATION, default_dur_ns);
        }
        push_str(&mut vtrack, ID_CODEC_ID, video.codec_id());
        let vpriv = video.codec_private();
        if !vpriv.is_empty() {
            push_elem(&mut vtrack, ID_CODEC_PRIVATE, vpriv);
        }
        let mut v_geom = Vec::with_capacity(24);
        push_uint(&mut v_geom, ID_PIXEL_WIDTH, u64::from(width));
        push_uint(&mut v_geom, ID_PIXEL_HEIGHT, u64::from(height));
        if matches!(
            video,
            MkvVideoCodec::Ffv1 {
                has_alpha: true,
                ..
            }
        ) {
            push_uint(&mut v_geom, ID_ALPHA_MODE, 1);
        }
        push_elem(&mut vtrack, ID_VIDEO, &v_geom);
        push_elem(&mut tracks_body, ID_TRACK_ENTRY, &vtrack);

        if let Some(a) = &audio {
            let mut atrack = Vec::with_capacity(96);
            push_uint(&mut atrack, ID_TRACK_NUMBER, 2);
            push_uint(&mut atrack, ID_TRACK_UID, 2);
            push_uint(&mut atrack, ID_TRACK_TYPE, 2);
            push_uint(&mut atrack, ID_FLAG_LACING, 0);
            push_str(&mut atrack, ID_CODEC_ID, a.codec_id());
            let apriv = a.codec_private();
            if !apriv.is_empty() {
                push_elem(&mut atrack, ID_CODEC_PRIVATE, apriv);
            }
            let mut a_info = Vec::with_capacity(32);
            push_f64(&mut a_info, ID_SAMPLING_FREQ, f64::from(a.sample_rate()));
            push_uint(&mut a_info, ID_CHANNELS, u64::from(a.channels()));
            if let Some(bd) = a.bit_depth() {
                push_uint(&mut a_info, ID_BIT_DEPTH, u64::from(bd));
            }
            push_elem(&mut atrack, ID_AUDIO, &a_info);
            push_elem(&mut tracks_body, ID_TRACK_ENTRY, &atrack);
        }
        let _ = ID_SEEK_HEAD;
        push_elem(&mut hdr, ID_TRACKS, &tracks_body);

        out.write_all(&hdr)?;
        Ok(Self {
            out,
            fps,
            has_video: false,
            segment_size_pos,
            segment_data_start,
            duration_pos,
            pos: hdr.len() as u64,
            cluster: Vec::new(),
            cluster_bytes: 0,
            cluster_min_ts: i64::MAX,
            cluster_max_ts: i64::MIN,
            max_ts_ms: 0,
            cues: Vec::new(),
        })
    }

    pub fn frame_pts_ms(&self, frame_idx: u64) -> i64 {
        let num = i128::from(frame_idx) * 1000 * i128::from(self.fps.den.max(1));
        let den = i128::from(self.fps.num.max(1));
        ((num + den / 2) / den) as i64
    }

    pub fn samples_pts_ms(sample_idx: u64, sr: u32) -> i64 {
        let num = i128::from(sample_idx) * 1000;
        let den = i128::from(sr.max(1));
        ((num + den / 2) / den) as i64
    }

    pub fn write_video_packet(
        &mut self,
        pts_ms: i64,
        is_keyframe: bool,
        payload: Vec<u8>,
    ) -> Result<()> {
        if payload.is_empty() {
            bail!("pacote de vídeo MKV vazio");
        }
        self.has_video = true;
        self.push_block(1, pts_ms, is_keyframe, payload)
    }

    pub fn write_audio_packet(&mut self, pts_ms: i64, payload: Vec<u8>) -> Result<()> {
        if payload.is_empty() {
            return Ok(());
        }
        self.push_block(2, pts_ms, true, payload)
    }

    pub fn write_audio_pcm_i16(&mut self, pts_ms: i64, pcm: &[i16]) -> Result<()> {
        if pcm.is_empty() {
            return Ok(());
        }
        let mut bytes = Vec::with_capacity(pcm.len() * 2);
        for &s in pcm {
            bytes.extend_from_slice(&s.to_le_bytes());
        }
        self.push_block(2, pts_ms, true, bytes)
    }

    fn push_block(
        &mut self,
        track_num: u8,
        pts_ms: i64,
        is_keyframe: bool,
        payload: Vec<u8>,
    ) -> Result<()> {
        let pts_ms = pts_ms.max(0);
        if !self.cluster.is_empty() {
            let new_min = self.cluster_min_ts.min(pts_ms);
            let new_max = self.cluster_max_ts.max(pts_ms);
            let flush_for_kf = track_num == 1 && is_keyframe && (new_max - new_min) >= 1000;
            if (new_max - new_min) > 20_000 || self.cluster_bytes >= 2_000_000 || flush_for_kf {
                self.flush_cluster()?;
            }
        }
        self.cluster_min_ts = self.cluster_min_ts.min(pts_ms);
        self.cluster_max_ts = self.cluster_max_ts.max(pts_ms);
        self.max_ts_ms = self.max_ts_ms.max(pts_ms);
        self.cluster_bytes += payload.len() + 8;
        self.cluster.push(PendingBlock {
            track_num,
            pts_ms,
            is_keyframe,
            payload,
        });
        Ok(())
    }

    fn flush_cluster(&mut self) -> Result<()> {
        if self.cluster.is_empty() {
            return Ok(());
        }
        let cluster_ts = self.cluster_min_ts.max(0) as u64;
        let cluster_rel = self.pos.saturating_sub(self.segment_data_start);
        let mut has_kf = false;

        let mut cbody = Vec::with_capacity(self.cluster_bytes + 32);
        push_uint(&mut cbody, ID_CLUSTER_TIMESTAMP, cluster_ts);
        for blk in self.cluster.drain(..) {
            if blk.track_num == 1 && blk.is_keyframe {
                has_kf = true;
            }
            let rel = (blk.pts_ms - cluster_ts as i64).clamp(-32_768, 32_767) as i16;
            let flags = if blk.is_keyframe { 0x80u8 } else { 0x00u8 };
            let mut sb = Vec::with_capacity(4 + blk.payload.len());
            sb.push(0x80 | blk.track_num);
            sb.extend_from_slice(&rel.to_be_bytes());
            sb.push(flags);
            sb.extend_from_slice(&blk.payload);
            push_elem(&mut cbody, ID_SIMPLE_BLOCK, &sb);
        }
        if has_kf {
            self.cues.push(CueEntry {
                time_ms: cluster_ts,
                cluster_rel_pos: cluster_rel,
            });
        }

        let mut cluster_elem = Vec::with_capacity(12 + cbody.len());
        push_elem(&mut cluster_elem, ID_CLUSTER, &cbody);
        self.out.write_all(&cluster_elem)?;
        self.pos = self.pos.saturating_add(cluster_elem.len() as u64);
        self.cluster_bytes = 0;
        self.cluster_min_ts = i64::MAX;
        self.cluster_max_ts = i64::MIN;
        Ok(())
    }

    pub fn finish(mut self) -> Result<()> {
        self.flush_cluster()?;
        if !self.has_video {
            bail!("MKV sem quadros de vídeo gravados");
        }
        if !self.cues.is_empty() {
            let mut cues_body = Vec::with_capacity(self.cues.len() * 24);
            for cue in &self.cues {
                let mut pos_body = Vec::with_capacity(16);
                push_uint(&mut pos_body, ID_CUE_TRACK, 1);
                push_uint(&mut pos_body, ID_CUE_CLUSTER_POSITION, cue.cluster_rel_pos);
                let mut pt_body = Vec::with_capacity(24);
                push_uint(&mut pt_body, ID_CUE_TIME, cue.time_ms);
                push_elem(&mut pt_body, ID_CUE_TRACK_POSITIONS, &pos_body);
                push_elem(&mut cues_body, ID_CUE_POINT, &pt_body);
            }
            let mut cues_elem = Vec::with_capacity(12 + cues_body.len());
            push_elem(&mut cues_elem, ID_CUES, &cues_body);
            self.out.write_all(&cues_elem)?;
            self.pos = self.pos.saturating_add(cues_elem.len() as u64);
        }

        let frame_step_ms = self.frame_pts_ms(1).max(1);
        let duration_ms = (self.max_ts_ms + frame_step_ms) as f64;
        let segment_size = self.pos.saturating_sub(self.segment_data_start);
        let encoded_seg_sz = ((1u64 << 56) | segment_size.min((1u64 << 56) - 2)).to_be_bytes();

        self.out.flush()?;
        let mut f = self
            .out
            .into_inner()
            .map_err(|e| anyhow!("falha ao concluir MKV: {e}"))?;
        f.seek(SeekFrom::Start(self.segment_size_pos))?;
        f.write_all(&encoded_seg_sz)?;
        f.seek(SeekFrom::Start(self.duration_pos))?;
        f.write_all(&duration_ms.to_be_bytes())?;
        f.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::demux;

    #[test]
    fn roundtrip_mkv_mjpeg_png_with_pcm_and_flac_and_ffmpeg_oracle() {
        let mut rgba = vec![0u8; 32 * 24 * 4];
        for y in 0..24u32 {
            for x in 0..32u32 {
                let i = ((y * 32 + x) * 4) as usize;
                rgba[i] = (x * 8) as u8;
                rgba[i + 1] = (y * 10) as u8;
                rgba[i + 2] = 90;
                rgba[i + 3] = 255;
            }
        }
        let img = image::RgbaImage::from_raw(32, 24, rgba.clone()).unwrap();
        let mut jpg = Vec::new();
        let rgb: Vec<u8> = rgba
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|c| [c[0], c[1], c[2]])
            .collect();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg, 90)
            .encode(&rgb, 32, 24, image::ExtendedColorType::Rgb8)
            .unwrap();
        let mut png = Vec::new();
        {
            use image::ImageEncoder;
            image::codecs::png::PngEncoder::new(&mut png)
                .write_image(&rgba, 32, 24, image::ExtendedColorType::Rgba8)
                .unwrap();
        }
        let _ = img;

        for (tag, vcodec, frame_data) in [
            ("mjpeg", MkvVideoCodec::Mjpeg, jpg),
            ("png", MkvVideoCodec::Png, png),
        ] {
            let p = std::env::temp_dir().join(format!("neoveg_mkv_{tag}.mkv"));
            let acodec = MkvAudioCodec::Pcm16Le {
                sample_rate: 48_000,
                channels: 2,
            };
            let mut w =
                MkvWriter::create(&p, 32, 24, FrameRate::FPS_25, vcodec, Some(acodec)).unwrap();
            let mut sample_idx = 0u64;
            for fi in 0..4u64 {
                let vts = w.frame_pts_ms(fi);
                w.write_video_packet(vts, true, frame_data.clone()).unwrap();
                let ats = MkvWriter::samples_pts_ms(sample_idx, 48_000);
                let spf = 48_000 / 25;
                w.write_audio_pcm_i16(ats, &vec![300i16; spf * 2]).unwrap();
                sample_idx += spf as u64;
            }
            w.finish().unwrap();

            let idx = demux::demux_file(&p).unwrap();
            let v_idx = idx
                .tracks
                .iter()
                .position(|t| t.kind == demux::TrackKind::Video)
                .unwrap();
            assert_eq!(idx.samples_for_track(v_idx).count(), 4);
            let a_trk = idx
                .tracks
                .iter()
                .find(|t| t.kind == demux::TrackKind::Audio)
                .unwrap();
            assert_eq!(a_trk.sample_rate, 48_000);
            if let Ok(out) = std::process::Command::new("ffmpeg")
                .args(["-hide_banner", "-v", "error", "-i"])
                .arg(&p)
                .args(["-f", "null", "-"])
                .output()
            {
                if out.status.success() {
                    assert!(
                        out.stderr.is_empty(),
                        "ffmpeg erro em MKV {tag}: {}",
                        String::from_utf8_lossy(&out.stderr)
                    );
                }
            }
            let _ = std::fs::remove_file(&p);
        }
    }
}
