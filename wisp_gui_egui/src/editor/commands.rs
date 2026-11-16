// SPDX-License-Identifier: MIT OR Apache-2.0
use super::document::{DocumentId, DocumentSnapshot, EditorNodeId};
use egui::Vec2;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequestId(pub u64);
#[derive(Clone, Debug)]
pub enum EditorCommand {
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
