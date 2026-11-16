// SPDX-License-Identifier: MIT OR Apache-2.0
//! One document's file action, with owned paths and request-scoped completions.
use crate::{
    dialog::DialogKind,
    editor::{commands::RequestId, document::DocumentId, persistence::Fingerprint},
};
use std::path::PathBuf;
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Open(PathBuf),
    New(String),
    Close,
}
#[derive(Clone, Debug)]
pub enum Phase {
    Idle,
    DirtyDecision,
    Choosing {
        request: RequestId,
        document: Option<DocumentId>,
        kind: DialogKind,
    },
    Entering {
        kind: DialogKind,
    },
    WaitingForEdits {
        path: Option<PathBuf>,
        overwrite: Option<Fingerprint>,
    },
    Saving {
        request: RequestId,
        document: DocumentId,
    },
    Loading {
        request: RequestId,
    },
    Conflict {
        path: PathBuf,
        observed: Fingerprint,
    },
}
#[derive(Debug)]
pub struct FileWorkflow {
    pub phase: Phase,
    pub deferred: Option<Action>,
}
impl Default for FileWorkflow {
    fn default() -> Self {
        Self {
            phase: Phase::Idle,
            deferred: None,
        }
    }
}
impl FileWorkflow {
    pub fn busy(&self) -> bool {
        !matches!(self.phase, Phase::Idle)
    }
    pub fn editable(&self) -> bool {
        matches!(self.phase, Phase::Idle | Phase::Saving { .. })
    }
    pub fn cancel(&mut self) {
        self.phase = Phase::Idle;
        self.deferred = None;
    }
    pub fn request_action(&mut self, action: Action, dirty: bool) -> Option<Action> {
        if dirty {
            self.deferred = Some(action);
            self.phase = Phase::DirtyDecision;
            None
        } else {
            Some(action)
        }
    }
    pub fn accepts_dialog(&self, request: RequestId, document: Option<DocumentId>) -> bool {
        matches!(self.phase,Phase::Choosing{request:r,document:d,..} if r==request && d==document)
    }
    pub fn saved(&mut self, dirty: bool) -> Option<Action> {
        self.phase = Phase::Idle;
        if self.deferred.is_some() && dirty {
            self.phase = Phase::DirtyDecision;
            None
        } else {
            self.deferred.take()
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dirty_save_discard_cancel_and_edits_during_save() {
        let mut w = FileWorkflow::default();
        assert!(w.request_action(Action::Close, true).is_none());
        assert!(matches!(w.phase, Phase::DirtyDecision));
        w.cancel();
        assert!(w.deferred.is_none());
        w.request_action(Action::New("new".into()), true);
        assert_eq!(w.saved(false), Some(Action::New("new".into())));
        w.request_action(Action::Close, true);
        assert!(w.saved(true).is_none());
        assert!(matches!(w.phase, Phase::DirtyDecision));
        assert_eq!(w.deferred.take(), Some(Action::Close));
    }
    #[test]
    fn stale_dialogs_and_cancellation_do_not_replace_documents() {
        let mut w = FileWorkflow {
            phase: Phase::Choosing {
                request: RequestId(3),
                document: Some(DocumentId(1)),
                kind: DialogKind::Save,
            },
            deferred: Some(Action::Close),
        };
        assert!(w.accepts_dialog(RequestId(3), Some(DocumentId(1))));
        assert!(!w.accepts_dialog(RequestId(2), Some(DocumentId(1))));
        assert!(!w.accepts_dialog(RequestId(3), Some(DocumentId(2))));
        w.cancel();
        assert!(!w.accepts_dialog(RequestId(3), Some(DocumentId(1))));
        assert!(w.deferred.is_none());
    }
}
