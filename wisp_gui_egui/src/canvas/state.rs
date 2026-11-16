// SPDX-License-Identifier: MIT OR Apache-2.0
//! Application view identity. Snarl retains native pan/zoom state per document.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CanvasTab {
    #[default]
    Graph,
    Lab,
}

/// Selection is view state; native slab IDs never leave the canvas adapter.
#[derive(Default)]
pub struct CanvasViewState {
    pub selected: std::collections::BTreeSet<crate::editor::document::EditorNodeId>,
}
