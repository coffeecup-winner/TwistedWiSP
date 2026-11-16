// SPDX-License-Identifier: MIT OR Apache-2.0
//! Backend-backed editor state. Only owned snapshots cross the service boundary.
use egui::Vec2;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use wisp::core::{
    DataType, FlowFunction, FlowNodeExtraData, FlowNodeIndex, FunctionHandle, WispContext,
    WispFunction,
};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DocumentId(pub u64);
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct EditorNodeId(pub usize);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeType {
    Control,
    Flow,
    IO,
    Subflow,
}
#[derive(Clone, Debug)]
pub struct Port {
    pub name: String,
    pub data_type: DataType,
}
#[derive(Clone, Debug)]
pub struct NodeSnapshot {
    pub id: EditorNodeId,
    pub label: String,
    pub function_name: String,
    pub node_type: NodeType,
    pub expression: Option<String>,
    pub position: Vec2,
    pub custom_size: Option<Vec2>,
    pub inputs: Vec<Port>,
    pub outputs: Vec<Port>,
    pub control_values: BTreeMap<usize, f64>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wire {
    pub from: (EditorNodeId, usize),
    pub to: (EditorNodeId, usize),
}
#[derive(Clone, Debug)]
pub struct DocumentSnapshot {
    pub id: DocumentId,
    pub revision: u64,
    pub semantic_revision: u64,
    pub flow_name: String,
    pub source_file: Option<PathBuf>,
    pub dirty: bool,
    pub nodes: Vec<(EditorNodeId, NodeSnapshot)>,
    pub edges: Vec<Wire>,
    pub diagnostics: Vec<String>,
    pub start_error: Option<String>,
}

/// Constructed and retained on the service thread: Rc/RefCell never enter UI events.
pub struct EditorDocument {
    context: WispContext,
    flow: FunctionHandle,
    node_indices: BTreeMap<EditorNodeId, FlowNodeIndex>,
    snapshot: DocumentSnapshot,
}
impl EditorDocument {
    pub fn load(path: &Path, core_path: &Path, id: DocumentId) -> Result<Self, String> {
        let content =
            std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::from_text(&content, core_path, id, Some(path.to_owned()))
    }
    pub fn from_text(
        content: &str,
        core_path: &Path,
        id: DocumentId,
        source_file: Option<PathBuf>,
    ) -> Result<Self, String> {
        Self::from_text_config(content, core_path, id, source_file, 44_100, 2)
    }
    pub fn from_text_config(
        content: &str,
        core_path: &Path,
        id: DocumentId,
        source_file: Option<PathBuf>,
        sample_rate: u32,
        outputs: u32,
    ) -> Result<Self, String> {
        if sample_rate == 0 || outputs == 0 {
            return Err("Sample rate/output count must be positive".into());
        }
        // Validate endpoints before FlowFunction::load: its graph insertion assumes valid indices.
        let value: toml::Value =
            toml::from_str(content).map_err(|e| format!("Invalid flow TOML: {e}"))?;
        let flow_table = value
            .get("flow")
            .and_then(toml::Value::as_table)
            .ok_or("Missing [flow]")?;
        let nodes = flow_table
            .get("nodes")
            .and_then(toml::Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let edges = flow_table
            .get("edges")
            .and_then(toml::Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        for edge in edges {
            for key in ["from", "to"] {
                let n = edge
                    .get(key)
                    .and_then(toml::Value::as_integer)
                    .ok_or_else(|| format!("Missing edge {key}"))?;
                if n < 0 || n as usize >= nodes.len() {
                    return Err(format!("Edge {key} index {n} is out of bounds"));
                }
            }
        }
        let mut context = WispContext::new(outputs, sample_rate);
        context.add_builtin_functions();
        context
            .load_core_functions(core_path)
            .map_err(|e| format!("Cannot load core {}: {e}", core_path.display()))?;
        let function =
            FlowFunction::load(content, &context).ok_or("Invalid .twf flow definition")?;
        let flow_name = function.name().to_owned();
        context.add_function(function);
        let flow = context
            .get_function(&flow_name)
            .ok_or("Loaded flow not found")?;
        let mut snapshot = DocumentSnapshot {
            id,
            revision: 0,
            semantic_revision: 0,
            flow_name,
            source_file,
            dirty: false,
            nodes: vec![],
            edges: vec![],
            diagnostics: vec![],
            start_error: None,
        };
        let mut node_indices = BTreeMap::new();
        let mut reverse = BTreeMap::new();
        {
            let graph = flow.as_flow().ok_or("Not a flow function")?;
            for (ordinal, idx) in graph.node_indices().enumerate() {
                let node = graph.get_node(idx).ok_or("Missing graph node")?;
                let node_id = EditorNodeId(ordinal + 1); // deliberately independent of snarl's slab IDs
                node_indices.insert(node_id, idx);
                reverse.insert(idx.index(), node_id);
                let metadata = context.get_function(&node.name);
                let (inputs, outputs) = if let Some(ref handle) = metadata {
                    let func = handle.borrow();
                    (
                        func.inputs()
                            .iter()
                            .map(|p| Port {
                                name: p.name.clone(),
                                data_type: p.type_,
                            })
                            .collect(),
                        func.outputs()
                            .iter()
                            .map(|p| Port {
                                name: p.name.clone(),
                                data_type: p.type_,
                            })
                            .collect(),
                    )
                } else {
                    snapshot
                        .diagnostics
                        .push(format!("Unknown function: {}", node.display_text));
                    (vec![], vec![])
                };
                for key in ["x", "y", "w", "h", "value"] {
                    if let Some(value) = node.extra_data.get(key) {
                        let number = match value {
                            FlowNodeExtraData::Integer(n) => Some(*n as f32),
                            FlowNodeExtraData::Float(n) if n.is_finite() => Some(*n),
                            _ => None,
                        };
                        if number.is_none()
                            || (matches!(key, "w" | "h") && number.is_some_and(|n| n <= 0.0))
                        {
                            snapshot
                                .diagnostics
                                .push(format!("Invalid {key} property on {}", node.display_text));
                        }
                    }
                }
                let number = |key: &str, fallback: f32| -> f32 {
                    match node.extra_data.get(key) {
                        Some(FlowNodeExtraData::Integer(n)) => *n as f32,
                        Some(FlowNodeExtraData::Float(n)) if n.is_finite() => *n,
                        _ => fallback,
                    }
                };
                let mut control_values = BTreeMap::new();
                if matches!(node.name.as_str(), "control" | "button" | "toggle") {
                    control_values.insert(0, number("value", 0.0).clamp(0.0, 1.0) as f64);
                }
                let node_type = if node.name == "control" {
                    NodeType::Control
                } else if matches!(node.name.as_str(), "out" | "inputs" | "outputs") {
                    NodeType::IO
                } else if metadata.as_ref().is_some_and(|f| f.as_flow().is_some()) {
                    NodeType::Subflow
                } else {
                    NodeType::Flow
                };
                snapshot.nodes.push((
                    node_id,
                    NodeSnapshot {
                        id: node_id,
                        label: node.display_text.clone(),
                        function_name: node.name.clone(),
                        node_type,
                        expression: node
                            .display_text
                            .starts_with('=')
                            .then(|| node.display_text.clone()),
                        position: Vec2::new(
                            number("x", (ordinal % 4) as f32 * 160.0),
                            number("y", (ordinal / 4) as f32 * 120.0),
                        ),
                        custom_size: Some(Vec2::new(
                            number("w", 160.0).max(80.0),
                            number("h", 100.0).max(60.0),
                        )),
                        inputs,
                        outputs,
                        control_values,
                    },
                ));
            }
            for idx in graph.edge_indices() {
                let (from, to, edge) = graph
                    .get_connection(idx)
                    .ok_or("Missing graph connection")?;
                snapshot.edges.push(Wire {
                    from: (reverse[&from.index()], edge.output_index as usize),
                    to: (reverse[&to.index()], edge.input_index as usize),
                });
            }
        }
        for wire in &snapshot.edges {
            let from = snapshot
                .nodes
                .iter()
                .find(|(id, _)| *id == wire.from.0)
                .unwrap();
            let to = snapshot
                .nodes
                .iter()
                .find(|(id, _)| *id == wire.to.0)
                .unwrap();
            match (from.1.outputs.get(wire.from.1), to.1.inputs.get(wire.to.1)) {
                (Some(a), Some(b)) if a.data_type == b.data_type => {}
                _ => snapshot.diagnostics.push(format!(
                    "Invalid ports/types on connection {:?} → {:?}",
                    wire.from, wire.to
                )),
            }
        }
        if let Err(error) = context.validate_function(&snapshot.flow_name) {
            snapshot.diagnostics.push(error);
        }
        Ok(Self {
            context,
            flow,
            node_indices,
            snapshot,
        })
    }
    pub fn snapshot(&self) -> DocumentSnapshot {
        let mut snapshot = self.snapshot.clone();
        snapshot.start_error = self.can_start().err();
        snapshot
    }
    pub fn id(&self) -> DocumentId {
        self.snapshot.id
    }
    pub fn set_value(&mut self, id: EditorNodeId, value: f64) -> Result<(), String> {
        if !value.is_finite() {
            return Err("Control value must be finite".into());
        }
        let node = self
            .snapshot
            .nodes
            .iter_mut()
            .find(|(n, _)| *n == id)
            .ok_or("Unknown node")?;
        if node.1.node_type != NodeType::Control {
            return Err("Node is not a control".into());
        }
        let value = value.clamp(0.0, 1.0) as f32;
        if node.1.control_values.get(&0) == Some(&(value as f64)) {
            return Ok(());
        }
        node.1.control_values.insert(0, value as f64);
        self.flow
            .as_flow_mut()
            .ok_or("Missing flow")?
            .get_node_mut(self.node_indices[&id])
            .ok_or("Missing node")?
            .extra_data
            .insert("value".into(), FlowNodeExtraData::Float(value));
        self.changed();
        self.snapshot.semantic_revision += 1;
        Ok(())
    }
    pub fn set_position(&mut self, id: EditorNodeId, position: Vec2) -> Result<(), String> {
        if !position.is_finite() {
            return Err("Node position must be finite".into());
        }
        let position = position.round();
        let node = self
            .snapshot
            .nodes
            .iter_mut()
            .find(|(n, _)| *n == id)
            .ok_or("Unknown node")?;
        if node.1.position == position {
            return Ok(());
        }
        node.1.position = position;
        {
            let mut graph = self.flow.as_flow_mut().ok_or("Missing flow")?;
            let data = &mut graph
                .get_node_mut(self.node_indices[&id])
                .ok_or("Missing node")?
                .extra_data;
            data.insert("x".into(), FlowNodeExtraData::Integer(position.x as i32));
            data.insert("y".into(), FlowNodeExtraData::Integer(position.y as i32));
        }
        self.changed();
        Ok(())
    }
    fn changed(&mut self) {
        self.snapshot.dirty = true;
        self.snapshot.revision += 1;
    }
    pub fn can_start(&self) -> Result<(), String> {
        if !self.snapshot.diagnostics.is_empty() {
            return Err(self.snapshot.diagnostics.join("\n"));
        }
        if !self.flow.borrow().inputs().is_empty() {
            return Err("DSP root cannot require function inputs".into());
        }
        // M1 deliberately does not bind arrays. Include nested functions, not only visible buffer nodes.
        fn arrays(
            ctx: &WispContext,
            name: &str,
            seen: &mut std::collections::HashSet<String>,
        ) -> bool {
            if !seen.insert(name.to_owned()) {
                return false;
            }
            let Some(f) = ctx.get_function(name) else {
                return false;
            };
            let f = f.borrow();
            if f.inputs().iter().any(|p| p.type_ == DataType::Array)
                || f.outputs().iter().any(|p| p.type_ == DataType::Array)
            {
                return true;
            }
            if let Some(flow) = f.as_flow() {
                if !flow.buffers().is_empty() {
                    return true;
                }
                return flow
                    .node_indices()
                    .any(|id| arrays(ctx, &flow.get_node(id).unwrap().name, seen));
            }
            false
        }
        if arrays(
            &self.context,
            &self.snapshot.flow_name,
            &mut Default::default(),
        ) {
            return Err(
                "Buffer runtime support pending milestone 4; this flow remains editable offline"
                    .into(),
            );
        }
        Ok(())
    }
    /// Saved files compact stable graph indices; root CallIndex must use this order.
    pub fn runtime_call_index(&self, id: EditorNodeId) -> Result<u32, String> {
        let core_index = *self.node_indices.get(&id).ok_or("Unknown node")?;
        self.flow
            .as_flow()
            .ok_or("Missing flow")?
            .node_indices()
            .position(|idx| idx == core_index)
            .map(|n| n as u32)
            .ok_or("Node is no longer present".into())
    }
    pub fn scalar_bindings(&self) -> Result<Vec<(u32, f32)>, String> {
        self.snapshot
            .nodes
            .iter()
            .filter_map(|(id, node)| node.control_values.get(&0).map(|v| (*id, *v as f32)))
            .map(|(id, value)| self.runtime_call_index(id).map(|call| (call, value)))
            .collect()
    }
    pub fn reconfigure(
        &mut self,
        core_path: &Path,
        sample_rate: u32,
        outputs: u32,
    ) -> Result<(), String> {
        if self.context.sample_rate() == sample_rate && self.context.num_outputs() == outputs {
            return Ok(());
        }
        let mut candidate = Self::from_text_config(
            &self.serialize(),
            core_path,
            self.id(),
            self.snapshot.source_file.clone(),
            sample_rate,
            outputs,
        )?;
        candidate.snapshot.dirty = self.snapshot.dirty;
        candidate.snapshot.revision = self.snapshot.revision + 1;
        candidate.snapshot.semantic_revision = self.snapshot.semantic_revision + 1;
        *self = candidate;
        Ok(())
    }
    pub fn serialize(&self) -> String {
        // Retaining the context owns all metadata/functions used by the flow.
        debug_assert!(
            self.context
                .get_function(&self.snapshot.flow_name)
                .is_some()
        );
        self.flow.borrow().save()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
    }
    fn load(name: &str) -> EditorDocument {
        EditorDocument::load(
            &root().join("wisp_gui").join(name),
            &root().join("wisp/core"),
            DocumentId(1),
        )
        .unwrap()
    }
    #[test]
    fn sparse_source_indices_map_to_compacted_runtime_and_real_output() {
        let text = "[flow]\nname='sparse'\n[[flow.nodes]]\ntext='control'\n[[flow.nodes]]\ntext='control'\nvalue=0.375\n[[flow.nodes]]\ntext='out'\n[[flow.edges]]\nfrom=1\nto=2\noutput_index=0\ninput_index=0\n";
        let mut doc =
            EditorDocument::from_text(text, &root().join("wisp/core"), DocumentId(1), None)
                .unwrap();
        let unused = doc.node_indices.remove(&EditorNodeId(1)).unwrap();
        doc.flow.as_flow_mut().unwrap().remove_node(unused);
        doc.snapshot.nodes.remove(0);
        assert_eq!(doc.runtime_call_index(EditorNodeId(2)).unwrap(), 0);
        let mut offline = wisp::OfflineSession::new(44100, 2, &root().join("wisp/core")).unwrap();
        let name = offline.load_flow_text(&doc.serialize()).unwrap();
        let mut processor = offline.compile(&name).unwrap();
        for (call, value) in doc.scalar_bindings().unwrap() {
            processor
                .set_data_value(&name, wisp::CallIndex(call), wisp::DataIndex(0), value)
                .unwrap();
        }
        let mut output = [0.; 2];
        processor.process_one(&mut output);
        assert_eq!(output, [0.375; 2]);
    }
    #[test]
    fn validation_and_fixture_sweep() {
        let core = root().join("wisp/core");
        for entry in std::fs::read_dir(root().join("wisp_gui")).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|s| s == "twf") {
                EditorDocument::load(&path, &core, DocumentId(1))
                    .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            }
        }
        let simple = "[flow]\nname='validation'\n[[flow.nodes]]\ntext='control'\n[[flow.nodes]]\ntext='out'\n[[flow.edges]]\nfrom=0\nto=1\noutput_index=0\ninput_index=0\n";
        let make =
            |text: &str| EditorDocument::from_text(text, &core, DocumentId(1), None).unwrap();
        assert!(make(simple).can_start().is_ok());
        assert!(
            make(&simple.replace("text='control'", "text='buffer'"))
                .can_start()
                .unwrap_err()
                .contains("Incompatible")
        );
        assert!(
            make(&simple.replace("text='control'", "text='validation'"))
                .can_start()
                .unwrap_err()
                .contains("Recursive")
        );
        assert!(
            make(&simple.replace("input_index=0", "input_index=99"))
                .can_start()
                .is_err()
        );
        assert!(
            make(&simple.replace("text='control'", "text='missing'"))
                .can_start()
                .is_err()
        );
        assert!(
            make(&simple.replace("text='control'", "text='control'\nx='bad'"))
                .snapshot()
                .diagnostics
                .iter()
                .any(|d| d.contains("Invalid x"))
        );
        let cycle = "[flow]\nname='cycle'\n[[flow.nodes]]\ntext='= $0'\n[[flow.edges]]\nfrom=0\nto=0\noutput_index=0\ninput_index=0\n";
        assert!(make(cycle).can_start().unwrap_err().contains("cycle"));
        assert!(load("control.twf").can_start().is_ok());
        assert!(load("sine.twf").can_start().is_err());
        let mut doc = make(simple);
        let id = doc.snapshot.nodes[0].0;
        doc.set_value(id, 0.7).unwrap();
        let semantic = doc.snapshot.semantic_revision;
        doc.set_position(id, Vec2::new(10., 20.)).unwrap();
        assert_eq!(doc.snapshot.semantic_revision, semantic);
        doc.reconfigure(&core, 48000, 1).unwrap();
        assert_eq!(doc.snapshot.nodes[0].0, id);
        assert_eq!(doc.snapshot.nodes[0].1.control_values[&0], 0.7f32 as f64);
        assert!(doc.snapshot.dirty);
    }
    #[test]
    fn real_fixture_metadata_and_values() {
        let doc = load("control.twf");
        let s = doc.snapshot();
        assert_eq!(s.nodes.len(), 7);
        assert_eq!(s.edges.len(), 7);
        assert!(s.diagnostics.is_empty(), "{:?}", s.diagnostics);
        let control = &s.nodes[1].1;
        assert_eq!(control.node_type, NodeType::Control);
        assert!(control.inputs.is_empty());
        assert_eq!(control.outputs.len(), 1);
        assert_eq!(control.position, Vec2::new(240.0, 380.0));
        assert!((control.control_values[&0] - 0.103).abs() < 1e-6);
        assert!(
            s.nodes
                .iter()
                .any(|(_, n)| n.function_name.contains(":math$"))
        );
        assert_eq!(load("phasor_test.twf").snapshot().nodes.len(), 3);
    }
    #[test]
    fn edits_update_backend_and_preserve_unknown_fields() {
        let text = std::fs::read_to_string(root().join("wisp_gui/control.twf"))
            .unwrap()
            .replacen("w = 237", "w = 237\neditor_note = 'preserve me'", 1);
        let mut doc =
            EditorDocument::from_text(&text, &root().join("wisp/core"), DocumentId(1), None)
                .unwrap();
        let id = doc.snapshot.nodes[1].0;
        doc.set_value(id, 2.0).unwrap();
        doc.set_position(id, Vec2::new(12.4, 42.7)).unwrap();
        let s = doc.snapshot();
        assert!(s.dirty);
        assert_eq!(s.revision, 2);
        assert_eq!(s.nodes[1].1.control_values[&0], 1.0);
        let text = doc.serialize();
        assert!(text.contains("value = 1.0"));
        assert!(text.contains("y = 43"));
        assert!(text.contains("preserve me"));
        assert!(doc.set_value(id, f64::NAN).is_err());
        assert!(doc.set_value(EditorNodeId(900), 0.2).is_err());
    }
    #[test]
    fn bad_endpoints_are_errors_not_panics() {
        let text = "[flow]\nname='broken'\nnodes=[]\n[[flow.edges]]\nfrom=42\nto=0\noutput_index=0\ninput_index=0\n";
        assert!(
            EditorDocument::from_text(text, &root().join("wisp/core"), DocumentId(1), None)
                .is_err()
        );
    }
}
