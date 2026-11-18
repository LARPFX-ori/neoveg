//! Multiplexador QuickTime `.mov` (ISO/IEC 14496-12 / QuickTime File Format)
//! em Rust puro (`#[forbid(unsafe_code)]`, Sala Limpa).
//!
//! Suporta vídeo:
//! - RGB não comprimido (`BGRA` 32 bits com canal alfa, topo para baixo)
//! - YUV 4:2:2 não comprimido (`2vuy`, `[U, Y0, V, Y1]` BT.601)
//! - Photo-JPEG (`jpeg`)
//! - Apple PNG (`png `)
//!
//! Suporta faixa de áudio opcional PCM 16 bits little-endian (`sowt`),
//! deslocamentos 32/64 bits (`stco` / `co64`) e marca `qt  ` em `ftyp`.

use super::avi::AviWriter;
use crate::model::FrameRate;
use anyhow::{anyhow, bail, Context, Result};
use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MovVideoCodec {
    UncompressedBgra,
    UncompressedYuv422,
    PhotoJpeg,
    Png,
}

impl MovVideoCodec {
    fn fourcc(self) -> [u8; 4] {
        match self {
            Self::UncompressedBgra => *b"BGRA",
            Self::UncompressedYuv422 => *b"2vuy",
            Self::PhotoJpeg => *b"jpeg",
            Self::Png => *b"png ",
        }
    }

    fn depth(self) -> u16 {
        match self {
            Self::UncompressedBgra | Self::Png => 32,
            Self::UncompressedYuv422 | Self::PhotoJpeg => 24,
        }
    }
}

struct AudioChunk {
    offset: u64,
    frames: u32,
}

pub struct MovWriter {
    out: BufWriter<File>,
    width: u32,
    height: u32,
    fps: FrameRate,
    codec: MovVideoCodec,
    audio_sr: Option<u32>,
    mdat_header_pos: u64,
    pos: u64,
    video_offsets: Vec<u64>,
    video_sizes: Vec<u32>,
    audio_chunks: Vec<AudioChunk>,
    total_audio_frames: u64,
}

fn push_box(out: &mut Vec<u8>, fcc: &[u8; 4], body: &[u8]) -> Result<()> {
    let total = 8usize
        .checked_add(body.len())
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| anyhow!("átomo QuickTime excede 4 GiB"))?;
    out.extend_from_slice(&total.to_be_bytes());
    out.extend_from_slice(fcc);
    out.extend_from_slice(body);
    Ok(())
}

fn push_full_box(out: &mut Vec<u8>, fcc: &[u8; 4], ver: u8, flags: u32, body: &[u8]) -> Result<()> {
    let mut full = Vec::with_capacity(4 + body.len());
    full.push(ver);
    full.push(((flags >> 16) & 0xFF) as u8);
    full.push(((flags >> 8) & 0xFF) as u8);
    full.push((flags & 0xFF) as u8);
    full.extend_from_slice(body);
    push_box(out, fcc, &full)
}

fn identity_matrix() -> [u8; 36] {
    let mut m = [0u8; 36];
    m[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    m[16..20].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    m[32..36].copy_from_slice(&0x4000_0000u32.to_be_bytes());
    m
}

impl MovWriter {
    pub fn create(
        path: &Path,
        width: u32,
        height: u32,
        fps: FrameRate,
        audio_sr: Option<u32>,
        codec: MovVideoCodec,
    ) -> Result<Self> {
        if width == 0 || height == 0 {
            bail!("dimensões MOV inválidas: {width}x{height}");
        }
        if let Some(sr) = audio_sr {
            if sr == 0 {
                bail!("taxa de amostragem de áudio MOV inválida: {sr}");
            }
        }
        let mut out = BufWriter::new(
            File::create(path).with_context(|| format!("criando {}", path.display()))?,
        );

        let mut ftyp_body = Vec::with_capacity(16);
        ftyp_body.extend_from_slice(b"qt  ");
        ftyp_body.extend_from_slice(&0x0000_0200u32.to_be_bytes());
        ftyp_body.extend_from_slice(b"qt  ");
        let mut hdr = Vec::with_capacity(40);
        push_box(&mut hdr, b"ftyp", &ftyp_body)?;
        let mdat_header_pos = hdr.len() as u64;
        // Caixa `mdat` de 64 bits (size=1 + "mdat" + largesize=u64) permite escrita em fluxo >4 GiB.
        hdr.extend_from_slice(&1u32.to_be_bytes());
        hdr.extend_from_slice(b"mdat");
        hdr.extend_from_slice(&0u64.to_be_bytes());
        out.write_all(&hdr)?;

        Ok(Self {
            out,
            width,
            height,
            fps,
            codec,
            audio_sr,
            mdat_header_pos,
            pos: hdr.len() as u64,
            video_offsets: Vec::new(),
            video_sizes: Vec::new(),
            audio_chunks: Vec::new(),
            total_audio_frames: 0,
        })
    }

    /// Converte RGBA8 (topo para baixo) em BGRA 32 bits topo para baixo para `MOV BGRA`.
    pub fn rgba_to_bgra_topdown(rgba: &[u8], w: u32, h: u32) -> Vec<u8> {
        let pixels = (w as usize).saturating_mul(h as usize);
        let mut out = vec![0u8; pixels * 4];
        for i in 0..pixels {
            let s = rgba.get(i * 4..i * 4 + 4).unwrap_or(&[0, 0, 0, 255]);
            out[i * 4] = s[2];
            out[i * 4 + 1] = s[1];
            out[i * 4 + 2] = s[0];
            out[i * 4 + 3] = s[3];
        }
        out
    }

    /// Converte RGBA8 em QuickTime `2vuy` (`[U, Y0, V, Y1]` topo para baixo).
    pub fn rgba_to_2vuy(rgba: &[u8], w: u32, h: u32) -> Vec<u8> {
        AviWriter::rgba_to_uyvy(rgba, w, h)
    }

    pub fn write_video_frame(&mut self, encoded: &[u8]) -> Result<()> {
        let len = u32::try_from(encoded.len()).map_err(|_| anyhow!("quadro MOV excede 4 GiB"))?;
        let off = self.pos;
        self.out.write_all(encoded)?;
        self.pos = self.pos.saturating_add(encoded.len() as u64);
        self.video_offsets.push(off);
        self.video_sizes.push(len);
        Ok(())
    }

    pub fn write_audio(&mut self, pcm_stereo_i16: &[i16]) -> Result<()> {
        if self.audio_sr.is_none() || pcm_stereo_i16.is_empty() {
            return Ok(());
        }
        let n_frames = pcm_stereo_i16.len() / 2;
        if n_frames == 0 {
            return Ok(());
        }
        let mut bytes = Vec::with_capacity(n_frames * 4);
        for &s in &pcm_stereo_i16[..n_frames * 2] {
            bytes.extend_from_slice(&s.to_le_bytes());
        }
        let off = self.pos;
        self.out.write_all(&bytes)?;
        self.pos = self.pos.saturating_add(bytes.len() as u64);
        self.audio_chunks.push(AudioChunk {
            offset: off,
            frames: n_frames as u32,
        });
        self.total_audio_frames = self.total_audio_frames.saturating_add(n_frames as u64);
        Ok(())
    }

    fn build_chunk_offset_box(offsets: &[u64]) -> Result<Vec<u8>> {
        let mut box_buf = Vec::new();
        let need_64 = offsets.iter().any(|&o| o > u64::from(u32::MAX));
        let count = u32::try_from(offsets.len())
            .map_err(|_| anyhow!("tabela de chunks MOV grande demais"))?;
        if need_64 {
            let mut body = Vec::with_capacity(4 + offsets.len() * 8);
            body.extend_from_slice(&count.to_be_bytes());
            for &o in offsets {
                body.extend_from_slice(&o.to_be_bytes());
            }
            push_full_box(&mut box_buf, b"co64", 0, 0, &body)?;
        } else {
            let mut body = Vec::with_capacity(4 + offsets.len() * 4);
            body.extend_from_slice(&count.to_be_bytes());
            for &o in offsets {
                body.extend_from_slice(&(o as u32).to_be_bytes());
            }
            push_full_box(&mut box_buf, b"stco", 0, 0, &body)?;
        }
        Ok(box_buf)
    }

    fn build_dinf() -> Result<Vec<u8>> {
        let mut url = Vec::new();
        push_full_box(&mut url, b"url ", 0, 1, &[])?;
        let mut dref_body = Vec::with_capacity(4 + url.len());
        dref_body.extend_from_slice(&1u32.to_be_bytes());
        dref_body.extend_from_slice(&url);
        let mut dref = Vec::new();
        push_full_box(&mut dref, b"dref", 0, 0, &dref_body)?;
        let mut dinf = Vec::new();
        push_box(&mut dinf, b"dinf", &dref)?;
        Ok(dinf)
    }

    fn build_tkhd(track_id: u32, duration: u64, volume: u16, w: u32, h: u32) -> Result<Vec<u8>> {
        let mut body = Vec::new();
        let ver = if duration > u64::from(u32::MAX) {
            1u8
        } else {
            0u8
        };
        if ver == 1 {
            body.extend_from_slice(&0u64.to_be_bytes()); // creation
            body.extend_from_slice(&0u64.to_be_bytes()); // mod
            body.extend_from_slice(&track_id.to_be_bytes());
            body.extend_from_slice(&0u32.to_be_bytes());
            body.extend_from_slice(&duration.to_be_bytes());
        } else {
            body.extend_from_slice(&0u32.to_be_bytes());
            body.extend_from_slice(&0u32.to_be_bytes());
            body.extend_from_slice(&track_id.to_be_bytes());
            body.extend_from_slice(&0u32.to_be_bytes());
            body.extend_from_slice(&(duration as u32).to_be_bytes());
        }
        body.extend_from_slice(&[0u8; 8]); // reserved
        body.extend_from_slice(&0u16.to_be_bytes()); // layer
        body.extend_from_slice(&0u16.to_be_bytes()); // alt_group
        body.extend_from_slice(&volume.to_be_bytes());
        body.extend_from_slice(&0u16.to_be_bytes());
        body.extend_from_slice(&identity_matrix());
        body.extend_from_slice(&(w << 16).to_be_bytes());
        body.extend_from_slice(&(h << 16).to_be_bytes());
        let mut out = Vec::new();
        push_full_box(&mut out, b"tkhd", ver, 0x0F, &body)?;
        Ok(out)
    }

    fn build_mdhd(timescale: u32, duration: u64) -> Result<Vec<u8>> {
        let mut body = Vec::new();
        let ver = if duration > u64::from(u32::MAX) {
            1u8
        } else {
            0u8
        };
        if ver == 1 {
            body.extend_from_slice(&0u64.to_be_bytes());
            body.extend_from_slice(&0u64.to_be_bytes());
            body.extend_from_slice(&timescale.to_be_bytes());
            body.extend_from_slice(&duration.to_be_bytes());
        } else {
            body.extend_from_slice(&0u32.to_be_bytes());
            body.extend_from_slice(&0u32.to_be_bytes());
            body.extend_from_slice(&timescale.to_be_bytes());
            body.extend_from_slice(&(duration as u32).to_be_bytes());
        }
        body.extend_from_slice(&0x55C4u16.to_be_bytes()); // lang 'und'
        body.extend_from_slice(&0u16.to_be_bytes()); // quality
        let mut out = Vec::new();
        push_full_box(&mut out, b"mdhd", ver, 0, &body)?;
        Ok(out)
    }

    fn build_hdlr(handler: &[u8; 4], name: &[u8]) -> Result<Vec<u8>> {
        let mut body = Vec::with_capacity(24 + name.len());
        body.extend_from_slice(b"mhlr");
        body.extend_from_slice(handler);
        body.extend_from_slice(&[0u8; 12]);
        body.push(name.len().min(255) as u8);
        body.extend_from_slice(&name[..name.len().min(255)]);
        let mut out = Vec::new();
        push_full_box(&mut out, b"hdlr", 0, 0, &body)?;
        Ok(out)
    }

    fn build_video_trak(&self, movie_ts: u32, movie_dur: u64) -> Result<Vec<u8>> {
        let rate = self.fps.num.max(1);
        let scale = self.fps.den.max(1);
        let n_frames = self.video_offsets.len() as u64;
        let media_dur = n_frames.saturating_mul(u64::from(scale));
        let _ = movie_ts;

        // Visual Sample Entry (86 bytes total)
        let mut entry = Vec::with_capacity(86);
        entry.extend_from_slice(&86u32.to_be_bytes());
        entry.extend_from_slice(&self.codec.fourcc());
        entry.extend_from_slice(&[0u8; 6]); // reserved
        entry.extend_from_slice(&1u16.to_be_bytes()); // data_reference_index
        entry.extend_from_slice(&0u16.to_be_bytes()); // version
        entry.extend_from_slice(&0u16.to_be_bytes()); // revision
        entry.extend_from_slice(b"neov"); // vendor
        entry.extend_from_slice(&0u32.to_be_bytes()); // temporal_quality
        entry.extend_from_slice(&512u32.to_be_bytes()); // spatial_quality
        entry.extend_from_slice(&(self.width as u16).to_be_bytes());
        entry.extend_from_slice(&(self.height as u16).to_be_bytes());
        entry.extend_from_slice(&0x0048_0000u32.to_be_bytes()); // 72 dpi H
        entry.extend_from_slice(&0x0048_0000u32.to_be_bytes()); // 72 dpi V
        entry.extend_from_slice(&0u32.to_be_bytes()); // data_size
        entry.extend_from_slice(&1u16.to_be_bytes()); // frame_count
        entry.extend_from_slice(&[0u8; 32]); // compressor_name
        entry.extend_from_slice(&self.codec.depth().to_be_bytes());
        entry.extend_from_slice(&(-1i16).to_be_bytes()); // color_table_id = none

        let mut stsd_body = Vec::with_capacity(4 + entry.len());
        stsd_body.extend_from_slice(&1u32.to_be_bytes());
        stsd_body.extend_from_slice(&entry);
        let mut stsd = Vec::new();
        push_full_box(&mut stsd, b"stsd", 0, 0, &stsd_body)?;

        let mut stts_body = Vec::with_capacity(12);
        stts_body.extend_from_slice(&1u32.to_be_bytes());
        stts_body.extend_from_slice(&(n_frames as u32).to_be_bytes());
        stts_body.extend_from_slice(&scale.to_be_bytes());
        let mut stts = Vec::new();
        push_full_box(&mut stts, b"stts", 0, 0, &stts_body)?;

        let mut stsc_body = Vec::with_capacity(16);
        stsc_body.extend_from_slice(&1u32.to_be_bytes());
        stsc_body.extend_from_slice(&1u32.to_be_bytes());
        stsc_body.extend_from_slice(&1u32.to_be_bytes());
        stsc_body.extend_from_slice(&1u32.to_be_bytes());
        let mut stsc = Vec::new();
        push_full_box(&mut stsc, b"stsc", 0, 0, &stsc_body)?;

        let mut stsz_body = Vec::with_capacity(8 + self.video_sizes.len() * 4);
        stsz_body.extend_from_slice(&0u32.to_be_bytes()); // sample_size = 0 (tabela)
        stsz_body.extend_from_slice(&(self.video_sizes.len() as u32).to_be_bytes());
        for &sz in &self.video_sizes {
            stsz_body.extend_from_slice(&sz.to_be_bytes());
        }
        let mut stsz = Vec::new();
        push_full_box(&mut stsz, b"stsz", 0, 0, &stsz_body)?;

        let chunk_box = Self::build_chunk_offset_box(&self.video_offsets)?;

        let mut stbl_inner = Vec::new();
        stbl_inner.extend_from_slice(&stsd);
        stbl_inner.extend_from_slice(&stts);
        stbl_inner.extend_from_slice(&stsc);
        stbl_inner.extend_from_slice(&stsz);
        stbl_inner.extend_from_slice(&chunk_box);
        let mut stbl = Vec::new();
        push_box(&mut stbl, b"stbl", &stbl_inner)?;

        let mut vmhd = Vec::new();
        push_full_box(&mut vmhd, b"vmhd", 0, 1, &[0u8; 8])?;

        let mut minf_inner = Vec::new();
        minf_inner.extend_from_slice(&vmhd);
        minf_inner.extend_from_slice(&Self::build_dinf()?);
        minf_inner.extend_from_slice(&stbl);
        let mut minf = Vec::new();
        push_box(&mut minf, b"minf", &minf_inner)?;

        let mut mdia_inner = Vec::new();
        mdia_inner.extend_from_slice(&Self::build_mdhd(rate, media_dur)?);
        mdia_inner.extend_from_slice(&Self::build_hdlr(b"vide", b"Video")?);
        mdia_inner.extend_from_slice(&minf);
        let mut mdia = Vec::new();
        push_box(&mut mdia, b"mdia", &mdia_inner)?;

        let mut trak_inner = Vec::new();
        trak_inner.extend_from_slice(&Self::build_tkhd(1, movie_dur, 0, self.width, self.height)?);
        trak_inner.extend_from_slice(&mdia);
        let mut trak = Vec::new();
        push_box(&mut trak, b"trak", &trak_inner)?;
        Ok(trak)
    }

    fn build_audio_trak(&self, sr: u32, movie_dur: u64) -> Result<Vec<u8>> {
        // Sound Sample Entry v0 `sowt` PCM 16-bit LE estéreo (36 bytes)
        let mut entry = Vec::with_capacity(36);
        entry.extend_from_slice(&36u32.to_be_bytes());
        entry.extend_from_slice(b"sowt");
        entry.extend_from_slice(&[0u8; 6]);
        entry.extend_from_slice(&1u16.to_be_bytes()); // data_reference_index
        entry.extend_from_slice(&0u16.to_be_bytes()); // version = 0
        entry.extend_from_slice(&0u16.to_be_bytes()); // revision
        entry.extend_from_slice(&0u32.to_be_bytes()); // vendor
        entry.extend_from_slice(&2u16.to_be_bytes()); // channelcount
        entry.extend_from_slice(&16u16.to_be_bytes()); // samplesize
        entry.extend_from_slice(&0u16.to_be_bytes()); // compression_id
        entry.extend_from_slice(&0u16.to_be_bytes()); // packet_size
        entry.extend_from_slice(&(sr.saturating_mul(1 << 16)).to_be_bytes()); // 16.16 fixed

        let mut stsd_body = Vec::with_capacity(4 + entry.len());
        stsd_body.extend_from_slice(&1u32.to_be_bytes());
        stsd_body.extend_from_slice(&entry);
        let mut stsd = Vec::new();
        push_full_box(&mut stsd, b"stsd", 0, 0, &stsd_body)?;

        let total_frames = u32::try_from(self.total_audio_frames)
            .map_err(|_| anyhow!("faixa de áudio MOV excede 2^32 amostras"))?;
        let mut stts_body = Vec::with_capacity(12);
        stts_body.extend_from_slice(&1u32.to_be_bytes());
        stts_body.extend_from_slice(&total_frames.to_be_bytes());
        stts_body.extend_from_slice(&1u32.to_be_bytes());
        let mut stts = Vec::new();
        push_full_box(&mut stts, b"stts", 0, 0, &stts_body)?;

        // Run-length `stsc`: colapsa chunks consecutivos com mesma contagem de amostras
        let mut runs: Vec<(u32, u32)> = Vec::new();
        for (idx, chunk) in self.audio_chunks.iter().enumerate() {
            let chunk_num = (idx + 1) as u32;
            if runs.last().is_some_and(|(_, cnt)| *cnt == chunk.frames) {
                continue;
            }
            runs.push((chunk_num, chunk.frames));
        }
        let mut stsc_body = Vec::with_capacity(4 + runs.len() * 12);
        stsc_body.extend_from_slice(&(runs.len() as u32).to_be_bytes());
        for (first_chunk, spc) in runs {
            stsc_body.extend_from_slice(&first_chunk.to_be_bytes());
            stsc_body.extend_from_slice(&spc.to_be_bytes());
            stsc_body.extend_from_slice(&1u32.to_be_bytes());
        }
        let mut stsc = Vec::new();
        push_full_box(&mut stsc, b"stsc", 0, 0, &stsc_body)?;

        let mut stsz_body = Vec::with_capacity(8);
        stsz_body.extend_from_slice(&4u32.to_be_bytes()); // constante: 4 bytes por quadro PCM 16b estéreo
        stsz_body.extend_from_slice(&total_frames.to_be_bytes());
        let mut stsz = Vec::new();
        push_full_box(&mut stsz, b"stsz", 0, 0, &stsz_body)?;

        let offsets: Vec<u64> = self.audio_chunks.iter().map(|c| c.offset).collect();
        let chunk_box = Self::build_chunk_offset_box(&offsets)?;

        let mut stbl_inner = Vec::new();
        stbl_inner.extend_from_slice(&stsd);
        stbl_inner.extend_from_slice(&stts);
        stbl_inner.extend_from_slice(&stsc);
        stbl_inner.extend_from_slice(&stsz);
        stbl_inner.extend_from_slice(&chunk_box);
        let mut stbl = Vec::new();
        push_box(&mut stbl, b"stbl", &stbl_inner)?;

        let mut smhd = Vec::new();
        push_full_box(&mut smhd, b"smhd", 0, 0, &[0u8; 4])?;

        let mut minf_inner = Vec::new();
        minf_inner.extend_from_slice(&smhd);
        minf_inner.extend_from_slice(&Self::build_dinf()?);
        minf_inner.extend_from_slice(&stbl);
        let mut minf = Vec::new();
        push_box(&mut minf, b"minf", &minf_inner)?;

        let mut mdia_inner = Vec::new();
        mdia_inner.extend_from_slice(&Self::build_mdhd(sr, self.total_audio_frames)?);
        mdia_inner.extend_from_slice(&Self::build_hdlr(b"soun", b"Sound")?);
        mdia_inner.extend_from_slice(&minf);
        let mut mdia = Vec::new();
        push_box(&mut mdia, b"mdia", &mdia_inner)?;

        let mut trak_inner = Vec::new();
        trak_inner.extend_from_slice(&Self::build_tkhd(2, movie_dur, 0x0100, 0, 0)?);
        trak_inner.extend_from_slice(&mdia);
        let mut trak = Vec::new();
        push_box(&mut trak, b"trak", &trak_inner)?;
        Ok(trak)
    }

    pub fn finish(mut self) -> Result<()> {
        if self.video_offsets.is_empty() {
            bail!("MOV sem quadros de vídeo gravados");
        }
        let mdat_end = self.pos;
        let mdat_total = mdat_end.saturating_sub(self.mdat_header_pos);

        let movie_ts = self.fps.num.max(1);
        let movie_dur =
            (self.video_offsets.len() as u64).saturating_mul(u64::from(self.fps.den.max(1)));
        let has_audio_track = self.audio_sr.is_some() && !self.audio_chunks.is_empty();
        let next_track_id = if has_audio_track { 3u32 } else { 2u32 };

        let mvhd_ver = if movie_dur > u64::from(u32::MAX) {
            1u8
        } else {
            0u8
        };
        let mut mvhd_body = Vec::new();
        if mvhd_ver == 1 {
            mvhd_body.extend_from_slice(&0u64.to_be_bytes());
            mvhd_body.extend_from_slice(&0u64.to_be_bytes());
            mvhd_body.extend_from_slice(&movie_ts.to_be_bytes());
            mvhd_body.extend_from_slice(&movie_dur.to_be_bytes());
        } else {
            mvhd_body.extend_from_slice(&0u32.to_be_bytes());
            mvhd_body.extend_from_slice(&0u32.to_be_bytes());
            mvhd_body.extend_from_slice(&movie_ts.to_be_bytes());
            mvhd_body.extend_from_slice(&(movie_dur as u32).to_be_bytes());
        }
        mvhd_body.extend_from_slice(&0x0001_0000u32.to_be_bytes()); // rate 1.0
        mvhd_body.extend_from_slice(&0x0100u16.to_be_bytes()); // volume 1.0
        mvhd_body.extend_from_slice(&[0u8; 10]); // reserved
        mvhd_body.extend_from_slice(&identity_matrix());
        mvhd_body.extend_from_slice(&[0u8; 24]); // predefined
        mvhd_body.extend_from_slice(&next_track_id.to_be_bytes());
        let mut mvhd = Vec::new();
        push_full_box(&mut mvhd, b"mvhd", mvhd_ver, 0, &mvhd_body)?;

        let mut moov_inner = Vec::new();
        moov_inner.extend_from_slice(&mvhd);
        moov_inner.extend_from_slice(&self.build_video_trak(movie_ts, movie_dur)?);
        if let (Some(sr), true) = (self.audio_sr, has_audio_track) {
            moov_inner.extend_from_slice(&self.build_audio_trak(sr, movie_dur)?);
        }
        let mut moov = Vec::new();
        push_box(&mut moov, b"moov", &moov_inner)?;

        self.out.write_all(&moov)?;
        self.out.flush()?;
        let mut f = self
            .out
            .into_inner()
            .map_err(|e| anyhow!("falha ao concluir MOV: {e}"))?;
        f.seek(SeekFrom::Start(self.mdat_header_pos + 8))?;
        f.write_all(&mdat_total.to_be_bytes())?;
        f.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::demux;

    #[test]
    fn roundtrip_mov_all_four_pure_rust_codecs_and_ffmpeg_oracle() {
        let mut rgba = vec![0u8; 32 * 24 * 4];
        for y in 0..24u32 {
            for x in 0..32u32 {
                let i = ((y * 32 + x) * 4) as usize;
                rgba[i] = (x * 7) as u8;
                rgba[i + 1] = (y * 10) as u8;
                rgba[i + 2] = 180;
                rgba[i + 3] = 255;
            }
        }
        let img = image::RgbaImage::from_raw(32, 24, rgba.clone()).unwrap();

        for (tag, codec) in [
            ("bgra", MovVideoCodec::UncompressedBgra),
            ("2vuy", MovVideoCodec::UncompressedYuv422),
            ("jpeg", MovVideoCodec::PhotoJpeg),
            ("png", MovVideoCodec::Png),
        ] {
            let p = std::env::temp_dir().join(format!("neoveg_mov_{tag}.mov"));
            let mut w =
                MovWriter::create(&p, 32, 24, FrameRate::FPS_25, Some(48_000), codec).unwrap();
            let frame_bytes = match codec {
                MovVideoCodec::UncompressedBgra => MovWriter::rgba_to_bgra_topdown(&rgba, 32, 24),
                MovVideoCodec::UncompressedYuv422 => MovWriter::rgba_to_2vuy(&rgba, 32, 24),
                MovVideoCodec::PhotoJpeg => {
                    let mut jpg = Vec::new();
                    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg, 90)
                        .encode(
                            &img.as_raw()
                                .as_chunks::<4>()
                                .0
                                .iter()
                                .flat_map(|c| [c[0], c[1], c[2]])
                                .collect::<Vec<u8>>(),
                            32,
                            24,
                            image::ExtendedColorType::Rgb8,
                        )
                        .unwrap();
                    jpg
                }
                MovVideoCodec::Png => {
                    let mut png = Vec::new();
                    use image::ImageEncoder;
                    image::codecs::png::PngEncoder::new(&mut png)
                        .write_image(&rgba, 32, 24, image::ExtendedColorType::Rgba8)
                        .unwrap();
                    png
                }
            };
            for _ in 0..4 {
                w.write_video_frame(&frame_bytes).unwrap();
                w.write_audio(&vec![500i16; 48_000 / 25 * 2]).unwrap();
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
                        "ffmpeg erro em MOV {tag}: {}",
                        String::from_utf8_lossy(&out.stderr)
                    );
                }
            }
            let _ = std::fs::remove_file(&p);
        }
    }
}
