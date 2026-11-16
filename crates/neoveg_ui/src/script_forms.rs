//! Janelas pedidas por scripts .NET pela camada de Forms própria (Linux,
//! macOS e Windows sem o runtime WindowsDesktop): `MessageBox`,
//! `Form.ShowDialog` e diálogos de arquivo, desenhados pelo Neoveg.
//!
//! O script roda numa thread própria (`ScriptJob` em `app.rs`) e espera a
//! resposta em [`UiBridge::request`]; a interface recebe o pedido por um
//! canal, desenha a janela como modal e responde. Se a interface sumir no
//! meio, o script recebe [`ScriptUiReply::NoUi`] e segue com o padrão.

use eframe::egui;
use neoveg_scripting::{
    ControlKind, ControlValue, FileDialogMode, FileDialogSpec, FormSpec, MessageButtons, ScriptUi,
    ScriptUiReply, ScriptUiRequest,
};
use std::sync::mpsc::{channel, sync_channel, Receiver, Sender, SyncSender};
use std::sync::Arc;

/// Um pedido do script esperando resposta.
pub(crate) struct UiCall {
    req: ScriptUiRequest,
    reply: SyncSender<ScriptUiReply>,
}

/// Lado do script: manda o pedido à interface e espera.
pub(crate) struct UiBridge {
    tx: Sender<UiCall>,
}

impl ScriptUi for UiBridge {
    fn request(&self, req: ScriptUiRequest) -> ScriptUiReply {
        let (reply, rx) = sync_channel(1);
        if self.tx.send(UiCall { req, reply }).is_err() {
            return ScriptUiReply::NoUi;
        }
        rx.recv().unwrap_or(ScriptUiReply::NoUi)
    }
}

pub(crate) fn bridge() -> (Arc<UiBridge>, Receiver<UiCall>) {
    let (tx, rx) = channel();
    (Arc::new(UiBridge { tx }), rx)
}

/// Janela do script aberta na interface.
pub(crate) enum Active {
    Message {
        call: UiCall,
    },
    Form {
        call: UiCall,
        values: Vec<ControlValue>,
    },
}

impl Active {
    /// Começa a atender um pedido. Diálogos de arquivo são nativos e
    /// respondem na hora (`None`).
    pub(crate) fn start(call: UiCall) -> Option<Self> {
        match &call.req {
            ScriptUiRequest::FileDialog(spec) => {
                let reply = run_file_dialog(spec);
                let _ = call.reply.send(reply);
                None
            }
            ScriptUiRequest::MessageBox { .. } => Some(Self::Message { call }),
            ScriptUiRequest::Form(f) => {
                let values = f
                    .controls
                    .iter()
                    .map(|c| ControlValue {
                        id: c.id,
                        text: c.text.clone(),
                        checked: c.checked,
                        selected: c.selected,
                        value: c.value,
                    })
                    .collect();
                Some(Self::Form { call, values })
            }
        }
    }

    /// Desenha a janela; `true` quando respondeu (a janela fechou ou o
    /// script precisa rodar um handler).
    pub(crate) fn show(&mut self, ctx: &egui::Context) -> bool {
        let reply = match self {
            Self::Message { call } => match &call.req {
                ScriptUiRequest::MessageBox {
                    text,
                    caption,
                    buttons,
                    icon,
                } => message_box(ctx, text, caption, *buttons, icon),
                _ => Some(ScriptUiReply::NoUi),
            },
            Self::Form { call, values } => match &call.req {
                ScriptUiRequest::Form(spec) => form(ctx, spec, values),
                _ => Some(ScriptUiReply::NoUi),
            },
        };
        match (reply, self) {
            (Some(r), Self::Message { call } | Self::Form { call, .. }) => {
                let _ = call.reply.send(r);
                true
            }
            _ => false,
        }
    }
}

fn button_label(result: &str) -> &'static str {
    match result {
        "OK" => "OK",
        "Cancel" => "Cancelar",
        "Yes" => "Sim",
        "No" => "Não",
        "Abort" => "Anular",
        "Retry" => "Repetir",
        "Ignore" => "Ignorar",
        "TryAgain" => "Tentar novamente",
        "Continue" => "Continuar",
        _ => "OK",
    }
}

fn message_box(
    ctx: &egui::Context,
    text: &str,
    caption: &str,
    buttons: MessageButtons,
    icon: &str,
) -> Option<ScriptUiReply> {
    let results = buttons.results();
    let mut chosen: Option<&str> = None;
    egui::Modal::new(egui::Id::new("script_message_box")).show(ctx, |ui| {
        ui.set_max_width(460.0);
        if !caption.is_empty() {
            ui.strong(caption);
            ui.separator();
        }
        ui.horizontal(|ui| {
            let glyph = match icon {
                "Error" => "✖",
                "Warning" => "⚠",
                "Question" => "?",
                "Information" => "ℹ",
                _ => "",
            };
            if !glyph.is_empty() {
                ui.label(egui::RichText::new(glyph).size(22.0));
            }
            ui.add(egui::Label::new(text).wrap());
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            for r in results {
                if ui.button(button_label(r)).clicked() {
                    chosen = Some(r);
                }
            }
        });
        // Enter: primeiro botão; Esc: Cancelar se houver (ou o único botão)
        if chosen.is_none() {
            if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                chosen = results.first().copied();
            } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                chosen = if results.contains(&"Cancel") {
                    Some("Cancel")
                } else if results.len() == 1 {
                    results.first().copied()
                } else {
                    None
                };
            }
        }
    });
    chosen.map(|r| ScriptUiReply::MessageBox {
        result: r.to_string(),
    })
}

fn form(
    ctx: &egui::Context,
    spec: &FormSpec,
    values: &mut [ControlValue],
) -> Option<ScriptUiReply> {
    let mut clicked: Option<u32> = None;
    let mut closed = false;
    egui::Modal::new(egui::Id::new("script_form")).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.strong(if spec.title.is_empty() {
                "Script"
            } else {
                spec.title.as_str()
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("✖").on_hover_text("Fechar").clicked() {
                    closed = true;
                }
            });
        });
        ui.separator();
        let (area, _) =
            ui.allocate_exact_size(egui::vec2(spec.width, spec.height), egui::Sense::hover());
        let origin = area.min;
        let mut radio_clicked: Option<usize> = None;
        for (i, c) in spec.controls.iter().enumerate() {
            let (Some(kind), true) = (c.kind, c.visible) else {
                continue;
            };
            let Some(v) = values.get_mut(i) else {
                continue;
            };
            let r = egui::Rect::from_min_size(
                origin + egui::vec2(c.x, c.y),
                egui::vec2(c.w.max(8.0), c.h.max(8.0)),
            );
            let mut changed = false;
            let mut hit = false;
            ui.add_enabled_ui(c.enabled, |ui| match kind {
                ControlKind::Label => {
                    ui.put(r, egui::Label::new(&c.text).wrap());
                }
                ControlKind::GroupBox => {
                    ui.painter().rect_stroke(
                        r,
                        2.0,
                        ui.visuals().widgets.noninteractive.bg_stroke,
                        egui::StrokeKind::Inside,
                    );
                    ui.painter().text(
                        r.min + egui::vec2(6.0, 2.0),
                        egui::Align2::LEFT_TOP,
                        &c.text,
                        egui::FontId::proportional(12.0),
                        ui.visuals().text_color(),
                    );
                }
                ControlKind::Panel => {}
                ControlKind::TextBox => {
                    let te = if c.multiline {
                        egui::TextEdit::multiline(&mut v.text)
                    } else {
                        egui::TextEdit::singleline(&mut v.text)
                    };
                    let resp = ui.put(r, te.password(c.password).interactive(!c.read_only));
                    changed = resp.changed();
                }
                ControlKind::Button => {
                    hit = ui.put(r, egui::Button::new(&c.text)).clicked();
                }
                ControlKind::CheckBox => {
                    let resp = ui.put(r, egui::Checkbox::new(&mut v.checked, &c.text));
                    changed = resp.changed();
                    hit = resp.clicked();
                }
                ControlKind::RadioButton => {
                    if ui
                        .put(r, egui::RadioButton::new(v.checked, &c.text))
                        .clicked()
                        && !v.checked
                    {
                        v.checked = true;
                        radio_clicked = Some(i);
                        changed = true;
                    }
                }
                ControlKind::ComboBox => {
                    let before = v.selected;
                    let sel_text = usize::try_from(v.selected)
                        .ok()
                        .and_then(|k| c.items.get(k))
                        .cloned()
                        .unwrap_or_default();
                    ui.scope_builder(egui::UiBuilder::new().max_rect(r), |ui| {
                        egui::ComboBox::from_id_salt(("script_combo", c.id))
                            .width(r.width())
                            .selected_text(sel_text)
                            .show_ui(ui, |ui| {
                                for (k, item) in c.items.iter().enumerate() {
                                    let k = i32::try_from(k).unwrap_or(i32::MAX);
                                    ui.selectable_value(&mut v.selected, k, item);
                                }
                            });
                    });
                    changed = v.selected != before;
                }
                ControlKind::ListBox => {
                    let before = v.selected;
                    ui.scope_builder(egui::UiBuilder::new().max_rect(r), |ui| {
                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            egui::ScrollArea::vertical()
                                .id_salt(("script_list", c.id))
                                .max_height((r.height() - 8.0).max(16.0))
                                .show(ui, |ui| {
                                    ui.set_min_width(r.width() - 12.0);
                                    for (k, item) in c.items.iter().enumerate() {
                                        let k = i32::try_from(k).unwrap_or(i32::MAX);
                                        ui.selectable_value(&mut v.selected, k, item);
                                    }
                                });
                        });
                    });
                    changed = v.selected != before;
                }
                ControlKind::Numeric => {
                    let resp = ui.put(
                        r,
                        egui::DragValue::new(&mut v.value)
                            .range(c.minimum..=c.maximum)
                            .speed(c.increment)
                            .fixed_decimals(c.decimals as usize),
                    );
                    changed = resp.changed();
                }
            });
            // botões sempre avisam; os outros, só se o script tem handlers
            if hit && (kind == ControlKind::Button || c.notify) || changed && c.notify {
                clicked.get_or_insert(c.id);
            }
        }
        // RadioButton: um marcado por contêiner
        if let Some(i) = radio_clicked {
            let parent = spec.controls.get(i).and_then(|c| c.parent);
            for (k, c) in spec.controls.iter().enumerate() {
                if k != i && c.kind == Some(ControlKind::RadioButton) && c.parent == parent {
                    if let Some(v) = values.get_mut(k) {
                        v.checked = false;
                    }
                }
            }
        }
        if clicked.is_none() && !closed {
            let multiline_focus = spec
                .controls
                .iter()
                .any(|c| c.multiline && c.kind == Some(ControlKind::TextBox))
                && ui.ctx().memory(|m| m.focused().is_some());
            if !multiline_focus && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                clicked = spec.accept;
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                match spec.cancel {
                    Some(id) => clicked = Some(id),
                    None => closed = true,
                }
            }
        }
    });
    if clicked.is_none() && !closed {
        return None;
    }
    Some(ScriptUiReply::Form {
        closed,
        result: closed.then(|| "Cancel".to_string()),
        clicked,
        values: values.to_vec(),
    })
}

fn run_file_dialog(spec: &FileDialogSpec) -> ScriptUiReply {
    let mut d = rfd::FileDialog::new();
    if !spec.title.is_empty() {
        d = d.set_title(&spec.title);
    }
    for (name, exts) in spec.filters() {
        d = d.add_filter(&name, &exts);
    }
    let dir = std::path::Path::new(&spec.initial_dir);
    if !spec.initial_dir.is_empty() && dir.is_dir() {
        d = d.set_directory(dir);
    }
    if !spec.file_name.is_empty() {
        let name = std::path::Path::new(&spec.file_name)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if !name.is_empty() {
            d = d.set_file_name(name);
        }
    }
    let paths: Vec<std::path::PathBuf> = match spec.mode {
        FileDialogMode::Open if spec.multiselect => d.pick_files().unwrap_or_default(),
        FileDialogMode::Open => d.pick_file().into_iter().collect(),
        FileDialogMode::Save => d.save_file().into_iter().collect(),
        FileDialogMode::Folder => d.pick_folder().into_iter().collect(),
    };
    ScriptUiReply::File {
        ok: !paths.is_empty(),
        paths: paths
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect(),
    }
}
