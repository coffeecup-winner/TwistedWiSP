// SPDX-License-Identifier: MIT OR Apache-2.0
use super::document::{DocumentId, DocumentSnapshot, EditorNodeId};
use egui::Vec2;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequestId(pub u64);
#[derive(Clone, Debug)]
pub enum EditorCommand {
    Save {
        document_id: DocumentId,
        revision: u64,
        state_id: u64,
        path: Option<PathBuf>,
        overwrite: Option<super::persistence::Fingerprint>,
        req_id: RequestId,
    },
    SetGeometry {
        document_id: DocumentId,
        node_id: EditorNodeId,
        position: Vec2,
        size: Vec2,
    },
    /// A control acknowledgement can move with a newer value in the same gesture.
    /// Ordinary barriers and gesture boundaries always retain their ordering.
    ControlFence {
        document_id: DocumentId,
        node_id: EditorNodeId,
        req_id: RequestId,
    },
    Barrier {
        req_id: RequestId,
    },
    Gesture {
        document_id: DocumentId,
        begin: bool,
    },
    NewFlow {
        name: String,
        req_id: RequestId,
    },
    Edit {
        document_id: DocumentId,
        revision: u64,
        edit: super::document::Edit,
        req_id: RequestId,
    },
    LoadFlow {
        path: PathBuf,
        req_id: RequestId,
    },
    MoveNode {
        document_id: DocumentId,
        node_id: EditorNodeId,
        position: Vec2,
    },
    SetControlValue {
        document_id: DocumentId,
        node_id: EditorNodeId,
        value: f64,
    },
    DspStart {
        req_id: RequestId,
    },
    DspStop {
        req_id: RequestId,
    },
}
#[derive(Clone, Debug)]
pub enum EditorEvent {
    Saved {
        req_id: RequestId,
        document_id: DocumentId,
        result: Result<super::persistence::SaveReceipt, super::persistence::SaveError>,
    },
    Response {
        req_id: RequestId,
        result: Result<(), String>,
    },
    Runtime {
        document_id: Option<DocumentId>,
        generation: u64,
        semantic_revision: Option<u64>,
        status: RuntimeStatus,
    },
    Error(String),
}
// Snapshots use a latest-value mailbox, not the reliable event queue.
pub type SnapshotMailbox = std::sync::Arc<std::sync::Mutex<Option<DocumentSnapshot>>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeStatus {
    Offline,
    Stopped,
    Compiling,
    Running,
    Error,
}

pub(crate) fn same_edit(a: &EditorCommand, b: &EditorCommand) -> bool {
    match (a, b) {
        (
            EditorCommand::SetControlValue {
                document_id: a,
                node_id: n,
                ..
            },
            EditorCommand::SetControlValue {
                document_id: b,
                node_id: m,
                ..
            },
        )
        | (
            EditorCommand::MoveNode {
                document_id: a,
                node_id: n,
                ..
            },
            EditorCommand::MoveNode {
                document_id: b,
                node_id: m,
                ..
            },
        ) => a == b && n == m,
        _ => false,
    }
}
