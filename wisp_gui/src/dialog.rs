// SPDX-License-Identifier: MIT OR Apache-2.0
//! A single bounded native-dialog worker; no dialog wait touches the UI/service.
use crate::editor::{commands::RequestId, document::DocumentId};
use crossbeam::channel::{Receiver, Sender, bounded};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogKind {
    Open,
    Save,
    Wave,
}
#[derive(Clone, Debug)]
pub struct DialogRequest {
    pub id: RequestId,
    pub document_id: Option<DocumentId>,
    pub kind: DialogKind,
    pub initial: Option<PathBuf>,
}
#[derive(Debug)]
pub struct DialogResult {
    pub request: DialogRequest,
    pub path: Option<PathBuf>,
}
pub struct DialogWorker {
    pub tx: Sender<DialogRequest>,
    pub rx: Receiver<DialogResult>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl DialogWorker {
    pub fn spawn(repaint: egui::Context) -> Self {
        let (tx, requests) = bounded::<DialogRequest>(1);
        let (results, rx) = bounded(1);
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let worker = std::thread::spawn(move || {
            while !stopped.load(Ordering::Acquire) {
                let request = match requests.recv_timeout(Duration::from_millis(25)) {
                    Ok(r) => r,
                    Err(crossbeam::channel::RecvTimeoutError::Timeout) => continue,
                    Err(_) => break,
                };
                let mut dialog = if request.kind == DialogKind::Wave {
                    rfd::AsyncFileDialog::new().add_filter("Waveform audio", &["wav"])
                } else {
                    rfd::AsyncFileDialog::new().add_filter("TwistedWiSP flow", &["twf"])
                };
                if let Some(path) = &request.initial {
                    if let Some(parent) = path.parent() {
                        dialog = dialog.set_directory(parent);
                    }
                    if request.kind == DialogKind::Save
                        && let Some(name) = path.file_name()
                    {
                        dialog = dialog.set_file_name(name.to_string_lossy());
                    }
                }
                let kind = request.kind;
                let mut pending = Box::pin(async move {
                    match kind {
                        DialogKind::Open | DialogKind::Wave => dialog.pick_file().await,
                        DialogKind::Save => dialog.save_file().await,
                    }
                    .map(|file| file.path().to_owned())
                });
                let waker = std::task::Waker::from(Arc::new(DialogWake(std::thread::current())));
                let mut context = std::task::Context::from_waker(&waker);
                let path = loop {
                    if stopped.load(Ordering::Acquire) {
                        return;
                    }
                    match std::future::Future::poll(pending.as_mut(), &mut context) {
                        std::task::Poll::Ready(path) => break path,
                        std::task::Poll::Pending => {
                            std::thread::park_timeout(Duration::from_millis(20))
                        }
                    }
                };
                if stopped.load(Ordering::Acquire) {
                    break;
                }
                let _ = results.try_send(DialogResult { request, path });
                repaint.request_repaint();
            }
        });
        Self {
            tx,
            rx,
            stop,
            worker: Some(worker),
        }
    }
}
// Dropping the pending async portal request cancels our wait. No detached
// application worker can retain the document or reopen a closing window.
struct DialogWake(std::thread::Thread);
impl std::task::Wake for DialogWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}
impl Drop for DialogWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            worker.thread().unpark();
            let _ = worker.join();
        }
    }
}
