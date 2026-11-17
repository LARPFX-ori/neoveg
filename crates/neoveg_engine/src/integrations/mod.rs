//! Integração em Sala Limpa com a suíte ArtCraft (PhotoCraft, VectorCraft,
//! FilmCraft, EffectCraft, AniCraft, SoundCraft) e com o Blender (§2.3).
//!
//! Toda comunicação ocorre fora do processo via binários CLI, troca de
//! arquivos abertos (WAV/BWF, PNG/EXR, SVG, OTIO, FCPXML, EDL, AAF, JSON) e
//! canal TCP loopback (`127.0.0.1`) documentado publicamente, sem leitura ou
//! vinculação de código proprietário ou GPL/LGPL (§0 / §2.3).

#[forbid(unsafe_code)]
pub mod blender;
#[forbid(unsafe_code)]
pub mod craft_rpc;
#[forbid(unsafe_code)]
pub mod discovery;
#[forbid(unsafe_code)]
pub mod workflows;

use serde::{Deserialize, Serialize};

/// Aplicativos externos suportados pela camada de integrações do Neoveg.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExternalApp {
    PhotoCraft,
    VectorCraft,
    FilmCraft,
    EffectCraft,
    AniCraft,
    SoundCraft,
    Blender,
}

/// Protocolo de controle em loopback usado pelo aplicativo quando em execução.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ControlProtocolKind {
    /// Desafio HMAC-SHA256 + JSON-lines em `127.0.0.1` (`agent-connection.json`).
    PhotoCraftChallengeJsonLines,
    /// JSON-lines em `127.0.0.1` sem autenticação obrigatória (`--control`).
    PlainJsonLines,
    /// JSON-lines com campo `token` por requisição (`FILMCRAFT_CONTROL_TOKEN`).
    TokenPerRequestJsonLines,
    /// Servidor MCP stdio (`anicraft-cli mcp`) e CLI subcommands (`render-seq`, `new`).
    McpStdioAndCli,
    /// Invocação em segundo plano via `--background --python` (`bpy` API pública).
    BlenderPythonCli,
}

impl ExternalApp {
    pub const ALL: [ExternalApp; 7] = [
        ExternalApp::PhotoCraft,
        ExternalApp::VectorCraft,
        ExternalApp::FilmCraft,
        ExternalApp::EffectCraft,
        ExternalApp::AniCraft,
        ExternalApp::SoundCraft,
        ExternalApp::Blender,
    ];

    pub const fn id(self) -> &'static str {
        match self {
            ExternalApp::PhotoCraft => "photocraft",
            ExternalApp::VectorCraft => "vectorcraft",
            ExternalApp::FilmCraft => "filmcraft",
            ExternalApp::EffectCraft => "effectcraft",
            ExternalApp::AniCraft => "anicraft",
            ExternalApp::SoundCraft => "soundcraft",
            ExternalApp::Blender => "blender",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match id.trim().to_ascii_lowercase().as_str() {
            "photocraft" => Some(ExternalApp::PhotoCraft),
            "vectorcraft" => Some(ExternalApp::VectorCraft),
            "filmcraft" => Some(ExternalApp::FilmCraft),
            "effectcraft" => Some(ExternalApp::EffectCraft),
            "anicraft" => Some(ExternalApp::AniCraft),
            "soundcraft" => Some(ExternalApp::SoundCraft),
            "blender" => Some(ExternalApp::Blender),
            _ => None,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            ExternalApp::PhotoCraft => "PhotoCraft",
            ExternalApp::VectorCraft => "VectorCraft",
            ExternalApp::FilmCraft => "FilmCraft",
            ExternalApp::EffectCraft => "EffectCraft",
            ExternalApp::AniCraft => "AniCraft",
            ExternalApp::SoundCraft => "SoundCraft",
            ExternalApp::Blender => "Blender",
        }
    }

    pub const fn category_pt(self) -> &'static str {
        match self {
            ExternalApp::PhotoCraft => "Imagem raster / PSD / EXR",
            ExternalApp::VectorCraft => "Ilustração vetorial / SVG / PDF",
            ExternalApp::FilmCraft => "Edição não-linear (NLE) / OTIO",
            ExternalApp::EffectCraft => "Composição / Motion / VFX",
            ExternalApp::AniCraft => "Animação 2D vetorial",
            ExternalApp::SoundCraft => "Estação de áudio (DAW / BWF)",
            ExternalApp::Blender => "Suíte 3D / Render e Câmera",
        }
    }

    pub const fn workflow_hint_pt(self) -> &'static str {
        match self {
            ExternalApp::PhotoCraft => {
                "Envia quadro atual ou cópia da imagem para edição em camadas (PSD/PNG/EXR) e recarrega automaticamente."
            }
            ExternalApp::VectorCraft => {
                "Exporta placa/vetor SVG editável no tamanho do projeto e rasteriza ou atualiza take ao salvar."
            }
            ExternalApp::FilmCraft => {
                "Intercâmbio completo de timeline via OpenTimelineIO (.otio), FCPXML, EDL e AAF."
            }
            ExternalApp::EffectCraft => {
                "Envia vídeo/imagem ou projeto para composição/VFX e importa sequências PNG/EXR ou vídeo renderizado."
            }
            ExternalApp::AniCraft => {
                "Cria cenas .anicraft no FPS/resolução do projeto ou renderiza sequências PNG diretamente na timeline."
            }
            ExternalApp::SoundCraft => {
                "Exporta clipes ou mix com timecode BWF (chunk bext) para o SoundCraft e sincroniza a volta como take."
            }
            ExternalApp::Blender => {
                "Renderiza cenas .blend em PNG para a timeline, exporta marcadores/Pan-Crop (.py) e importa câmera 3D (.json)."
            }
        }
    }

    pub const fn control_protocol(self) -> ControlProtocolKind {
        match self {
            ExternalApp::PhotoCraft => ControlProtocolKind::PhotoCraftChallengeJsonLines,
            ExternalApp::VectorCraft | ExternalApp::EffectCraft | ExternalApp::SoundCraft => {
                ControlProtocolKind::PlainJsonLines
            }
            ExternalApp::FilmCraft => ControlProtocolKind::TokenPerRequestJsonLines,
            ExternalApp::AniCraft => ControlProtocolKind::McpStdioAndCli,
            ExternalApp::Blender => ControlProtocolKind::BlenderPythonCli,
        }
    }

    pub const fn supports_loopback_rpc(self) -> bool {
        matches!(
            self,
            ExternalApp::PhotoCraft
                | ExternalApp::VectorCraft
                | ExternalApp::FilmCraft
                | ExternalApp::EffectCraft
                | ExternalApp::SoundCraft
        )
    }

    pub const fn is_artcraft(self) -> bool {
        !matches!(self, ExternalApp::Blender)
    }

    pub const fn native_project_ext(self) -> &'static str {
        match self {
            ExternalApp::PhotoCraft => "pcraft",
            ExternalApp::VectorCraft => "vectorcraft",
            ExternalApp::FilmCraft => "fcproj",
            ExternalApp::EffectCraft => "ecproj",
            ExternalApp::AniCraft => "anicraft",
            ExternalApp::SoundCraft => "scraft",
            ExternalApp::Blender => "blend",
        }
    }

    pub const fn desktop_binary_stems(self) -> &'static [&'static str] {
        match self {
            ExternalApp::PhotoCraft => &["photocraft"],
            ExternalApp::VectorCraft => &["vectorcraft"],
            ExternalApp::FilmCraft => &["filmcraft"],
            ExternalApp::EffectCraft => &["effectcraft"],
            ExternalApp::AniCraft => &["anicraft"],
            ExternalApp::SoundCraft => &["soundcraft"],
            ExternalApp::Blender => &["blender"],
        }
    }

    pub const fn cli_binary_stems(self) -> &'static [&'static str] {
        match self {
            ExternalApp::PhotoCraft => &["photocraft-cli"],
            ExternalApp::VectorCraft => &["vectorcraft-cli"],
            ExternalApp::FilmCraft => &["filmcraft-cli"],
            ExternalApp::EffectCraft => &["effectcraft-cli"],
            ExternalApp::AniCraft => &["anicraft-cli"],
            ExternalApp::SoundCraft => &["soundcraft-cli"],
            ExternalApp::Blender => &["blender"],
        }
    }

    pub const fn app_folder_name(self) -> &'static str {
        self.name()
    }

    pub const fn mac_bundle_name(self) -> &'static str {
        match self {
            ExternalApp::PhotoCraft => "PhotoCraft.app",
            ExternalApp::VectorCraft => "VectorCraft.app",
            ExternalApp::FilmCraft => "FilmCraft.app",
            ExternalApp::EffectCraft => "EffectCraft.app",
            ExternalApp::AniCraft => "AniCraft.app",
            ExternalApp::SoundCraft => "SoundCraft.app",
            ExternalApp::Blender => "Blender.app",
        }
    }

    pub const fn can_edit_video_or_still(self) -> bool {
        matches!(
            self,
            ExternalApp::PhotoCraft
                | ExternalApp::VectorCraft
                | ExternalApp::EffectCraft
                | ExternalApp::AniCraft
                | ExternalApp::FilmCraft
                | ExternalApp::Blender
        )
    }

    pub const fn can_edit_audio(self) -> bool {
        matches!(
            self,
            ExternalApp::SoundCraft
                | ExternalApp::FilmCraft
                | ExternalApp::EffectCraft
                | ExternalApp::Blender
        )
    }
}
