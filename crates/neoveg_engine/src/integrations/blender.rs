//! Integração em Sala Limpa com o Blender fora do processo (`AGENTS.md` §2.3).
//!
//! Nenhum código do Blender (GPL) é linkado ao Neoveg: toda integração ocorre
//! por invocação de subcomando (`blender -b ... --python ...`), inspeção
//! estruturada via JSON limitado e troca de sequências PNG/EXR, marcadores e
//! dados de câmera (`neoveg.blender.camera.v1`).

use crate::model::{
    new_id, Event, FrameRate, Marker, MotionKey, PanCropKey, Project, Ticks, Track,
};
use anyhow::{anyhow, bail, Context, Result};
use neoveg_common::util;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const MAX_CAMERA_JSON_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_CAMERA_FRAMES: usize = 100_000;
pub const MAX_CAMERA_MARKERS: usize = 10_000;
const BLENDER_PROBE_PREFIX: &str = "@neoveg-blender ";

/// Quadro individual de câmera exportada do Blender (ou para o Blender).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BlenderCameraFrame {
    pub frame: i64,
    pub pos: [f64; 3],
    #[serde(default)]
    pub rot_euler_deg: Option<[f64; 3]>,
    #[serde(default)]
    pub quat: Option<[f64; 4]>,
    #[serde(default)]
    pub lens_mm: Option<f64>,
}

/// Marcador de timeline na troca entre Blender e Neoveg.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlenderMarkerExchange {
    pub name: String,
    pub frame: i64,
}

/// Estrutura pública de intercâmbio de câmera/cena Blender (`neoveg.blender.camera.v1`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BlenderCameraExchange {
    #[serde(default = "default_schema_id")]
    pub schema: String,
    #[serde(default)]
    pub scene_name: String,
    #[serde(default)]
    pub camera_name: String,
    #[serde(default = "default_width")]
    pub width: u32,
    #[serde(default = "default_height")]
    pub height: u32,
    #[serde(default = "default_fps_num")]
    pub fps_num: u32,
    #[serde(default = "default_fps_den")]
    pub fps_den: u32,
    #[serde(default = "default_start_frame")]
    pub start_frame: i64,
    #[serde(default = "default_sensor_mm")]
    pub sensor_width_mm: f64,
    #[serde(default = "default_unit_scale")]
    pub unit_scale: f64,
    #[serde(default)]
    pub markers: Vec<BlenderMarkerExchange>,
    #[serde(default)]
    pub frames: Vec<BlenderCameraFrame>,
}

fn default_schema_id() -> String {
    "neoveg.blender.camera.v1".to_string()
}
fn default_width() -> u32 {
    1920
}
fn default_height() -> u32 {
    1080
}
fn default_fps_num() -> u32 {
    24
}
fn default_fps_den() -> u32 {
    1
}
fn default_start_frame() -> i64 {
    1
}
fn default_sensor_mm() -> f64 {
    36.0
}
fn default_unit_scale() -> f64 {
    0.01
}

/// Metadados sondados de um arquivo `.blend` em modo headless.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlendSceneInfo {
    pub active_scene: String,
    pub scenes: Vec<String>,
    pub cameras: Vec<String>,
    pub frame_start: i64,
    pub frame_end: i64,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}

/// Lê e valida JSON de câmera/marcadores do Blender tratando toda entrada como hostil (§0.4).
pub fn load_camera_json(path: &Path) -> Result<BlenderCameraExchange> {
    let meta = std::fs::metadata(path)
        .with_context(|| format!("não foi possível inspecionar {}", path.display()))?;
    if !meta.is_file() {
        bail!("caminho não é arquivo: {}", path.display());
    }
    if meta.len() > MAX_CAMERA_JSON_BYTES as u64 {
        bail!("arquivo JSON de câmera excede o limite de 16 MiB");
    }
    let bytes = std::fs::read(path)?;
    parse_camera_json_bytes(&bytes)
}

pub fn parse_camera_json_bytes(bytes: &[u8]) -> Result<BlenderCameraExchange> {
    if bytes.len() > MAX_CAMERA_JSON_BYTES {
        bail!("arquivo JSON de câmera excede o limite de 16 MiB");
    }
    let mut data: BlenderCameraExchange =
        serde_json::from_slice(bytes).context("JSON de câmera Blender inválido")?;
    if data.fps_num == 0 || data.fps_den == 0 || data.width == 0 || data.height == 0 {
        bail!("resolução ou taxa de quadros nula no JSON de câmera Blender");
    }
    if !data.sensor_width_mm.is_finite()
        || data.sensor_width_mm <= 0.0
        || !data.unit_scale.is_finite()
        || data.unit_scale <= 0.0
    {
        bail!("sensor_width_mm ou unit_scale inválido no JSON de câmera Blender");
    }
    if data.frames.len() > MAX_CAMERA_FRAMES {
        bail!("excesso de quadros no JSON de câmera (máx. {MAX_CAMERA_FRAMES})");
    }
    if data.markers.len() > MAX_CAMERA_MARKERS {
        bail!("excesso de marcadores no JSON de câmera (máx. {MAX_CAMERA_MARKERS})");
    }
    for f in &data.frames {
        if !f.pos.iter().all(|v| v.is_finite()) {
            bail!("posição não-finita no quadro {}", f.frame);
        }
        if let Some(e) = f.rot_euler_deg {
            if !e.iter().all(|v| v.is_finite()) {
                bail!("rotação euler não-finita no quadro {}", f.frame);
            }
        }
        if let Some(q) = f.quat {
            if !q.iter().all(|v| v.is_finite()) {
                bail!("quaternion não-finito no quadro {}", f.frame);
            }
        }
        if let Some(l) = f.lens_mm {
            if !l.is_finite() || l <= 0.1 {
                bail!("distância focal inválida no quadro {}", f.frame);
            }
        }
    }
    data.frames.sort_by_key(|f| f.frame);
    data.markers.sort_by_key(|m| m.frame);
    Ok(data)
}

fn roll_deg_from_frame(f: &BlenderCameraFrame) -> f32 {
    if let Some(euler) = f.rot_euler_deg {
        return util::clampf(-(euler[2] as f32), -360.0, 360.0);
    }
    if let Some([w, x, y, z]) = f.quat {
        // Ângulo em torno de Z a partir de quaternion unitário normalizado [w, x, y, z]
        let norm_sq = w * w + x * x + y * y + z * z;
        if norm_sq > 1e-12 {
            let siny_cosp = 2.0 * (w * z + x * y);
            let cosy_cosp = 1.0 - 2.0 * (y * y + z * z);
            let deg = siny_cosp.atan2(cosy_cosp).to_degrees() as f32;
            return util::clampf(-deg, -360.0, 360.0);
        }
    }
    0.0
}

fn relative_motion_from_frames(data: &BlenderCameraExchange) -> Vec<(Ticks, MotionKey)> {
    let Some(first) = data.frames.first() else {
        return Vec::new();
    };
    let fr = FrameRate {
        num: data.fps_num.max(1),
        den: data.fps_den.max(1),
    };
    let ref_pos = first.pos;
    let ref_lens = first.lens_mm.unwrap_or(50.0).max(1.0);
    // Fator de translação em espaço normalizado do quadro baseado em escala métrica
    let span = (10.0 * data.unit_scale).max(0.01);

    let mut out = Vec::with_capacity(data.frames.len());
    for f in &data.frames {
        let rel_frame = f.frame.saturating_sub(data.start_frame).max(0);
        let t = fr.frame_to_ticks(rel_frame);
        let dx = util::clampf(((f.pos[0] - ref_pos[0]) / span) as f32 * 0.25, -2.0, 2.0);
        // No Blender, +Z aponta para cima; no Neoveg Track Motion, +y move para baixo.
        let dy = util::clampf(-((f.pos[2] - ref_pos[2]) / span) as f32 * 0.25, -2.0, 2.0);
        let lens = f.lens_mm.unwrap_or(ref_lens).max(1.0);
        let zoom = util::clampf((lens / ref_lens) as f32, 0.1, 8.0);
        let rot = roll_deg_from_frame(f);
        out.push((
            t,
            MotionKey {
                x: dx,
                y: dy,
                scale_x: zoom,
                scale_y: zoom,
                rot,
            },
        ));
    }
    out
}

/// Aplica os quadros-chave da câmera Blender ao Track Motion de uma trilha de vídeo.
pub fn apply_camera_to_track(
    data: &BlenderCameraExchange,
    track: &mut Track,
    base_time: Ticks,
) -> usize {
    let keys = relative_motion_from_frames(data);
    let fr = FrameRate {
        num: data.fps_num.max(1),
        den: data.fps_den.max(1),
    };
    let tol = fr.frame_to_ticks(1).div_i64(2);
    let mut count = 0usize;
    for (rel_t, mk) in keys {
        track.motion.set(base_time + rel_t, mk, tol);
        count += 1;
    }
    count
}

/// Aplica os quadros-chave da câmera Blender ao Pan/Crop de um evento de vídeo.
pub fn apply_camera_to_event(data: &BlenderCameraExchange, event: &mut Event) -> usize {
    let keys = relative_motion_from_frames(data);
    let fr = FrameRate {
        num: data.fps_num.max(1),
        den: data.fps_den.max(1),
    };
    let tol = fr.frame_to_ticks(1).div_i64(2);
    let mut count = 0usize;
    for (rel_t, mk) in keys {
        if rel_t <= event.length {
            let size = util::clampf(1.0 / mk.scale_x.max(0.1), 0.1, 4.0);
            let pk = PanCropKey {
                cx: util::clampf(0.5 + mk.x, -1.0, 2.0),
                cy: util::clampf(0.5 + mk.y, -1.0, 2.0),
                size,
                rot: mk.rot,
            };
            event.pan_crop.set(rel_t, pk, tol);
            count += 1;
        }
    }
    count
}

/// Mescla os marcadores da cena Blender na timeline do Neoveg evitando duplicatas exatas.
pub fn merge_markers_into_project(
    data: &BlenderCameraExchange,
    project: &mut Project,
    base_time: Ticks,
) -> usize {
    let fr = FrameRate {
        num: data.fps_num.max(1),
        den: data.fps_den.max(1),
    };
    let tol = fr.frame_to_ticks(1).div_i64(2);
    let mut added = 0usize;
    for bm in &data.markers {
        let rel = bm.frame.saturating_sub(data.start_frame).max(0);
        let t = (base_time + fr.frame_to_ticks(rel)).max(Ticks::ZERO);
        let label = sanitize_label(&bm.name, "Blender");
        let exists = project
            .markers
            .iter()
            .any(|m| m.t.abs_diff(t) <= tol && m.label == label);
        if !exists && project.markers.len() < 4096 {
            project.markers.push(Marker {
                id: new_id(),
                t,
                label,
            });
            added += 1;
        }
    }
    project.markers.sort_by_key(|m| m.t);
    added
}

fn sanitize_label(raw: &str, fallback: &str) -> String {
    let clean: String = raw
        .chars()
        .filter(|c| !c.is_control())
        .take(128)
        .collect::<String>()
        .trim()
        .to_string();
    if clean.is_empty() {
        fallback.to_string()
    } else {
        clean
    }
}

/// Serializa uma string UTF-8 arbitrária como literal `json.loads(...)` seguro para Python.
fn py_json_expr<T: Serialize>(val: &T) -> Result<String> {
    let json_text = serde_json::to_string(val).context("falha ao serializar carga Python")?;
    // Como `json_text` já é string JSON válida, serializá-lo outra vez gera um literal JSON string válido em Python!
    let escaped =
        serde_json::to_string(&json_text).context("falha ao escapar string para Python")?;
    Ok(format!("json.loads({escaped})"))
}

/// Gera script Python em Sala Limpa usando apenas a API pública `bpy` para exportar
/// FPS, resolução, marcadores, câmera/keyframes e referência de áudio/vídeo do Neoveg.
pub fn generate_export_to_blender_py(
    project: &Project,
    selected_track: Option<&Track>,
    selected_event: Option<&Event>,
    audio_ref_wav: Option<&Path>,
    plate_media: Option<&Path>,
) -> Result<String> {
    let fps = project.settings.fps;
    let total_frames = fps.ticks_to_frame_floor(project.duration()).max(1);

    let markers: Vec<BlenderMarkerExchange> = project
        .markers
        .iter()
        .take(MAX_CAMERA_MARKERS)
        .map(|m| BlenderMarkerExchange {
            name: sanitize_label(&m.label, "Marcador"),
            frame: fps.ticks_to_frame_floor(m.t).saturating_add(1),
        })
        .collect();

    let mut cam_frames: Vec<(i64, f32, f32, f32, f32)> = Vec::new();
    if let Some(ev) = selected_event {
        for kf in &ev.pan_crop.keys {
            let f = fps.ticks_to_frame_floor(ev.start + kf.t).saturating_add(1);
            let lens_mm: f32 = (50.0 / kf.value.size.max(0.1)).clamp(8.0, 250.0);
            cam_frames.push((
                f,
                kf.value.cx - 0.5,
                -(kf.value.cy - 0.5),
                lens_mm,
                -kf.value.rot,
            ));
        }
    } else if let Some(tr) = selected_track {
        for kf in &tr.motion.keys {
            let f = fps.ticks_to_frame_floor(kf.t).saturating_add(1);
            let lens_mm: f32 = (50.0 * kf.value.scale_x.max(0.1)).clamp(8.0, 250.0);
            cam_frames.push((f, kf.value.x, kf.value.y, lens_mm, -kf.value.rot));
        }
    }

    #[derive(Serialize)]
    struct ScriptPayload<'a> {
        scene_name: &'a str,
        width: u32,
        height: u32,
        fps_num: u32,
        fps_den: u32,
        frame_end: i64,
        markers: Vec<BlenderMarkerExchange>,
        cam_frames: Vec<(i64, f32, f32, f32, f32)>,
        audio_ref: Option<String>,
        plate_media: Option<String>,
    }

    let payload = ScriptPayload {
        scene_name: "Neoveg_Sync",
        width: project.settings.width.max(16),
        height: project.settings.height.max(16),
        fps_num: fps.num.max(1),
        fps_den: fps.den.max(1),
        frame_end: total_frames,
        markers,
        cam_frames,
        audio_ref: audio_ref_wav.map(|p| p.to_string_lossy().to_string()),
        plate_media: plate_media.map(|p| p.to_string_lossy().to_string()),
    };

    let payload_expr = py_json_expr(&payload)?;

    Ok(format!(
        r#"# Script gerado pelo Neoveg (Sala Limpa — API pública bpy)
# Execute no Blender (Scripting -> Run Script) ou via `blender --python arquivo.py`.
import json
import math
import os
import bpy

DATA = {payload_expr}

def main():
    name = DATA["scene_name"]
    scene = bpy.data.scenes.new(name=name) if name not in bpy.data.scenes else bpy.data.scenes[name]
    bpy.context.window.scene = scene

    r = scene.render
    r.resolution_x = int(DATA["width"])
    r.resolution_y = int(DATA["height"])
    r.resolution_percentage = 100
    r.fps = max(1, int(DATA["fps_num"]))
    r.fps_base = float(max(1, int(DATA["fps_den"])))
    scene.frame_start = 1
    scene.frame_end = max(1, int(DATA["frame_end"]))

    for mk in DATA["markers"]:
        scene.timeline_markers.new(mk["name"], frame=int(mk["frame"]))

    cam_data = bpy.data.cameras.new(name="NeovegCamera")
    cam_data.sensor_width = 36.0
    cam_obj = bpy.data.objects.new("NeovegCamera", cam_data)
    scene.collection.objects.link(cam_obj)
    scene.camera = cam_obj
    cam_obj.location = (0.0, -5.0, 0.0)
    cam_obj.rotation_euler = (math.radians(90.0), 0.0, 0.0)

    for (frame, x, y, lens, roll_deg) in DATA["cam_frames"]:
        fr = max(1, int(frame))
        cam_obj.location = (float(x) * 4.0, -5.0, float(y) * 4.0)
        cam_obj.rotation_euler = (math.radians(90.0), math.radians(float(roll_deg)), 0.0)
        cam_data.lens = float(lens)
        cam_obj.keyframe_insert(data_path="location", frame=fr)
        cam_obj.keyframe_insert(data_path="rotation_euler", frame=fr)
        cam_data.keyframe_insert(data_path="lens", frame=fr)

    plate = DATA.get("plate_media")
    if plate and os.path.isfile(plate):
        try:
            img = bpy.data.images.load(plate, check_existing=True)
            bg = cam_data.background_images.new()
            bg.image = img
            bg.alpha = 1.0
            cam_data.show_background_images = True
        except Exception as exc:
            print("Neoveg: aviso ao anexar plate:", exc)

    audio = DATA.get("audio_ref")
    if audio and os.path.isfile(audio):
        if not scene.sequence_editor:
            scene.sequence_editor_create()
        try:
            scene.sequence_editor.sequences.new_sound("NeovegAudioRef", audio, channel=1, frame_start=1)
        except Exception as exc:
            print("Neoveg: aviso ao anexar áudio VSE:", exc)

if __name__ == "__main__":
    main()
"#
    ))
}

/// Gera script Python limpo para extrair câmera ativa + marcadores do Blender para `BlenderCameraExchange` JSON.
pub fn generate_camera_extractor_py(out_json: Option<&Path>) -> Result<String> {
    let out_expr = match out_json {
        Some(p) => py_json_expr(&p.to_string_lossy().to_string())?,
        None => "None".to_string(),
    };
    Ok(format!(
        r#"# Extrator de Câmera e Marcadores para Neoveg (API pública bpy)
import json
import math
import bpy

OUT_PATH = {out_expr}

def main():
    scene = bpy.context.scene
    cam = scene.camera
    frames = []
    if cam is not None:
        orig_frame = scene.frame_current
        for fr in range(int(scene.frame_start), int(scene.frame_end) + 1):
            scene.frame_set(fr)
            loc = cam.matrix_world.to_translation()
            quat = cam.matrix_world.to_quaternion()
            euler = cam.matrix_world.to_euler('XYZ')
            lens = float(getattr(cam.data, "lens", 50.0))
            frames.append({{
                "frame": fr,
                "pos": [float(loc.x), float(loc.y), float(loc.z)],
                "quat": [float(quat.w), float(quat.x), float(quat.y), float(quat.z)],
                "rot_euler_deg": [
                    math.degrees(float(euler.x)),
                    math.degrees(float(euler.y)),
                    math.degrees(float(euler.z)),
                ],
                "lens_mm": lens,
            }})
        scene.frame_set(orig_frame)

    payload = {{
        "schema": "neoveg.blender.camera.v1",
        "scene_name": str(scene.name),
        "camera_name": str(cam.name) if cam else "",
        "width": int(scene.render.resolution_x),
        "height": int(scene.render.resolution_y),
        "fps_num": int(scene.render.fps),
        "fps_den": max(1, int(round(scene.render.fps_base))),
        "start_frame": int(scene.frame_start),
        "sensor_width_mm": float(getattr(cam.data, "sensor_width", 36.0)) if cam else 36.0,
        "unit_scale": 0.01,
        "markers": [{{"name": str(m.name), "frame": int(m.frame)}} for m in scene.timeline_markers],
        "frames": frames,
    }}

    if OUT_PATH:
        with open(OUT_PATH, "w", encoding="utf-8") as f:
            json.dump(payload, f, ensure_ascii=False, indent=2)
    else:
        print("{BLENDER_PROBE_PREFIX}" + json.dumps(payload, ensure_ascii=False))

if __name__ == "__main__":
    main()
"#
    ))
}

fn generate_probe_script_py() -> String {
    format!(
        r#"import json, bpy
s = bpy.context.scene
info = {{
    "active_scene": str(s.name),
    "scenes": [str(x.name) for x in bpy.data.scenes],
    "cameras": [str(o.name) for o in bpy.data.objects if o.type == 'CAMERA'],
    "frame_start": int(s.frame_start),
    "frame_end": int(s.frame_end),
    "width": int(s.render.resolution_x),
    "height": int(s.render.resolution_y),
    "fps": int(s.render.fps),
}}
print("{BLENDER_PROBE_PREFIX}" + json.dumps(info, ensure_ascii=False))
"#
    )
}

/// Analisa linhas de saída padrão do Blender como `Fra:12 Mem:...` durante `--render-anim`.
pub fn parse_fra_progress_line(line: &str) -> Option<u64> {
    let trimmed = line.trim_start();
    let rest = trimmed.strip_prefix("Fra:")?;
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<u64>().ok()
}

pub fn extract_prefixed_json_line(stdout: &str, prefix: &str) -> Option<String> {
    stdout
        .lines()
        .find_map(|line| line.strip_prefix(prefix).map(str::to_string))
}

fn temp_script_path(tag: &str) -> PathBuf {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "neoveg_blender_{tag}_{}_{ms}.py",
        std::process::id()
    ))
}

/// Inspeciona um arquivo `.blend` usando `--background --factory-startup --python`.
pub fn probe_blend_scene(
    blender_exe: &Path,
    blend_file: &Path,
    timeout_secs: u64,
) -> Result<BlendSceneInfo> {
    if !blend_file.is_file() {
        bail!("arquivo .blend não encontrado: {}", blend_file.display());
    }
    let script_path = temp_script_path("probe");
    std::fs::write(&script_path, generate_probe_script_py())
        .context("não foi possível gravar script temporário do Blender")?;

    let mut child = Command::new(blender_exe)
        .arg("-b")
        .arg(blend_file)
        .arg("--factory-startup")
        .arg("--python-exit-code")
        .arg("1")
        .arg("--python")
        .arg(&script_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("falha ao executar Blender ({})", blender_exe.display()))?;

    let cancel = AtomicBool::new(false);
    let output = wait_child_with_timeout(
        &mut child,
        Duration::from_secs(timeout_secs.clamp(5, 120)),
        &cancel,
    );
    let _ = std::fs::remove_file(&script_path);
    let out = output?;

    let stdout = String::from_utf8_lossy(&out.stdout);
    let Some(json_line) = extract_prefixed_json_line(&stdout, BLENDER_PROBE_PREFIX) else {
        let stderr = String::from_utf8_lossy(&out.stderr);
        bail!(
            "Blender não retornou metadados da cena (status {:?}): {}",
            out.status.code(),
            stderr.lines().take(6).collect::<Vec<_>>().join(" | ")
        );
    };
    let info: BlendSceneInfo =
        serde_json::from_str(&json_line).context("metadados JSON do Blender inválidos")?;
    Ok(info)
}

/// Extrai a animação de câmera e marcadores diretamente de um `.blend` via CLI headless.
pub fn extract_camera_from_blend(
    blender_exe: &Path,
    blend_file: &Path,
    timeout_secs: u64,
) -> Result<BlenderCameraExchange> {
    if !blend_file.is_file() {
        bail!("arquivo .blend não encontrado: {}", blend_file.display());
    }
    let script_path = temp_script_path("cam");
    let json_path = script_path.with_extension("json");
    let py = generate_camera_extractor_py(Some(&json_path))?;
    std::fs::write(&script_path, py)?;

    let mut child = Command::new(blender_exe)
        .arg("-b")
        .arg(blend_file)
        .arg("--factory-startup")
        .arg("--python-exit-code")
        .arg("1")
        .arg("--python")
        .arg(&script_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("falha ao iniciar Blender ({})", blender_exe.display()))?;

    let cancel = AtomicBool::new(false);
    let res = wait_child_with_timeout(
        &mut child,
        Duration::from_secs(timeout_secs.clamp(5, 180)),
        &cancel,
    );
    let _ = std::fs::remove_file(&script_path);
    let _ = res?;

    let parsed = load_camera_json(&json_path);
    let _ = std::fs::remove_file(&json_path);
    parsed
}

/// Parâmetros para renderização headless de uma cena `.blend` para a Timeline do Neoveg.
#[derive(Clone, Debug)]
pub struct BlenderRenderRequest {
    pub blender_exe: PathBuf,
    pub blend_file: PathBuf,
    pub out_dir: PathBuf,
    pub scene_name: Option<String>,
    pub frame_start: Option<i64>,
    pub frame_end: Option<i64>,
    pub format: String,
    pub timeout_secs: u64,
}

/// Executa `blender -b <blend> ... -a` atualizando progresso (`Fra:<N>`) e respeitando cancelamento.
pub fn render_blend_scene(
    req: &BlenderRenderRequest,
    done_frames: &Arc<AtomicU64>,
    cancel: &Arc<AtomicBool>,
) -> Result<Vec<PathBuf>> {
    if !req.blend_file.is_file() {
        bail!(
            "arquivo .blend não encontrado: {}",
            req.blend_file.display()
        );
    }
    std::fs::create_dir_all(&req.out_dir)
        .with_context(|| format!("não foi possível criar pasta {}", req.out_dir.display()))?;

    let pattern = req.out_dir.join("frame_#####");
    let fmt = match req.format.to_ascii_uppercase().as_str() {
        "OPEN_EXR" | "EXR" => "PNG", // Sequências importadas diretamente pelo ImageSequence usam PNG
        _ => "PNG",
    };

    let mut cmd = Command::new(&req.blender_exe);
    cmd.arg("-b").arg(&req.blend_file);
    if let Some(ref sc) = req.scene_name {
        if !sc.trim().is_empty() {
            cmd.arg("-S").arg(sc.trim());
        }
    }
    cmd.arg("-o").arg(&pattern).arg("-F").arg(fmt);
    if let Some(s) = req.frame_start {
        cmd.arg("-s").arg(s.to_string());
    }
    if let Some(e) = req.frame_end {
        cmd.arg("-e").arg(e.to_string());
    }
    cmd.arg("-a").stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .with_context(|| format!("falha ao iniciar Blender ({})", req.blender_exe.display()))?;

    let stdout_pipe = child.stdout.take();
    let progress_counter = done_frames.clone();
    let mut reader_handle = std::thread::Builder::new()
        .name("blender-stdout".into())
        .spawn(move || {
            if let Some(out) = stdout_pipe {
                let rd = BufReader::new(out);
                for line in rd.lines().map_while(Result::ok) {
                    if let Some(fr) = parse_fra_progress_line(&line) {
                        progress_counter.store(fr, Ordering::Relaxed);
                    }
                }
            }
        })
        .ok();

    let deadline = Instant::now() + Duration::from_secs(req.timeout_secs.clamp(10, 3600));
    let mut cancelled = false;
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            cancelled = true;
            break;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            if let Some(h) = reader_handle.take() {
                let _ = h.join();
            }
            bail!("tempo limite excedido ao renderizar cena Blender");
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                if let Some(h) = reader_handle.take() {
                    let _ = h.join();
                }
                if !status.success() {
                    let stderr_msg = child
                        .stderr
                        .take()
                        .and_then(|mut s| {
                            let mut buf = String::new();
                            std::io::Read::read_to_string(&mut s, &mut buf)
                                .ok()
                                .map(|_| buf)
                        })
                        .unwrap_or_default();
                    bail!(
                        "renderização Blender falhou (código {:?}): {}",
                        status.code(),
                        stderr_msg.lines().take(5).collect::<Vec<_>>().join(" | ")
                    );
                }
                break;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(60)),
            Err(e) => {
                let _ = child.kill();
                return Err(anyhow!("erro ao aguardar processo Blender: {e}"));
            }
        }
    }

    if cancelled {
        let _ = child.wait();
        if let Some(h) = reader_handle.take() {
            let _ = h.join();
        }
        bail!("renderização Blender cancelada pelo usuário");
    }

    let frames = collect_png_sequence(&req.out_dir)?;
    if frames.is_empty() {
        bail!(
            "nenhum quadro PNG gerado em {} pelo Blender",
            req.out_dir.display()
        );
    }
    Ok(frames)
}

pub fn collect_png_sequence(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| e.eq_ignore_ascii_case("png"))
        })
        .collect();
    out.sort();
    Ok(out)
}

struct ChildOutput {
    status: std::process::ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn wait_child_with_timeout(
    child: &mut Child,
    timeout: Duration,
    cancel: &AtomicBool,
) -> Result<ChildOutput> {
    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();
    let out_thread = std::thread::spawn(move || read_bounded_stream(stdout_pipe, 8 * 1024 * 1024));
    let err_thread = std::thread::spawn(move || read_bounded_stream(stderr_pipe, 1024 * 1024));

    let deadline = Instant::now() + timeout;
    let status = loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            bail!("operação Blender cancelada");
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            bail!("tempo limite esgotado aguardando resposta do Blender");
        }
        match child.try_wait()? {
            Some(st) => break st,
            None => std::thread::sleep(Duration::from_millis(40)),
        }
    };

    let stdout = out_thread.join().unwrap_or_default();
    let stderr = err_thread.join().unwrap_or_default();
    Ok(ChildOutput {
        status,
        stdout,
        stderr,
    })
}

fn read_bounded_stream<R: std::io::Read>(pipe: Option<R>, cap: usize) -> Vec<u8> {
    let Some(mut r) = pipe else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut chunk = [0u8; 4096];
    while out.len() < cap {
        let Ok(n) = r.read(&mut chunk) else {
            break;
        };
        if n == 0 {
            break;
        }
        let room = cap - out.len();
        out.extend_from_slice(&chunk[..n.min(room)]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Track, TrackKind};

    #[test]
    fn parser_camera_blender_converte_keyframes_e_marcadores() {
        let raw = r#"{
            "schema": "neoveg.blender.camera.v1",
            "scene_name": "Shot01",
            "camera_name": "MainCam",
            "width": 1920,
            "height": 1080,
            "fps_num": 24,
            "fps_den": 1,
            "start_frame": 1,
            "sensor_width_mm": 36.0,
            "unit_scale": 0.01,
            "markers": [{"name": "Batida 1", "frame": 25}],
            "frames": [
                {"frame": 1, "pos": [0.0, -5.0, 0.0], "rot_euler_deg": [90.0, 0.0, 0.0], "lens_mm": 50.0},
                {"frame": 25, "pos": [0.2, -5.0, 0.1], "rot_euler_deg": [90.0, 0.0, -10.0], "lens_mm": 75.0}
            ]
        }"#;
        let data = parse_camera_json_bytes(raw.as_bytes()).expect("deve ler JSON");
        let mut tr = Track::new(TrackKind::Video, "Blender Cam");
        let applied = apply_camera_to_track(&data, &mut tr, Ticks::ZERO);
        assert_eq!(applied, 2);
        assert!(tr.motion.keys.len() >= 2);
        let at_1s = tr.motion.eval(Ticks::from_secs_i64(1));
        assert!(at_1s.x > 0.0);
        assert!((at_1s.scale_x - 1.5).abs() < 0.01);
        assert!((at_1s.rot - 10.0).abs() < 0.01);

        let mut proj = Project::default();
        let added = merge_markers_into_project(&data, &mut proj, Ticks::ZERO);
        assert_eq!(added, 1);
        assert_eq!(proj.markers[0].label, "Batida 1");
        assert_eq!(proj.markers[0].t, Ticks::from_secs_i64(1));
    }

    #[test]
    fn parser_camera_rejeita_entradas_hostis() {
        assert!(parse_camera_json_bytes(b"not json").is_err());
        assert!(
            parse_camera_json_bytes(br#"{"width":0,"height":1080,"fps_num":24,"fps_den":1}"#)
                .is_err()
        );
        assert!(parse_camera_json_bytes(
            br#"{"width":1920,"height":1080,"fps_num":0,"fps_den":1}"#
        )
        .is_err());
    }

    #[test]
    fn script_export_blender_escapa_caminhos_com_aspas_e_barras() {
        let mut proj = Project::default();
        proj.markers.push(Marker {
            id: new_id(),
            t: Ticks::from_secs_i64(2),
            label: "Cena \"Alfa\"\nLinha2".into(),
        });
        let py = generate_export_to_blender_py(
            &proj,
            None,
            None,
            Some(Path::new(r#"C:\Users\Teste\áudio '1'.wav"#)),
            Some(Path::new("/tmp/render \"plate\".png")),
        )
        .expect("gera script");
        assert!(py.contains("import bpy"));
        assert!(py.contains("json.loads("));
        assert!(!py.contains("Cena \"Alfa\"\nLinha2"));
    }

    #[test]
    fn fra_progress_e_probe_line_sao_extraidos() {
        assert_eq!(
            parse_fra_progress_line("Fra:42 Mem:88.4M (Peak 91.0M) | Time:00:01.20"),
            Some(42)
        );
        assert_eq!(parse_fra_progress_line("Saved: 'frame_00042.png'"), None);
        let stdout =
            "Read blend: scene.blend\n@neoveg-blender {\"active_scene\":\"Scene\"}\nBlender quit";
        assert_eq!(
            extract_prefixed_json_line(stdout, BLENDER_PROBE_PREFIX).as_deref(),
            Some("{\"active_scene\":\"Scene\"}")
        );
    }
}
