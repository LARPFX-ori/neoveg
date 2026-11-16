//! Demuxer de contêineres MP4/MOV e MKV/WEBM em Rust puro (`#[forbid(unsafe_code)]`).
//!
//! Implementado em Sala Limpa (`AGENTS.md` §2.3) a partir das especificações
//! públicas:
//! - ISO/IEC 14496-12 (*ISO Base Media File Format*) e Apple *QuickTime File Format*;
//! - EBML (RFC 8794) e Matroska / WebM (RFC 9559).
//!
//! Extrai metadados das trilhas (codec, dados de inicialização como
//! `avcC`/`hvcC`/`esds`/`CodecPrivate`, dimensões, taxa de quadros, taxa de
//! amostragem, canais) e o índice de amostras (`offset`, `size`, `pts`, `dts`,
//! `duration`, `is_keyframe`) em `Ticks` exatos sem carregar `mdat` ou payloads
//! em memória (§0.4, §3.6).

use crate::time::{FrameRate, Ticks, TICKS_PER_SEC};
use anyhow::{bail, Context, Result};
use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek, SeekFrom};
use std::path::Path;

pub mod mkv;
pub mod mp4;

/// Profundidade máxima de caixas / elementos aninhados (§0.4, §0.5).
pub(crate) const MAX_CONTAINER_DEPTH: usize = 32;
/// Máximo de trilhas por contêiner.
pub(crate) const MAX_TRACKS: usize = 64;
/// Máximo total de amostras indexadas por arquivo.
pub(crate) const MAX_SAMPLES: usize = 2_000_000;
/// Maior tabela ou bloco de metadados lido inteiro para RAM (64 MiB).
pub(crate) const MAX_METADATA_READ: usize = 64 * 1024 * 1024;
/// Maior extradata de codec (`avcC`/`hvcC`/`esds`/`CodecPrivate`).
pub(crate) const MAX_CODEC_EXTRA: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContainerKind {
    Mp4,
    Mov,
    Mkv,
    WebM,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackKind {
    Video,
    Audio,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DemuxTrack {
    pub id: u32,
    pub kind: TrackKind,
    pub codec: String,
    pub extra: Vec<u8>,
    pub timescale: u64,
    pub duration: Option<Ticks>,
    pub width: u32,
    pub height: u32,
    pub fps: Option<FrameRate>,
    pub sample_rate: u32,
    pub channels: u16,
    pub codec_delay: Ticks,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DemuxSample {
    pub track_index: usize,
    /// Offset absoluto em bytes do início do pacote/payload comprimido no arquivo.
    pub offset: u64,
    /// Tamanho em bytes do pacote/payload comprimido.
    pub size: u32,
    pub pts: Ticks,
    pub dts: Ticks,
    pub duration: Ticks,
    pub is_keyframe: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DemuxIndex {
    pub container: ContainerKind,
    pub duration: Option<Ticks>,
    pub tracks: Vec<DemuxTrack>,
    pub samples: Vec<DemuxSample>,
}

impl DemuxIndex {
    pub fn samples_for_track(&self, track_index: usize) -> impl Iterator<Item = &DemuxSample> {
        self.samples
            .iter()
            .filter(move |s| s.track_index == track_index)
    }
}

/// Conversão racional exata `value * TICKS_PER_SEC * unit_num / unit_den` em inteiros `i128`
/// (`AGENTS.md` Regra Rust 5; retorna `None` se `unit_den == 0` ou fora de `i64`).
pub(crate) fn scale_to_ticks(value: i128, unit_num: u64, unit_den: u64) -> Option<Ticks> {
    if unit_den == 0 {
        return None;
    }
    let mul = i128::from(TICKS_PER_SEC).checked_mul(i128::from(unit_num))?;
    let numer = value.checked_mul(mul)?;
    let den = i128::from(unit_den);
    let q = if numer >= 0 {
        numer.checked_add(den / 2)? / den
    } else {
        numer.checked_sub(den / 2)? / den
    };
    i64::try_from(q).ok().map(Ticks)
}

pub(crate) fn gcd_u64(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let r = a % b;
        a = b;
        b = r;
    }
    a.max(1)
}

/// Constrói `FrameRate` racional reduzido para `num / den`. Retorna `None` para valores nulos.
pub(crate) fn rational_fps(num: u64, den: u64) -> Option<FrameRate> {
    if num == 0 || den == 0 {
        return None;
    }
    let g = gcd_u64(num, den);
    let n = u32::try_from(num / g).ok().filter(|&v| v > 0)?;
    let d = u32::try_from(den / g).ok().filter(|&v| v > 0)?;
    FrameRate::new(n, d)
}

pub(crate) fn read_exact_at<R: Read + Seek>(r: &mut R, offset: u64, dst: &mut [u8]) -> Result<()> {
    r.seek(SeekFrom::Start(offset))
        .with_context(|| format!("seek para offset {offset}"))?;
    r.read_exact(dst)
        .with_context(|| format!("leitura de {} bytes em {offset}", dst.len()))
}

/// Lê e indexa um arquivo de mídia MP4/MOV/M4V/M4A ou MKV/WEBM a partir do caminho.
pub fn demux_file(path: &Path) -> Result<DemuxIndex> {
    let f = File::open(path).with_context(|| format!("abrindo {}", path.display()))?;
    let len = f.metadata()?.len();
    let mut br = BufReader::new(f);
    demux_reader(&mut br, len)
}

/// Lê e indexa um contêiner a partir de bytes em memória (ideal para testes e fuzzing).
pub fn demux_bytes(data: &[u8]) -> Result<DemuxIndex> {
    let mut cur = Cursor::new(data);
    demux_reader(&mut cur, data.len() as u64)
}

/// Detecta automaticamente o contêiner (EBML vs ISO-BMFF) e executa o demuxer
/// limitado sob proteção contra pânico (§0.6).
pub fn demux_reader<R: Read + Seek>(r: &mut R, len: u64) -> Result<DemuxIndex> {
    if len < 8 {
        bail!("contêiner curto demais ({len} bytes)");
    }
    let mut head = [0u8; 8];
    read_exact_at(r, 0, &mut head)?;
    r.seek(SeekFrom::Start(0))?;
    if head.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        mkv::parse_mkv(r, len)
    } else {
        mp4::parse_mp4(r, len)
    }
}

#[cfg(test)]
mod tests;
