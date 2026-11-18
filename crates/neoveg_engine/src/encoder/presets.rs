//! Navegador e catálogo de predefinições do Neoveg Encoder:
//! predefinições do sistema por categoria, predefinições personalizadas `.neovegpreset`,
//! grupos de múltiplas saídas e apelidos (aliases) vinculados à definição original.

use super::effects::{PreEncodeEffects, SourceScalingMode};
use crate::model::FrameRate;
use crate::render::{RenderFormat, WavBits};
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

const MAX_PRESET_BYTES: u64 = 1 << 20;
const PRESET_MAGIC: &str = "neovegpreset";
const PRESET_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PresetCategory {
    Web,
    Broadcast,
    MobileAndGif,
    ImageSequence,
    Audio,
    Custom,
}

impl PresetCategory {
    pub const ALL: [Self; 6] = [
        Self::Web,
        Self::Broadcast,
        Self::MobileAndGif,
        Self::ImageSequence,
        Self::Audio,
        Self::Custom,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Web => "Web e Redes Sociais",
            Self::Broadcast => "Transmissão e Produção",
            Self::MobileAndGif => "Dispositivos e GIF",
            Self::ImageSequence => "Sequências de Imagem",
            Self::Audio => "Áudio",
            Self::Custom => "Predefinições do Usuário",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EncoderPreset {
    pub id: String,
    pub name: String,
    pub category: PresetCategory,
    #[serde(default)]
    pub system: bool,
    pub format: RenderFormat,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    #[serde(default)]
    pub fps: Option<FrameRate>,
    #[serde(default = "yes")]
    pub include_video: bool,
    #[serde(default = "yes")]
    pub include_audio: bool,
    #[serde(default = "default_vkbps")]
    pub video_bitrate_kbps: u32,
    #[serde(default = "default_akbps")]
    pub audio_bitrate_kbps: u32,
    #[serde(default = "default_jq")]
    pub jpeg_quality: u8,
    #[serde(default = "default_wb")]
    pub wav_bits: WavBits,
    #[serde(default)]
    pub scaling: SourceScalingMode,
    #[serde(default)]
    pub effects: PreEncodeEffects,
    #[serde(default)]
    pub notes: String,
}

fn yes() -> bool {
    true
}
fn default_vkbps() -> u32 {
    12_000
}
fn default_akbps() -> u32 {
    192
}
fn default_jq() -> u8 {
    90
}
fn default_wb() -> WavBits {
    WavBits::Pcm16
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresetGroup {
    pub id: String,
    pub name: String,
    pub preset_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresetAlias {
    pub id: String,
    pub name: String,
    pub target_preset_id: String,
    #[serde(default)]
    pub group_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PresetFile {
    pub magic: String,
    pub version: u32,
    pub preset: EncoderPreset,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct UserPresetStore {
    #[serde(default)]
    user_presets: Vec<EncoderPreset>,
    #[serde(default)]
    groups: Vec<PresetGroup>,
    #[serde(default)]
    aliases: Vec<PresetAlias>,
}

#[derive(Clone, Debug)]
pub struct PresetCatalog {
    pub system: Vec<EncoderPreset>,
    pub user: Vec<EncoderPreset>,
    pub groups: Vec<PresetGroup>,
    pub aliases: Vec<PresetAlias>,
}

impl Default for PresetCatalog {
    fn default() -> Self {
        Self {
            system: system_presets(),
            user: Vec::new(),
            groups: default_system_groups(),
            aliases: Vec::new(),
        }
    }
}

impl PresetCatalog {
    pub fn load(store_path: &Path, presets_dir: &Path) -> Self {
        let mut cat = Self::default();
        if let Ok(meta) = std::fs::metadata(store_path) {
            if meta.len() <= MAX_PRESET_BYTES {
                if let Ok(txt) = std::fs::read_to_string(store_path) {
                    if let Ok(store) = serde_json::from_str::<UserPresetStore>(&txt) {
                        cat.user = store.user_presets;
                        for p in &mut cat.user {
                            p.system = false;
                            p.category = PresetCategory::Custom;
                        }
                        for g in store.groups {
                            if !cat.groups.iter().any(|eg| eg.id == g.id) {
                                cat.groups.push(g);
                            } else if let Some(slot) =
                                cat.groups.iter_mut().find(|eg| eg.id == g.id)
                            {
                                *slot = g;
                            }
                        }
                        cat.aliases = store.aliases;
                    }
                }
            }
        }
        // Também importa `.neovegpreset` soltos na pasta de predefinições do usuário.
        if let Ok(rd) = std::fs::read_dir(presets_dir) {
            for ent in rd.flatten().take(200) {
                let p = ent.path();
                if p.extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.eq_ignore_ascii_case("neovegpreset"))
                    .unwrap_or(false)
                {
                    if let Ok(mut pr) = import_preset_file(&p) {
                        pr.system = false;
                        pr.category = PresetCategory::Custom;
                        if !cat.user.iter().any(|u| u.id == pr.id) {
                            cat.user.push(pr);
                        }
                    }
                }
            }
        }
        cat
    }

    pub fn save(&self, store_path: &Path) -> Result<()> {
        if let Some(parent) = store_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let store = UserPresetStore {
            user_presets: self.user.clone(),
            groups: self.groups.clone(),
            aliases: self.aliases.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&store)?;
        let tmp = store_path.with_extension("json.tmp");
        std::fs::write(&tmp, &bytes)?;
        std::fs::rename(&tmp, store_path)?;
        Ok(())
    }

    pub fn all_presets(&self) -> impl Iterator<Item = &EncoderPreset> {
        self.system.iter().chain(self.user.iter())
    }

    /// Resolve predefinição por ID ou por ID de apelido (limite contra ciclos).
    pub fn resolve(&self, id: &str) -> Option<&EncoderPreset> {
        let mut cur = id;
        let mut seen = HashSet::new();
        for _ in 0..8 {
            if !seen.insert(cur) {
                return None;
            }
            if let Some(p) = self.system.iter().chain(&self.user).find(|p| p.id == cur) {
                return Some(p);
            }
            let alias = self.aliases.iter().find(|a| a.id == cur)?;
            cur = &alias.target_preset_id;
        }
        None
    }

    /// Resolve um grupo em lista de predefinições concretas (ou uma única predefinição).
    pub fn resolve_group_or_preset(&self, id: &str) -> Vec<EncoderPreset> {
        if let Some(g) = self.groups.iter().find(|g| g.id == id) {
            return g
                .preset_ids
                .iter()
                .filter_map(|pid| self.resolve(pid).cloned())
                .collect();
        }
        self.resolve(id).cloned().into_iter().collect()
    }

    pub fn search(&self, query: &str) -> Vec<&EncoderPreset> {
        let q = query.trim().to_ascii_lowercase();
        self.all_presets()
            .filter(|p| {
                q.is_empty()
                    || p.name.to_ascii_lowercase().contains(&q)
                    || p.category.name().to_ascii_lowercase().contains(&q)
                    || p.format.name().to_ascii_lowercase().contains(&q)
                    || p.notes.to_ascii_lowercase().contains(&q)
            })
            .collect()
    }

    pub fn save_custom(&mut self, mut preset: EncoderPreset) -> String {
        if preset.id.trim().is_empty() || preset.system {
            preset.id = format!("usr_{}", crate::model::new_id());
        }
        preset.system = false;
        preset.category = PresetCategory::Custom;
        let id = preset.id.clone();
        if let Some(existing) = self.user.iter_mut().find(|u| u.id == id) {
            *existing = preset;
        } else {
            self.user.push(preset);
        }
        id
    }

    pub fn rename_custom(&mut self, id: &str, new_name: &str) -> bool {
        let clean = new_name.trim();
        if clean.is_empty() {
            return false;
        }
        if let Some(p) = self.user.iter_mut().find(|p| p.id == id) {
            p.name = clean.to_string();
            true
        } else {
            false
        }
    }

    pub fn delete_custom(&mut self, id: &str) -> bool {
        let before = self.user.len();
        self.user.retain(|p| p.id != id);
        if self.user.len() != before {
            self.aliases.retain(|a| a.target_preset_id != id);
            for g in &mut self.groups {
                g.preset_ids.retain(|pid| pid != id);
            }
            return true;
        }
        false
    }

    pub fn create_group(&mut self, name: &str, preset_ids: Vec<String>) -> String {
        let id = format!("grp_{}", crate::model::new_id());
        self.groups.push(PresetGroup {
            id: id.clone(),
            name: name.trim().to_string(),
            preset_ids,
        });
        id
    }

    pub fn create_alias(
        &mut self,
        name: &str,
        target_preset_id: &str,
        group_id: Option<&str>,
    ) -> Result<String> {
        if self.resolve(target_preset_id).is_none() {
            bail!("predefinição de destino não encontrada: {target_preset_id}");
        }
        let id = format!("als_{}", crate::model::new_id());
        self.aliases.push(PresetAlias {
            id: id.clone(),
            name: name.trim().to_string(),
            target_preset_id: target_preset_id.to_string(),
            group_id: group_id.map(str::to_string),
        });
        if let Some(gid) = group_id {
            if let Some(g) = self.groups.iter_mut().find(|g| g.id == gid) {
                g.preset_ids.push(id.clone());
            }
        }
        Ok(id)
    }
}

pub fn export_preset_file(preset: &EncoderPreset, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let pf = PresetFile {
        magic: PRESET_MAGIC.to_string(),
        version: PRESET_VERSION,
        preset: preset.clone(),
    };
    let json = serde_json::to_vec_pretty(&pf)?;
    let tmp = path.with_extension("neovegpreset.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

pub fn import_preset_file(path: &Path) -> Result<EncoderPreset> {
    let meta = std::fs::metadata(path)
        .with_context(|| format!("abrindo predefinição {}", path.display()))?;
    if meta.len() > MAX_PRESET_BYTES {
        bail!("arquivo .neovegpreset grande demais ({} bytes)", meta.len());
    }
    let text = std::fs::read_to_string(path)?;
    import_preset_str(&text)
}

pub fn import_preset_str(text: &str) -> Result<EncoderPreset> {
    if text.len() > MAX_PRESET_BYTES as usize {
        bail!("conteúdo .neovegpreset grande demais");
    }
    let mut pf: PresetFile =
        serde_json::from_str(text).map_err(|_| anyhow!("arquivo .neovegpreset inválido"))?;
    if pf.magic != PRESET_MAGIC || pf.version == 0 || pf.version > PRESET_VERSION {
        bail!("versão ou assinatura .neovegpreset incompatível");
    }
    if pf.preset.name.trim().is_empty() {
        pf.preset.name = "Predefinição Importada".to_string();
    }
    if pf.preset.id.trim().is_empty() {
        pf.preset.id = format!("usr_{}", crate::model::new_id());
    }
    pf.preset.system = false;
    pf.preset.category = PresetCategory::Custom;
    Ok(pf.preset)
}

struct SysSpec<'a>(
    &'a str,
    &'a str,
    PresetCategory,
    RenderFormat,
    Option<(u32, u32)>,
    Option<FrameRate>,
    u32,
    u32,
    WavBits,
    &'a str,
);

fn sys(s: SysSpec<'_>) -> EncoderPreset {
    EncoderPreset {
        id: s.0.to_string(),
        name: s.1.to_string(),
        category: s.2,
        system: true,
        format: s.3,
        width: s.4.map(|d| d.0),
        height: s.4.map(|d| d.1),
        fps: s.5,
        include_video: !s.3.is_audio_only(),
        include_audio: true,
        video_bitrate_kbps: s.6,
        audio_bitrate_kbps: s.7,
        jpeg_quality: 92,
        wav_bits: s.8,
        scaling: SourceScalingMode::ScaleToFit,
        effects: PreEncodeEffects::default(),
        notes: s.9.to_string(),
    }
}

pub fn system_presets() -> Vec<EncoderPreset> {
    vec![
        sys(SysSpec(
            "sys.web_1080p_hq",
            "Web 1080p Alta Qualidade (MP4 H.264)",
            PresetCategory::Web,
            RenderFormat::Mp4H264,
            Some((1920, 1080)),
            None,
            16_000,
            192,
            WavBits::Pcm16,
            "Vídeo Full HD 16:9 para publicação web com áudio AAC estéreo.",
        )),
        sys(SysSpec(
            "sys.web_720p_fast",
            "Web 720p Compacto (MP4 H.264)",
            PresetCategory::Web,
            RenderFormat::Mp4H264,
            Some((1280, 720)),
            None,
            8_000,
            160,
            WavBits::Pcm16,
            "Entrega leve em HD 720p para streaming e revisão rápida.",
        )),
        sys(SysSpec(
            "sys.web_vertical_9x16",
            "Redes Sociais Vertical 9:16 (1080x1920)",
            PresetCategory::Web,
            RenderFormat::Mp4H264,
            Some((1080, 1920)),
            Some(FrameRate::FPS_30),
            12_000,
            192,
            WavBits::Pcm16,
            "Formato vertical 1080x1920 @ 30 qps.",
        )),
        sys(SysSpec(
            "sys.web_4k_hevc",
            "Ultra HD 4K (MP4 HEVC / H.265)",
            PresetCategory::Web,
            RenderFormat::Mp4Hevc,
            Some((3840, 2160)),
            None,
            28_000,
            256,
            WavBits::Pcm16,
            "4K UHD em alta eficiência HEVC com áudio AAC 256 kbps.",
        )),
        sys(SysSpec(
            "sys.bc_match_mp4",
            "Corresponder à Origem (MP4 H.264 Alta Taxa)",
            PresetCategory::Broadcast,
            RenderFormat::Mp4H264,
            None,
            None,
            25_000,
            320,
            WavBits::Pcm16,
            "Mantém resolução e taxa da origem em MP4 H.264 25 Mbps.",
        )),
        sys(SysSpec(
            "sys.bc_mov_h264",
            "Produção QuickTime (MOV H.264)",
            PresetCategory::Broadcast,
            RenderFormat::MovH264,
            None,
            None,
            25_000,
            256,
            WavBits::Pcm16,
            "Contêiner MOV compatível com fluxos de pós-produção.",
        )),
        sys(SysSpec(
            "sys.bc_avi_mjpeg",
            "Master Intermediário AVI MJPEG",
            PresetCategory::Broadcast,
            RenderFormat::AviMjpeg,
            None,
            None,
            12_000,
            192,
            WavBits::Pcm16,
            "AVI OpenDML multiplataforma quadro-a-quadro.",
        )),
        sys(SysSpec(
            "sys.bc_avi_rgb24",
            "Sem Compressão AVI RGB24 + PCM",
            PresetCategory::Broadcast,
            RenderFormat::AviUncompressed,
            None,
            None,
            12_000,
            192,
            WavBits::Pcm16,
            "Saída RGB 24 bits sem perdas em AVI OpenDML.",
        )),
        sys(SysSpec(
            "sys.mob_3gp",
            "Móvel Compacto (3GP 480p)",
            PresetCategory::MobileAndGif,
            RenderFormat::ThreeGp,
            Some((854, 480)),
            Some(FrameRate::FPS_25),
            2_500,
            128,
            WavBits::Pcm16,
            "Formato portátil 3GPP para dispositivos móveis.",
        )),
        sys(SysSpec(
            "sys.mob_gif",
            "GIF Animado (480p @ 15 qps)",
            PresetCategory::MobileAndGif,
            RenderFormat::AnimatedGif,
            Some((854, 480)),
            Some(FrameRate { num: 15, den: 1 }),
            12_000,
            192,
            WavBits::Pcm16,
            "GIF animado em loop infinito sem faixa de áudio.",
        )),
        sys(SysSpec(
            "sys.seq_dpx",
            "Sequência Cinema DPX (SMPTE 268M)",
            PresetCategory::ImageSequence,
            RenderFormat::DpxSequence,
            None,
            None,
            12_000,
            192,
            WavBits::Pcm24,
            "Sequência DPX quadro a quadro + áudio WAV 24-bit auxiliar.",
        )),
        sys(SysSpec(
            "sys.seq_exr",
            "Sequência HDR OpenEXR",
            PresetCategory::ImageSequence,
            RenderFormat::ExrSequence,
            None,
            None,
            12_000,
            192,
            WavBits::Float32,
            "Sequência EXR de alta faixa dinâmica + WAV Float32 auxiliar.",
        )),
        sys(SysSpec(
            "sys.seq_png",
            "Sequência PNG Sem Perdas",
            PresetCategory::ImageSequence,
            RenderFormat::PngSequence,
            None,
            None,
            12_000,
            192,
            WavBits::Pcm16,
            "Quadros individuais PNG 8-bit.",
        )),
        sys(SysSpec(
            "sys.aud_bwf_24",
            "Broadcast Wave BWF 48 kHz 24-bit",
            PresetCategory::Audio,
            RenderFormat::Bwf,
            None,
            None,
            12_000,
            192,
            WavBits::Pcm24,
            "BWF EBU Tech 3285 para transmissão e pós de áudio.",
        )),
        sys(SysSpec(
            "sys.aud_rf64",
            "EBU RF64 64-bit Float32",
            PresetCategory::Audio,
            RenderFormat::Rf64,
            None,
            None,
            12_000,
            192,
            WavBits::Float32,
            "Áudio de alta duração >4 GB em Float32.",
        )),
        sys(SysSpec(
            "sys.aud_m4a_256",
            "AAC Estéreo 256 kbps (.m4a)",
            PresetCategory::Audio,
            RenderFormat::AacM4a,
            None,
            None,
            12_000,
            256,
            WavBits::Pcm16,
            "Exportação de áudio comprimido AAC em contêiner M4A.",
        )),
        sys(SysSpec(
            "sys.aud_wav_16",
            "PCM Linear WAV 16-bit",
            PresetCategory::Audio,
            RenderFormat::Wav,
            None,
            None,
            12_000,
            192,
            WavBits::Pcm16,
            "PCM padrão estéreo ou 5.1.",
        )),
        sys(SysSpec(
            "sys.aud_qoa",
            "QOA Comprimido (.qoa)",
            PresetCategory::Audio,
            RenderFormat::Qoa,
            None,
            None,
            12_000,
            192,
            WavBits::Pcm16,
            "Quite OK Audio v1 em Rust puro.",
        )),
        sys(SysSpec(
            "sys.aud_flac",
            "FLAC Sem Perdas (.flac)",
            PresetCategory::Audio,
            RenderFormat::Flac,
            None,
            None,
            12_000,
            192,
            WavBits::Pcm24,
            "Compressão sem perdas FLAC (RFC 9639) em 16/24 bits estéreo ou 5.1.",
        )),
        sys(SysSpec(
            "sys.aud_aac_adts",
            "Áudio AAC ADTS (.aac)",
            PresetCategory::Audio,
            RenderFormat::AacAdts,
            None,
            None,
            12_000,
            192,
            WavBits::Pcm16,
            "Fluxo AAC ADTS bruto via Media Foundation.",
        )),
        sys(SysSpec(
            "sys.aud_mp3",
            "MP3 Estéreo 192 kbps (.mp3)",
            PresetCategory::Audio,
            RenderFormat::Mp3,
            None,
            None,
            12_000,
            192,
            WavBits::Pcm16,
            "Áudio comprimido MP3 via Media Foundation.",
        )),
        sys(SysSpec(
            "sys.aud_wma",
            "Windows Media Audio (.wma)",
            PresetCategory::Audio,
            RenderFormat::Wma,
            None,
            None,
            12_000,
            192,
            WavBits::Pcm16,
            "Áudio WMA comprimido via Media Foundation.",
        )),
        sys(SysSpec(
            "sys.aud_w64",
            "Sony Wave64 24-bit (.w64)",
            PresetCategory::Audio,
            RenderFormat::Wave64,
            None,
            None,
            12_000,
            192,
            WavBits::Pcm24,
            "Contêiner Wave64 de 64 bits compatível com projetos VEGAS longos.",
        )),
        sys(SysSpec(
            "sys.aud_aiff",
            "Apple AIFF 24-bit (.aiff)",
            PresetCategory::Audio,
            RenderFormat::Aiff,
            None,
            None,
            12_000,
            192,
            WavBits::Pcm24,
            "Áudio PCM sem compressão big-endian em formato AIFF.",
        )),
        sys(SysSpec(
            "sys.aud_au",
            "Sun / NeXT AU PCM (.au)",
            PresetCategory::Audio,
            RenderFormat::Au,
            None,
            None,
            12_000,
            192,
            WavBits::Pcm16,
            "Áudio clássico Unix/NeXT big-endian estéreo ou 5.1.",
        )),
        sys(SysSpec(
            "sys.aud_caf",
            "Apple Core Audio Format (.caf)",
            PresetCategory::Audio,
            RenderFormat::Caf,
            None,
            None,
            12_000,
            192,
            WavBits::Float32,
            "Core Audio Format de 64 bits estéreo ou 5.1.",
        )),
        sys(SysSpec(
            "sys.seq_dpx_10bit",
            "Sequência Cinema DPX 10-bit (SMPTE 268M)",
            PresetCategory::ImageSequence,
            RenderFormat::Dpx10Sequence,
            None,
            None,
            12_000,
            192,
            WavBits::Pcm24,
            "Sequência DPX 10 bits Packing Method A + WAV 24-bit auxiliar.",
        )),
        sys(SysSpec(
            "sys.seq_cineon",
            "Sequência Kodak Cineon 10-bit (.cin)",
            PresetCategory::ImageSequence,
            RenderFormat::CineonSequence,
            None,
            None,
            12_000,
            192,
            WavBits::Pcm24,
            "Quadros Kodak Cineon v4.5 10-bit + WAV 24-bit auxiliar.",
        )),
        sys(SysSpec(
            "sys.seq_jpeg",
            "Sequência JPEG (.jpg)",
            PresetCategory::ImageSequence,
            RenderFormat::JpegSequence,
            None,
            None,
            12_000,
            192,
            WavBits::Pcm16,
            "Quadros JPEG enumerados + áudio WAV auxiliar.",
        )),
        sys(SysSpec(
            "sys.seq_tiff",
            "Sequência TIFF Sem Perdas (.tif)",
            PresetCategory::ImageSequence,
            RenderFormat::TiffSequence,
            None,
            None,
            12_000,
            192,
            WavBits::Pcm24,
            "Quadros TIFF sem perdas + WAV auxiliar.",
        )),
        sys(SysSpec(
            "sys.bc_y4m",
            "Master YUV4MPEG2 .y4m",
            PresetCategory::Broadcast,
            RenderFormat::Y4m,
            None,
            None,
            12_000,
            192,
            WavBits::Pcm24,
            "Fluxo bruto YUV4MPEG2 interoperável em Rust puro + WAV auxiliar.",
        )),
        sys(SysSpec(
            "sys.bc_avi_bgra",
            "Sem Compressão AVI BGRA 32-bit (+ PCM)",
            PresetCategory::Broadcast,
            RenderFormat::AviUncompressedBgra,
            None,
            None,
            12_000,
            192,
            WavBits::Pcm16,
            "Saída AVI BI_RGB de 32 bits em Rust puro com áudio PCM.",
        )),
        sys(SysSpec(
            "sys.bc_avi_yuv422",
            "Sem Compressão AVI YUV 4:2:2 UYVY (+ PCM)",
            PresetCategory::Broadcast,
            RenderFormat::AviUncompressedYuv422,
            None,
            None,
            12_000,
            192,
            WavBits::Pcm16,
            "Vídeo AVI 8 bits YUV 4:2:2 UYVY sem compressão.",
        )),
        sys(SysSpec(
            "sys.bc_avi_rgb10",
            "Sem Compressão AVI RGB 10-bit r210 (+ PCM)",
            PresetCategory::Broadcast,
            RenderFormat::AviUncompressedRgb10,
            None,
            None,
            12_000,
            192,
            WavBits::Pcm16,
            "Saída AVI 10 bits RGB (r210) de alta precisão.",
        )),
        sys(SysSpec(
            "sys.bc_mov_rgb",
            "QuickTime MOV Sem Compressão RGB / BGRA (+ PCM)",
            PresetCategory::Broadcast,
            RenderFormat::MovUncompressedRgb,
            None,
            None,
            12_000,
            192,
            WavBits::Pcm16,
            "Contêiner QuickTime puro com vídeo BGRA 32-bit + áudio PCM.",
        )),
        sys(SysSpec(
            "sys.bc_mov_yuv422",
            "QuickTime MOV Sem Compressão YUV 4:2:2 2vuy (+ PCM)",
            PresetCategory::Broadcast,
            RenderFormat::MovUncompressedYuv422,
            None,
            None,
            12_000,
            192,
            WavBits::Pcm16,
            "Contêiner QuickTime puro 2vuy 4:2:2 + áudio PCM.",
        )),
        sys(SysSpec(
            "sys.bc_mov_mjpeg",
            "QuickTime MOV Photo-JPEG (+ PCM)",
            PresetCategory::Broadcast,
            RenderFormat::MovMjpeg,
            None,
            None,
            12_000,
            192,
            WavBits::Pcm16,
            "Contêiner QuickTime Photo-JPEG quadro a quadro.",
        )),
        sys(SysSpec(
            "sys.bc_mov_png",
            "QuickTime MOV PNG com Alfa (+ PCM)",
            PresetCategory::Broadcast,
            RenderFormat::MovPng,
            None,
            None,
            12_000,
            192,
            WavBits::Pcm16,
            "Contêiner QuickTime sem perdas com transparência PNG.",
        )),
        sys(SysSpec(
            "sys.bc_mkv_ffv1",
            "Master Matroska MKV FFV1 Sem Perdas (+ PCM)",
            PresetCategory::Broadcast,
            RenderFormat::MkvFfv1,
            None,
            None,
            12_000,
            192,
            WavBits::Pcm16,
            "Arquivo de preservação sem perdas FFV1 v3 (RFC 9043) com alfa em MKV.",
        )),
        sys(SysSpec(
            "sys.bc_mkv_mjpeg",
            "Matroska MKV Motion-JPEG (+ PCM)",
            PresetCategory::Broadcast,
            RenderFormat::MkvMjpeg,
            None,
            None,
            12_000,
            192,
            WavBits::Pcm16,
            "Contêiner MKV em Rust puro com fluxo MJPEG.",
        )),
        sys(SysSpec(
            "sys.bc_mkv_png",
            "Matroska MKV PNG Sem Perdas (+ PCM)",
            PresetCategory::Broadcast,
            RenderFormat::MkvPng,
            None,
            None,
            12_000,
            192,
            WavBits::Pcm16,
            "Contêiner MKV em Rust puro com quadros PNG sem perdas.",
        )),
        sys(SysSpec(
            "sys.bc_mov_hevc",
            "QuickTime MOV HEVC / H.265 (+ AAC)",
            PresetCategory::Broadcast,
            RenderFormat::MovHevc,
            None,
            None,
            24_000,
            256,
            WavBits::Pcm16,
            "MOV HEVC via Media Foundation com áudio AAC.",
        )),
        sys(SysSpec(
            "sys.web_mp4_av1",
            "Web MP4 AV1 (+ AAC)",
            PresetCategory::Web,
            RenderFormat::Mp4Av1,
            None,
            None,
            10_000,
            192,
            WavBits::Pcm16,
            "MP4 com codificador de vídeo AV1 do sistema.",
        )),
        sys(SysSpec(
            "sys.bc_mov_av1",
            "QuickTime MOV AV1 (+ AAC)",
            PresetCategory::Broadcast,
            RenderFormat::MovAv1,
            None,
            None,
            14_000,
            256,
            WavBits::Pcm16,
            "MOV com codificador de vídeo AV1 via Media Foundation.",
        )),
        sys(SysSpec(
            "sys.bc_mov_mpeg4",
            "QuickTime MOV MPEG-4 Part 2 (+ AAC)",
            PresetCategory::Broadcast,
            RenderFormat::MovMpeg4,
            None,
            None,
            8_000,
            192,
            WavBits::Pcm16,
            "MOV legado compatível com MPEG-4 Part 2 via Media Foundation.",
        )),
        sys(SysSpec(
            "sys.web_mkv_h264",
            "Matroska MKV H.264 / AVC (+ AAC)",
            PresetCategory::Web,
            RenderFormat::MkvH264,
            None,
            None,
            16_000,
            192,
            WavBits::Pcm16,
            "Entrega MKV H.264 + AAC em remux nativo após codificação MF.",
        )),
        sys(SysSpec(
            "sys.web_mkv_hevc",
            "Matroska MKV HEVC / H.265 (+ AAC)",
            PresetCategory::Web,
            RenderFormat::MkvHevc,
            None,
            None,
            20_000,
            256,
            WavBits::Pcm16,
            "MKV em alta eficiência HEVC + AAC via Media Foundation.",
        )),
        sys(SysSpec(
            "sys.web_mkv_av1",
            "Matroska MKV AV1 (+ AAC)",
            PresetCategory::Web,
            RenderFormat::MkvAv1,
            None,
            None,
            10_000,
            192,
            WavBits::Pcm16,
            "MKV AV1 + AAC via Media Foundation.",
        )),
        sys(SysSpec(
            "sys.web_mkv_vp9",
            "Matroska MKV VP9 (+ AAC)",
            PresetCategory::Web,
            RenderFormat::MkvVp9,
            None,
            None,
            12_000,
            192,
            WavBits::Pcm16,
            "MKV VP9 + AAC via Media Foundation.",
        )),
        sys(SysSpec(
            "sys.web_wmv",
            "Windows Media Video (.wmv)",
            PresetCategory::Broadcast,
            RenderFormat::Wmv,
            Some((1920, 1080)),
            None,
            10_000,
            192,
            WavBits::Pcm16,
            "WMV WMV3 + WMA via Media Foundation.",
        )),
    ]
}

fn default_system_groups() -> Vec<PresetGroup> {
    vec![PresetGroup {
        id: "grp.web_multi".to_string(),
        name: "Pacote Web (MP4 1080p + MP4 720p + Áudio AAC)".to_string(),
        preset_ids: vec![
            "sys.web_1080p_hq".to_string(),
            "sys.web_720p_fast".to_string(),
            "sys.aud_m4a_256".to_string(),
        ],
    }]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn system_presets_ids_are_unique_and_cover_new_resolve_formats() {
        let presets = system_presets();
        let mut ids = HashSet::new();
        let mut formats = HashSet::new();
        for p in &presets {
            assert!(ids.insert(p.id.as_str()), "ID duplicado: {}", p.id);
            formats.insert(p.format);
        }
        for req in [
            RenderFormat::Flac,
            RenderFormat::Dpx10Sequence,
            RenderFormat::CineonSequence,
            RenderFormat::AviUncompressedYuv422,
            RenderFormat::AviUncompressedBgra,
            RenderFormat::AviUncompressedRgb10,
            RenderFormat::MovUncompressedRgb,
            RenderFormat::MovUncompressedYuv422,
            RenderFormat::MovMjpeg,
            RenderFormat::MovPng,
            RenderFormat::MkvFfv1,
            RenderFormat::MkvMjpeg,
            RenderFormat::MkvPng,
            RenderFormat::MovHevc,
            RenderFormat::Mp4Av1,
            RenderFormat::MovAv1,
            RenderFormat::MovMpeg4,
            RenderFormat::MkvH264,
            RenderFormat::MkvHevc,
            RenderFormat::MkvAv1,
            RenderFormat::MkvVp9,
            RenderFormat::AacAdts,
            RenderFormat::Mp3,
            RenderFormat::Y4m,
            RenderFormat::Wave64,
            RenderFormat::Aiff,
            RenderFormat::Au,
            RenderFormat::Caf,
            RenderFormat::JpegSequence,
            RenderFormat::TiffSequence,
        ] {
            assert!(
                formats.contains(&req),
                "RenderFormat ausente em system_presets(): {req:?}"
            );
        }
    }
}
