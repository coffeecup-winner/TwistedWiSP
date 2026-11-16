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
}
impl DialogWorker {
    pub fn spawn(repaint: egui::Context) -> Self {
        let (tx, requests) = bounded::<DialogRequest>(1);
        let (results, rx) = bounded(1);
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        std::thread::spawn(move || {
            while !stopped.load(Ordering::Acquire) {
                let request = match requests.recv_timeout(Duration::from_millis(25)) {
                    Ok(r) => r,
                    Err(crossbeam::channel::RecvTimeoutError::Timeout) => continue,
                    Err(_) => break,
                };
                let mut dialog = if request.kind == DialogKind::Wave {
                    rfd::FileDialog::new().add_filter("Waveform audio", &["wav"])
                } else {
                    rfd::FileDialog::new().add_filter("TwistedWiSP flow", &["twf"])
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
                let path = match request.kind {
                    DialogKind::Open | DialogKind::Wave => dialog.pick_file(),
                    DialogKind::Save => dialog.save_file(),
                };
                if stopped.load(Ordering::Acquire) {
                    break;
                }
                let _ = results.try_send(DialogResult { request, path });
                repaint.request_repaint();
            }
        });
        Self { tx, rx, stop }
    }
}
// Portals/native dialogs do not expose portable cancellation. Closing the app
// detaches this one worker instead of hanging on an open chooser. It owns no
// document/runtime resources, and drops its result if the UI has gone away.
impl Drop for DialogWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}
