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
    mut factory: super::runtime::RuntimeFactory,
) -> EditorServiceHandle {
    let (tx, commands) = bounded(128);
    let (events, rx) = bounded(128);
    let stop = Arc::new(AtomicBool::new(false));
    let snapshots = Arc::new(Mutex::new(None));
    let worker_stop = stop.clone();
    let worker_snapshots = snapshots.clone();
    let thread = thread::spawn(move || {
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
        while !worker_stop.load(Ordering::Acquire) {
            let mut command = match pending_command
                .take()
                .map(Ok)
                .unwrap_or_else(|| commands.recv_timeout(Duration::from_millis(25)))
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
            // Collapse adjacent edits; loads and DSP requests remain ordering barriers.
            for _ in 0..127 {
                match commands.try_recv() {
                    Ok(next) if same_edit(&command, &next) => command = next,
                    Ok(next) => {
                        pending_command = Some(next);
                        break;
                    }
                    Err(_) => break,
                }
            }
            let event = match command {
                EditorCommand::Barrier { req_id } => EditorEvent::Response {
                    req_id,
                    result: Ok(()),
                },
                EditorCommand::Gesture { document_id, begin } => {
                    match document.as_mut().filter(|d| d.id() == document_id) {
                        Some(doc) => {
                            if begin {
                                doc.begin_gesture();
                            } else {
                                doc.end_gesture();
                            }
                            publish_doc(doc, &worker_snapshots, &repaint);
                            continue;
                        }
                        None => EditorEvent::Error("Stale document".into()),
                    }
                }
                EditorCommand::NewFlow { name, req_id } => {
                    let result =
                        EditorDocument::new_flow(&name, &core_path, DocumentId(next_document)).map(
                            |doc| {
                                runtime = None;
                                desired_running = false;
                                next_document += 1;
                                generation += 1;
                                status = if offline {
                                    RuntimeStatus::Offline
                                } else {
                                    RuntimeStatus::Stopped
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
                    let result = (|| {
                        let doc = document
                            .as_mut()
                            .filter(|d| d.id() == document_id)
                            .ok_or("Stale document")?;
                        let old_revision = doc.snapshot().revision;
                        let old_bindings = doc.scalar_bindings()?;
                        let structural = doc.edit(revision, edit)?;
                        if structural {
                            runtime_current = false;
                        }
                        if doc.snapshot().revision == old_revision {
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
                                if let Err(e) = live.install_prepared() {
                                    runtime = None;
                                    status = RuntimeStatus::Error;
                                    return Err(e);
                                }
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
                    let result = EditorDocument::load(&path, &core_path, DocumentId(next_document))
                        .map(|doc| {
                            runtime = None;
                            desired_running = false;
                            generation += 1;
                            status = if offline {
                                RuntimeStatus::Offline
                            } else {
                                RuntimeStatus::Stopped
                            };
                            next_document += 1;
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
                            let snapshot = doc.snapshot();
                            let result = live.scalar(
                                &snapshot.flow_name,
                                doc.runtime_call_index(node_id)?,
                                value.clamp(0.0, 1.0) as f32,
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
                    publish(&document, &worker_snapshots, &repaint);
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
            publish(&document, &worker_snapshots, &repaint);
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
        stop,
        thread: Some(thread),
    }
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
            document.as_ref().map(|d| d.snapshot().semantic_revision)
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
}
