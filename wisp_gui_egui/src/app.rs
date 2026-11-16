// SPDX-License-Identifier: MIT OR Apache-2.0
use crate::{
    canvas::{lab::CanvasLab, snarl_adapter::GraphCanvas, state::CanvasTab},
    editor::{
        commands::{EditorCommand, EditorEvent, RequestId, RuntimeStatus, same_edit},
        document::DocumentSnapshot,
        service::{EditorServiceHandle, spawn_configured_service},
    },
};
use std::path::PathBuf;

#[derive(Clone, Debug, clap::Parser)]
#[command(about = "TwistedWiSP native graph editor ")]
pub struct Options {
    #[arg(long)]
    pub flow: Option<PathBuf>,
    #[arg(long, default_value_os_t = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../wisp/core"))]
    pub core_lib_path: PathBuf,
    /// Forbid audio and MIDI initialization.
    #[arg(long)]
    pub offline: bool,
    #[arg(long)]
    pub audio_host: Option<String>,
    #[arg(long)]
    pub audio_device: Option<String>,
    #[arg(long, value_parser = clap::value_parser!(u16).range(1..))]
    pub audio_output_channels: Option<u16>,
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    pub audio_buffer_size: Option<u32>,
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    pub audio_sample_rate: Option<u32>,
    #[arg(long)]
    pub midi_in_port: Option<String>,
    /// Reserved search paths for milestone 4 buffer binding.
    #[arg(long)]
    pub data_path: Vec<PathBuf>,
}
enum PendingAction {
    Open(PathBuf),
    New(String),
    Close,
}
pub struct WispGuiApp {
    service: EditorServiceHandle,
    runtime_status: RuntimeStatus,
    offline: bool,
    operation: Option<RequestId>,
    snapshot: Option<DocumentSnapshot>,
    canvas: GraphCanvas,
    canvas_revision: Option<(crate::editor::document::DocumentId, u64)>,
    lab: CanvasLab,
    tab: CanvasTab,
    load_path: String,
    errors: Vec<String>,
    next_req: u64,
    pending: Option<PendingAction>,
    allow_close: bool,
    new_name: String,
    awaiting: std::collections::BTreeSet<u64>,
    queued: Vec<EditorCommand>,
}
impl WispGuiApp {
    pub fn new(cc: &eframe::CreationContext<'_>, options: Options) -> Self {
        let config = wisp::TwistedWispEngineConfig {
            audio_host: options.audio_host,
            audio_device: options.audio_device,
            audio_output_channels: options.audio_output_channels,
            audio_buffer_size: options.audio_buffer_size,
            audio_sample_rate: options.audio_sample_rate,
            midi_in_port: options.midi_in_port,
            core_path: Some(options.core_lib_path.clone()),
        };
        let service = spawn_configured_service(
            options.core_lib_path,
            cc.egui_ctx.clone(),
            options.offline,
            config,
            crate::editor::runtime::live_factory(),
        );
        let mut app = Self {
            service,
            runtime_status: if options.offline {
                RuntimeStatus::Offline
            } else {
                RuntimeStatus::Stopped
            },
            offline: options.offline,
            operation: None,
            snapshot: None,
            canvas: GraphCanvas::default(),
            canvas_revision: None,
            lab: CanvasLab::default(),
            tab: CanvasTab::Graph,
            load_path: options
                .flow
                .as_ref()
                .map_or_else(String::new, |p| p.to_string_lossy().into_owned()),
            errors: vec![],
            next_req: 1,
            pending: None,
            allow_close: false,
            new_name: "untitled".into(),
            awaiting: Default::default(),
            queued: vec![],
        };
        if let Some(path) = options.flow {
            app.open(path);
        }
        app
    }
    fn request(&mut self) -> RequestId {
        let id = RequestId(self.next_req);
        self.next_req += 1;
        id
    }
    fn open(&mut self, path: PathBuf) {
        let req_id = self.request();
        if let Err(e) = self
            .service
            .submit(EditorCommand::LoadFlow { path, req_id })
        {
            self.error(e);
        } else {
            self.operation = Some(req_id);
        }
    }
    fn error(&mut self, message: String) {
        if self.errors.len() == 16 {
            self.errors.remove(0);
        }
        self.errors.push(message);
    }
    fn dirty(&self) -> bool {
        self.snapshot.as_ref().is_some_and(|s| {
            s.dirty
                || self.operation.is_some()
                || !self.queued.is_empty()
                || !self.awaiting.is_empty()
        })
    }
    fn action(&mut self, action: PendingAction, ctx: &egui::Context) {
        if self.dirty() || !self.queued.is_empty() {
            self.pending = Some(action);
        } else {
            self.perform(action, ctx);
        }
    }
    fn perform(&mut self, action: PendingAction, ctx: &egui::Context) {
        match action {
            PendingAction::Open(path) => self.open(path),
            PendingAction::New(name) => {
                let req_id = self.request();
                match self.service.submit(EditorCommand::NewFlow { name, req_id }) {
                    Ok(()) => self.operation = Some(req_id),
                    Err(e) => self.error(e),
                }
            }
            PendingAction::Close => {
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
    fn poll(&mut self, _ctx: &egui::Context) {
        while let Ok(event) = self.service.rx.try_recv() {
            match event {
                EditorEvent::Response { req_id, result } => {
                    self.awaiting.remove(&req_id.0);
                    if self.operation == Some(req_id) {
                        self.operation = None;
                        self.canvas.edit_result(result.is_ok());
                        self.canvas_revision = None;
                    }
                    if let Err(error) = result {
                        self.error(error);
                    }
                }
                EditorEvent::Error(error) => self.error(error),
                EditorEvent::Runtime { status, .. } => self.runtime_status = status,
            }
        }

        if let Some(snapshot) = self.service.take_snapshot() {
            self.snapshot = Some(snapshot);
        }
        // Do not overwrite local slider state with older service snapshots during a drag.
        if let Some(snapshot) = &self.snapshot {
            let revision = (snapshot.id, snapshot.revision);
            if self.canvas_revision != Some(revision) {
                self.canvas.sync(snapshot);
                self.canvas_revision = Some(revision);
            }
        }
        let queued = std::mem::take(&mut self.queued);
        let mut commands = queued.into_iter();
        while let Some(command) = commands.next() {
            match self.service.tx.try_send(command) {
                Ok(()) => {}
                Err(crossbeam::channel::TrySendError::Full(command)) => {
                    self.queued.push(command);
                    self.queued.extend(commands);
                    break;
                }
                Err(crossbeam::channel::TrySendError::Disconnected(_)) => {
                    self.error("Editor service disconnected".into());
                    break;
                }
            }
        }
        if !self.queued.is_empty() {
            _ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
    }
    fn queue(&mut self, command: EditorCommand) {
        let mut command = command;
        if let EditorCommand::Edit { req_id, .. } = &mut command {
            if self.operation.is_some() {
                return;
            }
            *req_id = self.request();
            self.operation = Some(*req_id);
        }
        // Keep the newest value/position per node while the bounded worker queue is busy.
        if self
            .queued
            .last()
            .is_some_and(|old| same_edit(old, &command))
        {
            self.queued.pop();
        }
        self.queued.push(command);
        let req_id = self.request();
        self.awaiting.insert(req_id.0);
        self.queued.push(EditorCommand::Barrier { req_id });
    }
}
impl eframe::App for WispGuiApp {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll(&ctx);
        if ctx.input(|i| i.viewport().close_requested())
            && !self.allow_close
            && (self.dirty() || !self.queued.is_empty())
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.pending = Some(PendingAction::Close);
        }
        egui::Panel::top("toolbar").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.text_edit_singleline(&mut self.new_name);
                if ui
                    .add_enabled(
                        self.operation.is_none()
                            && self.queued.is_empty()
                            && self.awaiting.is_empty(),
                        egui::Button::new("New"),
                    )
                    .clicked()
                {
                    self.action(PendingAction::New(self.new_name.clone()), &ctx);
                }
                ui.label("Flow:");
                ui.text_edit_singleline(&mut self.load_path);
                if ui
                    .add_enabled(
                        self.operation.is_none()
                            && self.queued.is_empty()
                            && self.awaiting.is_empty(),
                        egui::Button::new("Open"),
                    )
                    .clicked()
                    && !self.load_path.trim().is_empty()
                {
                    self.action(PendingAction::Open(PathBuf::from(&self.load_path)), &ctx);
                }
            });
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, CanvasTab::Graph, "Graph");
                ui.selectable_value(&mut self.tab, CanvasTab::Lab, "Canvas lab");
                ui.separator();
                ui.checkbox(&mut self.canvas.snap, "Snap (20)");
                for (label, edit, enabled) in [
                    (
                        "Undo",
                        crate::editor::document::Edit::Undo,
                        self.snapshot.as_ref().is_some_and(|s| s.can_undo),
                    ),
                    (
                        "Redo",
                        crate::editor::document::Edit::Redo,
                        self.snapshot.as_ref().is_some_and(|s| s.can_redo),
                    ),
                ] {
                    if ui
                        .add_enabled(
                            enabled
                                && self.operation.is_none()
                                && self.queued.is_empty()
                                && self.awaiting.is_empty(),
                            egui::Button::new(label),
                        )
                        .clicked()
                        && let Some(s) = &self.snapshot
                    {
                        self.queue(EditorCommand::Edit {
                            document_id: s.id,
                            revision: s.revision,
                            edit,
                            req_id: RequestId(0),
                        });
                    }
                }

                let running = self.runtime_status == RuntimeStatus::Running;
                let valid = self
                    .snapshot
                    .as_ref()
                    .is_some_and(|s| s.start_error.is_none());
                if ui
                    .add_enabled(
                        self.operation.is_none()
                            && self.queued.is_empty()
                            && !self.offline
                            && valid,
                        egui::Button::new(if running { "Stop DSP" } else { "Start DSP" }),
                    )
                    .clicked()
                {
                    let req_id = self.request();
                    let command = if running {
                        EditorCommand::DspStop { req_id }
                    } else {
                        EditorCommand::DspStart { req_id }
                    };
                    match self.service.submit(command) {
                        Ok(()) => self.operation = Some(req_id),
                        Err(e) => self.error(e),
                    }
                }
                ui.label(if self.operation.is_some() {
                    "Working…".to_owned()
                } else {
                    format!("{:?}", self.runtime_status)
                });
                if let Some(s) = &self.snapshot {
                    ui.label(&s.flow_name);
                    if s.dirty {
                        ui.label("Unsaved (saving: milestone 3)");
                    }
                }
            });
        });
        egui::Panel::bottom("diagnostics").show(ui, |ui| {
            if ui.button("Clear errors").clicked() {
                self.errors.clear();
            }
            egui::ScrollArea::vertical()
                .max_height(120.0)
                .show(ui, |ui| {
                    for error in &self.errors {
                        ui.colored_label(egui::Color32::LIGHT_RED, error);
                    }
                    if let Some(snapshot) = &self.snapshot {
                        if snapshot.diagnostics.is_empty()
                            && let Some(reason) = &snapshot.start_error
                        {
                            ui.label(reason);
                        }
                        for error in &snapshot.diagnostics {
                            ui.colored_label(egui::Color32::LIGHT_RED, error);
                        }
                    }
                });
        });
        if self.tab == CanvasTab::Graph
            && !self.canvas.picker_open()
            && !ctx.egui_wants_keyboard_input()
            && self.operation.is_none()
            && self.queued.is_empty()
        {
            let undo = ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Z));
            let redo = ctx.input(|i| {
                i.modifiers.command
                    && (i.key_pressed(egui::Key::Y)
                        || (i.modifiers.shift && i.key_pressed(egui::Key::Z)))
            });
            if (undo || redo)
                && let Some(s) = &self.snapshot
            {
                self.queue(EditorCommand::Edit {
                    document_id: s.id,
                    revision: s.revision,
                    edit: if redo {
                        crate::editor::document::Edit::Redo
                    } else {
                        crate::editor::document::Edit::Undo
                    },
                    req_id: RequestId(0),
                });
            }
        }
        egui::CentralPanel::default().show(ui, |ui| match self.tab {
            CanvasTab::Graph => {
                ui.add_enabled_ui(self.operation.is_none(), |ui| {
                    for command in self.canvas.show(ui) {
                        self.queue(command);
                    }
                });
            }
            CanvasTab::Lab => {
                if let Some(command) = self.canvas.finish_gesture() {
                    self.queue(command);
                }
                self.lab.show(ui);
            }
        });
        if self.pending.is_some() {
            egui::Window::new("Discard unsaved changes?")
                .collapsible(false)
                .resizable(false)
                .show(&ctx, |ui| {
                    ui.label("Saving is not implemented yet. Your edits are only in memory.");
                    ui.horizontal(|ui| {
                        if ui.button("Discard changes").clicked()
                            && let Some(action) = self.pending.take()
                        {
                            self.perform(action, &ctx);
                        }
                        if ui.button("Cancel").clicked() {
                            self.pending = None;
                        }
                    });
                });
        }
        // Flush new intents next frame even when the graph is otherwise idle.
        if !self.queued.is_empty() {
            ctx.request_repaint();
        }
    }
}
