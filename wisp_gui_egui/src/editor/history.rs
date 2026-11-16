// SPDX-License-Identifier: MIT OR Apache-2.0
//! Owned flow mementos. IDs are an ordered sidecar, never recycled backend indices.
use super::document::EditorNodeId;
#[derive(Clone)]
pub(super) struct Memento {
    pub text: String,
    pub ids: Vec<EditorNodeId>,
    pub state: u64,
}
/// Bound each direction to 64 entries and 8 MiB, including the identity sidecar.
pub(super) fn bound(stack: &mut Vec<Memento>) {
    while stack.len() > 64
        || stack
            .iter()
            .map(|m| m.text.len() + m.ids.len() * std::mem::size_of::<EditorNodeId>())
            .sum::<usize>()
            > 8 * 1024 * 1024
    {
        stack.remove(0);
    }
}
