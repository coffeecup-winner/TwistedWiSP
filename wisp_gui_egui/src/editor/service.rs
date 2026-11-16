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
                EditorCommand::LoadFlow { path, req_id } => {
                    let result = EditorDocument::load(&path, &core_path, DocumentId(next_document))
                        .map(|doc| {
                            runtime = None;
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
                        if let Some(live) = runtime.as_mut() {
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
                    let result = (|| {
                        if offline {
                            return Err("This session is forced offline".to_owned());
                        }
                        let doc = document.as_mut().ok_or("Open a flow first")?;
                        doc.can_start()?;
                        runtime = None;
                        generation += 1;
                        deliver(
                            EditorEvent::Runtime {
                                document_id: Some(doc.id()),
                                generation,
                                semantic_revision: Some(doc.snapshot().semantic_revision),
                                status: RuntimeStatus::Compiling,
                            },
                            &events,
                            &worker_stop,
                            &repaint,
                        );
                        let mut live = factory(&config)?;
                        let (rate, outputs) = live.configuration();
                        doc.reconfigure(&core_path, rate, outputs)?;
                        doc.can_start()?;
                        publish_doc(doc, &worker_snapshots, &repaint);
                        live.install(&doc.serialize())?;
                        let root = doc.snapshot().flow_name;
                        for (call, value) in doc.scalar_bindings()? {
                            live.scalar(&root, call, value)?;
                        }
                        live.start()?;
                        runtime = Some(live);
                        Ok::<(), String>(())
                    })();
                    status = if result.is_ok() {
                        RuntimeStatus::Running
                    } else if offline {
                        RuntimeStatus::Offline
                    } else {
                        runtime = None;
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
        fn install(&mut self, _: &str) -> Result<(), String> {
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
        assert_eq!(calls.len(), 4);
        assert_eq!(calls[0], "install");
        assert!(calls[1].starts_with("scalar:1:0.103"));
        assert!(calls[2].starts_with("scalar:4:0.455"));
        assert_eq!(calls[3], "start");
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
