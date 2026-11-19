//! Interface de integração com os aplicativos ArtCraft (PhotoCraft,
//! VectorCraft, FilmCraft, EffectCraft, AniCraft, SoundCraft) e Blender.
//!
//! Implementada nativamente em `egui` mantendo os 7 temas do Neoveg (incluindo
//! Alto Contraste WCAG AAA) e comunicação segura fora de processo (§0 / §2.3).

use crate::app::{App, LoadMsg, PlaceTarget};
use crate::dialogs::PrefsCategory;
use crate::icons::{self, Icon};
use eframe::egui;
use neoveg_common::util;
use neoveg_engine::integrations::blender::{
    apply_camera_to_event, apply_camera_to_track, extract_camera_from_blend,
    generate_camera_extractor_py, generate_export_to_blender_py, load_camera_json,
    merge_markers_into_project, probe_blend_scene, render_blend_scene, BlendSceneInfo,
    BlenderCameraExchange, BlenderRenderRequest,
};
use neoveg_engine::integrations::discovery::{discover_all, AppDiscoveryStatus, DiscoveryReport};
use neoveg_engine::integrations::workflows::{
    create_anicraft_project, export_clean_svg_for_vectorcraft, export_event_audio_bwf,
    export_frame_png_for_photocraft, export_timeline_range_bwf, interchange_work_dir,
    render_anicraft_sequence, render_effectcraft_sequence, send_file_to_app, sibling_copy_path,
    DeliveryMethod, WatchedExternalEdit,
};
use neoveg_engine::integrations::ExternalApp;
use neoveg_engine::media;
use neoveg_engine::model::{Event, FrameRate, Id, MediaItem, MediaSource, Ticks, Track, TrackKind};
use neoveg_engine::takes;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub enum IntegrationMsg {
    DiscoveryDone(DiscoveryReport),
    BlendProbed(Result<BlendSceneInfo, String>),
    SequenceRendered {
        app: ExternalApp,
        label: String,
        result: Box<Result<media::Imported, String>>,
        place: bool,
    },
    BlenderCameraImported(Result<BlenderCameraExchange, String>),
    ActionFinished(Result<String, String>),
}

struct SequenceRenderSpec {
    thread_name: &'static str,
    guard_label: &'static str,
    app: ExternalApp,
    label: String,
    fps: FrameRate,
    place: bool,
}

fn spawn_sequence_render(
    tx: Sender<IntegrationMsg>,
    spec: SequenceRenderSpec,
    produce_first_frame: impl FnOnce() -> Result<PathBuf, String> + Send + 'static,
) -> Result<(), String> {
    util::spawn(spec.thread_name, move || {
        let res = util::guard(spec.guard_label, || {
            let first = produce_first_frame()?;
            media::import_sequence(&first, spec.fps).map_err(|e| format!("{e:#}"))
        })
        .and_then(|r| r);
        let _ = tx.send(IntegrationMsg::SequenceRendered {
            app: spec.app,
            label: spec.label,
            result: Box::new(res),
            place: spec.place,
        });
    })
}

#[derive(Clone)]
pub struct BlenderRenderJobUi {
    pub scene_label: String,
    pub done: Arc<AtomicU64>,
    pub total: u64,
    pub cancel: Arc<AtomicBool>,
}

pub struct IntegrationsState {
    pub open: bool,
    pub report: Option<DiscoveryReport>,
    pub probing: bool,
    pub watched: Vec<WatchedExternalEdit>,
    pub last_watch_poll: Option<Instant>,
    pub blender_blend_path: String,
    pub blender_scene_name: String,
    pub blender_frame_start: i64,
    pub blender_frame_end: i64,
    pub blender_scene_info: Option<BlendSceneInfo>,
    pub blender_job: Option<BlenderRenderJobUi>,
    pub anicraft_file_path: String,
    pub effectcraft_file_path: String,
    pub effectcraft_comp_name: String,
    pub last_message: Option<String>,
    pub(crate) tx: Sender<IntegrationMsg>,
    pub(crate) rx: Receiver<IntegrationMsg>,
}

impl Default for IntegrationsState {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self {
            open: false,
            report: None,
            probing: false,
            watched: Vec::new(),
            last_watch_poll: None,
            blender_blend_path: String::new(),
            blender_scene_name: String::new(),
            blender_frame_start: 1,
            blender_frame_end: 120,
            blender_scene_info: None,
            blender_job: None,
            anicraft_file_path: String::new(),
            effectcraft_file_path: String::new(),
            effectcraft_comp_name: String::new(),
            last_message: None,
            tx,
            rx,
        }
    }
}

impl App {
    pub fn open_integrations_dialog(&mut self) {
        self.integrations.open = true;
        if self.integrations.report.is_none() {
            let report = discover_all(&self.prefs.external_apps, false);
            self.integrations.report = Some(report);
        }
    }

    pub fn refresh_integrations_async(&mut self, probe_sockets: bool) {
        if self.integrations.probing {
            return;
        }
        let prefs = self.prefs.external_apps.clone();
        let tx = self.integrations.tx.clone();
        self.integrations.probing = true;
        if util::spawn("scan-integracoes", move || {
            let r = util::guard("descoberta de integrações", || {
                discover_all(&prefs, probe_sockets)
            });
            if let Ok(rep) = r {
                let _ = tx.send(IntegrationMsg::DiscoveryDone(rep));
            }
        })
        .is_err()
        {
            self.integrations.probing = false;
        }
    }

    pub(crate) fn poll_integrations(&mut self) {
        while let Ok(msg) = self.integrations.rx.try_recv() {
            match msg {
                IntegrationMsg::DiscoveryDone(rep) => {
                    self.integrations.probing = false;
                    let found = rep.apps.iter().filter(|a| a.available()).count();
                    let live = rep.apps.iter().filter(|a| a.live_reachable).count();
                    self.integrations.report = Some(rep);
                    let summary = format!(
                        "Integrações verificadas: {found}/{} app(s) disponíveis ({live} em canal RPC ao vivo).",
                        ExternalApp::ALL.len()
                    );
                    self.integrations.last_message = Some(summary.clone());
                    self.set_status(summary);
                }
                IntegrationMsg::BlendProbed(res) => {
                    self.integrations.probing = false;
                    match res {
                        Ok(info) => {
                            self.integrations.blender_scene_name = info.active_scene.clone();
                            self.integrations.blender_frame_start = info.frame_start;
                            self.integrations.blender_frame_end = info.frame_end;
                            let summary = format!(
                                "Cena Blender '{}': quadros {}..{}, {}x{} @ {} fps.",
                                info.active_scene,
                                info.frame_start,
                                info.frame_end,
                                info.width,
                                info.height,
                                info.fps
                            );
                            self.integrations.blender_scene_info = Some(info);
                            self.integrations.last_message = Some(summary.clone());
                            self.set_status(summary);
                        }
                        Err(e) => {
                            self.integrations.last_message = Some(format!("Erro: {e}"));
                            self.set_status(format!("Blender: {e}"));
                        }
                    }
                }
                IntegrationMsg::SequenceRendered {
                    app,
                    label,
                    result,
                    place,
                } => {
                    self.loading = self.loading.saturating_sub(1);
                    if app == ExternalApp::Blender {
                        self.integrations.blender_job = None;
                    }
                    match *result {
                        Ok(imp) => {
                            let id = imp.item.id;
                            let name = imp.item.name.clone();
                            self.media.insert(id, imp.entry);
                            self.checkpoint();
                            self.session.register_imported_media(imp.item, None);
                            if place {
                                let t = self.cursor;
                                self.add_media_to_timeline(id, t, self.sel_track);
                            }
                            self.changed();
                            let msg = format!(
                                "{}: '{name}' ({label}) importado para a timeline.",
                                app.name()
                            );
                            self.integrations.last_message = Some(msg.clone());
                            self.set_status(msg);
                        }
                        Err(e) => {
                            let msg = format!("{} ({label}): {e}", app.name());
                            self.integrations.last_message = Some(msg.clone());
                            self.set_status(msg);
                        }
                    }
                }
                IntegrationMsg::BlenderCameraImported(res) => {
                    self.integrations.probing = false;
                    match res {
                        Ok(cam) => {
                            let msg = self.apply_imported_blender_camera(&cam);
                            self.integrations.last_message = Some(msg.clone());
                            self.set_status(msg);
                        }
                        Err(e) => {
                            let msg = format!("Erro ao importar câmera Blender: {e}");
                            self.integrations.last_message = Some(msg.clone());
                            self.set_status(msg);
                        }
                    }
                }
                IntegrationMsg::ActionFinished(res) => {
                    self.integrations.probing = false;
                    match res {
                        Ok(m) => {
                            self.integrations.last_message = Some(m.clone());
                            self.set_status(m);
                        }
                        Err(e) => {
                            self.integrations.last_message = Some(e.clone());
                            self.set_status(e);
                        }
                    }
                }
            }
        }

        // Observador leve de cópias em edição (verifica mtime a cada 1.2s)
        if !self.prefs.external_apps.watch_edited_copies || self.integrations.watched.is_empty() {
            return;
        }
        let due = self
            .integrations
            .last_watch_poll
            .is_none_or(|t| t.elapsed() >= Duration::from_millis(1200));
        if !due {
            return;
        }
        self.integrations.last_watch_poll = Some(Instant::now());

        let mut reload_ids: Vec<(ExternalApp, Id, PathBuf)> = Vec::new();
        for w in &mut self.integrations.watched {
            if w.poll_modified() {
                reload_ids.push((w.app, w.media_id, w.path.clone()));
            }
        }
        for (app, mid, path) in reload_ids {
            self.reload_watched_media(app, mid, &path);
        }
    }

    fn reload_watched_media(&mut self, app: ExternalApp, media_id: Id, path: &Path) {
        self.thumbs.remove(&media_id);
        let Some(item) = self
            .project
            .media
            .iter()
            .find(|m| m.id == media_id)
            .cloned()
        else {
            return;
        };
        let tx = self.loader_tx.clone();
        self.loading += 1;
        let label = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());
        if util::spawn("recarregar-copia-externa", move || {
            let mut m = item;
            let r = media::load_entry(&mut m).map_err(|e| format!("{e:#}"));
            let _ = tx.send(LoadMsg::Reloaded(m.id, r));
        })
        .is_err()
        {
            self.loading = self.loading.saturating_sub(1);
        } else {
            self.set_status(format!("Recarregado do {}: {label}", app.name()));
        }
    }

    fn app_status(&mut self, app: ExternalApp) -> AppDiscoveryStatus {
        let rep = self
            .integrations
            .report
            .get_or_insert_with(|| discover_all(&self.prefs.external_apps, false));
        rep.apps
            .iter()
            .find(|s| s.app == app)
            .cloned()
            .unwrap_or_else(|| {
                discover_all(&self.prefs.external_apps, false)
                    .apps
                    .into_iter()
                    .find(|s| s.app == app)
                    .unwrap_or(AppDiscoveryStatus {
                        app,
                        id: app.id().to_string(),
                        name: app.name().to_string(),
                        desktop_exe: None,
                        cli_exe: None,
                        from_manual_pref: false,
                        settings_dir: None,
                        live_port: None,
                        live_pid: None,
                        live_token_available: false,
                        live_reachable: false,
                        version_hint: None,
                    })
            })
    }

    fn first_selected_event(&self) -> Option<Event> {
        let eid = self.sel_events.iter().next().copied()?;
        self.project.event(eid).cloned()
    }

    fn integration_video_track(&self) -> Option<usize> {
        self.selected_video_track().or_else(|| {
            self.project
                .tracks
                .iter()
                .position(|t| t.kind == TrackKind::Video)
        })
    }

    fn selected_or_event_media(&self, event: Option<&Event>) -> Option<MediaItem> {
        event.and_then(|e| self.project.media(e.media_id)).cloned()
    }

    fn apply_imported_blender_camera(&mut self, cam: &BlenderCameraExchange) -> String {
        self.checkpoint();
        let base_t = self.cursor;
        let markers_added = merge_markers_into_project(cam, &mut self.project, base_t);

        let mut keys_applied = 0usize;
        let mut target_desc = "projeto (marcadores)".to_string();

        if let Some(ev_id) = self.sel_events.iter().next().copied() {
            if let Some((ti, ei)) = self.project.find_event(ev_id) {
                if self.project.tracks[ti].kind == TrackKind::Video {
                    let ev = &mut self.project.tracks[ti].events[ei];
                    keys_applied = apply_camera_to_event(cam, ev);
                    target_desc = format!("Pan/Crop de '{}'", ev.name);
                }
            }
        }
        if keys_applied == 0 {
            if let Some(ti) = self.integration_video_track() {
                let tr = &mut self.project.tracks[ti];
                keys_applied = apply_camera_to_track(cam, tr, base_t);
                target_desc = format!("Track Motion de '{}'", tr.name);
            }
        }
        self.changed();
        format!(
            "Câmera Blender importada em {target_desc}: {keys_applied} quadro(s)-chave e {markers_added} marcador(es)."
        )
    }

    /// Fluxo principal VEGAS / sala limpa para enviar evento/mídia/timeline a um app ArtCraft ou Blender.
    pub fn send_to_external_app(
        &mut self,
        app: ExternalApp,
        explicit_event: Option<Id>,
        explicit_media: Option<Id>,
    ) {
        let status = self.app_status(app);
        let event = explicit_event
            .and_then(|id| self.project.event(id).cloned())
            .or_else(|| self.first_selected_event());
        let media_item = explicit_media
            .and_then(|id| self.project.media(id).cloned())
            .or_else(|| self.selected_or_event_media(event.as_ref()));

        match app {
            ExternalApp::PhotoCraft => self.workflow_photocraft(&status, event, media_item),
            ExternalApp::VectorCraft => self.workflow_vectorcraft(&status, event, media_item),
            ExternalApp::SoundCraft => self.workflow_soundcraft(&status, event, media_item),
            ExternalApp::EffectCraft => self.workflow_effectcraft(&status, event, media_item),
            ExternalApp::AniCraft => self.workflow_anicraft(&status),
            ExternalApp::FilmCraft => self.workflow_filmcraft(&status),
            ExternalApp::Blender => self.workflow_blender_export_script(Some(&status)),
        }
    }

    fn media_item_primary_path(m: &MediaItem) -> Option<PathBuf> {
        match &m.source {
            MediaSource::File { path } | MediaSource::NestedProject { path } => {
                Some(PathBuf::from(path))
            }
            MediaSource::ImageSequence { paths, .. } => paths.first().map(PathBuf::from),
            MediaSource::Generated(_) => None,
        }
    }

    fn workflow_photocraft(
        &mut self,
        status: &AppDiscoveryStatus,
        event: Option<Event>,
        media_item: Option<MediaItem>,
    ) {
        let src_path = media_item.as_ref().and_then(Self::media_item_primary_path);
        let stem = event
            .as_ref()
            .map(|e| e.name.as_str())
            .or_else(|| media_item.as_ref().map(|m| m.name.as_str()))
            .unwrap_or("quadro");
        let work_dir = match src_path
            .as_deref()
            .and_then(|p| p.parent())
            .filter(|d| d.is_dir())
        {
            Some(d) => d.to_path_buf(),
            None => match interchange_work_dir(self.session.path.as_deref(), "photocraft") {
                Ok(d) => d,
                Err(e) => {
                    self.set_status(format!("PhotoCraft: {e:#}"));
                    return;
                }
            },
        };
        let copy_path = sibling_copy_path(&work_dir, stem, ExternalApp::PhotoCraft, "png");
        let is_still_file = src_path
            .as_deref()
            .and_then(|p| p.extension().and_then(|e| e.to_str()))
            .is_some_and(|e| {
                neoveg_engine::media::IMAGE_EXTS.contains(&e.to_ascii_lowercase().as_str())
            });

        if let Err(e) = export_frame_png_for_photocraft(
            &self.project,
            &self.media,
            self.cursor,
            if is_still_file {
                src_path.as_deref()
            } else {
                None
            },
            &copy_path,
        ) {
            self.set_status(format!("Erro ao preparar PNG para PhotoCraft: {e:#}"));
            return;
        }

        self.register_copy_as_take_and_deliver(
            status,
            &copy_path,
            event.as_ref(),
            "cópia PNG gerada e vinculada como Take",
        );
    }

    fn workflow_vectorcraft(
        &mut self,
        status: &AppDiscoveryStatus,
        event: Option<Event>,
        media_item: Option<MediaItem>,
    ) {
        let existing_vec = media_item.as_ref().and_then(|m| match &m.source {
            MediaSource::File { path } => {
                let p = PathBuf::from(path);
                let ext = p
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if matches!(
                    ext.as_str(),
                    "svg" | "svgz" | "pdf" | "ai" | "eps" | "vectorcraft"
                ) {
                    Some(p)
                } else {
                    None
                }
            }
            _ => None,
        });

        let target_svg = if let Some(p) = existing_vec {
            p
        } else {
            let Ok(dir) = interchange_work_dir(self.session.path.as_deref(), "vectorcraft") else {
                self.set_status("Não foi possível criar pasta de trabalho para VectorCraft.");
                return;
            };
            let stem = event
                .as_ref()
                .map(|e| e.name.as_str())
                .or_else(|| media_item.as_ref().map(|m| m.name.as_str()))
                .unwrap_or("placa");
            let svg = sibling_copy_path(&dir, stem, ExternalApp::VectorCraft, "svg");
            if let Err(e) = export_clean_svg_for_vectorcraft(
                &self.project,
                event.as_ref(),
                media_item.as_ref(),
                &svg,
            ) {
                self.set_status(format!("Erro ao gerar SVG: {e:#}"));
                return;
            }
            svg
        };

        self.deliver_with_status(status, &target_svg, "SVG preparado");
    }

    fn workflow_soundcraft(
        &mut self,
        status: &AppDiscoveryStatus,
        event: Option<Event>,
        media_item: Option<MediaItem>,
    ) {
        let dir = match interchange_work_dir(self.session.path.as_deref(), "soundcraft") {
            Ok(d) => d,
            Err(e) => {
                self.set_status(format!("SoundCraft: {e:#}"));
                return;
            }
        };
        let stem = event
            .as_ref()
            .map(|e| e.name.as_str())
            .or_else(|| media_item.as_ref().map(|m| m.name.as_str()))
            .unwrap_or("mix_timeline");
        let wav_path = sibling_copy_path(&dir, stem, ExternalApp::SoundCraft, "wav");

        let res = if let Some(ref ev) = event {
            export_event_audio_bwf(&self.project, &self.media, ev, &wav_path)
        } else {
            let dur = self.project.duration();
            if dur <= Ticks::ZERO {
                self.set_status("Projeto sem áudio para exportar ao SoundCraft.");
                return;
            }
            export_timeline_range_bwf(&self.project, &self.media, Ticks::ZERO, dur, &wav_path, 0)
        };
        if let Err(e) = res {
            self.set_status(format!("Erro ao exportar WAV/BWF para SoundCraft: {e:#}"));
            return;
        }

        self.register_copy_as_take_and_deliver(
            status,
            &wav_path,
            event.as_ref(),
            "áudio BWF (bext) exportado",
        );
    }

    fn workflow_effectcraft(
        &mut self,
        status: &AppDiscoveryStatus,
        event: Option<Event>,
        media_item: Option<MediaItem>,
    ) {
        let media_path = media_item.as_ref().and_then(Self::media_item_primary_path);

        let target = if let Some(p) = media_path.filter(|p| p.exists()) {
            p
        } else {
            let Ok(dir) = interchange_work_dir(self.session.path.as_deref(), "effectcraft") else {
                self.set_status("EffectCraft: falha ao criar pasta de intercâmbio.");
                return;
            };
            let stem = event
                .as_ref()
                .map(|e| e.name.as_str())
                .unwrap_or("placa_comp");
            let png = sibling_copy_path(&dir, stem, ExternalApp::EffectCraft, "png");
            if let Err(e) =
                export_frame_png_for_photocraft(&self.project, &self.media, self.cursor, None, &png)
            {
                self.set_status(format!("EffectCraft: erro ao exportar quadro base: {e:#}"));
                return;
            }
            png
        };

        self.deliver_with_status(status, &target, "arquivo preparado");
    }

    fn workflow_anicraft(&mut self, status: &AppDiscoveryStatus) {
        let dir = match interchange_work_dir(self.session.path.as_deref(), "anicraft") {
            Ok(d) => d,
            Err(e) => {
                self.set_status(format!("AniCraft: {e:#}"));
                return;
            }
        };
        let scene_path = sibling_copy_path(&dir, "cena", ExternalApp::AniCraft, "anicraft");
        if let Some(ref cli) = status.cli_exe {
            let fps = self.project.settings.fps;
            if let Err(e) = create_anicraft_project(
                cli,
                &scene_path,
                self.project.settings.width,
                self.project.settings.height,
                fps.num,
                fps.den,
            ) {
                self.set_status(format!("anicraft-cli new: {e:#}"));
                return;
            }
            self.integrations.anicraft_file_path = scene_path.to_string_lossy().to_string();
        } else {
            self.set_status(
                "AniCraft CLI/app não detectado localmente. Configure em Preferências > Aplicativos externos.",
            );
            self.integrations.open = true;
            return;
        }
        self.deliver_with_status(status, &scene_path, "cena .anicraft criada");
    }

    fn workflow_filmcraft(&mut self, status: &AppDiscoveryStatus) {
        let dir = match interchange_work_dir(self.session.path.as_deref(), "filmcraft") {
            Ok(d) => d,
            Err(e) => {
                self.set_status(format!("FilmCraft: {e:#}"));
                return;
            }
        };
        let otio_path = sibling_copy_path(&dir, "timeline", ExternalApp::FilmCraft, "otio");
        if let Err(e) = neoveg_engine::otio::export_otio_file(&self.project, &otio_path) {
            self.set_status(format!("Erro ao gerar .otio para o FilmCraft: {e:#}"));
            return;
        }
        self.deliver_with_status(status, &otio_path, "timeline OTIO exportada");
    }

    fn register_copy_as_take_and_deliver(
        &mut self,
        status: &AppDiscoveryStatus,
        copy_path: &Path,
        event: Option<&Event>,
        label: &str,
    ) {
        match media::import_file(copy_path) {
            Ok(imp) => {
                let new_mid = imp.item.id;
                let sr = imp.entry.audio.as_ref().map(|a| a.sample_rate);
                self.media.insert(new_mid, imp.entry);
                self.checkpoint();
                self.session.register_imported_media(imp.item, sr);

                let ev_id = event.map(|e| e.id);
                if let Some(eid) = ev_id {
                    if let Some(ev) = self.project.event_mut(eid) {
                        let take_name = copy_path
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| status.name.clone());
                        let _ = takes::add_take(
                            ev,
                            neoveg_engine::model::Take {
                                media_id: new_mid,
                                offset: Ticks::ZERO,
                                name: take_name,
                            },
                            true,
                        );
                    }
                }
                self.changed();
                self.integrations.watched.push(WatchedExternalEdit::new(
                    status.app,
                    new_mid,
                    ev_id,
                    copy_path.to_path_buf(),
                ));
            }
            Err(e) => {
                self.set_status(format!(
                    "{}: cópia criada em {}, mas falhou ao importar como take: {e:#}",
                    status.name,
                    copy_path.display()
                ));
                return;
            }
        }

        self.deliver_with_status(status, copy_path, label);
    }

    fn deliver_with_status(&mut self, status: &AppDiscoveryStatus, file_path: &Path, label: &str) {
        let timeout = self.prefs.external_apps.rpc_timeout_secs;
        match send_file_to_app(status, file_path, timeout) {
            Ok(DeliveryMethod::LiveControlRpc { port }) => {
                let msg = format!(
                    "{} ({label}): enviado para janela ativa em 127.0.0.1:{port} ({}).",
                    status.name,
                    file_path.display()
                );
                self.integrations.last_message = Some(msg.clone());
                self.set_status(msg);
            }
            Ok(DeliveryMethod::SpawnedDesktop { .. }) => {
                let msg = format!(
                    "{} aberto com {} ({label}).",
                    status.name,
                    file_path.display()
                );
                self.integrations.last_message = Some(msg.clone());
                self.set_status(msg);
            }
            Err(e) => {
                let msg = format!(
                    "Arquivo pronto em {} ({label}). Aviso ao abrir {}: {e:#}",
                    file_path.display(),
                    status.name
                );
                self.integrations.last_message = Some(msg.clone());
                self.set_status(msg);
                self.integrations.open = true;
            }
        }
    }

    /// Exporta script Python limpo do Blender com FPS, resolução, marcadores, Pan-Crop/Track Motion e áudio de referência.
    pub fn workflow_blender_export_script(&mut self, launch_status: Option<&AppDiscoveryStatus>) {
        let picked = rfd::FileDialog::new()
            .set_file_name("neoveg_blender_sync.py")
            .add_filter("Script Python do Blender", &["py"])
            .save_file();
        let Some(py_path) = picked else {
            return;
        };

        let track_ref: Option<Track> = self
            .integration_video_track()
            .and_then(|i| self.project.tracks.get(i))
            .cloned();
        let event_ref = self.first_selected_event();
        let audio_ref = py_path.with_extension("wav");
        let audio_opt = if self.project.duration() > Ticks::ZERO
            && !self.media.audio_map().is_empty()
            && export_timeline_range_bwf(
                &self.project,
                &self.media,
                Ticks::ZERO,
                self.project.duration(),
                &audio_ref,
                0,
            )
            .is_ok()
        {
            Some(audio_ref.as_path())
        } else {
            None
        };

        let plate_opt = event_ref.as_ref().and_then(|ev| {
            self.project
                .media(ev.media_id)
                .and_then(Self::media_item_primary_path)
        });

        match generate_export_to_blender_py(
            &self.project,
            track_ref.as_ref(),
            event_ref.as_ref(),
            audio_opt,
            plate_opt.as_deref(),
        ) {
            Ok(py) => {
                if let Err(e) = std::fs::write(&py_path, py.as_bytes()) {
                    self.set_status(format!("Erro ao salvar script .py: {e}"));
                    return;
                }
                let msg = format!(
                    "Script Blender exportado: {} (abra no Text Editor > Run Script do Blender).",
                    py_path.display()
                );
                self.integrations.last_message = Some(msg.clone());
                self.set_status(msg);
                let _ = launch_status;
            }
            Err(e) => self.set_status(format!("Erro ao gerar script Blender: {e:#}")),
        }
    }

    pub fn blender_render_scene_dialog(&mut self) {
        self.open_integrations_dialog();
        if let Some(p) = rfd::FileDialog::new()
            .add_filter("Cena do Blender", &["blend"])
            .pick_file()
        {
            self.integrations.blender_blend_path = p.to_string_lossy().to_string();
            self.start_probe_blender_scene(p);
        }
    }

    pub fn start_probe_blender_scene(&mut self, blend_path: PathBuf) {
        let status = self.app_status(ExternalApp::Blender);
        let Some(exe) = status.cli_exe.or(status.desktop_exe) else {
            self.set_status(
                "Blender não encontrado. Configure o caminho em Preferências > Aplicativos externos.",
            );
            return;
        };
        let timeout = self.prefs.external_apps.rpc_timeout_secs;
        let tx = self.integrations.tx.clone();
        self.integrations.probing = true;
        if util::spawn("probe-blender", move || {
            let r = util::guard("sondagem Blender", || {
                probe_blend_scene(&exe, &blend_path, timeout).map_err(|e| format!("{e:#}"))
            })
            .and_then(|inner| inner);
            let _ = tx.send(IntegrationMsg::BlendProbed(r));
        })
        .is_err()
        {
            self.integrations.probing = false;
        }
    }

    pub fn start_render_blender_to_timeline(&mut self) {
        if self.integrations.blender_job.is_some() {
            return;
        }
        let blend_file = PathBuf::from(self.integrations.blender_blend_path.trim());
        if !blend_file.is_file() {
            self.set_status("Selecione um arquivo .blend válido primeiro.");
            return;
        }
        let status = self.app_status(ExternalApp::Blender);
        let Some(exe) = status.cli_exe.or(status.desktop_exe) else {
            self.set_status("Executável do Blender não configurado.");
            return;
        };
        let Ok(work_dir) = interchange_work_dir(self.session.path.as_deref(), "blender_renders")
        else {
            self.set_status("Falha ao criar pasta de renderização do Blender.");
            return;
        };
        let stem = blend_file
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("cena");
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let out_dir = work_dir.join(format!("{stem}_{stamp}"));

        let f_start = self.integrations.blender_frame_start.max(1);
        let f_end = self.integrations.blender_frame_end.max(f_start);
        let total = (f_end - f_start + 1).max(1) as u64;
        let done = Arc::new(AtomicU64::new(0));
        let cancel = Arc::new(AtomicBool::new(false));

        let req = BlenderRenderRequest {
            blender_exe: exe,
            blend_file: blend_file.clone(),
            out_dir,
            scene_name: if self.integrations.blender_scene_name.trim().is_empty() {
                None
            } else {
                Some(self.integrations.blender_scene_name.clone())
            },
            frame_start: Some(f_start),
            frame_end: Some(f_end),
            format: "PNG".to_string(),
            timeout_secs: self.prefs.external_apps.rpc_timeout_secs.max(120),
        };

        let fps = self
            .integrations
            .blender_scene_info
            .as_ref()
            .and_then(|i| FrameRate::from_f64(i.fps.max(1) as f64))
            .unwrap_or(self.project.settings.fps);

        self.integrations.blender_job = Some(BlenderRenderJobUi {
            scene_label: stem.to_string(),
            done: done.clone(),
            total,
            cancel: cancel.clone(),
        });
        self.loading += 1;

        let tx = self.integrations.tx.clone();
        let label = format!("{stem} ({f_start}..{f_end})");
        if spawn_sequence_render(
            tx,
            SequenceRenderSpec {
                thread_name: "render-blender",
                guard_label: "renderização Blender",
                app: ExternalApp::Blender,
                label,
                fps,
                place: true,
            },
            move || {
                let frames =
                    render_blend_scene(&req, &done, &cancel).map_err(|e| format!("{e:#}"))?;
                frames
                    .into_iter()
                    .next()
                    .ok_or_else(|| "nenhum quadro PNG encontrado".to_string())
            },
        )
        .is_err()
        {
            self.loading = self.loading.saturating_sub(1);
            self.integrations.blender_job = None;
        } else {
            self.set_status("Renderizando cena Blender em segundo plano...");
        }
    }

    pub fn blender_import_camera_dialog(&mut self) {
        if let Some(p) = rfd::FileDialog::new()
            .add_filter("Câmera / Marcadores Blender", &["json"])
            .pick_file()
        {
            match load_camera_json(&p) {
                Ok(cam) => {
                    let msg = self.apply_imported_blender_camera(&cam);
                    self.integrations.last_message = Some(msg.clone());
                    self.set_status(msg);
                }
                Err(e) => self.set_status(format!("JSON de câmera Blender inválido: {e:#}")),
            }
        }
    }

    pub fn start_extract_camera_from_blend(&mut self) {
        let blend_file = PathBuf::from(self.integrations.blender_blend_path.trim());
        if !blend_file.is_file() {
            self.set_status("Selecione um arquivo .blend válido para extrair câmera.");
            return;
        }
        let status = self.app_status(ExternalApp::Blender);
        let Some(exe) = status.cli_exe.or(status.desktop_exe) else {
            self.set_status("Blender não encontrado nas Preferências.");
            return;
        };
        let timeout = self.prefs.external_apps.rpc_timeout_secs;
        let tx = self.integrations.tx.clone();
        self.integrations.probing = true;
        if util::spawn("extract-blender-cam", move || {
            let res = util::guard("extrair câmera Blender", || {
                extract_camera_from_blend(&exe, &blend_file, timeout).map_err(|e| format!("{e:#}"))
            })
            .and_then(|r| r);
            let _ = tx.send(IntegrationMsg::BlenderCameraImported(res));
        })
        .is_err()
        {
            self.integrations.probing = false;
        }
    }

    pub fn start_render_anicraft_to_timeline(&mut self) {
        let file = PathBuf::from(self.integrations.anicraft_file_path.trim());
        if !file.is_file() {
            self.set_status("Escolha um projeto .anicraft válido primeiro.");
            return;
        }
        let status = self.app_status(ExternalApp::AniCraft);
        let Some(cli) = status.cli_exe else {
            self.set_status("anicraft-cli não encontrado.");
            return;
        };
        let Ok(work_dir) = interchange_work_dir(self.session.path.as_deref(), "anicraft_renders")
        else {
            return;
        };
        let stem = file
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("animacao")
            .to_string();
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let out_dir = work_dir.join(format!("{stem}_{stamp}"));
        let fps = self.project.settings.fps;
        let tx = self.integrations.tx.clone();
        self.loading += 1;
        if spawn_sequence_render(
            tx,
            SequenceRenderSpec {
                thread_name: "render-anicraft",
                guard_label: "render AniCraft",
                app: ExternalApp::AniCraft,
                label: stem,
                fps,
                place: true,
            },
            move || {
                let seq = render_anicraft_sequence(&cli, &file, &out_dir)
                    .map_err(|e| format!("{e:#}"))?;
                seq.frames
                    .into_iter()
                    .next()
                    .ok_or_else(|| "nenhum quadro PNG gerado".to_string())
            },
        )
        .is_err()
        {
            self.loading = self.loading.saturating_sub(1);
        }
    }

    pub fn start_render_effectcraft_to_timeline(&mut self) {
        let file = PathBuf::from(self.integrations.effectcraft_file_path.trim());
        if !file.is_file() {
            self.set_status("Escolha um projeto .ecproj válido primeiro.");
            return;
        }
        let status = self.app_status(ExternalApp::EffectCraft);
        let Some(cli) = status.cli_exe else {
            self.set_status("effectcraft-cli não encontrado.");
            return;
        };
        let Ok(work_dir) =
            interchange_work_dir(self.session.path.as_deref(), "effectcraft_renders")
        else {
            return;
        };
        let comp = if self.integrations.effectcraft_comp_name.trim().is_empty() {
            None
        } else {
            Some(self.integrations.effectcraft_comp_name.clone())
        };
        let stem = file
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("comp")
            .to_string();
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let out_dir = work_dir.join(format!("{stem}_{stamp}"));
        let fps = self.project.settings.fps;
        let tx = self.integrations.tx.clone();
        self.loading += 1;
        if spawn_sequence_render(
            tx,
            SequenceRenderSpec {
                thread_name: "render-effectcraft",
                guard_label: "render EffectCraft",
                app: ExternalApp::EffectCraft,
                label: stem,
                fps,
                place: true,
            },
            move || {
                let frames = render_effectcraft_sequence(&cli, &file, comp.as_deref(), &out_dir)
                    .map_err(|e| format!("{e:#}"))?;
                frames
                    .into_iter()
                    .next()
                    .ok_or_else(|| "nenhum quadro PNG gerado".to_string())
            },
        )
        .is_err()
        {
            self.loading = self.loading.saturating_sub(1);
        }
    }

    pub(crate) fn show_integrations_window(&mut self, ctx: &egui::Context) {
        if !self.integrations.open {
            return;
        }
        if self.integrations.report.is_none() {
            self.integrations.report = Some(discover_all(&self.prefs.external_apps, false));
        }

        let mut open = self.integrations.open;
        let mut send_app: Option<ExternalApp> = None;
        let mut open_prefs = false;
        let mut rescan_live = false;
        let mut pick_blend = false;
        let mut do_probe_blend = false;
        let mut do_render_blend = false;
        let mut do_extract_cam = false;
        let mut do_export_py = false;
        let mut do_save_extractor = false;
        let mut do_import_cam = false;
        let mut pick_ani = false;
        let mut do_render_ani = false;
        let mut pick_ec = false;
        let mut do_render_ec = false;
        let mut reload_now: Option<(ExternalApp, Id, PathBuf)> = None;
        let mut remove_watch_idx: Option<usize> = None;

        let report = self.integrations.report.clone();
        let theme = self.theme.clone();
        let screen = ctx.screen_rect().size();
        let max_w = if screen.x.is_finite() {
            (screen.x - 24.0).clamp(320.0, 920.0)
        } else {
            760.0
        };
        let max_h = if screen.y.is_finite() {
            (screen.y - 36.0).clamp(260.0, 720.0)
        } else {
            580.0
        };

        crate::dialog_window::DialogWindow::new("Integrações (ArtCraft & Blender)")
            .id(egui::Id::new("integrations_win"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .constrain_to(ctx.screen_rect())
            .default_size([740.0_f32.min(max_w), 540.0_f32.min(max_h)])
            .max_size([max_w, max_h])
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    icons::paint_inline(ui, Icon::ExternalApps, 16.0, &theme);
                    ui.label(
                        egui::RichText::new("Ecossistema ArtCraft & Blender (Sala Limpa)").strong(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Configurar caminhos...").clicked() {
                            open_prefs = true;
                        }
                        if ui
                            .add_enabled(
                                !self.integrations.probing,
                                egui::Button::new("Reescanear apps / portas RPC"),
                            )
                            .clicked()
                        {
                            rescan_live = true;
                        }
                    });
                });
                if let Some(ref msg) = self.integrations.last_message {
                    ui.label(egui::RichText::new(msg).small().color(theme.accent));
                }
                ui.separator();

                egui::ScrollArea::vertical()
                    .id_salt("integrations_studio_scroll")
                    .max_height((max_h - 90.0).max(150.0))
                    .show(ui, |ui| {
                        if let Some(ref rep) = report {
                            egui::Grid::new("integrations_apps_grid")
                                .striped(true)
                                .num_columns(4)
                                .spacing([10.0, 6.0])
                                .show(ui, |ui| {
                                    ui.label(egui::RichText::new("Aplicativo").strong());
                                    ui.label(egui::RichText::new("Função").strong());
                                    ui.label(egui::RichText::new("Estado / Canal").strong());
                                    ui.label(egui::RichText::new("Fluxo").strong());
                                    ui.end_row();

                                    for st in &rep.apps {
                                        ui.label(egui::RichText::new(&st.name).strong())
                                            .on_hover_text(st.app.workflow_hint_pt());
                                        ui.label(st.app.category_pt());
                                        let badge = if st.live_reachable {
                                            egui::RichText::new(format!(
                                                "Ativo RPC :{}",
                                                st.live_port.unwrap_or(0)
                                            ))
                                            .color(theme.accent)
                                        } else if st.available() {
                                            egui::RichText::new("Detectado")
                                                .color(theme.text_primary)
                                        } else {
                                            egui::RichText::new("Não encontrado")
                                                .color(theme.text_muted)
                                        };
                                        ui.label(badge);

                                        let btn_lbl = match st.app {
                                            ExternalApp::PhotoCraft => "Enviar quadro/imagem",
                                            ExternalApp::VectorCraft => "Enviar vetor SVG",
                                            ExternalApp::SoundCraft => "Enviar áudio BWF",
                                            ExternalApp::EffectCraft => "Enviar para comp",
                                            ExternalApp::AniCraft => "Criar cena .anicraft",
                                            ExternalApp::FilmCraft => "Enviar timeline OTIO",
                                            ExternalApp::Blender => "Exportar script .py",
                                        };
                                        if ui.small_button(btn_lbl).clicked() {
                                            send_app = Some(st.app);
                                        }
                                        ui.end_row();
                                    }
                                });
                        }

                        ui.add_space(8.0);
                        ui.separator();
                        ui.heading("Blender 3D — Renderização, Câmera e Marcadores");
                        ui.horizontal(|ui| {
                            ui.label("Cena (.blend):");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.integrations.blender_blend_path)
                                    .desired_width(280.0),
                            );
                            if ui.button("Procurar...").clicked() {
                                pick_blend = true;
                            }
                            if ui
                                .add_enabled(
                                    !self.integrations.probing,
                                    egui::Button::new("Inspecionar"),
                                )
                                .clicked()
                            {
                                do_probe_blend = true;
                            }
                        });
                        ui.horizontal_wrapped(|ui| {
                            ui.label("Cena:");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.integrations.blender_scene_name)
                                    .desired_width(110.0),
                            );
                            ui.label("Quadros:");
                            ui.add(
                                egui::DragValue::new(&mut self.integrations.blender_frame_start)
                                    .range(1..=100_000),
                            );
                            ui.label("até");
                            ui.add(
                                egui::DragValue::new(&mut self.integrations.blender_frame_end)
                                    .range(1..=100_000),
                            );
                            if ui
                                .add_enabled(
                                    self.integrations.blender_job.is_none(),
                                    egui::Button::new("Renderizar PNGs -> Timeline"),
                                )
                                .clicked()
                            {
                                do_render_blend = true;
                            }
                        });
                        if let Some(ref job) = self.integrations.blender_job {
                            let d = job.done.load(Ordering::Relaxed);
                            let frac = (d as f32 / job.total.max(1) as f32).clamp(0.0, 1.0);
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::ProgressBar::new(frac).text(format!(
                                        "Blender '{}': quadro {d}/{}",
                                        job.scene_label, job.total
                                    )),
                                );
                                if ui.small_button("Cancelar").clicked() {
                                    job.cancel.store(true, Ordering::Relaxed);
                                }
                            });
                        }
                        ui.horizontal_wrapped(|ui| {
                            if ui.button("Exportar Timeline/Pan-Crop -> .py").clicked() {
                                do_export_py = true;
                            }
                            if ui.button("Extrair câmera do .blend -> Timeline").clicked() {
                                do_extract_cam = true;
                            }
                            if ui.button("Importar câmera JSON...").clicked() {
                                do_import_cam = true;
                            }
                            if ui.button("Salvar script extrator (.py)...").clicked() {
                                do_save_extractor = true;
                            }
                        });

                        ui.add_space(6.0);
                        ui.separator();
                        ui.heading("AniCraft & EffectCraft — Render Direto para Timeline");
                        ui.horizontal(|ui| {
                            ui.label("Projeto .anicraft:");
                            ui.add(
                                egui::TextEdit::singleline(
                                    &mut self.integrations.anicraft_file_path,
                                )
                                .desired_width(240.0),
                            );
                            if ui.button("Escolher...").clicked() {
                                pick_ani = true;
                            }
                            if ui.button("Renderizar sequência -> Timeline").clicked() {
                                do_render_ani = true;
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.label("Projeto .ecproj:");
                            ui.add(
                                egui::TextEdit::singleline(
                                    &mut self.integrations.effectcraft_file_path,
                                )
                                .desired_width(190.0),
                            );
                            ui.label("Comp:");
                            ui.add(
                                egui::TextEdit::singleline(
                                    &mut self.integrations.effectcraft_comp_name,
                                )
                                .desired_width(80.0),
                            );
                            if ui.button("Escolher...").clicked() {
                                pick_ec = true;
                            }
                            if ui.button("Renderizar PNGs -> Timeline").clicked() {
                                do_render_ec = true;
                            }
                        });

                        ui.add_space(6.0);
                        ui.separator();
                        ui.heading("Cópias em edição monitoradas (Round-Trip / Takes)");
                        if self.integrations.watched.is_empty() {
                            ui.label(
                                egui::RichText::new(
                                    "Nenhuma cópia externa monitorada no momento. Use 'Enviar quadro/imagem' ou 'Enviar áudio BWF'.",
                                )
                                .small()
                                .color(theme.text_muted),
                            );
                        } else {
                            let mut idx = 0usize;
                            while idx < self.integrations.watched.len() {
                                let w = &mut self.integrations.watched[idx];
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(egui::RichText::new(w.app.name()).strong());
                                    ui.label(
                                        w.path
                                            .file_name()
                                            .map(|n| n.to_string_lossy().to_string())
                                            .unwrap_or_default(),
                                    );
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "({} recarga(s))",
                                            w.reload_count
                                        ))
                                        .small()
                                        .color(theme.text_muted),
                                    );
                                    if ui.small_button("Recarregar").clicked() {
                                        reload_now = Some((w.app, w.media_id, w.path.clone()));
                                    }
                                    let p_lbl = if w.paused { "Retomar" } else { "Pausar" };
                                    if ui.small_button(p_lbl).clicked() {
                                        w.paused = !w.paused;
                                    }
                                    if ui.small_button("Remover").clicked() {
                                        remove_watch_idx = Some(idx);
                                    }
                                });
                                idx += 1;
                            }
                        }
                    });
            });

        self.integrations.open = open;

        if open_prefs {
            self.dlg.prefs_tab = PrefsCategory::Integrations;
            self.dlg.open_prefs(&self.prefs);
        }
        if rescan_live {
            self.refresh_integrations_async(true);
        }
        if let Some(app) = send_app {
            self.send_to_external_app(app, None, None);
        }
        if pick_blend {
            if let Some(p) = rfd::FileDialog::new()
                .add_filter("Cena Blender", &["blend"])
                .pick_file()
            {
                self.integrations.blender_blend_path = p.to_string_lossy().to_string();
                self.start_probe_blender_scene(p);
            }
        }
        if do_probe_blend {
            let p = PathBuf::from(self.integrations.blender_blend_path.trim());
            self.start_probe_blender_scene(p);
        }
        if do_render_blend {
            self.start_render_blender_to_timeline();
        }
        if do_export_py {
            self.workflow_blender_export_script(None);
        }
        if do_extract_cam {
            self.start_extract_camera_from_blend();
        }
        if do_import_cam {
            self.blender_import_camera_dialog();
        }
        if do_save_extractor {
            if let Some(p) = rfd::FileDialog::new()
                .set_file_name("extrair_camera_neoveg.py")
                .add_filter("Script Python", &["py"])
                .save_file()
            {
                if let Ok(py) = generate_camera_extractor_py(Some(&p.with_extension("json"))) {
                    if let Err(e) = std::fs::write(&p, py.as_bytes()) {
                        self.set_status(format!("Erro ao salvar extrator: {e}"));
                    } else {
                        self.set_status(format!("Extrator salvo em {}", p.display()));
                    }
                }
            }
        }
        if pick_ani {
            if let Some(p) = rfd::FileDialog::new()
                .add_filter("AniCraft", &["anicraft"])
                .pick_file()
            {
                self.integrations.anicraft_file_path = p.to_string_lossy().to_string();
            }
        }
        if do_render_ani {
            self.start_render_anicraft_to_timeline();
        }
        if pick_ec {
            if let Some(p) = rfd::FileDialog::new()
                .add_filter("EffectCraft", &["ecproj"])
                .pick_file()
            {
                self.integrations.effectcraft_file_path = p.to_string_lossy().to_string();
            }
        }
        if do_render_ec {
            self.start_render_effectcraft_to_timeline();
        }
        if let Some((app, mid, path)) = reload_now {
            self.reload_watched_media(app, mid, &path);
        }
        if let Some(idx) = remove_watch_idx {
            if idx < self.integrations.watched.len() {
                self.integrations.watched.remove(idx);
            }
        }
        let _ = PlaceTarget::At(Ticks::ZERO);
    }
}
