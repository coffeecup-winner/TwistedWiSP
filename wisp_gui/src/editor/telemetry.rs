// SPDX-License-Identifier: MIT OR Apache-2.0
use super::{
    document::{DocumentId, EditorNodeId},
    resources::BufferView,
};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{Arc, Mutex},
};
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SpecialSnapshot {
    pub document: Option<DocumentId>,
    pub generation: u64,
    pub revision: u64,
    pub running: bool,
    pub watch_error: Option<String>,
    pub traces: BTreeMap<EditorNodeId, VecDeque<f32>>,
    pub buffers: BTreeMap<EditorNodeId, BufferView>,
    pub midi: String,
}
pub type SpecialMailbox = Arc<Mutex<Option<SpecialSnapshot>>>;
pub fn append(trace: &mut VecDeque<f32>, values: impl IntoIterator<Item = f32>) {
    for value in values {
        if trace.len() == 4096 {
            trace.pop_front();
        }
        trace.push_back(if value.is_finite() { value } else { 0. });
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn bounded_trace() {
        let mut t = Default::default();
        super::append(&mut t, (0..10000).map(|v| v as f32));
        assert_eq!(t.len(), 4096);
        assert_eq!(t[0], 5904.);
    }
}
