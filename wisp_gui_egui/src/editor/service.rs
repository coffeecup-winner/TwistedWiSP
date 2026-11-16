// SPDX-License-Identifier: MIT OR Apache-2.0
//! Authoritative document and lazy native runtime live on one worker thread.
use super::{
    commands::{EditorCommand, EditorEvent, RuntimeStatus, SnapshotMailbox, same_edit},
    document::{DocumentId, EditorDocument},
};
use crossbeam::channel::{Receiver, SendTimeoutError, Sender, TrySendError, bounded};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

pub struct EditorServiceHandle {
    pub tx: Sender<EditorCommand>,
    pub rx: Receiver<EditorEvent>,
    pub snapshots: SnapshotMailbox,
    pub special: super::telemetry::SpecialMailbox,
    pub graphs_visible: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}
impl EditorServiceHandle {
    /// UI submission must never block behind a busy worker.
    pub fn submit(&self, command: EditorCommand) -> Result<(), String> {
        self.tx.try_send(command).map_err(|e| match e {
            TrySendError::Full(_) => "Editor busy; please retry".into(),
            TrySendError::Disconnected(_) => "Editor service disconnected".into(),
        })
    }
    pub fn take_snapshot(&self) -> Option<super::document::DocumentSnapshot> {
        self.snapshots.lock().unwrap().take()
    }
}
impl Drop for EditorServiceHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
pub fn spawn_editor_service(core_path: PathBuf, repaint: egui::Context) -> EditorServiceHandle {
    spawn_configured_service(
        core_path,
        repaint,
        true,
        wisp::TwistedWispEngineConfig::default(),
        super::runtime::live_factory(),
    )
}
pub fn spawn_configured_service(
    core_path: PathBuf,
    repaint: egui::Context,
    offline: bool,
    config: wisp::TwistedWispEngineConfig,
    factory: super::runtime::RuntimeFactory,
) -> EditorServiceHandle {
    spawn_service_with_paths(core_path, repaint, offline, config, factory, vec![])
}
pub fn spawn_service_with_paths(
    core_path: PathBuf,
    repaint: egui::Context,
    offline: bool,
    config: wisp::TwistedWispEngineConfig,
    mut factory: super::runtime::RuntimeFactory,
    data_paths: Vec<PathBuf>,
) -> EditorServiceHandle {
    let (tx, commands) = bounded(128);
    let (events, rx) = bounded(128);
    let stop = Arc::new(AtomicBool::new(false));
    let snapshots = Arc::new(Mutex::new(None));
    let special = Arc::new(Mutex::new(None));
    let worker_special = special.clone();
    let graphs_visible = Arc::new(AtomicBool::new(true));
    let worker_visible = graphs_visible.clone();
    let worker_stop = stop.clone();
    let worker_snapshots = snapshots.clone();
    let thread = thread::spawn(move || {
        let io = super::persistence::IoWorker::spawn();
        let mut resources = super::resources::Resources::default();
        let mut special_view = super::telemetry::SpecialSnapshot {
            midi: "MIDI unavailable (start audio to initialize)".into(),
            ..Default::default()
        };
        let mut last_special: Option<super::telemetry::SpecialSnapshot> = None;
        let mut hidden_refresh = std::time::Instant::now();
        let mut watches = std::collections::HashMap::new();
        let mut queued_jobs = std::collections::VecDeque::new();
        let mut pressed = std::collections::BTreeSet::new();
        let mut midi_pending = std::collections::BTreeMap::new();
        let mut query_after = std::time::Instant::now();
        let mut next_tick = std::time::Instant::now();
        let mut saving: Option<(DocumentId, super::commands::RequestId, u64)> = None;
        let mut document: Option<EditorDocument> = None;
        let mut next_document = 1;
        let mut runtime: Option<Box<dyn super::runtime::EditorRuntime>> = None;
        let mut generation = 0;
        let mut desired_running = false;
        let mut runtime_current = false;
        let mut status = if offline {
            RuntimeStatus::Offline
        } else {
            RuntimeStatus::Stopped
        };
        let mut pending_command = None;
        let mut control_gesture = None;
        while !worker_stop.load(Ordering::Acquire) {
            for result in resources.poll(document.as_ref()) {
                match result {
                    Ok(command) => queued_jobs.push_back(command),
                    Err((req_id, error)) => {
                        deliver(
                            EditorEvent::Response {
                                req_id,
                                result: Err(error),
                            },
                            &events,
                            &worker_stop,
                            &repaint,
                        );
                    }
                }
            }
            if std::time::Instant::now() >= next_tick {
                next_tick = std::time::Instant::now() + Duration::from_millis(33);
                if let Some(doc) = document.as_mut() {
                    if special_view.document != Some(doc.id()) {
                        special_view = super::telemetry::SpecialSnapshot {
                            document: Some(doc.id()),
                            midi: "MIDI unavailable (start audio to initialize)".into(),
                            ..Default::default()
                        };
                        watches.clear();
                        midi_pending.clear();
                        pressed.clear();
                    }
                    if special_view.generation != generation {
                        special_view.traces.clear();
                    }
                    special_view.generation = generation;
                    special_view.revision = doc.semantic_revision();
                    special_view.running = status == RuntimeStatus::Running && runtime_current;
                    special_view.buffers = resources.sync(doc);
                    let snapshot = doc.snapshot();
                    let revision_before = snapshot.revision;
                    special_view
                        .traces
                        .retain(|id, _| snapshot.nodes.iter().any(|(n, _)| n == id));
                    if let Some(live) = runtime.as_mut() {
                        let valid = |id: &super::document::EditorNodeId| {
                            snapshot.nodes.iter().any(|(n, v)| {
                                n == id && matches!(v.function_name.as_str(), "control" | "toggle")
                            })
                        };
                        for target in live.midi_targets() {
                            let id =
                                super::document::EditorNodeId(target.saturating_sub(1) as usize);
                            if !valid(&id) {
                                live.midi_remove(target);
                            }
                        }
                        midi_pending.retain(|id, _| valid(id));
                        for observation in live.midi_poll() {
                            let id = super::document::EditorNodeId(
                                observation.target.saturating_sub(1) as usize,
                            );
                            if valid(&id)
                                && (midi_pending.contains_key(&id) || midi_pending.len() < 128)
                            {
                                special_view.midi = format!(
                                    "Bound: channel {} CC {}",
                                    observation.channel + 1,
                                    observation.controller
                                );
                                midi_pending.insert(id, observation.value);
                            }
                        }
                        if runtime_current {
                            midi_pending.retain(|id, value| {
                                let value = if snapshot
                                    .nodes
                                    .iter()
                                    .any(|(n, v)| n == id && v.function_name == "toggle")
                                {
                                    f32::from(*value > 0.)
                                } else {
                                    *value
                                };
                                let Ok(call) = doc.runtime_call_index(*id) else {
                                    return false;
                                };
                                if live.scalar_try(doc.flow_name(), call, value).is_err() {
                                    return true;
                                }
                                let _ = doc.observe_midi(*id, value as f64);
                                false
                            });
                        }
                        if special_view.running
                            && !watches.is_empty()
                            && std::time::Instant::now() >= query_after
                        {
                            match live.query() {
                                Ok(batch) => {
                                    special_view.watch_error = None;
                                    for (watch, values) in batch.values {
                                        if let Some(id) = watches.get(&watch) {
                                            super::telemetry::append(
                                                special_view.traces.entry(*id).or_default(),
                                                values.into_iter().take(4096),
                                            );
                                        }
                                    }
                                }
                                Err(error) => {
                                    query_after =
                                        std::time::Instant::now() + Duration::from_secs(1);
                                    special_view.watch_error = Some(error.clone());
                                    special_view.traces.clear();
                                    deliver(
                                        EditorEvent::Error(error),
                                        &events,
                                        &worker_stop,
                                        &repaint,
                                    );
                                }
                            }
                        }
                    }
                    if doc.snapshot().revision != revision_before {
                        publish_doc(doc, &worker_snapshots, &repaint);
                    }
                }
                let visible = worker_visible.load(Ordering::Acquire);
                let important = last_special.as_ref().is_none_or(|last| {
                    last.document != special_view.document
                        || last.generation != special_view.generation
                        || last.revision != special_view.revision
                        || last.running != special_view.running
                        || last.midi != special_view.midi
                        || last.buffers != special_view.buffers
                        || last.watch_error != special_view.watch_error
                });
                if last_special.as_ref() != Some(&special_view)
                    && (visible || important || std::time::Instant::now() >= hidden_refresh)
                {
                    hidden_refresh = std::time::Instant::now() + Duration::from_millis(500);
                    *worker_special.lock().unwrap() = Some(special_view.clone());
                    last_special = Some(special_view.clone());
                    repaint.request_repaint();
                }
            }
            if let Ok(result) = io.rx.try_recv()
                && let Some((document_id, req_id, state)) = saving.take()
            {
                if let Ok(receipt) = &result
                    && let Some(doc) = document.as_mut().filter(|d| d.id() == document_id)
                {
                    doc.saved(state, receipt);
                }
                publish(&document, &worker_snapshots, &repaint);
                deliver(
                    EditorEvent::Saved {
                        req_id,
                        document_id,
                        result,
                    },
                    &events,
                    &worker_stop,
                    &repaint,
                );
            }
            let mut command = match pending_command
                .take()
                .or_else(|| queued_jobs.pop_front())
                .map(Ok)
                .unwrap_or_else(|| commands.recv_timeout(Duration::from_millis(5)))
            {
                Ok(cmd) => cmd,
                Err(crossbeam::channel::RecvTimeoutError::Timeout) => {
                    if runtime.as_ref().is_some_and(|r| !r.healthy()) {
                        runtime = None;
                        status = RuntimeStatus::Error;
                        deliver(
                            EditorEvent::Error(
                                "Audio device stopped; retry Start or continue offline".into(),
                            ),
                            &events,
                            &worker_stop,
                            &repaint,
                        );
                        deliver(
                            runtime_event(&document, generation, status),
                            &events,
                            &worker_stop,
                            &repaint,
                        );
                    }
                    continue;
                }
                Err(_) => break,
            };
            if let EditorCommand::ApplyBuffer {
                document_id,
                binding_revision,
                node_id,
                name,
                path,
                req_id,
            } = command
            {
                let Some(doc) = document
                    .as_ref()
                    .filter(|d| d.id() == document_id && d.binding_revision() == binding_revision)
                else {
                    deliver(
                        EditorEvent::Response {
                            req_id,
                            result: Err("Stale buffer completion".into()),
                        },
                        &events,
                        &worker_stop,
                        &repaint,
                    );
                    continue;
                };
                // Scalar/MIDI/geometry edits during decode do not invalidate this
                // resource operation; topology/binding changes do.
                command = EditorCommand::Edit {
                    document_id,
                    revision: doc.snapshot().revision,
                    edit: super::document::Edit::Buffer {
                        node: node_id,
                        name,
                        path,
                    },
                    req_id,
                };
            }
            let control_fences = coalesce_edits(
                &mut command,
                &commands,
                &mut pending_command,
                control_gesture,
            );
            let event = match command {
                EditorCommand::ApplyBuffer { .. } => {
                    EditorEvent::Error("Unexpected buffer transition".into())
                }
                EditorCommand::ChooseBuffer { .. } => {
                    EditorEvent::Error("Buffer chooser belongs to the UI".into())
                }
                EditorCommand::LoadBuffer {
                    document_id,
                    node_id,
                    path,
                    req_id,
                } => {
                    let result = document
                        .as_ref()
                        .filter(|d| d.id() == document_id)
                        .ok_or("Stale document".into())
                        .and_then(|d| resources.relink(d, node_id, path, req_id));
                    match result {
                        Ok(()) => continue,
                        Err(e) => EditorEvent::Response {
                            req_id,
                            result: Err(e),
                        },
                    }
                }
                EditorCommand::Momentary {
                    document_id,
                    node_id,
                    pressed: down,
                } => {
                    let result = (|| {
                        let doc = document
                            .as_ref()
                            .filter(|d| d.id() == document_id)
                            .ok_or("Stale document")?;
                        if !doc
                            .snapshot()
                            .nodes
                            .iter()
                            .any(|(id, n)| *id == node_id && n.function_name == "button")
                        {
                            return Err("Not a button".to_owned());
                        }
                        if down {
                            pressed.insert(node_id);
                        } else {
                            pressed.remove(&node_id);
                        }
                        if let Some(live) = runtime.as_mut().filter(|_| runtime_current) {
                            live.scalar(
                                doc.flow_name(),
                                doc.runtime_call_index(node_id)?,
                                f32::from(down),
                            )?;
                        }
                        Ok::<_, String>(())
                    })();
                    match result {
                        Ok(()) => continue,
                        Err(e) => EditorEvent::Error(e),
                    }
                }
                EditorCommand::MidiLearn {
                    document_id,
                    node_id,
                    req_id,
                } => {
                    let result = (|| {
                        let doc = document
                            .as_ref()
                            .filter(|d| d.id() == document_id)
                            .ok_or("Stale document")?;
                        let live = runtime
                            .as_mut()
                            .ok_or("MIDI unavailable; start audio first")?;
                        if let Some(id) = node_id {
                            if !doc.snapshot().nodes.iter().any(|(n, v)| {
                                *n == id && matches!(v.function_name.as_str(), "control" | "toggle")
                            }) {
                                return Err("Select a root control or toggle".into());
                            }
                            live.midi_learn(id.0 as u64 + 1)?;
                            special_view.midi = "Learning… move a MIDI CC".into();
                        } else {
                            live.midi_cancel();
                            special_view.midi = "Learning canceled".into();
                        }
                        Ok::<_, String>(())
                    })();
                    EditorEvent::Response { req_id, result }
                }

                EditorCommand::Save {
                    document_id,
                    revision,
                    state_id,
                    path,
                    overwrite,
                    req_id,
                } => {
                    let result = (|| {
                        if saving.is_some() {
                            return Err("A save is already in progress".to_owned());
                        }
                        let doc = document
                            .as_mut()
                            .filter(|d| d.id() == document_id)
                            .ok_or("Stale document")?;
                        let snapshot = doc.snapshot();
                        if snapshot.revision != revision || snapshot.state_id != state_id {
                            return Err(
                                "Document changed before save; retry after pending edits finish"
                                    .into(),
                            );
                        }
                        let (job, state) = doc.save_capture(path, overwrite)?;
                        io.tx
                            .try_send(job)
                            .map_err(|_| "File worker is busy".to_owned())?;
                        saving = Some((document_id, req_id, state));
                        Ok::<(), String>(())
                    })();
                    if let Err(e) = result {
                        deliver(
                            EditorEvent::Saved {
                                req_id,
                                document_id,
                                result: Err(super::persistence::SaveError::Failed(e)),
                            },
                            &events,
                            &worker_stop,
                            &repaint,
                        );
                    }
                    publish(&document, &worker_snapshots, &repaint);
                    continue;
                }
                EditorCommand::SetGeometry {
                    document_id,
                    node_id,
                    position,
                    size,
                } => {
                    let result = document
                        .as_mut()
                        .filter(|d| d.id() == document_id)
                        .ok_or_else(|| "Stale document".to_owned())
                        .and_then(|d| d.set_geometry(node_id, position, size));
                    match result {
                        Ok(()) => {
                            publish(&document, &worker_snapshots, &repaint);
                            continue;
                        }
                        Err(e) => EditorEvent::Error(e),
                    }
                }
                EditorCommand::ControlFence { req_id, .. } | EditorCommand::Barrier { req_id } => {
                    EditorEvent::Response {
                        req_id,
                        result: Ok(()),
                    }
                }
                EditorCommand::Gesture { document_id, begin } => {
                    match document.as_mut().filter(|d| d.id() == document_id) {
                        Some(doc) => {
                            if begin {
                                doc.begin_gesture();
                                control_gesture = Some(document_id);
                            } else {
                                doc.end_gesture();
                                control_gesture = None;
                            }
                            publish_doc(doc, &worker_snapshots, &repaint);
                            continue;
                        }
                        None => EditorEvent::Error("Stale document".into()),
                    }
                }
                EditorCommand::NewFlow { name, req_id } => {
                    if saving.is_some() {
                        deliver(
                            EditorEvent::Response {
                                req_id,
                                result: Err(
                                    "Wait for the active save before replacing the document".into(),
                                ),
                            },
                            &events,
                            &worker_stop,
                            &repaint,
                        );
                        continue;
                    }
                    let result =
                        EditorDocument::new_flow(&name, &core_path, DocumentId(next_document)).map(
                            |mut doc| {
                                doc.set_data_paths(data_paths.clone());
                                runtime = None;
                                watches.clear();
                                control_gesture = None;
                                desired_running = false;
                                next_document += 1;
                                generation += 1;
                                status = if offline {
                                    RuntimeStatus::Offline
                                } else {
                                    RuntimeStatus::Stopped
                                };
                                midi_pending.clear();
                                pressed.clear();
                                special_view = super::telemetry::SpecialSnapshot {
                                    document: Some(doc.id()),
                                    midi: "MIDI unavailable (start audio to initialize)".into(),
                                    ..Default::default()
                                };
                                document = Some(doc);
                            },
                        );
                    EditorEvent::Response { req_id, result }
                }
                EditorCommand::Edit {
                    document_id,
                    revision,
                    edit,
                    req_id,
                } => {
                    control_gesture = None;
                    let result = (|| {
                        let doc = document
                            .as_mut()
                            .filter(|d| d.id() == document_id)
                            .ok_or("Stale document")?;
                        let old_revision = doc.snapshot().revision;
                        let old_bindings = doc.scalar_bindings()?;
                        // Reloading the same authored path refreshes resources without
                        // manufacturing an undo entry or changing the saved document.
                        let resource_change = matches!(&edit, super::document::Edit::Buffer { .. });
                        let structural = doc.edit(revision, edit)? || resource_change;
                        if structural {
                            runtime_current = false;
                        }
                        if doc.snapshot().revision == old_revision && !resource_change {
                            return Ok(());
                        }
                        publish_doc(doc, &worker_snapshots, &repaint);
                        if let Some(live) = runtime.as_mut() {
                            if structural {
                                if let Err(e) = live.stop() {
                                    runtime = None;
                                    status = RuntimeStatus::Error;
                                    return Err(e);
                                }
                                status = RuntimeStatus::Compiling;
                                deliver(
                                    EditorEvent::Runtime {
                                        document_id: Some(doc.id()),
                                        generation,
                                        semantic_revision: None,
                                        status,
                                    },
                                    &events,
                                    &worker_stop,
                                    &repaint,
                                );
                                // install compiles before replacing the installed processor.
                                // On compile failure the previous processor remains stopped.
                                if let Err(e) =
                                    doc.can_start().and_then(|_| live.prepare(&doc.serialize()))
                                {
                                    status = RuntimeStatus::Error;
                                    return Err(e);
                                }
                                status = RuntimeStatus::Error;
                                live.buffers(&resources.bindings(doc)?)?;
                                if let Err(e) = live.install_prepared() {
                                    runtime = None;
                                    status = RuntimeStatus::Error;
                                    return Err(e);
                                }
                                status = RuntimeStatus::Error;
                                watches = register_watches(doc, live.as_mut())?;
                                pressed.clear();
                                generation += 1;
                                runtime_current = true;
                                status = RuntimeStatus::Stopped;
                            }
                            if runtime_current
                                && (structural || old_bindings != doc.scalar_bindings()?)
                            {
                                let root = doc.snapshot().flow_name;
                                for (call, value) in doc.scalar_bindings()? {
                                    if let Err(e) = live.scalar(&root, call, value) {
                                        let _ = live.stop();
                                        runtime = None;
                                        status = RuntimeStatus::Error;
                                        return Err(e);
                                    }
                                }
                                if structural && desired_running {
                                    if let Err(e) = live.start() {
                                        runtime = None;
                                        status = RuntimeStatus::Error;
                                        return Err(e);
                                    }
                                    status = RuntimeStatus::Running;
                                }
                            }
                        }
                        Ok::<(), String>(())
                    })();
                    EditorEvent::Response { req_id, result }
                }
                EditorCommand::LoadFlow { path, req_id } => {
                    if saving.is_some() {
                        deliver(
                            EditorEvent::Response {
                                req_id,
                                result: Err(
                                    "Wait for the active save before replacing the document".into(),
                                ),
                            },
                            &events,
                            &worker_stop,
                            &repaint,
                        );
                        continue;
                    }
                    let result = EditorDocument::load(&path, &core_path, DocumentId(next_document))
                        .map(|mut doc| {
                            doc.set_data_paths(data_paths.clone());
                            runtime = None;
                            watches.clear();
                            control_gesture = None;
                            desired_running = false;
                            generation += 1;
                            status = if offline {
                                RuntimeStatus::Offline
                            } else {
                                RuntimeStatus::Stopped
                            };
                            next_document += 1;
                            midi_pending.clear();
                            pressed.clear();
                            special_view = super::telemetry::SpecialSnapshot {
                                document: Some(doc.id()),
                                midi: "MIDI unavailable (start audio to initialize)".into(),
                                ..Default::default()
                            };
                            document = Some(doc);
                        });
                    EditorEvent::Response { req_id, result }
                }
                EditorCommand::SetControlValue {
                    document_id,
                    node_id,
                    value,
                } => {
                    let mut runtime_failed = false;
                    let result = (|| {
                        let doc = document
                            .as_mut()
                            .filter(|d| d.id() == document_id)
                            .ok_or("Stale document")?;
                        doc.set_value(node_id, value)?;
                        if let Some(live) = runtime.as_mut().filter(|_| runtime_current) {
                            let result = live.scalar(
                                doc.flow_name(),
                                doc.runtime_call_index(node_id)?,
                                doc.snapshot()
                                    .nodes
                                    .iter()
                                    .find(|(id, _)| *id == node_id)
                                    .and_then(|(_, n)| n.control_values.get(&0))
                                    .copied()
                                    .unwrap_or(0.) as f32,
                            );
                            runtime_failed = result.is_err();
                            result?;
                        }
                        Ok::<(), String>(())
                    })();
                    if let Err(error) = result {
                        if runtime_failed {
                            runtime = None;
                            status = RuntimeStatus::Error;
                        }
                        deliver(EditorEvent::Error(error), &events, &worker_stop, &repaint);
                    }
                    runtime_event(&document, generation, status)
                }
                EditorCommand::MoveNode {
                    document_id,
                    node_id,
                    position,
                } => {
                    match document
                        .as_mut()
                        .filter(|d| d.id() == document_id)
                        .ok_or_else(|| "Stale document".to_string())
                        .and_then(|d| d.set_position(node_id, position))
                    {
                        Ok(()) => {
                            publish(&document, &worker_snapshots, &repaint);
                            continue;
                        }
                        Err(e) => EditorEvent::Error(e),
                    }
                }
                EditorCommand::DspStart { req_id } => {
                    desired_running = true;
                    let result = (|| {
                        if offline {
                            return Err("This session is forced offline".to_owned());
                        }
                        let doc = document.as_mut().ok_or("Open a flow first")?;
                        doc.can_start()?;
                        if runtime.is_none() {
                            runtime = Some(factory(&config)?);
                        }
                        let live = runtime.as_mut().unwrap();
                        let (rate, outputs) = live.configuration();
                        doc.reconfigure(&core_path, rate, outputs)?;
                        doc.can_start()?;
                        if let Err(e) = live.stop() {
                            runtime = None;
                            return Err(e);
                        }
                        runtime_current = false;
                        deliver(
                            EditorEvent::Runtime {
                                document_id: Some(doc.id()),
                                generation,
                                semantic_revision: None,
                                status: RuntimeStatus::Compiling,
                            },
                            &events,
                            &worker_stop,
                            &repaint,
                        );
                        publish_doc(doc, &worker_snapshots, &repaint);
                        live.prepare(&doc.serialize())?;
                        live.buffers(&resources.bindings(doc)?)?;
                        if let Err(e) = live.install_prepared() {
                            runtime = None;
                            return Err(e);
                        }
                        let root = doc.snapshot().flow_name;
                        for (call, value) in doc.scalar_bindings()? {
                            if let Err(e) = live.scalar(&root, call, value) {
                                runtime = None;
                                return Err(e);
                            }
                        }
                        watches = register_watches(doc, live.as_mut())?;
                        pressed.clear();
                        special_view.midi = if live.midi_available() {
                            "MIDI ready".into()
                        } else {
                            "MIDI unavailable".into()
                        };
                        if let Err(e) = live.start() {
                            runtime = None;
                            return Err(e);
                        }
                        generation += 1;
                        runtime_current = true;
                        Ok::<(), String>(())
                    })();
                    status = if result.is_ok() {
                        RuntimeStatus::Running
                    } else if offline {
                        RuntimeStatus::Offline
                    } else {
                        RuntimeStatus::Error
                    };
                    deliver(
                        runtime_event(&document, generation, status),
                        &events,
                        &worker_stop,
                        &repaint,
                    );
                    EditorEvent::Response { req_id, result }
                }
                EditorCommand::DspStop { req_id } => {
                    desired_running = false;
                    if let (Some(doc), Some(live)) = (document.as_ref(), runtime.as_mut()) {
                        for id in &pressed {
                            if let Ok(call) = doc.runtime_call_index(*id) {
                                let _ = live.scalar(doc.flow_name(), call, 0.);
                            }
                        }
                    }
                    pressed.clear();
                    let result = runtime.as_mut().map_or(Ok(()), |r| r.stop());
                    status = if result.is_err() {
                        runtime = None;
                        RuntimeStatus::Error
                    } else if offline {
                        RuntimeStatus::Offline
                    } else {
                        RuntimeStatus::Stopped
                    };
                    deliver(
                        runtime_event(&document, generation, status),
                        &events,
                        &worker_stop,
                        &repaint,
                    );
                    EditorEvent::Response { req_id, result }
                }
            };
            publish(&document, &worker_snapshots, &repaint);
            if !deliver(event, &events, &worker_stop, &repaint) {
                break;
            }
            for req_id in control_fences {
                if !deliver(
                    EditorEvent::Response {
                        req_id,
                        result: Ok(()),
                    },
                    &events,
                    &worker_stop,
                    &repaint,
                ) {
                    break;
                }
            }
            deliver(
                runtime_event(&document, generation, status),
                &events,
                &worker_stop,
                &repaint,
            );
        }
    });
    EditorServiceHandle {
        tx,
        rx,
        snapshots,
        special,
        graphs_visible,
        stop,
        thread: Some(thread),
    }
}
fn register_watches(
    doc: &EditorDocument,
    live: &mut dyn super::runtime::EditorRuntime,
) -> Result<std::collections::HashMap<wisp::WatchIndex, super::document::EditorNodeId>, String> {
    let mut result = std::collections::HashMap::new();
    for (id, node) in doc.snapshot().nodes {
        if matches!(node.function_name.as_str(), "watch" | "graph") {
            if result.len() >= 64 {
                return Err("Watch node limit (64)".into());
            }
            let watch = live.watch(
                doc.flow_name(),
                doc.runtime_call_index(id)?,
                node.function_name == "watch",
            )?;
            result.insert(watch, id);
        }
    }
    Ok(result)
}
/// Collapse queued drag values even when their acknowledgement fences intervene.
/// Fences are replied to only after the retained value reaches the callback.
fn coalesce_edits(
    command: &mut EditorCommand,
    commands: &Receiver<EditorCommand>,
    pending: &mut Option<EditorCommand>,
    gesture: Option<DocumentId>,
) -> Vec<super::commands::RequestId> {
    let mut fences = vec![];
    for _ in 0..127 {
        match commands.try_recv() {
            Ok(next) if same_edit(command, &next) => *command = next,
            Ok(EditorCommand::ControlFence {
                document_id,
                node_id,
                req_id,
            }) if gesture == Some(document_id)
                && matches!(command,EditorCommand::SetControlValue{document_id:d,node_id:n,..} if *d==document_id && *n==node_id) =>
            {
                fences.push(req_id);
            }
            Ok(next) => {
                *pending = Some(next);
                break;
            }
            Err(_) => break,
        }
    }
    fences
}
fn runtime_event(
    document: &Option<EditorDocument>,
    generation: u64,
    status: RuntimeStatus,
) -> EditorEvent {
    EditorEvent::Runtime {
        document_id: document.as_ref().map(|d| d.id()),
        generation,
        semantic_revision: if status == RuntimeStatus::Running {
            document.as_ref().map(|d| d.semantic_revision())
        } else {
            None
        },
        status,
    }
}
fn publish_doc(doc: &EditorDocument, mailbox: &SnapshotMailbox, ctx: &egui::Context) {
    *mailbox.lock().unwrap() = Some(doc.snapshot());
    ctx.request_repaint();
}
fn publish(document: &Option<EditorDocument>, mailbox: &SnapshotMailbox, ctx: &egui::Context) {
    if let Some(doc) = document {
        *mailbox.lock().unwrap() = Some(doc.snapshot());
        ctx.request_repaint();
    }
}
fn deliver(
    mut event: EditorEvent,
    tx: &Sender<EditorEvent>,
    stop: &AtomicBool,
    ctx: &egui::Context,
) -> bool {
    while !stop.load(Ordering::Acquire) {
        match tx.send_timeout(event, Duration::from_millis(25)) {
            Ok(()) => {
                ctx.request_repaint();
                return true;
            }
            Err(SendTimeoutError::Timeout(e)) => event = e,
            Err(SendTimeoutError::Disconnected(_)) => return false,
        }
    }
    false
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::commands::RequestId;
    fn response(service: &EditorServiceHandle) -> EditorEvent {
        loop {
            let event = service.rx.recv_timeout(Duration::from_secs(3)).unwrap();
            if matches!(event, EditorEvent::Response { .. }) {
                return event;
            }
        }
    }
    fn core() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../wisp/core")
    }
    struct Fake {
        log: Arc<Mutex<Vec<String>>>,
        fail_scalar: Arc<AtomicBool>,
    }
    impl super::super::runtime::EditorRuntime for Fake {
        fn configuration(&self) -> (u32, u32) {
            (48000, 2)
        }
        fn prepare(&mut self, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn install_prepared(&mut self) -> Result<(), String> {
            self.log.lock().unwrap().push("install".into());
            Ok(())
        }
        fn scalar(&mut self, _: &str, call: u32, value: f32) -> Result<(), String> {
            self.log
                .lock()
                .unwrap()
                .push(format!("scalar:{call}:{value}"));
            if self.fail_scalar.load(Ordering::Acquire) {
                Err("injected scalar failure".into())
            } else {
                Ok(())
            }
        }
        fn start(&mut self) -> Result<(), String> {
            self.log.lock().unwrap().push("start".into());
            Ok(())
        }
        fn stop(&mut self) -> Result<(), String> {
            self.log.lock().unwrap().push("stop".into());
            Ok(())
        }
        fn healthy(&self) -> bool {
            true
        }
    }
    impl Drop for Fake {
        fn drop(&mut self) {
            self.log.lock().unwrap().push("drop".into());
        }
    }
    #[test]
    fn live_sequence_restores_values_updates_without_recompile_and_recovers() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let worker_log = log.clone();
        let fail = Arc::new(AtomicBool::new(false));
        let worker_fail = fail.clone();
        let service = spawn_configured_service(
            core(),
            egui::Context::default(),
            false,
            Default::default(),
            Box::new(move |_| {
                Ok(Box::new(Fake {
                    log: worker_log.clone(),
                    fail_scalar: worker_fail.clone(),
                }))
            }),
        );
        service
            .submit(EditorCommand::LoadFlow {
                path: core().join("../../wisp_gui/control.twf"),
                req_id: RequestId(1),
            })
            .unwrap();
        assert!(matches!(
            response(&service),
            EditorEvent::Response { result: Ok(()), .. }
        ));
        service
            .submit(EditorCommand::DspStart {
                req_id: RequestId(2),
            })
            .unwrap();
        assert!(matches!(
            response(&service),
            EditorEvent::Response { result: Ok(()), .. }
        ));
        let calls = log.lock().unwrap().clone();
        assert_eq!(calls.len(), 5);
        assert_eq!(calls[0], "stop");
        assert_eq!(calls[1], "install");
        assert!(calls[2].starts_with("scalar:1:0.103"));
        assert!(calls[3].starts_with("scalar:4:0.455"));
        assert_eq!(calls[4], "start");
        // A failed replacement does not tear down the running processor.
        service
            .submit(EditorCommand::LoadFlow {
                path: core().join("absent"),
                req_id: RequestId(3),
            })
            .unwrap();
        assert!(matches!(
            response(&service),
            EditorEvent::Response { result: Err(_), .. }
        ));
        assert!(!log.lock().unwrap().iter().any(|s| s == "drop"));
        service
            .submit(EditorCommand::SetControlValue {
                document_id: DocumentId(99),
                node_id: super::super::document::EditorNodeId(2),
                value: 0.2,
            })
            .unwrap();
        loop {
            if let EditorEvent::Error(error) =
                service.rx.recv_timeout(Duration::from_secs(3)).unwrap()
            {
                assert!(error.contains("Stale"));
                break;
            }
        }
        assert!(!log.lock().unwrap().iter().any(|s| s == "drop"));
        fail.store(true, Ordering::Release);
        service
            .submit(EditorCommand::SetControlValue {
                document_id: DocumentId(1),
                node_id: super::super::document::EditorNodeId(2),
                value: 0.8,
            })
            .unwrap();
        loop {
            if matches!(
                service.rx.recv_timeout(Duration::from_secs(3)).unwrap(),
                EditorEvent::Error(_)
            ) {
                break;
            }
        }
        service
            .submit(EditorCommand::SetControlValue {
                document_id: DocumentId(1),
                node_id: super::super::document::EditorNodeId(2),
                value: 0.6,
            })
            .unwrap();
        service
            .submit(EditorCommand::DspStop {
                req_id: RequestId(4),
            })
            .unwrap();
        assert!(matches!(
            response(&service),
            EditorEvent::Response { result: Ok(()), .. }
        ));
        let snapshot = service.take_snapshot().unwrap();
        assert!((snapshot.nodes[1].1.control_values[&0] - 0.6).abs() < 1e-6);
        assert!(log.lock().unwrap().iter().any(|s| s == "drop"));
        assert_eq!(
            log.lock()
                .unwrap()
                .iter()
                .filter(|s| *s == "install")
                .count(),
            1
        );
    }
    #[test]
    fn offline_never_calls_factory_and_stale_edits_are_rejected() {
        let service = spawn_configured_service(
            core(),
            egui::Context::default(),
            true,
            Default::default(),
            Box::new(|_| panic!("offline must never open devices")),
        );
        service
            .submit(EditorCommand::LoadFlow {
                path: core().join("../../wisp_gui/control.twf"),
                req_id: RequestId(1),
            })
            .unwrap();
        response(&service);
        service
            .submit(EditorCommand::DspStart {
                req_id: RequestId(2),
            })
            .unwrap();
        assert!(matches!(
            response(&service),
            EditorEvent::Response { result: Err(_), .. }
        ));
        service
            .submit(EditorCommand::MoveNode {
                document_id: DocumentId(99),
                node_id: super::super::document::EditorNodeId(2),
                position: egui::Vec2::ZERO,
            })
            .unwrap();
        loop {
            if let EditorEvent::Error(e) = service.rx.recv_timeout(Duration::from_secs(3)).unwrap()
            {
                assert!(e.contains("Stale"));
                break;
            }
        }
    }
    #[test]
    fn failed_load_retains_document_and_dsp_never_fakes_success() {
        let service = spawn_editor_service(core(), egui::Context::default());
        service
            .submit(EditorCommand::LoadFlow {
                path: core().join("../../wisp_gui/control.twf"),
                req_id: RequestId(1),
            })
            .unwrap();
        assert!(matches!(
            response(&service),
            EditorEvent::Response { result: Ok(()), .. }
        ));
        service
            .submit(EditorCommand::LoadFlow {
                path: core().join("absent.twf"),
                req_id: RequestId(2),
            })
            .unwrap();
        assert!(matches!(
            response(&service),
            EditorEvent::Response { result: Err(_), .. }
        ));
        service
            .submit(EditorCommand::DspStart {
                req_id: RequestId(3),
            })
            .unwrap();
        assert!(matches!(
            response(&service),
            EditorEvent::Response { result: Err(_), .. }
        ));
        // The response is sent before the snapshot: wait for publication without requiring a device.
        for _ in 0..100 {
            if service
                .snapshots
                .lock()
                .unwrap()
                .as_ref()
                .is_some_and(|s| s.nodes.len() == 7)
            {
                return;
            }
            thread::sleep(Duration::from_millis(5));
        }
        panic!("Loaded document was lost");
    }
    struct RecoveryFake {
        log: Arc<Mutex<Vec<String>>>,
        fail: Arc<std::sync::atomic::AtomicUsize>,
    }
    impl RecoveryFake {
        fn step(&self, name: &str, n: usize) -> Result<(), String> {
            self.log.lock().unwrap().push(name.into());
            if self.fail.load(Ordering::Acquire) == n {
                Err(format!("injected {name} failure"))
            } else {
                Ok(())
            }
        }
    }
    impl super::super::runtime::EditorRuntime for RecoveryFake {
        fn configuration(&self) -> (u32, u32) {
            (44100, 2)
        }
        fn prepare(&mut self, _: &str) -> Result<(), String> {
            self.step("compile", 1)
        }
        fn install_prepared(&mut self) -> Result<(), String> {
            self.step("install", 2)
        }
        fn scalar(&mut self, _: &str, _: u32, _: f32) -> Result<(), String> {
            self.step("scalar", 3)
        }
        fn start(&mut self) -> Result<(), String> {
            self.step("start", 4)
        }
        fn stop(&mut self) -> Result<(), String> {
            self.step("stop", 5)
        }
        fn healthy(&self) -> bool {
            true
        }
    }
    impl Drop for RecoveryFake {
        fn drop(&mut self) {
            self.log.lock().unwrap().push("drop".into());
        }
    }
    fn recovery_service() -> (
        EditorServiceHandle,
        Arc<Mutex<Vec<String>>>,
        Arc<std::sync::atomic::AtomicUsize>,
    ) {
        let log = Arc::new(Mutex::new(vec![]));
        let fail = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let l = log.clone();
        let f = fail.clone();
        let service = spawn_configured_service(
            core(),
            egui::Context::default(),
            false,
            Default::default(),
            Box::new(move |_| {
                l.lock().unwrap().push("open".into());
                Ok(Box::new(RecoveryFake {
                    log: l.clone(),
                    fail: f.clone(),
                }))
            }),
        );
        service
            .submit(EditorCommand::LoadFlow {
                path: core().join("../../wisp_gui/control.twf"),
                req_id: RequestId(1),
            })
            .unwrap();
        assert!(matches!(
            response(&service),
            EditorEvent::Response { result: Ok(()), .. }
        ));
        service
            .submit(EditorCommand::DspStart {
                req_id: RequestId(2),
            })
            .unwrap();
        assert!(matches!(
            response(&service),
            EditorEvent::Response { result: Ok(()), .. }
        ));
        log.lock().unwrap().clear();
        (service, log, fail)
    }
    fn service_edit(
        service: &EditorServiceHandle,
        edit: super::super::document::Edit,
    ) -> Result<(), String> {
        let s = service.snapshots.lock().unwrap().as_ref().unwrap().clone();
        service
            .submit(EditorCommand::Edit {
                document_id: s.id,
                revision: s.revision,
                edit,
                req_id: RequestId(50),
            })
            .unwrap();
        match response(service) {
            EditorEvent::Response { result, .. } => result,
            _ => unreachable!(),
        }
    }
    #[test]
    fn structural_runtime_order_retains_compile_failure_and_undo_recovers() {
        use super::super::document::Edit;
        let (service, log, fail) = recovery_service();
        service_edit(
            &service,
            Edit::Create {
                text: "= 1".into(),
                position: egui::Vec2::ZERO,
            },
        )
        .unwrap();
        assert_eq!(
            *log.lock().unwrap(),
            ["stop", "compile", "install", "scalar", "scalar", "start"]
        );
        log.lock().unwrap().clear();
        service_edit(
            &service,
            Edit::Move(vec![(
                super::super::document::EditorNodeId(1),
                egui::vec2(20., 40.),
            )]),
        )
        .unwrap();
        assert!(log.lock().unwrap().is_empty());
        // Rejection leaves playback and history untouched.
        assert!(
            service_edit(
                &service,
                Edit::Create {
                    text: "= (".into(),
                    position: egui::Vec2::ZERO
                }
            )
            .is_err()
        );
        assert!(log.lock().unwrap().is_empty());
        fail.store(1, Ordering::Release);
        assert!(
            service_edit(
                &service,
                Edit::Create {
                    text: "= 2".into(),
                    position: egui::Vec2::ZERO
                }
            )
            .is_err()
        );
        assert_eq!(*log.lock().unwrap(), ["stop", "compile"]);
        assert_eq!(
            service
                .snapshots
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .nodes
                .len(),
            9
        );
        log.lock().unwrap().clear();
        fail.store(0, Ordering::Release);
        service_edit(&service, Edit::Undo).unwrap();
        assert_eq!(
            *log.lock().unwrap(),
            ["stop", "compile", "install", "scalar", "scalar", "start"]
        );
        // Explicit restart also reuses the device.
        log.lock().unwrap().clear();
        service
            .submit(EditorCommand::DspStop {
                req_id: RequestId(60),
            })
            .unwrap();
        response(&service);
        service
            .submit(EditorCommand::DspStart {
                req_id: RequestId(61),
            })
            .unwrap();
        response(&service);
        assert!(
            !log.lock()
                .unwrap()
                .iter()
                .any(|s| s == "open" || s == "drop")
        );
    }
    #[test]
    fn failed_install_initialization_and_device_leave_editable_history() {
        use super::super::document::Edit;
        for phase in [2, 3, 4, 5] {
            let (service, log, fail) = recovery_service();
            fail.store(phase, Ordering::Release);
            assert!(
                service_edit(
                    &service,
                    Edit::Create {
                        text: "= 1".into(),
                        position: egui::Vec2::ZERO
                    }
                )
                .is_err()
            );
            let calls = log.lock().unwrap().clone();
            assert_eq!(calls.first().unwrap(), "stop");
            assert_eq!(calls.last().unwrap(), "drop");
            if phase != 4 {
                assert!(!calls.iter().any(|s| s == "start"));
            }
            assert_eq!(
                service
                    .snapshots
                    .lock()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .nodes
                    .len(),
                8
            );
            fail.store(0, Ordering::Release);
            service_edit(&service, Edit::Undo).unwrap();
            assert_eq!(
                service
                    .snapshots
                    .lock()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .nodes
                    .len(),
                7
            );
            service
                .submit(EditorCommand::DspStart {
                    req_id: RequestId(70),
                })
                .unwrap();
            assert!(matches!(
                response(&service),
                EditorEvent::Response { result: Ok(()), .. }
            ));
        }
    }
    struct GatedScalar {
        values: Arc<Mutex<Vec<f32>>>,
        gate: Arc<AtomicBool>,
        entered: Sender<()>,
        resume: Receiver<()>,
    }
    impl super::super::runtime::EditorRuntime for GatedScalar {
        fn configuration(&self) -> (u32, u32) {
            (44100, 2)
        }
        fn prepare(&mut self, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn install_prepared(&mut self) -> Result<(), String> {
            Ok(())
        }
        fn scalar(&mut self, _: &str, _: u32, value: f32) -> Result<(), String> {
            self.values.lock().unwrap().push(value);
            if self.gate.swap(false, Ordering::AcqRel) {
                self.entered.send(()).unwrap();
                self.resume
                    .recv_timeout(Duration::from_secs(3))
                    .map_err(|e| e.to_string())?;
            }
            Ok(())
        }
        fn start(&mut self) -> Result<(), String> {
            Ok(())
        }
        fn stop(&mut self) -> Result<(), String> {
            Ok(())
        }
        fn healthy(&self) -> bool {
            true
        }
    }
    #[test]
    fn drag_uses_latest_value_after_callback_instead_of_replaying_backlog() {
        use super::super::document::{Edit, EditorNodeId};
        let values = Arc::new(Mutex::new(vec![]));
        let gate = Arc::new(AtomicBool::new(false));
        let (entered_tx, entered) = bounded(1);
        let (resume_tx, resume) = bounded(1);
        let v = values.clone();
        let g = gate.clone();
        let service = spawn_configured_service(
            core(),
            egui::Context::default(),
            false,
            Default::default(),
            Box::new(move |_| {
                Ok(Box::new(GatedScalar {
                    values: v.clone(),
                    gate: g.clone(),
                    entered: entered_tx.clone(),
                    resume: resume.clone(),
                }))
            }),
        );
        service
            .submit(EditorCommand::LoadFlow {
                path: core().join("../../wisp_gui/control.twf"),
                req_id: RequestId(1),
            })
            .unwrap();
        response(&service);
        service
            .submit(EditorCommand::DspStart {
                req_id: RequestId(2),
            })
            .unwrap();
        response(&service);
        values.lock().unwrap().clear();
        gate.store(true, Ordering::Release);
        service
            .submit(EditorCommand::Gesture {
                document_id: DocumentId(1),
                begin: true,
            })
            .unwrap();
        service
            .submit(EditorCommand::SetControlValue {
                document_id: DocumentId(1),
                node_id: EditorNodeId(2),
                value: 0.1,
            })
            .unwrap();
        entered.recv_timeout(Duration::from_secs(3)).unwrap();
        service
            .submit(EditorCommand::ControlFence {
                document_id: DocumentId(1),
                node_id: EditorNodeId(2),
                req_id: RequestId(100),
            })
            .unwrap();
        // Model fast UI input while one audio callback is still outstanding.
        for i in 0..40 {
            service
                .submit(EditorCommand::SetControlValue {
                    document_id: DocumentId(1),
                    node_id: EditorNodeId(2),
                    value: (20 + i) as f64 / 100.,
                })
                .unwrap();
            service
                .submit(EditorCommand::ControlFence {
                    document_id: DocumentId(1),
                    node_id: EditorNodeId(2),
                    req_id: RequestId(200 + i),
                })
                .unwrap();
        }
        service
            .submit(EditorCommand::Gesture {
                document_id: DocumentId(1),
                begin: false,
            })
            .unwrap();
        service
            .submit(EditorCommand::Barrier {
                req_id: RequestId(999),
            })
            .unwrap();
        resume_tx.send(()).unwrap();
        let mut replies = std::collections::BTreeSet::new();
        loop {
            if let EditorEvent::Response { req_id, result } =
                service.rx.recv_timeout(Duration::from_secs(3)).unwrap()
            {
                result.unwrap();
                if req_id == RequestId(999) {
                    break;
                }
                replies.insert(req_id.0);
            }
        }
        assert_eq!(replies.len(), 41);
        assert!(replies.contains(&239));
        assert_eq!(*values.lock().unwrap(), [0.1f32, 0.59f32]);
        assert_eq!(
            service.snapshots.lock().unwrap().as_ref().unwrap().nodes[1]
                .1
                .control_values[&0],
            0.59f32 as f64
        );
        service_edit(&service, Edit::Undo).unwrap();
        let snapshot = service.snapshots.lock().unwrap().as_ref().unwrap().clone();
        assert!(!snapshot.can_undo);
        assert!(!snapshot.dirty);
        assert!((snapshot.nodes[1].1.control_values[&0] - 0.103).abs() < 1e-6);
    }
    #[test]
    fn control_coalescing_respects_fences_outside_gestures_and_explicit_barriers() {
        use super::super::document::EditorNodeId;
        let value = || EditorCommand::SetControlValue {
            document_id: DocumentId(1),
            node_id: EditorNodeId(2),
            value: 0.7,
        };
        for boundary in [
            EditorCommand::Gesture {
                document_id: DocumentId(1),
                begin: false,
            },
            EditorCommand::Barrier {
                req_id: RequestId(1),
            },
            EditorCommand::DspStop {
                req_id: RequestId(2),
            },
        ] {
            let (tx, rx) = bounded(4);
            tx.send(boundary).unwrap();
            tx.send(value()).unwrap();
            let mut command = value();
            let mut pending = None;
            assert!(
                coalesce_edits(&mut command, &rx, &mut pending, Some(DocumentId(1))).is_empty()
            );
            assert!(pending.is_some());
            assert_eq!(rx.len(), 1);
        }
        let (tx, rx) = bounded(4);
        tx.send(EditorCommand::ControlFence {
            document_id: DocumentId(1),
            node_id: EditorNodeId(2),
            req_id: RequestId(3),
        })
        .unwrap();
        tx.send(value()).unwrap();
        let mut command = value();
        let mut pending = None;
        assert!(coalesce_edits(&mut command, &rx, &mut pending, None).is_empty());
        assert!(matches!(pending, Some(EditorCommand::ControlFence { .. })));
        assert_eq!(rx.len(), 1);
    }
    #[test]
    fn saturated_queue_preserves_gesture_end_and_final_values() {
        use super::super::document::{Edit, EditorNodeId};
        let service = spawn_editor_service(core(), egui::Context::default());
        service
            .submit(EditorCommand::LoadFlow {
                path: core().join("../../wisp_gui/control.twf"),
                req_id: RequestId(1),
            })
            .unwrap();
        response(&service);
        let original = service.snapshots.lock().unwrap().as_ref().unwrap().clone();
        let mut pending = std::collections::VecDeque::new();
        pending.push_back(EditorCommand::Gesture {
            document_id: DocumentId(1),
            begin: true,
        });
        for i in 0..512 {
            pending.push_back(EditorCommand::SetControlValue {
                document_id: DocumentId(1),
                node_id: EditorNodeId(if i % 2 == 0 { 2 } else { 5 }),
                value: i as f64 / 512.,
            });
        }
        pending.push_back(EditorCommand::Gesture {
            document_id: DocumentId(1),
            begin: false,
        });
        pending.push_back(EditorCommand::Barrier {
            req_id: RequestId(90),
        });
        let mut saturated = false;
        // Hold event consumption until the bounded queues really exert backpressure.
        while let Some(command) = pending.pop_front() {
            if let Err(crossbeam::channel::TrySendError::Full(command)) =
                service.tx.try_send(command)
            {
                pending.push_front(command);
                saturated = true;
                break;
            }
        }
        assert!(saturated);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while let Some(command) = pending.pop_front() {
            if let Err(crossbeam::channel::TrySendError::Full(command)) =
                service.tx.try_send(command)
            {
                pending.push_front(command);
            }
            while service.rx.try_recv().is_ok() {}
            assert!(std::time::Instant::now() < deadline);
            thread::yield_now();
        }
        // The last drain may already have consumed the barrier: use a fresh one.
        loop {
            match service.tx.try_send(EditorCommand::Barrier {
                req_id: RequestId(91),
            }) {
                Ok(()) => break,
                Err(crossbeam::channel::TrySendError::Full(_)) => {
                    while service.rx.try_recv().is_ok() {}
                    thread::yield_now();
                }
                Err(_) => panic!("worker disconnected"),
            }
            assert!(std::time::Instant::now() < deadline);
        }
        loop {
            if matches!(
                response(&service),
                EditorEvent::Response {
                    req_id: RequestId(91),
                    ..
                }
            ) {
                break;
            }
        }
        let edited = service.snapshots.lock().unwrap().as_ref().unwrap().clone();
        assert_eq!(
            edited.nodes[1].1.control_values[&0],
            (510.0f32 / 512.) as f64
        );
        assert_eq!(
            edited.nodes[4].1.control_values[&0],
            (511.0f32 / 512.) as f64
        );
        assert!(edited.can_undo);
        service_edit(&service, Edit::Undo).unwrap();
        let undone = service.snapshots.lock().unwrap().as_ref().unwrap().clone();
        assert!(!undone.can_undo);
        assert!(!undone.dirty);
        assert_eq!(
            undone.nodes[1].1.control_values,
            original.nodes[1].1.control_values
        );
        assert_eq!(
            undone.nodes[4].1.control_values,
            original.nodes[4].1.control_values
        );
    }
    #[test]
    fn shutdown_does_not_wait_for_ui_to_drain_full_events() {
        let service = spawn_editor_service(core(), egui::Context::default());
        for i in 0..300 {
            let _ = service.submit(EditorCommand::DspStart {
                req_id: RequestId(i),
            });
        }
        drop(service);
    }
    #[test]
    fn save_service_success_conflict_failure_and_retry() {
        use super::super::persistence::{Fingerprint, SaveError, tests::Temp};
        let dir = Temp::new();
        let path = dir.0.join("saved.twf");
        let service = spawn_configured_service(
            core(),
            egui::Context::default(),
            true,
            Default::default(),
            Box::new(|_| panic!("No devices during persistence")),
        );
        service
            .submit(EditorCommand::LoadFlow {
                path: core().join("../../wisp_gui/control.twf"),
                req_id: RequestId(1),
            })
            .unwrap();
        response(&service);
        let before = service.take_snapshot().unwrap();
        let save = |path: PathBuf, overwrite, request| {
            let snap = service
                .snapshots
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(|| before.clone());
            service
                .submit(EditorCommand::Save {
                    document_id: snap.id,
                    revision: snap.revision,
                    state_id: snap.state_id,
                    path: Some(path),
                    overwrite,
                    req_id: RequestId(request),
                })
                .unwrap();
            loop {
                if let EditorEvent::Saved { req_id, result, .. } =
                    service.rx.recv_timeout(Duration::from_secs(3)).unwrap()
                {
                    assert_eq!(req_id, RequestId(request));
                    break result;
                }
            }
        };
        let receipt = save(path.clone(), None, 2).unwrap();
        assert!(receipt.warnings.is_empty());
        let after = service.snapshots.lock().unwrap().clone().unwrap();
        assert!(!after.dirty);
        assert_eq!(before.id, after.id);
        assert_eq!(before.revision, after.revision);
        assert_eq!(before.semantic_revision, after.semantic_revision);
        assert_eq!(
            before.nodes.iter().map(|n| n.0).collect::<Vec<_>>(),
            after.nodes.iter().map(|n| n.0).collect::<Vec<_>>()
        );
        assert!(
            super::super::document::EditorDocument::load(&path, &core(), DocumentId(99)).is_ok()
        );
        std::fs::write(&path, b"external change").unwrap();
        assert!(matches!(
            save(path.clone(), None, 3),
            Err(SaveError::Conflict { .. })
        ));
        assert_eq!(std::fs::read(&path).unwrap(), b"external change");
        let fingerprint = Fingerprint::read(&path).unwrap();
        save(path.clone(), Some(fingerprint), 4).unwrap();
        let saved_path = service
            .snapshots
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .source_file
            .clone();
        assert!(save(dir.0.join("missing/flow.twf"), None, 5).is_err());
        assert_eq!(
            service
                .snapshots
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .source_file,
            saved_path
        );
        save(path, None, 6).unwrap();
    }
    struct SpecializedFake {
        midi: Arc<wisp::MidiMailbox>,
        writes: Arc<Mutex<Vec<(u32, f32)>>>,
        full: Arc<AtomicBool>,
        queries: Arc<std::sync::atomic::AtomicUsize>,
        query_fail: Arc<AtomicBool>,
        watches: Vec<wisp::WatchIndex>,
    }
    impl super::super::runtime::EditorRuntime for SpecializedFake {
        fn configuration(&self) -> (u32, u32) {
            (44100, 2)
        }
        fn prepare(&mut self, _: &str) -> Result<(), String> {
            self.watches.clear();
            Ok(())
        }
        fn install_prepared(&mut self) -> Result<(), String> {
            Ok(())
        }
        fn scalar(&mut self, _: &str, call: u32, value: f32) -> Result<(), String> {
            self.writes.lock().unwrap().push((call, value));
            Ok(())
        }
        fn scalar_try(&mut self, root: &str, call: u32, value: f32) -> Result<(), String> {
            if self.full.load(Ordering::Acquire) {
                Err("full".into())
            } else {
                self.scalar(root, call, value)
            }
        }
        fn watch(&mut self, _: &str, call: u32, _: bool) -> Result<wisp::WatchIndex, String> {
            let id = wisp::WatchIndex(call);
            self.watches.push(id);
            Ok(id)
        }
        fn query(&mut self) -> Result<wisp::WatchedDataValues, String> {
            self.queries.fetch_add(1, Ordering::Relaxed);
            if self.query_fail.swap(false, Ordering::AcqRel) {
                return Err("watch timeout".into());
            }
            Ok(wisp::WatchedDataValues {
                values: self
                    .watches
                    .iter()
                    .map(|id| (*id, vec![0.75; 4096]))
                    .collect(),
            })
        }
        fn midi_available(&self) -> bool {
            true
        }
        fn midi_learn(&mut self, target: u64) -> Result<(), String> {
            self.midi.learn(target);
            Ok(())
        }
        fn midi_targets(&self) -> Vec<u64> {
            self.midi.targets()
        }
        fn midi_cancel(&mut self) {
            self.midi.cancel();
        }
        fn midi_remove(&mut self, target: u64) {
            self.midi.remove(target);
        }
        fn midi_poll(&mut self) -> Vec<wisp::MidiObservation> {
            self.midi.poll()
        }
        fn start(&mut self) -> Result<(), String> {
            Ok(())
        }
        fn stop(&mut self) -> Result<(), String> {
            Ok(())
        }
        fn healthy(&self) -> bool {
            true
        }
    }
    #[track_caller]
    fn wait_for(mut condition: impl FnMut() -> bool) {
        let end = std::time::Instant::now() + Duration::from_secs(3);
        while !condition() {
            assert!(
                std::time::Instant::now() < end,
                "service condition timed out"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    #[test]
    fn real_service_telemetry_midi_retry_and_momentary_state() {
        use super::super::{document::EditorNodeId, persistence::tests::Temp};
        let dir = Temp::new();
        let path = dir.0.join("special.twf");
        std::fs::write(&path,"[flow]\nname='special'\n[[flow.nodes]]\ntext='control'\nvalue=0.25\n[[flow.nodes]]\ntext='toggle'\nvalue=1.0\n[[flow.nodes]]\ntext='button'\nvalue=1.0\n[[flow.nodes]]\ntext='watch'\n[[flow.nodes]]\ntext='graph'\n").unwrap();
        let midi = Arc::new(wisp::MidiMailbox::default());
        let writes = Arc::new(Mutex::new(vec![]));
        let full = Arc::new(AtomicBool::new(false));
        let queries = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let query_fail = Arc::new(AtomicBool::new(false));
        let factory = {
            let midi = midi.clone();
            let writes = writes.clone();
            let full = full.clone();
            let queries = queries.clone();
            let query_fail = query_fail.clone();
            Box::new(move |_: &wisp::TwistedWispEngineConfig| {
                Ok(Box::new(SpecializedFake {
                    midi: midi.clone(),
                    writes: writes.clone(),
                    full: full.clone(),
                    queries: queries.clone(),
                    query_fail: query_fail.clone(),
                    watches: vec![],
                })
                    as Box<dyn super::super::runtime::EditorRuntime>)
            })
        };
        let service = spawn_configured_service(
            core(),
            egui::Context::default(),
            false,
            Default::default(),
            factory,
        );
        service
            .submit(EditorCommand::LoadFlow {
                path,
                req_id: RequestId(1),
            })
            .unwrap();
        response(&service);
        service
            .submit(EditorCommand::DspStart {
                req_id: RequestId(2),
            })
            .unwrap();
        assert!(matches!(
            response(&service),
            EditorEvent::Response { result: Ok(()), .. }
        ));
        assert!(writes.lock().unwrap().contains(&(1, 1.)));
        assert!(writes.lock().unwrap().contains(&(2, 0.))); // stored button 1 never starts pressed
        wait_for(|| {
            service
                .special
                .lock()
                .unwrap()
                .as_ref()
                .is_some_and(|s| s.running && s.traces.len() == 2)
        });
        {
            let lock = service.special.lock().unwrap();
            let s = lock.as_ref().unwrap();
            assert!(
                s.traces
                    .values()
                    .all(|v| v.len() == 4096 && v.back() == Some(&0.75))
            );
        }
        let before = service.snapshots.lock().unwrap().clone().unwrap();
        service
            .submit(EditorCommand::MidiLearn {
                document_id: before.id,
                node_id: Some(EditorNodeId(1)),
                req_id: RequestId(3),
            })
            .unwrap();
        response(&service);
        wait_for(|| {
            service
                .special
                .lock()
                .unwrap()
                .as_ref()
                .is_some_and(|s| s.midi.starts_with("Learning"))
        });
        service
            .submit(EditorCommand::MidiLearn {
                document_id: before.id,
                node_id: None,
                req_id: RequestId(4),
            })
            .unwrap();
        response(&service);
        midi.cc(0, 7, 127);
        assert!(midi.poll().is_empty());
        service
            .submit(EditorCommand::MidiLearn {
                document_id: before.id,
                node_id: Some(EditorNodeId(1)),
                req_id: RequestId(5),
            })
            .unwrap();
        response(&service);
        full.store(true, Ordering::Release);
        midi.cc(0, 7, 127);
        wait_for(|| {
            service
                .special
                .lock()
                .unwrap()
                .as_ref()
                .is_some_and(|s| s.midi.starts_with("Bound"))
        });
        assert_eq!(
            service.snapshots.lock().unwrap().as_ref().unwrap().nodes[0]
                .1
                .control_values[&0],
            0.25
        );
        let write_count = writes.lock().unwrap().len();
        full.store(false, Ordering::Release);
        // No second MIDI event: the final pending value must still be retried.
        wait_for(|| {
            service
                .snapshots
                .lock()
                .unwrap()
                .as_ref()
                .is_some_and(|s| s.nodes[0].1.control_values[&0] == 1.)
        });
        assert_eq!(writes.lock().unwrap().len(), write_count + 1);
        let updated = service.snapshots.lock().unwrap().clone().unwrap();
        assert!(updated.dirty);
        assert_eq!(updated.state_id, before.state_id);
        assert!(!updated.can_undo);
        service
            .submit(EditorCommand::Momentary {
                document_id: before.id,
                node_id: EditorNodeId(3),
                pressed: true,
            })
            .unwrap();
        service
            .submit(EditorCommand::DspStop {
                req_id: RequestId(6),
            })
            .unwrap();
        response(&service);
        assert_eq!(writes.lock().unwrap().last(), Some(&(2, 0.)));
        assert_eq!(
            service.snapshots.lock().unwrap().as_ref().unwrap().state_id,
            before.state_id
        );
        let generation = service.special.lock().unwrap().as_ref().unwrap().generation;
        service
            .submit(EditorCommand::DspStart {
                req_id: RequestId(7),
            })
            .unwrap();
        response(&service);
        wait_for(|| {
            service
                .special
                .lock()
                .unwrap()
                .as_ref()
                .is_some_and(|s| s.generation > generation && s.traces.len() == 2)
        });
        let previous_queries = queries.load(Ordering::Relaxed);
        let until = std::time::Instant::now() + Duration::from_millis(120);
        while std::time::Instant::now() < until {
            let _ = service.tx.try_send(EditorCommand::Barrier {
                req_id: RequestId(100),
            });
            while service.rx.try_recv().is_ok() {}
        }
        wait_for(|| queries.load(Ordering::Relaxed) > previous_queries + 1);
        while service.rx.try_recv().is_ok() {}
        query_fail.store(true, Ordering::Release);
        wait_for(|| {
            while service.rx.try_recv().is_ok() {}
            service
                .special
                .lock()
                .unwrap()
                .as_ref()
                .is_some_and(|s| s.watch_error.is_some())
        });
        service
            .submit(EditorCommand::Barrier {
                req_id: RequestId(999),
            })
            .unwrap();
        loop {
            if matches!(
                service.rx.recv_timeout(Duration::from_secs(1)).unwrap(),
                EditorEvent::Response {
                    req_id: RequestId(999),
                    ..
                }
            ) {
                break;
            }
        }
        wait_for(|| {
            service
                .special
                .lock()
                .unwrap()
                .as_ref()
                .is_some_and(|s| s.watch_error.is_none() && s.traces.len() == 2)
        });
    }
}
