// SPDX-License-Identifier: MIT OR Apache-2.0
use crate::{
    canvas::{lab::CanvasLab, snarl_adapter::GraphCanvas, state::CanvasTab},
    editor::{
        commands::{EditorCommand, EditorEvent, RequestId, RuntimeStatus, same_edit},
        document::DocumentSnapshot,
        service::{EditorServiceHandle, spawn_service_with_paths},
    },
};
use std::path::PathBuf;

/// Prefer an installed relocatable layout, then the development checkout.
fn default_core_path() -> PathBuf {
    if let Some(path) = std::env::var_os("WISP_CORE_PATH") {
        return path.into();
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(bin) = exe.parent() {
            let installed = bin.join("../share/twistedwisp/core");
            if installed.is_dir() {
                return installed;
            }
            let checkout = bin.join("../../wisp/core");
            if checkout.is_dir() {
                return checkout;
            }
        }
    }
    PathBuf::from("wisp/core")
}

#[derive(Clone, Debug, clap::Parser)]
#[command(about = "TwistedWiSP native graph editor ")]
pub struct Options {
    /// Validate core resources without a window or devices.
    #[arg(long)]
    pub check_resources: bool,
    #[arg(long)]
    pub flow: Option<PathBuf>,
    #[arg(long, default_value_os_t = default_core_path())]
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
    /// Requested audio block size; devices may clamp this to their supported range.
    #[arg(long, default_value = "256", value_parser = clap::value_parser!(u32).range(1..))]
    pub audio_buffer_size: Option<u32>,
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    pub audio_sample_rate: Option<u32>,
    #[arg(long)]
    pub midi_in_port: Option<String>,
    /// Search paths for buffer references (playback remains gated).
    #[arg(long)]
    pub data_path: Vec<PathBuf>,
}
use crate::{
    dialog::{DialogKind, DialogRequest, DialogWorker},
    inspector::Inspector,
    workflow::{Action as PendingAction, FileWorkflow, Phase},
};
pub struct WispGuiApp {
    service: EditorServiceHandle,
    runtime_status: RuntimeStatus,
    runtime_generation: u64,
    midi_status: String,
    buffer_node: Option<crate::editor::document::EditorNodeId>,
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
    file: FileWorkflow,
    dialogs: DialogWorker,
    inspector: Inspector,
    entry_path: String,
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
        let service = spawn_service_with_paths(
            options.core_lib_path,
            cc.egui_ctx.clone(),
            options.offline,
            config,
            crate::editor::runtime::live_factory(),
            options
                .data_path
                .into_iter()
                .map(|p| {
                    if p.is_absolute() {
                        p
                    } else {
                        std::env::current_dir().unwrap_or_default().join(p)
                    }
                })
                .collect(),
        );
        let mut app = Self {
            service,
            runtime_generation: 0,
            midi_status: "MIDI unavailable (start audio first)".into(),
            buffer_node: None,
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
            file: FileWorkflow::default(),
            dialogs: DialogWorker::spawn(cc.egui_ctx.clone()),
            inspector: Inspector::default(),
            entry_path: String::new(),
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
            self.file.phase = Phase::Loading { request: req_id };
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
        let dirty = self.dirty();
        if let Some(action) = self.file.request_action(action, dirty) {
            self.perform(action, ctx);
        }
    }
    fn perform(&mut self, action: PendingAction, ctx: &egui::Context) {
        self.file.deferred = None;
        match action {
            PendingAction::Open(path) => self.open(path),
            PendingAction::New(name) => {
                let req_id = self.request();
                match self.service.submit(EditorCommand::NewFlow { name, req_id }) {
                    Ok(()) => {
                        self.operation = Some(req_id);
                        self.file.phase = Phase::Loading { request: req_id };
                    }
                    Err(e) => {
                        self.file.cancel();
                        self.error(e);
                    }
                }
            }
            PendingAction::Close => {
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
    fn choose_path(&mut self, kind: DialogKind) {
        let id = self.request();
        let document_id = self.snapshot.as_ref().map(|s| s.id);
        let initial = self
            .snapshot
            .as_ref()
            .and_then(|s| s.source_file.clone())
            .or_else(|| (!self.load_path.is_empty()).then(|| PathBuf::from(&self.load_path)));
        match self.dialogs.tx.try_send(DialogRequest {
            id,
            document_id,
            kind,
            initial,
        }) {
            Ok(()) => {
                self.file.phase = Phase::Choosing {
                    request: id,
                    document: document_id,
                    kind,
                }
            }
            Err(_) => {
                self.file.cancel();
                self.error("File dialog busy; use path entry instead".into());
            }
        }
    }
    fn finish_for_save(&mut self, ctx: &egui::Context) {
        for command in self.canvas.release_buttons() {
            self.queue(command);
        }
        for command in self.canvas.finish_for_file_action() {
            self.queue(command);
        }
        if let Some(command) = self.inspector.finish_gesture() {
            self.queue(command);
        }
        ctx.memory_mut(|m| {
            if let Some(id) = m.focused() {
                m.surrender_focus(id);
            }
        });
    }
    fn request_save(&mut self, save_as: bool, ctx: &egui::Context) {
        self.finish_for_save(ctx);
        if save_as
            || self
                .snapshot
                .as_ref()
                .is_none_or(|s| s.source_file.is_none())
        {
            self.choose_path(DialogKind::Save);
        } else {
            self.file.phase = Phase::WaitingForEdits {
                path: None,
                overwrite: None,
            };
        }
    }
    fn start_pending_save(&mut self) {
        if self.operation.is_some()
            || !self.queued.is_empty()
            || !self.awaiting.is_empty()
            || self.canvas.gesture_active()
        {
            return;
        }
        let Phase::WaitingForEdits { path, overwrite } = self.file.phase.clone() else {
            return;
        };
        let Some(snapshot) = &self.snapshot else {
            self.file.cancel();
            return;
        };
        let (document_id, revision, state_id) = (snapshot.id, snapshot.revision, snapshot.state_id);
        let req_id = self.request();
        match self.service.submit(EditorCommand::Save {
            document_id,
            revision,
            state_id,
            path,
            overwrite,
            req_id,
        }) {
            Ok(()) => {
                self.file.phase = Phase::Saving {
                    request: req_id,
                    document: document_id,
                }
            }
            Err(e) => {
                self.file.cancel();
                self.error(e);
            }
        }
    }
    fn poll(&mut self, ctx: &egui::Context) {
        let mut completed_save = false;
        while let Ok(result) = self.dialogs.rx.try_recv() {
            let document = self.snapshot.as_ref().map(|s| s.id);
            if !self.file.accepts_dialog(result.request.id, document) {
                continue;
            }
            match result.path {
                None => self.file.cancel(),
                Some(path) => match result.request.kind {
                    DialogKind::Open => {
                        self.file.phase = Phase::Idle;
                        self.action(PendingAction::Open(path), ctx);
                    }
                    DialogKind::Wave => {
                        if let (Some(node_id), Some(document_id)) =
                            (self.buffer_node.take(), document)
                        {
                            self.file.cancel();
                            self.queue(EditorCommand::LoadBuffer {
                                document_id,
                                node_id,
                                path: Some(path),
                                req_id: RequestId(0),
                            });
                        } else {
                            self.file.cancel();
                        }
                    }
                    DialogKind::Save => {
                        self.file.phase = Phase::WaitingForEdits {
                            path: Some(path),
                            overwrite: None,
                        }
                    }
                },
            }
        }
        while let Ok(event) = self.service.rx.try_recv() {
            match event {
                EditorEvent::Saved {
                    req_id,
                    document_id,
                    result,
                } => {
                    if !matches!(self.file.phase,Phase::Saving{request,document} if request==req_id && document==document_id)
                    {
                        continue;
                    }
                    match result {
                        Ok(receipt) => {
                            self.load_path = receipt.path.to_string_lossy().into_owned();
                            for warning in receipt.warnings {
                                self.error(warning);
                            }
                            completed_save = true;
                        }
                        Err(crate::editor::persistence::SaveError::Conflict { path, observed }) => {
                            self.file.phase = Phase::Conflict { path, observed }
                        }
                        Err(e) => {
                            self.error(e.to_string());
                            self.file.cancel();
                        }
                    }
                }
                EditorEvent::Response { req_id, result } => {
                    self.awaiting.remove(&req_id.0);
                    self.inspector.acknowledge(req_id);
                    if matches!(self.file.phase,Phase::Loading{request} if request==req_id) {
                        self.file.cancel();
                    }
                    if self.canvas.acknowledge_control(req_id) {
                        // The worker publishes before acknowledging the fence. Reapply
                        // even an already-seen revision now that the local edit is accepted.
                        self.canvas_revision = None;
                    }
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
                EditorEvent::Runtime {
                    status, generation, ..
                } => {
                    self.runtime_status = status;
                    self.runtime_generation = generation;
                    self.canvas
                        .set_runtime(status == RuntimeStatus::Running, generation);
                }
            }
        }

        if let Some(snapshot) = self.service.take_snapshot() {
            if self.snapshot.as_ref().map(|s| s.id) != Some(snapshot.id) {
                self.inspector.clear();
            }
            self.snapshot = Some(snapshot);
        }
        if completed_save {
            let dirty = self.snapshot.as_ref().is_some_and(|s| s.dirty)
                || !self.awaiting.is_empty()
                || !self.queued.is_empty();
            if let Some(action) = self.file.saved(dirty) {
                self.perform(action, ctx);
            }
        }
        // The canvas protects active and unacknowledged local control values.
        if let Some(snapshot) = &self.snapshot {
            let revision = (snapshot.id, snapshot.revision);
            if self.canvas_revision != Some(revision) {
                self.canvas.sync(snapshot);
                self.canvas_revision = Some(revision);
            }
        }
        if let Some(special) = self.service.special.lock().unwrap().take()
            && self.snapshot.as_ref().is_some_and(|s| {
                special.document == Some(s.id)
                    && special.generation == self.runtime_generation
                    && special.revision == s.semantic_revision
            })
        {
            self.midi_status = special.midi.clone();
            self.canvas.set_special(special);
        }
        self.flush_commands(ctx);
    }
    fn flush_commands(&mut self, ctx: &egui::Context) {
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
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
    }
    fn queue(&mut self, command: EditorCommand) {
        let mut command = command;
        if let EditorCommand::ChooseBuffer {
            document_id,
            node_id,
        } = command
        {
            if self.snapshot.as_ref().is_some_and(|s| s.id == document_id) && !self.file.busy() {
                self.buffer_node = Some(node_id);
                self.choose_path(DialogKind::Wave);
            }
            return;
        }
        if let EditorCommand::Edit { req_id, .. } | EditorCommand::LoadBuffer { req_id, .. } =
            &mut command
        {
            if self.operation.is_some() {
                return;
            }
            *req_id = self.request();
            self.operation = Some(*req_id);
        }
        // Replace only an unsent control/fence pair in this active gesture.
        // Never coalesce across a history, document, or gesture boundary.
        let n = self.queued.len();
        if self.canvas.gesture_active()
            && n >= 2
            && same_edit(&self.queued[n - 2], &command)
            && let EditorCommand::ControlFence { req_id, .. } = &self.queued[n - 1]
        {
            self.awaiting.remove(&req_id.0);
            self.queued.truncate(n - 2);
        }
        let geometry = match &command {
            EditorCommand::SetGeometry {
                document_id,
                node_id,
                position,
                size,
            } => Some((*document_id, *node_id, *position, *size)),
            _ => None,
        };
        let control = match &command {
            EditorCommand::SetControlValue {
                document_id,
                node_id,
                ..
            } => Some((*document_id, *node_id)),
            _ => None,
        };
        self.queued.push(command);
        let req_id = self.request();
        self.awaiting.insert(req_id.0);
        if let Some((document_id, node_id)) = control {
            self.canvas.track_control(document_id, node_id, req_id);
            self.inspector.track(document_id, node_id, req_id);
            self.queued.push(EditorCommand::ControlFence {
                document_id,
                node_id,
                req_id,
            });
        } else {
            if let Some((document_id, node_id, position, size)) = geometry {
                self.canvas
                    .track_geometry(document_id, node_id, position, size, req_id);
                self.inspector.track(document_id, node_id, req_id);
            }
            self.queued.push(EditorCommand::Barrier { req_id });
        }
    }
}
impl eframe::App for WispGuiApp {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll(&ctx);
        self.service.graphs_visible.store(
            self.tab == CanvasTab::Graph,
            std::sync::atomic::Ordering::Release,
        );
        if self.tab != CanvasTab::Graph
            || !ctx.input(|i| {
                i.focused
                    && (i.pointer.primary_down()
                        || i.key_down(egui::Key::Space)
                        || i.key_down(egui::Key::Enter))
            })
            || !self.file.editable()
        {
            for command in self.canvas.release_buttons() {
                self.queue(command);
            }
        }
        if ctx.input(|i| i.viewport().close_requested())
            && !self.allow_close
            && (self.dirty() || self.file.busy() || !self.queued.is_empty())
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if matches!(self.file.phase, Phase::Saving { .. }) {
                self.file.deferred = Some(PendingAction::Close);
            } else if !self.file.busy() {
                self.action(PendingAction::Close, &ctx);
            }
        }
        egui::Panel::top("toolbar").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.text_edit_singleline(&mut self.new_name);
                if ui
                    .add_enabled(
                        self.operation.is_none()
                            && self.queued.is_empty()
                            && self.awaiting.is_empty()
                            && !self.file.busy(),
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
                            && self.awaiting.is_empty()
                            && !self.file.busy(),
                        egui::Button::new("Open"),
                    )
                    .clicked()
                    && !self.load_path.trim().is_empty()
                {
                    self.action(PendingAction::Open(PathBuf::from(&self.load_path)), &ctx);
                }
            });
            ui.horizontal(|ui| {
                let available = !self.file.busy() && self.operation.is_none();
                if ui
                    .add_enabled(available, egui::Button::new("Open…"))
                    .clicked()
                {
                    self.choose_path(DialogKind::Open);
                }
                if ui
                    .add_enabled(
                        available && self.snapshot.is_some(),
                        egui::Button::new("Save"),
                    )
                    .clicked()
                {
                    self.request_save(false, &ctx);
                }
                if ui
                    .add_enabled(
                        available && self.snapshot.is_some(),
                        egui::Button::new("Save As…"),
                    )
                    .clicked()
                {
                    self.request_save(true, &ctx);
                }
                if ui
                    .add_enabled(
                        available && self.snapshot.is_some(),
                        egui::Button::new("Save to path…"),
                    )
                    .clicked()
                {
                    self.finish_for_save(&ctx);
                    self.entry_path = self.load_path.clone();
                    self.file.phase = Phase::Entering {
                        kind: DialogKind::Save,
                    };
                }
                if self.file.busy() {
                    ui.label("File operation…");
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
                                && self.file.editable()
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
                        ui.label("Unsaved");
                    }
                }
            });
        });
        egui::Panel::top("midi").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.small(&self.midi_status);
                let selection = self.canvas.selection().clone();
                let eligible = self.snapshot.as_ref().and_then(|s| {
                    if selection.len() != 1 {
                        return None;
                    }
                    s.nodes
                        .iter()
                        .find(|(id, n)| {
                            selection.contains(id)
                                && matches!(n.function_name.as_str(), "control" | "toggle")
                        })
                        .map(|(id, _)| (s.id, *id))
                });
                let learn = ui
                    .add_enabled(
                        eligible.is_some() && !self.file.busy(),
                        egui::Button::new("Learn CC (L)"),
                    )
                    .clicked()
                    || (!ctx.egui_wants_keyboard_input()
                        && !self.file.busy()
                        && ctx.input(|i| i.key_pressed(egui::Key::L)));
                if learn && let Some((document_id, node_id)) = eligible {
                    let req_id = self.request();
                    if let Err(e) = self.service.submit(EditorCommand::MidiLearn {
                        document_id,
                        node_id: Some(node_id),
                        req_id,
                    }) {
                        self.error(e);
                    }
                }
                if ui.button("Cancel learn").clicked()
                    && let Some(document_id) = self.snapshot.as_ref().map(|s| s.id)
                {
                    let req_id = self.request();
                    if let Err(e) = self.service.submit(EditorCommand::MidiLearn {
                        document_id,
                        node_id: None,
                        req_id,
                    }) {
                        self.error(e);
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
                        for buffer in &snapshot.buffers {
                            if let Some(warning) = &buffer.diagnostic {
                                ui.colored_label(egui::Color32::YELLOW, warning);
                            }
                        }
                        for error in &snapshot.diagnostics {
                            ui.colored_label(egui::Color32::LIGHT_RED, error);
                        }
                    }
                });
        });
        if !ctx.egui_wants_keyboard_input() && !self.file.busy() && self.operation.is_none() {
            if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::S))
                && self.snapshot.is_some()
            {
                self.request_save(ctx.input(|i| i.modifiers.shift), &ctx);
            } else if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::O)) {
                self.choose_path(DialogKind::Open);
            } else if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::N)) {
                self.action(PendingAction::New(self.new_name.clone()), &ctx);
            }
        }
        if self.tab == CanvasTab::Graph {
            egui::Panel::right("inspector")
                .default_size(230.)
                .show(ui, |ui| {
                    let selected = self.canvas.selection().clone();
                    let enabled = self.operation.is_none()
                        && (self.file.editable()
                            || matches!(self.file.phase, Phase::WaitingForEdits { .. }));
                    for command in
                        self.inspector
                            .show(ui, self.snapshot.as_ref(), &selected, enabled)
                    {
                        self.queue(command);
                    }
                });
        }
        if self.tab == CanvasTab::Graph
            && !self.canvas.picker_open()
            && self.file.editable()
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
                ui.add_enabled_ui(self.operation.is_none() && self.file.editable(), |ui| {
                    for command in self.canvas.show(ui) {
                        self.queue(command);
                    }
                });
            }
            CanvasTab::Lab => {
                if let Some(command) = self.inspector.finish_gesture() {
                    self.queue(command);
                }
                if let Some(command) = self.canvas.finish_gesture() {
                    self.queue(command);
                }
                self.lab.show(ui);
            }
        });
        if matches!(self.file.phase, Phase::DirtyDecision) {
            egui::Window::new("Unsaved changes")
                .collapsible(false)
                .resizable(false)
                .show(&ctx, |ui| {
                    ui.label("Save changes before continuing?");
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            self.request_save(false, &ctx);
                        }
                        if ui.button("Discard").clicked()
                            && let Some(action) = self.file.deferred.take()
                        {
                            self.file.phase = Phase::Idle;
                            self.perform(action, &ctx);
                        }
                        if ui.button("Cancel").clicked() {
                            self.file.cancel();
                        }
                    });
                });
        }
        if let Phase::Entering { kind } = self.file.phase.clone() {
            egui::Window::new("File path")
                .collapsible(false)
                .show(&ctx, |ui| {
                    ui.text_edit_singleline(&mut self.entry_path);
                    if ui.button("Apply").clicked() && !self.entry_path.trim().is_empty() {
                        let path = PathBuf::from(self.entry_path.trim());
                        if kind == DialogKind::Wave {
                            if let (Some(node_id), Some(document_id)) = (
                                self.buffer_node.take(),
                                self.snapshot.as_ref().map(|s| s.id),
                            ) {
                                self.file.cancel();
                                self.queue(EditorCommand::LoadBuffer {
                                    document_id,
                                    node_id,
                                    path: Some(path),
                                    req_id: RequestId(0),
                                });
                            } else {
                                self.file.cancel();
                            }
                        } else if kind == DialogKind::Save {
                            self.file.phase = Phase::WaitingForEdits {
                                path: Some(path),
                                overwrite: None,
                            };
                        } else {
                            self.file.phase = Phase::Idle;
                            self.action(PendingAction::Open(path), &ctx);
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        self.file.cancel();
                    }
                });
        }
        if let Phase::Conflict { path, observed } = self.file.phase.clone() {
            egui::Window::new("File changed externally")
                .collapsible(false)
                .show(&ctx, |ui| {
                    ui.label(format!(
                        "{} differs from the version opened/saved.",
                        path.display()
                    ));
                    if ui.button("Overwrite this version").clicked() {
                        self.file.phase = Phase::WaitingForEdits {
                            path: Some(path),
                            overwrite: Some(observed),
                        };
                    }
                    if ui.button("Save As…").clicked() {
                        self.choose_path(DialogKind::Save);
                    }
                    if ui.button("Enter another path…").clicked() {
                        self.entry_path = String::new();
                        self.file.phase = Phase::Entering {
                            kind: DialogKind::Save,
                        };
                    }
                    if ui.button("Cancel").clicked() {
                        self.file.cancel();
                    }
                });
        }
        if matches!(self.file.phase, Phase::Choosing { .. }) {
            egui::Window::new("Choosing flow path").collapsible(false).show(&ctx,|ui| {
                ui.label("Waiting for the native file dialog. Path entry remains available without a portal.");
                if ui.button("Cancel").clicked(){self.file.cancel();}
                if ui.button("Use path entry instead").clicked(){
                    let kind=match self.file.phase {Phase::Choosing{kind,..}=>kind,_=>DialogKind::Open};
                    self.entry_path=self.load_path.clone();self.file.phase=Phase::Entering{kind};
                }
            });
        }
        // Send this frame's input immediately instead of adding a repaint's
        // worth of latency before it can reach the audio callback.
        self.flush_commands(&ctx);
        self.start_pending_save();
    }
}
