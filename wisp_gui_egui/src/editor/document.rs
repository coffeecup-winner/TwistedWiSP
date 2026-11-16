// SPDX-License-Identifier: MIT OR Apache-2.0
//! Backend-backed editor state. Only owned snapshots cross the service boundary.
use super::history::{Memento, bound};
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
    pub buffer_name: Option<String>,
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
pub struct FunctionChoice {
    pub name: String,
    pub kind: NodeType,
    pub inputs: Vec<Port>,
    pub outputs: Vec<Port>,
}
#[derive(Clone, Debug)]
pub struct DocumentSnapshot {
    pub can_undo: bool,
    pub can_redo: bool,
    pub catalog: Vec<FunctionChoice>,
    pub buffers: Vec<super::assets::BufferReference>,
    pub id: DocumentId,
    pub revision: u64,
    pub semantic_revision: u64,
    pub flow_name: String,
    pub source_file: Option<PathBuf>,
    pub dirty: bool,
    pub state_id: u64,
    pub nodes: Vec<(EditorNodeId, NodeSnapshot)>,
    pub edges: Vec<Wire>,
    pub diagnostics: Vec<String>,
    pub start_error: Option<String>,
}

/// Owned, atomic edits applied on the service thread.
#[derive(Clone, Debug)]
pub enum Edit {
    Buffer {
        node: EditorNodeId,
        name: String,
        path: Option<PathBuf>,
    },
    Geometry {
        node: EditorNodeId,
        position: Vec2,
        size: Vec2,
    },
    Create {
        text: String,
        position: Vec2,
    },
    Replace {
        node: EditorNodeId,
        text: String,
    },
    Delete(Vec<EditorNodeId>),
    Connect(Wire),
    Disconnect(Vec<Wire>),
    Move(Vec<(EditorNodeId, Vec2)>),
    Undo,
    Redo,
}
/// Constructed and retained on the service thread; backend handles never enter UI events.
pub struct EditorDocument {
    core_path: PathBuf,
    asset_base: Option<PathBuf>,
    data_paths: Vec<PathBuf>,
    fingerprint: super::persistence::Fingerprint,
    binding_revision: u64,
    external: u64,
    saved_external: u64,
    captured_external: u64,
    next_node: usize,
    state: u64,
    next_state: u64,
    checkpoint: Option<u64>,
    undo: Vec<Memento>,
    redo: Vec<Memento>,
    gesture: Option<(Memento, Vec<Memento>)>,
    context: WispContext,
    flow: FunctionHandle,
    node_indices: BTreeMap<EditorNodeId, FlowNodeIndex>,
    snapshot: DocumentSnapshot,
}
impl EditorDocument {
    pub fn load(path: &Path, core_path: &Path, id: DocumentId) -> Result<Self, String> {
        let path = path
            .canonicalize()
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let content =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::from_text(&content, core_path, id, Some(path))
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
        // The backend's flattened metadata supports only i32, f32 and strings.
        // Reject unsupported fields explicitly rather than letting a save drop them.
        for (index, node) in nodes.iter().enumerate() {
            if let Some(table) = node.as_table() {
                for (key, value) in table {
                    if key == "text" {
                        continue;
                    }
                    let supported = match value {
                        toml::Value::Integer(n) => i32::try_from(*n).is_ok(),
                        toml::Value::Float(n) => n.is_finite() && (*n as f32).is_finite(),
                        toml::Value::String(_) => true,
                        _ => false,
                    };
                    if !supported {
                        return Err(format!(
                            "Node {index} property {key}: unsupported metadata; expected finite i32/f32 or string"
                        ));
                    }
                }
            }
        }
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
        if context.get_function(&flow_name).is_some() || flow_name.starts_with("wisp_") {
            return Err("Flow name shadows an existing/reserved function".into());
        }
        context.add_function(function);
        let flow = context
            .get_function(&flow_name)
            .ok_or("Loaded flow not found")?;
        let mut catalog: Vec<FunctionChoice> = context
            .functions_iter()
            .filter_map(|handle| {
                let f = handle.borrow();
                let name = f.name();
                if name.contains(':') || name.starts_with("wisp_") || name == flow_name {
                    return None;
                }
                Some(FunctionChoice {
                    name: name.to_owned(),
                    kind: if name == "control" {
                        NodeType::Control
                    } else if matches!(name, "inputs" | "outputs" | "out") {
                        NodeType::IO
                    } else if handle.as_flow().is_some() {
                        NodeType::Subflow
                    } else {
                        NodeType::Flow
                    },
                    inputs: f
                        .inputs()
                        .iter()
                        .map(|p| Port {
                            name: p.name.clone(),
                            data_type: p.type_,
                        })
                        .collect(),
                    outputs: f
                        .outputs()
                        .iter()
                        .map(|p| Port {
                            name: p.name.clone(),
                            data_type: p.type_,
                        })
                        .collect(),
                })
            })
            .collect();
        catalog.sort_by(|a, b| a.name.cmp(&b.name));
        let asset_base = source_file
            .as_ref()
            .and_then(|p| p.parent())
            .map(Path::to_owned);
        let mut snapshot = DocumentSnapshot {
            buffers: vec![],
            can_undo: false,
            can_redo: false,
            catalog,
            id,
            revision: 0,
            semantic_revision: 0,
            flow_name,
            source_file,
            dirty: false,
            state_id: 0,
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
                if node.display_text.starts_with('=')
                    && let Err(error) =
                        wisp::core::MathFunctionParser::parse_function_checked(&node.display_text)
                {
                    snapshot
                        .diagnostics
                        .push(format!("Invalid expression {}: {error}", node.display_text));
                }
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
                        buffer_name: node
                            .extra_data
                            .get("buffer")
                            .and_then(FlowNodeExtraData::as_string)
                            .map(str::to_owned),
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
        let mut document = Self {
            core_path: core_path.to_owned(),
            binding_revision: 0,
            external: 0,
            saved_external: 0,
            captured_external: 0,
            asset_base,
            data_paths: vec![],
            fingerprint: if snapshot.source_file.is_some() {
                super::persistence::Fingerprint::Content(content.as_bytes().to_vec())
            } else {
                super::persistence::Fingerprint::Missing
            },
            next_node: snapshot.nodes.len() + 1,
            state: 0,
            next_state: 1,
            checkpoint: Some(0),
            undo: vec![],
            redo: vec![],
            gesture: None,
            context,
            flow,
            node_indices,
            snapshot,
        };
        document.refresh_assets();
        Ok(document)
    }
    pub fn snapshot(&self) -> DocumentSnapshot {
        let mut snapshot = self.snapshot.clone();
        snapshot.start_error = self.can_start().err();
        snapshot.state_id = self.state;
        snapshot.can_undo = !self.undo.is_empty();
        snapshot.can_redo = !self.redo.is_empty();
        snapshot.dirty =
            self.checkpoint != Some(self.state) || self.external != self.saved_external;
        snapshot
    }
    /// M3 saves a captured state, which can differ from the latest accepted state.
    pub fn mark_saved(&mut self, state: u64) {
        self.checkpoint = Some(state);
    }
    pub fn flow_name(&self) -> &str {
        &self.snapshot.flow_name
    }
    pub fn binding_revision(&self) -> u64 {
        self.binding_revision
    }
    pub fn semantic_revision(&self) -> u64 {
        self.snapshot.semantic_revision
    }
    pub fn id(&self) -> DocumentId {
        self.snapshot.id
    }
    pub fn set_value(&mut self, id: EditorNodeId, value: f64) -> Result<(), String> {
        self.set_scalar(id, value, false)
    }
    pub fn observe_midi(&mut self, id: EditorNodeId, value: f64) -> Result<(), String> {
        self.set_scalar(id, value, true)
    }
    fn set_scalar(&mut self, id: EditorNodeId, value: f64, midi: bool) -> Result<(), String> {
        if !value.is_finite() {
            return Err("Control value must be finite".into());
        }
        let node = self
            .snapshot
            .nodes
            .iter_mut()
            .find(|(n, _)| *n == id)
            .ok_or("Unknown node")?;
        if !matches!(node.1.function_name.as_str(), "control" | "toggle") {
            return Err("Node is not a control".into());
        }
        let value = if node.1.function_name == "toggle" {
            f32::from(value > 0.)
        } else {
            value.clamp(0.0, 1.0) as f32
        };
        if node.1.control_values.get(&0) == Some(&(value as f64)) {
            return Ok(());
        }
        let previous = (!midi && self.gesture.is_none()).then(|| self.memento());
        let node = self
            .snapshot
            .nodes
            .iter_mut()
            .find(|(n, _)| *n == id)
            .unwrap();
        node.1.control_values.insert(0, value as f64);
        self.flow
            .as_flow_mut()
            .ok_or("Missing flow")?
            .get_node_mut(self.node_indices[&id])
            .ok_or("Missing node")?
            .extra_data
            .insert("value".into(), FlowNodeExtraData::Float(value));
        if midi {
            self.external += 1;
        } else if let Some(previous) = previous {
            self.record(previous);
        } else {
            // The gesture already owns its original memento; live samples need
            // only a new state ID, not a full-flow serialization per mouse move.
            self.state = self.next_state;
            self.next_state += 1;
        }
        self.changed();
        self.snapshot.semantic_revision += 1;
        Ok(())
    }
    pub fn set_geometry(
        &mut self,
        id: EditorNodeId,
        position: Vec2,
        size: Vec2,
    ) -> Result<(), String> {
        let (position, size) = validate_geometry(position, size)?;
        let node = &self
            .snapshot
            .nodes
            .iter()
            .find(|(n, _)| *n == id)
            .ok_or("Unknown node")?
            .1;
        if node.position == position && node.custom_size == Some(size) {
            return Ok(());
        }
        let previous = self.gesture.is_none().then(|| self.memento());
        let mut graph = self.flow.as_flow_mut().ok_or("Missing flow")?;
        let data = &mut graph
            .get_node_mut(*self.node_indices.get(&id).ok_or("Unknown node")?)
            .ok_or("Missing node")?
            .extra_data;
        for (k, v) in [
            ("x", position.x),
            ("y", position.y),
            ("w", size.x),
            ("h", size.y),
        ] {
            data.insert(k.into(), FlowNodeExtraData::Integer(v as i32));
        }
        drop(graph);
        let node = &mut self
            .snapshot
            .nodes
            .iter_mut()
            .find(|(n, _)| *n == id)
            .unwrap()
            .1;
        node.position = position;
        node.custom_size = Some(size);
        if let Some(previous) = previous {
            self.record(previous);
        } else {
            self.state = self.next_state;
            self.next_state += 1;
        }
        self.changed();
        Ok(())
    }
    fn copy_file_state(&mut self, source: &Self) {
        self.binding_revision = source.binding_revision;
        self.external = source.external;
        self.saved_external = source.saved_external;
        self.captured_external = source.captured_external;
        self.asset_base = source.asset_base.clone();
        self.data_paths = source.data_paths.clone();
        self.fingerprint = source.fingerprint.clone();
        self.refresh_assets();
    }
    pub fn buffer_retry_path(&self, id: EditorNodeId) -> Option<PathBuf> {
        let name = self
            .snapshot
            .nodes
            .iter()
            .find(|(n, _)| *n == id)?
            .1
            .buffer_name
            .as_ref()?;
        let authored = self.flow.as_flow()?.buffers().get(name)?.clone()?;
        let reference = super::assets::resolve(
            name.clone(),
            Some(authored.clone()),
            self.asset_base.as_deref(),
            &self.data_paths,
        );
        reference.resolved.or_else(|| {
            if authored.is_absolute() {
                Some(authored)
            } else {
                self.asset_base.as_ref().map(|base| base.join(authored))
            }
        })
    }
    pub fn set_data_paths(&mut self, paths: Vec<PathBuf>) {
        self.data_paths = paths;
        self.refresh_assets();
    }
    fn refresh_assets(&mut self) {
        let graph = self.flow.as_flow().unwrap();
        self.snapshot.buffers = graph
            .buffers()
            .iter()
            .map(|(name, path)| {
                super::assets::resolve(
                    name.clone(),
                    path.clone(),
                    self.asset_base.as_deref(),
                    &self.data_paths,
                )
            })
            .collect();
        self.snapshot.buffers.sort_by(|a, b| a.name.cmp(&b.name));
    }
    pub fn save_capture(
        &mut self,
        path: Option<PathBuf>,
        overwrite: Option<super::persistence::Fingerprint>,
    ) -> Result<(super::persistence::SaveJob, u64), String> {
        self.end_gesture();
        self.captured_external = self.external;
        let path = path
            .or_else(|| self.snapshot.source_file.clone())
            .ok_or("Choose a Save As path for this untitled flow")?;
        let path = super::persistence::destination_path(&path)?;
        let expected = overwrite.unwrap_or_else(|| {
            if self.snapshot.source_file.as_ref() == Some(&path) {
                self.fingerprint.clone()
            } else {
                super::persistence::Fingerprint::Missing
            }
        });
        Ok((
            super::persistence::SaveJob {
                path,
                bytes: self.serialize().into_bytes(),
                expected,
                references: self.snapshot.buffers.clone(),
            },
            self.state,
        ))
    }
    pub fn saved(&mut self, state: u64, receipt: &super::persistence::SaveReceipt) {
        self.snapshot.source_file = Some(receipt.path.clone());
        self.fingerprint = receipt.fingerprint.clone();
        self.mark_saved(state);
        self.saved_external = self.captured_external;
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
        let previous = self.memento();
        let node = self
            .snapshot
            .nodes
            .iter_mut()
            .find(|(n, _)| *n == id)
            .unwrap();
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
        self.record(previous);
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
        // Root buffers are bound before installation. Nested buffer state still
        // requires instance addressing (M6), so retain that narrower gate.
        for (_, node) in &self.snapshot.nodes {
            if node.function_name == "buffer" && node.buffer_name.is_none() {
                return Err("Choose a buffer source first".into());
            }
            if node.node_type == NodeType::Subflow {
                fn nested_arrays(
                    ctx: &WispContext,
                    name: &str,
                    seen: &mut std::collections::HashSet<String>,
                ) -> bool {
                    if !seen.insert(name.into()) {
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
                    f.as_flow().is_some_and(|flow| {
                        flow.node_indices()
                            .any(|id| nested_arrays(ctx, &flow.get_node(id).unwrap().name, seen))
                    })
                }
                if nested_arrays(&self.context, &node.function_name, &mut Default::default()) {
                    return Err("Nested array bindings require instance addressing".into());
                }
            }
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
            .map(|(id, value)| {
                let button = self
                    .snapshot
                    .nodes
                    .iter()
                    .any(|(n, v)| *n == id && v.function_name == "button");
                self.runtime_call_index(id)
                    .map(|call| (call, if button { 0. } else { value }))
            })
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
        candidate.remap(&self.memento().ids)?;
        candidate.copy_file_state(self);
        candidate.next_node = self.next_node;
        candidate.state = self.state;
        candidate.next_state = self.next_state;
        candidate.checkpoint = self.checkpoint;
        candidate.undo = self.undo.clone();
        candidate.redo = self.redo.clone();
        candidate.snapshot.dirty = self.snapshot.dirty;
        candidate.snapshot.revision = self.snapshot.revision + 1;
        candidate.snapshot.semantic_revision = self.snapshot.semantic_revision + 1;
        *self = candidate;
        Ok(())
    }
    fn memento(&self) -> Memento {
        Memento {
            text: self.serialize(),
            ids: self.snapshot.nodes.iter().map(|(id, _)| *id).collect(),
            state: self.state,
        }
    }
    pub fn begin_gesture(&mut self) {
        if self.gesture.is_none() {
            self.gesture = Some((self.memento(), self.redo.clone()));
        }
    }
    pub fn end_gesture(&mut self) {
        if let Some((before, redo)) = self.gesture.take() {
            if self.serialize() == before.text {
                self.state = before.state;
                self.redo = redo;
            } else {
                self.undo.push(before);
                self.redo.clear();
                bound(&mut self.undo);
            }
        }
    }
    fn record(&mut self, previous: Memento) {
        if self.gesture.is_some() {
            self.state = self.next_state;
            self.next_state += 1;
            return;
        }
        self.undo.push(previous);
        self.redo.clear();
        bound(&mut self.undo);
        self.state = self.next_state;
        self.next_state += 1;
    }
    fn remap(&mut self, ids: &[EditorNodeId]) -> Result<(), String> {
        if ids.len() != self.snapshot.nodes.len() {
            return Err("Invalid identity sidecar".into());
        }
        let old: BTreeMap<_, _> = self
            .snapshot
            .nodes
            .iter()
            .zip(ids)
            .map(|((id, _), new)| (*id, *new))
            .collect();
        self.node_indices = self
            .node_indices
            .iter()
            .map(|(id, idx)| (old[id], *idx))
            .collect();
        for ((id, node), new) in self.snapshot.nodes.iter_mut().zip(ids) {
            *id = *new;
            node.id = *new;
        }
        for edge in &mut self.snapshot.edges {
            edge.from.0 = old[&edge.from.0];
            edge.to.0 = old[&edge.to.0];
        }
        Ok(())
    }
    pub fn new_flow(name: &str, core: &Path, id: DocumentId) -> Result<Self, String> {
        if name.is_empty()
            || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            || name.starts_with("wisp_")
        {
            return Err("Use an alphanumeric flow name".into());
        }
        let mut doc = Self::from_text(
            &format!("[flow]\nname = {name:?}\nnodes = []\nedges = []\n"),
            core,
            id,
            None,
        )?;
        if doc.snapshot.catalog.iter().any(|n| n.name == name) {
            return Err("Name shadows an existing function".into());
        }
        doc.checkpoint = None;
        Ok(doc)
    }
    /// Returns whether the accepted transaction changes processor structure.
    pub fn edit(&mut self, revision: u64, edit: Edit) -> Result<bool, String> {
        if revision != self.snapshot.revision {
            return Err("Stale graph revision; retry the edit".into());
        }
        self.end_gesture();
        let before = self.memento();
        let is_undo = matches!(edit, Edit::Undo);
        let is_redo = matches!(edit, Edit::Redo);
        if is_undo || is_redo {
            let target = if is_undo {
                self.undo.last()
            } else {
                self.redo.last()
            }
            .cloned();
            let Some(target) = target else {
                return Ok(false);
            };
            let mut candidate = Self::from_text_config(
                &target.text,
                &self.core_path,
                self.id(),
                self.snapshot.source_file.clone(),
                self.context.sample_rate(),
                self.context.num_outputs(),
            )?;
            candidate.remap(&target.ids)?;
            candidate.copy_file_state(self);
            let structural = topology(&before.text)? != topology(&target.text)?;
            candidate.binding_revision += u64::from(structural);
            candidate.next_node = self.next_node;
            candidate.state = target.state;
            candidate.next_state = self.next_state;
            candidate.checkpoint = self.checkpoint;
            candidate.undo = self.undo.clone();
            candidate.redo = self.redo.clone();
            if is_undo {
                candidate.undo.pop();
                candidate.redo.push(before);
                bound(&mut candidate.redo);
            } else {
                candidate.redo.pop();
                candidate.undo.push(before);
                bound(&mut candidate.undo);
            }
            candidate.snapshot.revision = self.snapshot.revision + 1;
            candidate.snapshot.semantic_revision = self.snapshot.semantic_revision
                + u64::from(structural || self.scalar_bindings()? != candidate.scalar_bindings()?);
            *self = candidate;
            return Ok(structural);
        }
        let mut value: toml::Value = toml::from_str(&before.text).map_err(|e| format!("{e}"))?;
        let flow = value
            .get_mut("flow")
            .and_then(toml::Value::as_table_mut)
            .ok_or("Missing flow")?;
        let mut nodes = flow
            .remove("nodes")
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default();
        let mut edges = flow
            .remove("edges")
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default();
        let mut ids = before.ids.clone();
        let ordinal = |id: EditorNodeId| {
            before
                .ids
                .iter()
                .position(|n| *n == id)
                .ok_or_else(|| "Unknown or deleted node".to_owned())
        };
        let edge_value = |w: &Wire| -> Result<toml::Value, String> {
            let mut t = toml::map::Map::new();
            for (k, v) in [
                ("from", ordinal(w.from.0)?),
                ("output_index", w.from.1),
                ("to", ordinal(w.to.0)?),
                ("input_index", w.to.1),
            ] {
                t.insert(k.into(), toml::Value::Integer(v as i64));
            }
            Ok(toml::Value::Table(t))
        };
        let structural = !matches!(edit, Edit::Move(_) | Edit::Geometry { .. });
        let repair = matches!(edit, Edit::Delete(_) | Edit::Disconnect(_));
        let mut buffer_update = None;
        match edit {
            Edit::Buffer { node, name, path } => {
                let ordinal = ordinal(node)?;
                if nodes[ordinal]["text"].as_str() != Some("buffer") {
                    return Err("Not a buffer node".into());
                }
                nodes[ordinal]
                    .as_table_mut()
                    .ok_or("Invalid node")?
                    .insert("buffer".into(), name.clone().into());
                buffer_update = Some((name, path));
            }
            Edit::Geometry {
                node,
                position,
                size,
            } => {
                let (position, size) = validate_geometry(position, size)?;
                let n = &mut nodes[ordinal(node)?];
                for (key, v) in [
                    ("x", position.x),
                    ("y", position.y),
                    ("w", size.x),
                    ("h", size.y),
                ] {
                    n.as_table_mut()
                        .ok_or("Invalid node")?
                        .insert(key.into(), (v as i64).into());
                }
            }
            Edit::Create { text, position } => {
                self.check_text(&text)?;
                if !position.is_finite() {
                    return Err("Invalid position".into());
                }
                let mut t = toml::map::Map::new();
                t.insert("text".into(), text.into());
                t.insert("x".into(), (position.x.round() as i64).into());
                t.insert("y".into(), (position.y.round() as i64).into());
                nodes.push(toml::Value::Table(t));
                ids.push(EditorNodeId(self.next_node));
            }
            Edit::Replace { node, text } => {
                self.check_text(&text)?;
                nodes[ordinal(node)?]["text"] = text.into();
            }
            Edit::Delete(deleted) => {
                let removed: std::collections::BTreeSet<_> =
                    deleted.into_iter().map(ordinal).collect::<Result<_, _>>()?;
                let kept: Vec<_> = (0..nodes.len()).filter(|i| !removed.contains(i)).collect();
                edges.retain_mut(|e| {
                    let a = e["from"].as_integer().unwrap() as usize;
                    let b = e["to"].as_integer().unwrap() as usize;
                    let (Some(a), Some(b)) = (
                        kept.iter().position(|i| *i == a),
                        kept.iter().position(|i| *i == b),
                    ) else {
                        return false;
                    };
                    e["from"] = (a as i64).into();
                    e["to"] = (b as i64).into();
                    true
                });
                nodes = kept.iter().map(|i| nodes[*i].clone()).collect();
                ids = kept.iter().map(|i| ids[*i]).collect();
            }
            Edit::Connect(w) => {
                let a = &self.snapshot.nodes[ordinal(w.from.0)?].1;
                let b = &self.snapshot.nodes[ordinal(w.to.0)?].1;
                let output = a.outputs.get(w.from.1).ok_or("Invalid output port")?;
                let input = b.inputs.get(w.to.1).ok_or("Invalid input port")?;
                if output.data_type != input.data_type {
                    return Err("Incompatible port types".into());
                }
                let e = edge_value(&w)?;
                if !edges.contains(&e) {
                    if input.data_type == DataType::Array
                        && self.snapshot.edges.iter().any(|e| e.to == w.to)
                    {
                        return Err("Array inputs cannot sum multiple connections".into());
                    }
                    edges.push(e);
                }
            }
            Edit::Disconnect(wires) => {
                for w in wires {
                    let e = edge_value(&w)?;
                    edges.retain(|v| *v != e);
                }
            }
            Edit::Move(moves) => {
                for (id, p) in moves {
                    if !p.is_finite() {
                        return Err("Invalid position".into());
                    }
                    let n = &mut nodes[ordinal(id)?];
                    n.as_table_mut()
                        .ok_or("Invalid node")?
                        .insert("x".into(), (p.x.round() as i64).into());
                    n.as_table_mut()
                        .ok_or("Invalid node")?
                        .insert("y".into(), (p.y.round() as i64).into());
                }
            }
            Edit::Undo | Edit::Redo => unreachable!(),
        }
        flow.insert("nodes".into(), nodes.into());
        flow.insert("edges".into(), edges.into());
        for interface in ["inputs", "outputs"] {
            if !repair
                && flow["nodes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|n| n.get("text").and_then(toml::Value::as_str) == Some(interface))
                    .count()
                    > 1
            {
                return Err(format!("Only one {interface} interface node is supported"));
            }
        }
        if let Some((name, path)) = buffer_update {
            let buffers = value
                .as_table_mut()
                .unwrap()
                .entry("buffers")
                .or_insert_with(|| toml::Value::Array(vec![]))
                .as_array_mut()
                .ok_or("Invalid buffer list")?;
            buffers.retain(|b| b.get("name").and_then(toml::Value::as_str) != Some(&name));
            let mut entry = toml::map::Map::new();
            entry.insert("name".into(), name.into());
            if let Some(path) = path {
                entry.insert(
                    "path".into(),
                    path.to_str().ok_or("Non UTF-8 buffer path")?.into(),
                );
            }
            buffers.push(toml::Value::Table(entry));
        }
        let text = toml::to_string(&value).map_err(|e| e.to_string())?;
        let original: toml::Value = toml::from_str(&before.text).map_err(|e| e.to_string())?;
        if value == original {
            return Ok(false);
        }
        let mut candidate = Self::from_text_config(
            &text,
            &self.core_path,
            self.id(),
            self.snapshot.source_file.clone(),
            self.context.sample_rate(),
            self.context.num_outputs(),
        )?;
        candidate.remap(&ids)?;
        candidate.copy_file_state(self);
        // Deletion/disconnection can repair invalid files; additions/replacements cannot introduce diagnostics.
        if structural && !repair && !candidate.snapshot.diagnostics.is_empty() {
            return Err(format!(
                "Edit rejected (check affected wires): {}",
                candidate.snapshot.diagnostics.join("; ")
            ));
        }
        candidate.next_node = self
            .next_node
            .max(ids.iter().map(|id| id.0 + 1).max().unwrap_or(1));
        candidate.state = self.state;
        candidate.next_state = self.next_state;
        candidate.checkpoint = self.checkpoint;
        candidate.undo = self.undo.clone();
        candidate.redo = self.redo.clone();
        candidate.record(before);
        candidate.snapshot.revision = self.snapshot.revision + 1;
        candidate.snapshot.semantic_revision =
            self.snapshot.semantic_revision + u64::from(structural);
        candidate.binding_revision += u64::from(structural);
        *self = candidate;
        Ok(structural)
    }
    fn check_text(&self, text: &str) -> Result<(), String> {
        if text.starts_with('=') {
            wisp::core::MathFunctionParser::parse_function_checked(text)
                .map_err(|e| e.to_string())?;
        } else if text == self.snapshot.flow_name
            || !self.snapshot.catalog.iter().any(|n| n.name == text)
        {
            return Err(format!("Unknown or recursive function: {text}"));
        }
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

fn validate_geometry(position: Vec2, size: Vec2) -> Result<(Vec2, Vec2), String> {
    if !position.is_finite() || !size.is_finite() {
        return Err("Geometry must be finite".into());
    }
    if [position.x, position.y, size.x, size.y]
        .iter()
        .any(|n| n.abs() > 2_000_000_000.)
    {
        return Err("Geometry exceeds the integer file format range".into());
    }
    Ok((position.round(), size.max(Vec2::new(80., 60.)).round()))
}

fn topology(text: &str) -> Result<toml::Value, String> {
    let mut v: toml::Value = toml::from_str(text).map_err(|e| e.to_string())?;
    if let Some(nodes) = v
        .get_mut("flow")
        .and_then(|f| f.get_mut("nodes"))
        .and_then(toml::Value::as_array_mut)
    {
        for n in nodes {
            if let Some(t) = n.as_table_mut() {
                for key in ["x", "y", "w", "h", "value"] {
                    t.remove(key);
                }
            }
        }
    }
    Ok(v)
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
        assert!(load("sine.twf").can_start().is_ok());
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
    fn apply(doc: &mut EditorDocument, edit: Edit) -> bool {
        doc.edit(doc.snapshot().revision, edit).unwrap()
    }
    fn empty() -> EditorDocument {
        EditorDocument::new_flow("new_flow", &root().join("wisp/core"), DocumentId(1)).unwrap()
    }
    fn add(doc: &mut EditorDocument, text: &str) -> EditorNodeId {
        apply(
            doc,
            Edit::Create {
                text: text.into(),
                position: Vec2::ZERO,
            },
        );
        doc.snapshot.nodes.last().unwrap().0
    }
    fn wire(a: EditorNodeId, b: EditorNodeId) -> Wire {
        Wire {
            from: (a, 0),
            to: (b, 0),
        }
    }
    #[test]
    fn transactions_identity_history_and_checkpoint() {
        let mut d = load("control.twf");
        let before = d.serialize();
        let ids = d.memento().ids;
        assert!(!d.snapshot().dirty);
        apply(&mut d, Edit::Delete(vec![ids[1], ids[4]]));
        assert_eq!(d.snapshot.nodes.len(), 5);
        assert!(
            d.snapshot
                .edges
                .iter()
                .all(|w| w.from.0 != ids[1] && w.to.0 != ids[1])
        );
        let fresh = add(&mut d, "control");
        assert!(fresh.0 > 7);
        apply(&mut d, Edit::Undo);
        apply(&mut d, Edit::Undo);
        assert_eq!(d.serialize(), before);
        assert_eq!(d.memento().ids, ids);
        assert!(!d.snapshot().dirty);
        let next = add(&mut d, "control");
        assert!(next > fresh);
        assert!(!d.snapshot().can_redo);
        d.reconfigure(&root().join("wisp/core"), 48000, 2).unwrap();
        assert_eq!(d.snapshot.nodes.last().unwrap().0, next);
        assert_eq!(d.runtime_call_index(next).unwrap(), 7);
        let before = d.serialize();
        let rev = d.snapshot.revision;
        assert!(d.edit(rev - 1, Edit::Delete(vec![next])).is_err());
        assert_eq!(d.serialize(), before);
        assert!(d.set_value(fresh, 0.2).is_err());
    }
    #[test]
    fn connections_are_atomic_noops_and_sum_in_jit() {
        let mut d = empty();
        let a = add(&mut d, "control");
        let b = add(&mut d, "control");
        let out = add(&mut d, "out");
        d.set_value(a, 0.2).unwrap();
        d.set_value(b, 0.3).unwrap();
        apply(&mut d, Edit::Connect(wire(a, out)));
        apply(&mut d, Edit::Connect(wire(b, out)));
        let rev = d.snapshot.revision;
        assert!(!apply(&mut d, Edit::Connect(wire(a, out))));
        assert_eq!(d.snapshot.revision, rev);
        let invalid = Wire {
            from: (a, 90),
            to: (out, 0),
        };
        assert!(d.edit(rev, Edit::Connect(invalid)).is_err());
        assert_eq!(d.snapshot.revision, rev);
        let mut offline = wisp::OfflineSession::new(44100, 2, &root().join("wisp/core")).unwrap();
        let name = offline.load_flow_text(&d.serialize()).unwrap();
        let mut processor = offline.compile(&name).unwrap();
        for (call, value) in d.scalar_bindings().unwrap() {
            processor
                .set_data_value(&name, wisp::CallIndex(call), wisp::DataIndex(0), value)
                .unwrap();
        }
        let mut output = [0.; 2];
        processor.process_one(&mut output);
        assert!(output.iter().all(|v| (*v - 0.5f32).abs() < 1e-6));
        apply(&mut d, Edit::Disconnect(vec![wire(a, out), wire(b, out)]));
        assert!(d.snapshot.edges.is_empty());
        let rev = d.snapshot.revision;
        apply(&mut d, Edit::Disconnect(vec![wire(a, out)]));
        assert_eq!(d.snapshot.revision, rev);
        apply(&mut d, Edit::Undo);
        assert_eq!(d.snapshot.edges.len(), 2);
    }
    #[test]
    fn cycles_expressions_replacement_and_interfaces() {
        let mut d = empty();
        let a = add(&mut d, "= $0");
        let b = add(&mut d, "= $0 + 1");
        apply(&mut d, Edit::Connect(wire(a, b)));
        let before = d.serialize();
        assert!(
            d.edit(d.snapshot.revision, Edit::Connect(wire(b, a)))
                .is_err()
        );
        assert_eq!(d.serialize(), before);
        assert!(
            d.edit(
                d.snapshot.revision,
                Edit::Replace {
                    node: b,
                    text: "= 1".into()
                }
            )
            .is_err()
        );
        assert_eq!(d.serialize(), before);
        let lag = add(&mut d, "lag");
        apply(&mut d, Edit::Connect(wire(lag, lag)));
        for bad in ["=", "= (", "= sin", "= $4294967295", "= $1024", "= 1e999"] {
            assert!(
                d.edit(
                    d.snapshot.revision,
                    Edit::Create {
                        text: bad.into(),
                        position: Vec2::ZERO
                    }
                )
                .is_err()
            );
            let file = format!("[flow]\nname='bad_expression'\n[[flow.nodes]]\ntext={bad:?}\n");
            assert!(
                EditorDocument::from_text(&file, &root().join("wisp/core"), DocumentId(2), None)
                    .unwrap()
                    .can_start()
                    .is_err()
            );
        }
        let input = add(&mut d, "inputs");
        assert!(!d.flow.borrow().inputs().is_empty());
        assert!(
            d.edit(
                d.snapshot.revision,
                Edit::Create {
                    text: "inputs".into(),
                    position: Vec2::ZERO
                }
            )
            .is_err()
        );
        apply(&mut d, Edit::Delete(vec![input]));
        assert!(d.flow.borrow().inputs().is_empty());
        apply(&mut d, Edit::Undo);
        assert!(!d.flow.borrow().inputs().is_empty());
    }
    #[test]
    fn grouped_values_geometry_and_bounded_branching() {
        let mut d = load("control.twf");
        let id = d.snapshot.nodes[1].0;
        let before = d.serialize();
        d.begin_gesture();
        for v in [0.1, 0.2, 0.3] {
            d.set_value(id, v).unwrap();
        }
        d.end_gesture();
        assert_eq!(d.undo.len(), 1);
        assert!(!apply(&mut d, Edit::Undo));
        assert_eq!(d.serialize(), before);
        assert!(!d.snapshot().dirty);
        d.begin_gesture();
        d.end_gesture();
        assert_eq!(d.undo.len(), 0);
        assert!(d.snapshot().can_redo);
        let a = d.snapshot.nodes[0].0;
        assert!(!apply(
            &mut d,
            Edit::Move(vec![(id, Vec2::new(20., 40.)), (a, Vec2::new(60., 80.))])
        ));
        assert_eq!(d.undo.len(), 1);
        assert!(!d.snapshot().can_redo);
        apply(&mut d, Edit::Undo);
        assert_eq!(d.serialize(), before);
        for i in 0..70 {
            d.set_position(id, Vec2::new(i as f32, 0.)).unwrap();
        }
        assert_eq!(d.undo.len(), 64);
        assert!(d.snapshot().dirty);
        let huge = Memento {
            text: "x".repeat(9 * 1024 * 1024),
            ids: vec![],
            state: 0,
        };
        d.record(huge);
        assert!(d.undo.is_empty());
    }
    #[test]
    fn array_fanin_types_and_invalid_graph_repair() {
        let mut d = empty();
        let a = add(&mut d, "buffer");
        let b = add(&mut d, "buffer");
        let read = add(&mut d, "read");
        apply(&mut d, Edit::Connect(wire(a, read)));
        let before = d.serialize();
        let rev = d.snapshot.revision;
        assert!(
            d.edit(rev, Edit::Connect(wire(b, read)))
                .unwrap_err()
                .contains("Array")
        );
        assert_eq!(d.serialize(), before);
        let control = add(&mut d, "control");
        assert!(
            d.edit(d.snapshot.revision, Edit::Connect(wire(control, read)))
                .is_err()
        );
        let text = "[flow]\nname='repair'\n[[flow.nodes]]\ntext='= $0'\n[[flow.edges]]\nfrom=0\nto=0\noutput_index=0\ninput_index=0\n";
        let mut d = EditorDocument::from_text(text, &root().join("wisp/core"), DocumentId(4), None)
            .unwrap();
        assert!(!d.snapshot.diagnostics.is_empty());
        let bad = d.snapshot.edges.clone();
        apply(&mut d, Edit::Disconnect(bad));
        assert!(d.snapshot.diagnostics.is_empty());
        apply(&mut d, Edit::Undo);
        assert!(!d.snapshot.diagnostics.is_empty());
        let node = d.snapshot.nodes[0].0;
        apply(&mut d, Edit::Delete(vec![node]));
        assert!(d.snapshot.diagnostics.is_empty());
    }
    #[test]
    fn captured_checkpoints_stay_unique_after_branching_and_eviction() {
        let mut d = empty();
        assert!(d.snapshot().dirty);
        let a = add(&mut d, "control");
        let captured = d.snapshot().state_id;
        d.mark_saved(captured);
        assert!(!d.snapshot().dirty);
        d.set_value(a, 0.5).unwrap();
        let saved_later = d.snapshot().state_id;
        apply(&mut d, Edit::Undo);
        assert!(!d.snapshot().dirty);
        d.set_value(a, 0.7).unwrap();
        assert_ne!(d.snapshot().state_id, saved_later);
        d.mark_saved(saved_later);
        assert!(d.snapshot().dirty);
        for i in 0..70 {
            d.set_position(a, Vec2::new(i as f32, 0.)).unwrap();
        }
        while d.snapshot().can_undo {
            apply(&mut d, Edit::Undo);
        }
        assert!(d.snapshot().dirty);
    }
    #[test]
    fn bad_endpoints_are_errors_not_panics() {
        let text = "[flow]\nname='broken'\nnodes=[]\n[[flow.edges]]\nfrom=42\nto=0\noutput_index=0\ninput_index=0\n";
        assert!(
            EditorDocument::from_text(text, &root().join("wisp/core"), DocumentId(1), None)
                .is_err()
        );
    }
    #[test]
    fn fixture_roundtrips_and_unsupported_metadata() {
        let core = root().join("wisp/core");
        for entry in std::fs::read_dir(root().join("wisp_gui"))
            .unwrap()
            .chain(std::fs::read_dir(&core).unwrap())
        {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|e| e != "twf") {
                continue;
            }
            let source_text = std::fs::read_to_string(&path).unwrap();
            // core.twf is the legacy code-function language, not a TOML flow.
            if path == core.join("core.twf") {
                assert!(source_text.contains("func control()"));
                continue;
            }
            let mut source: toml::Value = toml::from_str(&source_text).unwrap();
            // Core library flows intentionally shadow installed function names;
            // give the root a distinct name to exercise the editor round trip.
            source["flow"]["name"] = "roundtrip_fixture".into();
            let doc = EditorDocument::from_text(
                &toml::to_string(&source).unwrap(),
                &core,
                DocumentId(1),
                Some(path.clone()),
            )
            .unwrap();
            let text = doc.serialize();
            let reopened = EditorDocument::from_text(&text, &core, DocumentId(2), None).unwrap();
            let a: toml::Value = toml::from_str(&text).unwrap();
            let b: toml::Value = toml::from_str(&reopened.serialize()).unwrap();
            assert_eq!(a, b, "{}", path.display());
            assert_eq!(doc.snapshot.nodes.len(), reopened.snapshot.nodes.len());
            assert_eq!(doc.snapshot.edges, reopened.snapshot.edges);
        }
        for value in ["true", "[1, 2]", "{a=1}", "2147483648", "inf"] {
            let text = format!(
                "[flow]\nname='metadata'\n[[flow.nodes]]\ntext='control'\ncustom={value}\n"
            );
            let error = EditorDocument::from_text(&text, &core, DocumentId(1), None)
                .err()
                .unwrap();
            assert!(error.contains("property custom"), "{error}");
        }
    }
    #[test]
    fn save_capture_preserves_history_identity_and_asset_target() {
        use super::super::persistence::{atomic_write, tests::Temp};
        use std::sync::atomic::AtomicBool;
        let dir = Temp::new();
        let old = dir.0.join("old");
        let new = dir.0.join("new");
        std::fs::create_dir(&old).unwrap();
        std::fs::create_dir(&new).unwrap();
        std::fs::write(old.join("sample.wav"), b"asset").unwrap();
        let path = old.join("flow.twf");
        let text = "[flow]\nname='save_test'\n[[flow.nodes]]\ntext='control'\nvalue=0.1234567\nnote='keep'\n[[buffers]]\nname='sample'\npath='sample.wav'\n[[buffers]]\nname='builtin'\n";
        std::fs::write(&path, text).unwrap();
        let mut doc =
            EditorDocument::load(&path, &root().join("wisp/core"), DocumentId(1)).unwrap();
        let id = doc.snapshot.nodes[0].0;
        doc.set_geometry(id, Vec2::new(1.4, -2.7), Vec2::new(-1., 0.))
            .unwrap();
        let snap = doc.snapshot();
        assert_eq!(snap.nodes[0].1.position, Vec2::new(1., -3.));
        assert_eq!(snap.nodes[0].1.custom_size, Some(Vec2::new(80., 60.)));
        assert_eq!(snap.semantic_revision, 0);
        assert!(
            doc.set_geometry(id, Vec2::ZERO, Vec2::splat(f32::NAN))
                .is_err()
        );
        let (job, captured) = doc.save_capture(Some(new.join("flow.twf")), None).unwrap();
        let (text, _) = super::super::assets::rebase(
            std::str::from_utf8(&job.bytes).unwrap(),
            &job.references,
            &new,
        )
        .unwrap();
        // A newer accepted edit must remain dirty when the older write completes.
        doc.set_value(id, 0.9).unwrap();
        let before = doc.snapshot();
        let receipt = atomic_write(
            &job.path,
            text.as_bytes(),
            job.expected,
            &AtomicBool::new(false),
        )
        .unwrap();
        doc.saved(captured, &receipt);
        let after = doc.snapshot();
        assert!(after.dirty);
        assert_eq!(after.revision, before.revision);
        assert_eq!(after.semantic_revision, before.semantic_revision);
        assert_eq!(after.nodes[0].0, id);
        apply(&mut doc, Edit::Undo);
        assert!(!doc.snapshot().dirty);
        let loaded =
            EditorDocument::load(&receipt.path, &root().join("wisp/core"), DocumentId(2)).unwrap();
        let asset = loaded
            .snapshot
            .buffers
            .iter()
            .find(|r| r.name == "sample")
            .unwrap();
        assert_eq!(
            asset.resolved.as_ref().unwrap().canonicalize().unwrap(),
            old.join("sample.wav")
        );
        assert!(
            loaded
                .snapshot
                .buffers
                .iter()
                .find(|r| r.name == "builtin")
                .unwrap()
                .authored
                .is_none()
        );
        assert!(loaded.serialize().contains("keep"));
        // A later edit/undo still resolves paths against the original authored base.
        doc.set_value(id, 0.5).unwrap();
        assert_eq!(
            doc.snapshot
                .buffers
                .iter()
                .find(|r| r.name == "sample")
                .unwrap()
                .resolved,
            Some(old.join("sample.wav"))
        );
    }
    #[test]
    fn midi_origin_checkpoints_toggle_and_transient_defaults() {
        use super::super::persistence::{atomic_write, tests::Temp};
        let mut doc = empty();
        let id = add(&mut doc, "control");
        doc.mark_saved(doc.snapshot().state_id);
        let before = doc.snapshot();
        doc.observe_midi(id, 0.5).unwrap();
        assert!(doc.snapshot().dirty);
        assert_eq!(doc.snapshot().state_id, before.state_id);
        assert_eq!(doc.undo.len(), 1);
        let dir = Temp::new();
        let (job, captured) = doc
            .save_capture(Some(dir.0.join("midi.twf")), None)
            .unwrap();
        doc.observe_midi(id, 0.6).unwrap();
        let receipt = atomic_write(
            &job.path,
            &job.bytes,
            job.expected,
            &std::sync::atomic::AtomicBool::new(false),
        )
        .unwrap();
        doc.saved(captured, &receipt);
        assert!(doc.snapshot().dirty);
        let (job, captured) = doc.save_capture(None, None).unwrap();
        let receipt = atomic_write(
            &job.path,
            &job.bytes,
            job.expected,
            &std::sync::atomic::AtomicBool::new(false),
        )
        .unwrap();
        doc.saved(captured, &receipt);
        assert!(!doc.snapshot().dirty);
        let toggle = add(&mut doc, "toggle");
        doc.set_value(toggle, 0.2).unwrap();
        assert_eq!(
            doc.snapshot()
                .nodes
                .iter()
                .find(|(n, _)| *n == toggle)
                .unwrap()
                .1
                .control_values[&0],
            1.
        );
        apply(&mut doc, Edit::Undo);
        assert_eq!(
            doc.snapshot()
                .nodes
                .iter()
                .find(|(n, _)| *n == toggle)
                .unwrap()
                .1
                .control_values[&0],
            0.
        );
        let button = add(&mut doc, "button");
        assert!(doc.set_value(button, 1.).is_err());
        assert_eq!(doc.scalar_bindings().unwrap().last().unwrap().1, 0.);
    }
}
