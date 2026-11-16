// SPDX-License-Identifier: MIT OR Apache-2.0
//! Snarl IDs are confined to this presentation adapter and never used as document IDs.
use super::node_content;
use crate::editor::{
    commands::{EditorCommand, RequestId},
    document::{DocumentId, DocumentSnapshot, Edit, EditorNodeId, NodeSnapshot, Wire},
};
use egui::Ui;
use egui_snarl::{
    InPin, InPinId, NodeId, OutPin, OutPinId, Snarl,
    ui::{PinInfo, SnarlViewer, SnarlWidget},
};
use std::collections::{BTreeMap, BTreeSet};
use wisp::core::DataType;
#[derive(Default)]
pub struct GraphCanvas {
    snarl: Snarl<NodeSnapshot>,
    expressions: BTreeMap<EditorNodeId, super::expression::ExpressionDraft>,
    special: crate::editor::telemetry::SpecialSnapshot,
    buttons: BTreeSet<EditorNodeId>,
    ids: BTreeMap<EditorNodeId, NodeId>,
    document_id: Option<DocumentId>,
    accepted_wires: Vec<(OutPinId, InPinId)>,
    pending_controls: BTreeMap<EditorNodeId, RequestId>,
    pending_geometry: BTreeMap<EditorNodeId, RequestId>,
    revision: u64,
    catalog: Vec<crate::editor::document::FunctionChoice>,
    pub snap: bool,
    picker: Option<(egui::Vec2, Option<EditorNodeId>)>,
    draft: String,
    transform: egui::emath::TSTransform,
    gesture: bool,
    view: super::state::CanvasViewState,
}
impl GraphCanvas {
    pub fn set_runtime(&mut self, running: bool, generation: u64) {
        if self.special.generation != generation {
            self.special.traces.clear();
        }
        self.special.generation = generation;
        self.special.running = running;
    }
    pub fn set_special(&mut self, special: crate::editor::telemetry::SpecialSnapshot) {
        if special.document == self.document_id {
            self.special = special;
        }
    }
    pub fn release_buttons(&mut self) -> Vec<EditorCommand> {
        let buttons = std::mem::take(&mut self.buttons);
        self.document_id
            .map(|document_id| {
                buttons
                    .into_iter()
                    .map(|node_id| EditorCommand::Momentary {
                        document_id,
                        node_id,
                        pressed: false,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn selection(&self) -> &BTreeSet<EditorNodeId> {
        &self.view.selected
    }
    pub fn track_geometry(
        &mut self,
        document: DocumentId,
        node: EditorNodeId,
        position: egui::Vec2,
        size: egui::Vec2,
        request: RequestId,
    ) {
        if self.document_id != Some(document) {
            return;
        }
        if let Some(native) = self.ids.get(&node) {
            let info = self.snarl.get_node_info_mut(*native).unwrap();
            info.pos = position.round().to_pos2();
            info.value.position = position.round();
            info.value.custom_size = Some(size.max(egui::vec2(80., 60.)).round());
            self.pending_geometry.insert(node, request);
        }
    }
    pub fn finish_for_file_action(&mut self) -> Vec<EditorCommand> {
        let mut commands = vec![];
        if let Some(document_id) = self.document_id {
            let moves: Vec<_> = self
                .snarl
                .nodes_pos_ids_mut()
                .filter_map(|(_, pos, node)| {
                    let p = pos.to_vec2().round();
                    (node.position != p).then_some((node.id, p))
                })
                .collect();
            if !moves.is_empty() {
                commands.push(EditorCommand::Edit {
                    document_id,
                    revision: self.revision,
                    edit: Edit::Move(moves),
                    req_id: RequestId(0),
                });
            }
        }
        if let Some(command) = self.finish_gesture() {
            commands.push(command);
        }
        commands
    }

    pub fn gesture_active(&self) -> bool {
        self.gesture
    }

    /// Retain each control's local value until its newest service fence arrives.
    pub fn track_control(
        &mut self,
        document_id: DocumentId,
        node_id: EditorNodeId,
        req_id: RequestId,
    ) {
        if self.document_id == Some(document_id) {
            self.pending_controls.insert(node_id, req_id);
        }
    }
    pub fn acknowledge_control(&mut self, req_id: RequestId) -> bool {
        let before = self.pending_controls.len() + self.pending_geometry.len();
        self.pending_controls
            .retain(|_, pending| *pending != req_id);
        self.pending_geometry
            .retain(|_, pending| *pending != req_id);
        self.pending_controls.len() + self.pending_geometry.len() != before
    }

    pub fn finish_gesture(&mut self) -> Option<EditorCommand> {
        if !self.gesture {
            return None;
        }
        self.gesture = false;
        self.document_id.map(|document_id| EditorCommand::Gesture {
            document_id,
            begin: false,
        })
    }
    pub fn picker_open(&self) -> bool {
        self.picker.is_some()
    }
    pub fn edit_result(&mut self, success: bool) {
        if success {
            self.picker = None;
        }
    }

    pub fn sync(&mut self, snapshot: &DocumentSnapshot) {
        if self.document_id != Some(snapshot.id) {
            self.snarl = Snarl::new();
            self.ids.clear();
            self.expressions.clear();
            self.pending_controls.clear();
            self.pending_geometry.clear();
            self.buttons.clear();
            self.special = Default::default();
            self.document_id = Some(snapshot.id);
            self.picker = None;
            self.gesture = false;
            self.view.selected.clear();
        }
        self.revision = snapshot.revision;
        self.catalog = snapshot.catalog.clone();
        self.expressions.retain(|id, _| {
            snapshot
                .nodes
                .iter()
                .any(|(node, value)| node == id && value.expression.is_some())
        });
        for (id, node) in &snapshot.nodes {
            if let Some(text) = &node.expression {
                self.expressions
                    .entry(*id)
                    .or_insert_with(|| {
                        super::expression::ExpressionDraft::new(text, snapshot.revision)
                    })
                    .reconcile(text, snapshot.revision);
            }
        }
        let desired: BTreeSet<_> = snapshot.nodes.iter().map(|(id, _)| *id).collect();
        self.view.selected.retain(|id| desired.contains(id));
        self.pending_controls.retain(|id, _| desired.contains(id));
        self.pending_geometry.retain(|id, _| desired.contains(id));
        let stale: Vec<_> = self
            .ids
            .keys()
            .filter(|id| !desired.contains(id))
            .copied()
            .collect();
        for id in stale {
            if let Some(native) = self.ids.remove(&id) {
                self.snarl.remove_node(native);
            }
        }
        for (id, node) in &snapshot.nodes {
            if let Some(native) = self.ids.get(id) {
                let info = self.snarl.get_node_info_mut(*native).unwrap();
                let moved = self.gesture && info.pos.to_vec2() != info.value.position;
                let values = info.value.control_values.clone();
                let geometry = (info.pos, info.value.position, info.value.custom_size);
                if !moved {
                    info.pos = node.position.to_pos2();
                }
                info.value = node.clone();
                if self.pending_geometry.contains_key(id) {
                    info.pos = geometry.0;
                    info.value.position = geometry.1;
                    info.value.custom_size = geometry.2;
                }
                if self.gesture || self.pending_controls.contains_key(id) {
                    info.value.control_values = values;
                }
            } else {
                let native = self
                    .snarl
                    .insert_node(node.position.to_pos2(), node.clone());
                self.ids.insert(*id, native);
            }
        }
        let old: Vec<_> = self.snarl.wires().collect();
        for (a, b) in old {
            self.snarl.disconnect(a, b);
        }
        for wire in &snapshot.edges {
            if let (Some(&a), Some(&b)) = (self.ids.get(&wire.from.0), self.ids.get(&wire.to.0))
                && wire.from.1 < self.snarl[a].outputs.len()
                && wire.to.1 < self.snarl[b].inputs.len()
            {
                self.snarl.connect(
                    OutPinId {
                        node: a,
                        output: wire.from.1,
                    },
                    InPinId {
                        node: b,
                        input: wire.to.1,
                    },
                );
            }
        }
        self.accepted_wires = self.snarl.wires().collect();
    }
    pub fn show(&mut self, ui: &mut Ui) -> Vec<EditorCommand> {
        let Some(document_id) = self.document_id else {
            ui.label("Open a .twf flow to begin.");
            return vec![];
        };
        let enabled = ui.is_enabled();
        let pointer_down = ui.input(|i| i.pointer.any_down() && i.focused);
        let end = !pointer_down && self.gesture;
        if end {
            self.gesture = false;
        }
        let mut viewer = SnarlAdapter {
            document_id,
            commands: vec![],
            revision: self.revision,
            picker: &mut self.picker,
            draft: &mut self.draft,
            transform: &mut self.transform,
            special: &self.special,
            expressions: &mut self.expressions,
            buttons: &mut self.buttons,
        };
        let response = SnarlWidget::new().id_salt(("flow", document_id.0)).show(
            &mut self.snarl,
            &mut viewer,
            ui,
        );
        // Node controls live on snarl's transformed sublayer. Testing the parent
        // Ui's hover misses them, so use the widget response to start gestures.
        if enabled && pointer_down && !self.gesture && response.contains_pointer() {
            self.gesture = true;
            viewer.commands.insert(
                0,
                EditorCommand::Gesture {
                    document_id,
                    begin: true,
                },
            );
        }
        // Snarl's command-drag branch bypasses viewer callbacks and removes canvas wires.
        // Restore the accepted projection every frame, including canceled wire drags.
        for &(a, b) in &self.accepted_wires {
            self.snarl.connect(a, b);
        }
        let selected = SnarlWidget::new()
            .id_salt(("flow", document_id.0))
            .get_selected_nodes(ui);
        self.view.selected = selected
            .iter()
            .filter_map(|id| self.snarl.get_node(*id).map(|n| n.id))
            .collect();
        let canvas_focus = enabled
            && viewer.picker.is_none()
            && response.contains_pointer()
            && !ui.ctx().egui_wants_keyboard_input();
        if canvas_focus && ui.input(|i| !i.modifiers.command && i.key_pressed(egui::Key::N)) {
            let p = ui
                .input(|i| i.pointer.hover_pos())
                .unwrap_or(ui.max_rect().center());
            *viewer.picker = Some((viewer.transform.inverse().mul_pos(p).to_vec2(), None));
            viewer.draft.clear();
        }
        if canvas_focus
            && ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace))
        {
            let ids = selected
                .iter()
                .filter_map(|id| self.snarl.get_node(*id).map(|n| n.id))
                .collect();
            viewer.edit(Edit::Delete(ids));
        }
        if enabled && !pointer_down {
            let moved: Vec<_> = self
                .snarl
                .nodes_pos_ids_mut()
                .filter_map(|(_, pos, node)| {
                    let p = pos.to_vec2().round();
                    (node.position != p).then_some((node.id, p))
                })
                .collect();
            if let Some((_, p)) = moved.first() {
                let offset = if self.snap {
                    ((*p / 20.0).round() * 20.0) - *p
                } else {
                    egui::Vec2::ZERO
                };

                let moved: Vec<_> = moved.into_iter().map(|(id, p)| (id, p + offset)).collect();
                for (id, p) in &moved {
                    let info = self.snarl.get_node_info_mut(self.ids[id]).unwrap();
                    info.pos = p.to_pos2();
                    info.value.position = *p;
                }
                viewer.edit(Edit::Move(moved));
            }
        }
        if let Some((position, node)) = *viewer.picker {
            let mut close = false;
            egui::Window::new(if node.is_some() {
                "Replace node"
            } else {
                "Add node"
            })
            .id(egui::Id::new("node_picker"))
            .show(ui.ctx(), |ui| {
                ui.label("Function name or expression beginning with =");
                ui.text_edit_singleline(viewer.draft);
                egui::ScrollArea::vertical()
                    .max_height(180.0)
                    .show(ui, |ui| {
                        for name in &self.catalog {
                            if name
                                .name
                                .to_lowercase()
                                .contains(&viewer.draft.to_lowercase())
                            {
                                let label = format!(
                                    "{} · {:?} · {} in / {} out",
                                    name.name,
                                    name.kind,
                                    name.inputs.len(),
                                    name.outputs.len()
                                );
                                let details = format!(
                                    "Inputs: {:?}\nOutputs: {:?}",
                                    name.inputs, name.outputs
                                );
                                if ui
                                    .selectable_label(false, label)
                                    .on_hover_text(details)
                                    .clicked()
                                {
                                    *viewer.draft = name.name.clone();
                                }
                            }
                        }
                    });
                if ui
                    .add_enabled(enabled, egui::Button::new("Apply"))
                    .clicked()
                {
                    let text = viewer.draft.trim().to_owned();
                    viewer.edit(if let Some(node) = node {
                        Edit::Replace { node, text }
                    } else {
                        Edit::Create { text, position }
                    });
                }
                if ui.button("Cancel").clicked() {
                    close = true;
                }
            });
            if close {
                *viewer.picker = None;
            }
        }
        if end {
            viewer.commands.push(EditorCommand::Gesture {
                document_id,
                begin: false,
            });
        }
        viewer.commands
    }
}
pub struct SnarlAdapter<'a> {
    document_id: DocumentId,
    commands: Vec<EditorCommand>,
    revision: u64,
    picker: &'a mut Option<(egui::Vec2, Option<EditorNodeId>)>,
    draft: &'a mut String,
    transform: &'a mut egui::emath::TSTransform,
    special: &'a crate::editor::telemetry::SpecialSnapshot,
    expressions: &'a mut BTreeMap<EditorNodeId, super::expression::ExpressionDraft>,
    buttons: &'a mut BTreeSet<EditorNodeId>,
}
impl SnarlAdapter<'_> {
    fn edit(&mut self, edit: Edit) {
        self.commands.push(EditorCommand::Edit {
            document_id: self.document_id,
            revision: self.revision,
            edit,
            req_id: RequestId(0),
        });
    }
    fn wire(from: &OutPin, to: &InPin, snarl: &Snarl<NodeSnapshot>) -> Wire {
        Wire {
            from: (snarl[from.id.node].id, from.id.output),
            to: (snarl[to.id.node].id, to.id.input),
        }
    }
}
impl SnarlViewer<NodeSnapshot> for SnarlAdapter<'_> {
    fn title(&mut self, node: &NodeSnapshot) -> String {
        node.label.clone()
    }
    fn inputs(&mut self, node: &NodeSnapshot) -> usize {
        node.inputs.len()
    }
    fn outputs(&mut self, node: &NodeSnapshot) -> usize {
        node.outputs.len()
    }
    fn has_body(&mut self, _: &NodeSnapshot) -> bool {
        true
    }
    fn show_body(
        &mut self,
        node: NodeId,
        _: &[InPin],
        _: &[OutPin],
        ui: &mut Ui,
        snarl: &mut Snarl<NodeSnapshot>,
    ) {
        use node_content::NodeAction;
        let id = snarl[node].id;
        if let Some(draft) = self.expressions.get_mut(&id) {
            if let Some(text) = draft.show(ui) {
                self.edit(Edit::Replace { node: id, text });
            }
            return;
        }
        if let Some(action) = node_content::show_node_body(&mut snarl[node], ui, self.special) {
            let command = match action {
                NodeAction::Scalar(value) => EditorCommand::SetControlValue {
                    document_id: self.document_id,
                    node_id: id,
                    value,
                },
                NodeAction::Momentary(pressed) => {
                    let changed = if pressed {
                        self.buttons.insert(id)
                    } else {
                        self.buttons.remove(&id)
                    };
                    if !changed {
                        return;
                    }
                    EditorCommand::Momentary {
                        document_id: self.document_id,
                        node_id: id,
                        pressed,
                    }
                }
                NodeAction::ChooseBuffer => EditorCommand::ChooseBuffer {
                    document_id: self.document_id,
                    node_id: id,
                },
                NodeAction::Sine => EditorCommand::LoadBuffer {
                    document_id: self.document_id,
                    node_id: id,
                    path: None,
                    req_id: RequestId(0),
                },
                NodeAction::Retry(path) => EditorCommand::LoadBuffer {
                    document_id: self.document_id,
                    node_id: id,
                    path: Some(path),
                    req_id: RequestId(0),
                },
            };
            self.commands.push(command);
        }
    }
    fn show_input(
        &mut self,
        pin: &InPin,
        ui: &mut Ui,
        snarl: &mut Snarl<NodeSnapshot>,
    ) -> impl egui_snarl::ui::SnarlPin + 'static {
        let p = &snarl[pin.id.node].inputs[pin.id.input];
        ui.label(&p.name);
        pin_info(p.data_type)
    }
    fn show_output(
        &mut self,
        pin: &OutPin,
        ui: &mut Ui,
        snarl: &mut Snarl<NodeSnapshot>,
    ) -> impl egui_snarl::ui::SnarlPin + 'static {
        let p = &snarl[pin.id.node].outputs[pin.id.output];
        ui.label(&p.name);
        pin_info(p.data_type)
    }
    fn node_frame(
        &mut self,
        mut default: egui::Frame,
        node: NodeId,
        _: &[InPin],
        _: &[OutPin],
        snarl: &Snarl<NodeSnapshot>,
    ) -> egui::Frame {
        default.stroke.color = node_content::node_type_color(&snarl[node].node_type);
        default
    }
    fn current_transform(&mut self, t: &mut egui::emath::TSTransform, _: &mut Snarl<NodeSnapshot>) {
        *self.transform = *t;
    }
    fn has_graph_menu(&mut self, _: egui::Pos2, _: &mut Snarl<NodeSnapshot>) -> bool {
        true
    }
    fn show_graph_menu(&mut self, pos: egui::Pos2, ui: &mut Ui, _: &mut Snarl<NodeSnapshot>) {
        if ui.button("Add node…").clicked() {
            *self.picker = Some((pos.to_vec2(), None));
            self.draft.clear();
            ui.close();
        }
    }
    fn has_node_menu(&mut self, _: &NodeSnapshot) -> bool {
        true
    }
    fn show_node_menu(
        &mut self,
        node: NodeId,
        _: &[InPin],
        _: &[OutPin],
        ui: &mut Ui,
        snarl: &mut Snarl<NodeSnapshot>,
    ) {
        if ui.button("Replace…").clicked() {
            *self.picker = Some((snarl[node].position, Some(snarl[node].id)));
            *self.draft = snarl[node].label.clone();
            ui.close();
        }
        if ui.button("Delete").clicked() {
            self.edit(Edit::Delete(vec![snarl[node].id]));
            ui.close();
        }
        if ui.button("Disconnect all").clicked() {
            let wires = snarl
                .wires()
                .filter(|(a, b)| a.node == node || b.node == node)
                .map(|(a, b)| Self::wire(&snarl.out_pin(a), &snarl.in_pin(b), snarl))
                .collect();
            self.edit(Edit::Disconnect(wires));
            ui.close();
        }
    }
    fn connect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<NodeSnapshot>) {
        self.edit(Edit::Connect(Self::wire(from, to, snarl)));
    }
    fn disconnect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<NodeSnapshot>) {
        self.edit(Edit::Disconnect(vec![Self::wire(from, to, snarl)]));
    }
    fn drop_inputs(&mut self, pin: &InPin, snarl: &mut Snarl<NodeSnapshot>) {
        self.edit(Edit::Disconnect(
            pin.remotes
                .iter()
                .map(|a| Self::wire(&snarl.out_pin(*a), pin, snarl))
                .collect(),
        ));
    }
    fn drop_outputs(&mut self, pin: &OutPin, snarl: &mut Snarl<NodeSnapshot>) {
        self.edit(Edit::Disconnect(
            pin.remotes
                .iter()
                .map(|b| Self::wire(pin, &snarl.in_pin(*b), snarl))
                .collect(),
        ));
    }
}
fn pin_info(kind: DataType) -> PinInfo {
    PinInfo::circle().with_fill(match kind {
        DataType::Float => egui::Color32::WHITE,
        DataType::Array => egui::Color32::GRAY,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::document::EditorDocument;
    fn fixture() -> (EditorDocument, GraphCanvas) {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
        let doc = EditorDocument::load(
            &root.join("wisp_gui/control.twf"),
            &root.join("wisp/core"),
            DocumentId(1),
        )
        .unwrap();
        let mut canvas = GraphCanvas::default();
        canvas.sync(&doc.snapshot());
        (doc, canvas)
    }
    #[test]
    fn expression_drafts_follow_accepted_source_and_node_kind() {
        let (mut doc, mut canvas) = fixture();
        doc.edit(
            doc.snapshot().revision,
            Edit::Create {
                text: "= 1".into(),
                position: egui::Vec2::ZERO,
            },
        )
        .unwrap();
        let snapshot = doc.snapshot();
        let id = snapshot
            .nodes
            .iter()
            .find(|(_, n)| n.expression.as_deref() == Some("= 1"))
            .unwrap()
            .0;
        canvas.sync(&snapshot);
        let before = doc.serialize();
        canvas.expressions.get_mut(&id).unwrap().draft_text = "= (".into();
        assert!(canvas.expressions.get_mut(&id).unwrap().commit().is_none());
        assert_eq!(doc.serialize(), before);
        canvas.expressions.get_mut(&id).unwrap().draft_text = "= 2\n+ 3".into();
        let text = canvas.expressions.get_mut(&id).unwrap().commit().unwrap();
        doc.edit(doc.snapshot().revision, Edit::Replace { node: id, text })
            .unwrap();
        canvas.sync(&doc.snapshot());
        assert!(!canvas.expressions[&id].conflict);
        assert_eq!(canvas.expressions[&id].accepted_text, "= 2\n+ 3");
        doc.edit(
            doc.snapshot().revision,
            Edit::Replace {
                node: id,
                text: "control".into(),
            },
        )
        .unwrap();
        canvas.sync(&doc.snapshot());
        assert!(!canvas.expressions.contains_key(&id));
        doc.edit(doc.snapshot().revision, Edit::Undo).unwrap();
        canvas.sync(&doc.snapshot());
        assert_eq!(canvas.expressions[&id].draft_text, "= 2\n+ 3");
    }
    #[test]
    fn pointer_on_node_sublayer_starts_and_ends_a_gesture() {
        let (_, mut canvas) = fixture();
        let ctx = egui::Context::default();
        let input = |events| egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200., 900.),
            )),
            events,
            ..Default::default()
        };
        for _ in 0..3 {
            let mut out = ctx.run_ui(input(vec![]), |ui| {
                canvas.show(ui);
            });
            out.textures_delta.clear();
        }
        let node = &canvas.snarl[canvas.ids[&EditorNodeId(2)]];
        let position = canvas
            .transform
            .mul_pos((node.position + egui::vec2(30., 40.)).to_pos2());
        let mut out = ctx.run_ui(input(vec![egui::Event::PointerMoved(position)]), |ui| {
            canvas.show(ui);
        });
        out.textures_delta.clear();
        let mut commands = vec![];
        let mut out = ctx.run_ui(
            input(vec![egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            }]),
            |ui| commands = canvas.show(ui),
        );
        out.textures_delta.clear();
        assert!(canvas.gesture_active());
        assert!(
            matches!(
                commands.first(),
                Some(EditorCommand::Gesture { begin: true, .. })
            ),
            "{commands:?}"
        );
        assert!(
            commands
                .iter()
                .any(|c| matches!(c, EditorCommand::SetControlValue { .. })),
            "Expected slider edit: {commands:?}"
        );
        let mut out = ctx.run_ui(
            input(vec![egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }]),
            |ui| commands = canvas.show(ui),
        );
        out.textures_delta.clear();
        assert!(!canvas.gesture_active());
        assert!(
            matches!(
                commands.last(),
                Some(EditorCommand::Gesture { begin: false, .. })
            ),
            "{commands:?}"
        );
    }
    #[test]
    fn released_slider_keeps_final_value_until_latest_edit_is_acknowledged() {
        let (mut doc, mut canvas) = fixture();
        let control = doc.snapshot().nodes[1].0;
        let native = canvas.ids[&control];
        // The pointer has already been released; older accepted drag values are
        // still arriving from the worker. They must not replay the drag on screen.
        canvas.snarl[native].control_values.insert(0, 0.8);
        canvas.track_control(DocumentId(1), control, RequestId(10));
        canvas.track_control(DocumentId(1), control, RequestId(11));
        assert!(!canvas.acknowledge_control(RequestId(10)));
        for value in [0.2, 0.4, 0.6] {
            doc.set_value(control, value).unwrap();
            canvas.sync(&doc.snapshot());
            assert_eq!(canvas.snarl[native].control_values[&0], 0.8);
        }
        doc.set_value(control, 0.8).unwrap();
        assert!(canvas.acknowledge_control(RequestId(11)));
        canvas.sync(&doc.snapshot());
        assert_eq!(canvas.snarl[native].control_values[&0], 0.8f32 as f64);
        // Once acknowledged, subsequent authoritative edits (including Undo)
        // must still update the displayed value.
        doc.edit(doc.snapshot().revision, Edit::Undo).unwrap();
        canvas.sync(&doc.snapshot());
        assert_eq!(canvas.snarl[native].control_values[&0], 0.6f32 as f64);
    }
    #[test]
    fn service_slider_snapshots_do_not_generate_edits_without_input() {
        let (mut doc, mut canvas) = fixture();
        let ctx = egui::Context::default();
        let control = doc.snapshot().nodes[1].0;
        // Replay backend f32 values, including a value between slider steps.
        // Snapshot delivery/painting must never submit those values as new edits.
        for value in [0.103, 0.455, 0.203, 0.317, 0.123456] {
            doc.set_value(control, value).unwrap();
            for _ in 0..3 {
                canvas.sync(&doc.snapshot());
                let mut commands = vec![];
                let mut output = ctx.run_ui(Default::default(), |ui| {
                    commands = canvas.show(ui);
                });
                output.textures_delta.clear();
                assert!(
                    !commands
                        .iter()
                        .any(|command| matches!(command, EditorCommand::SetControlValue { .. })),
                    "Snapshot {value} generated an unsolicited slider edit: {commands:?}"
                );
            }
        }
    }
    #[test]
    fn accepted_positions_and_wires_reconcile_after_undo_or_native_bypass() {
        let (mut doc, mut canvas) = fixture();
        let id = doc.snapshot().nodes[0].0;
        let native = canvas.ids[&id];
        let initial = canvas.snarl.get_node_info(native).unwrap().pos;
        doc.edit(
            doc.snapshot().revision,
            Edit::Move(vec![(id, egui::vec2(100., 200.))]),
        )
        .unwrap();
        canvas.sync(&doc.snapshot());
        assert_eq!(
            canvas.snarl.get_node_info(native).unwrap().pos,
            egui::pos2(100., 200.)
        );
        doc.edit(doc.snapshot().revision, Edit::Undo).unwrap();
        canvas.sync(&doc.snapshot());
        assert_eq!(canvas.snarl.get_node_info(native).unwrap().pos, initial);
        // Reproduce the internal command-drag drop without accepting a document edit.
        let (a, b) = canvas.accepted_wires[0];
        canvas.snarl.disconnect(a, b);
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(Default::default(), |ui| {
            canvas.show(ui);
        });
        output.textures_delta.clear();
        assert_eq!(canvas.snarl.wires().count(), doc.snapshot().edges.len());
    }
    #[test]
    fn callbacks_emit_owned_intents_without_mutating_native_topology() {
        let (_, mut canvas) = fixture();
        let mut picker = None;
        let mut draft = String::new();
        let mut transform = Default::default();
        let (a, b) = canvas.accepted_wires[0];
        let from = canvas.snarl.out_pin(a);
        let to = canvas.snarl.in_pin(b);
        let before = canvas.snarl.wires().count();
        let mut viewer = SnarlAdapter {
            document_id: DocumentId(1),
            revision: 9,
            commands: vec![],
            picker: &mut picker,
            draft: &mut draft,
            transform: &mut transform,
            special: &Default::default(),
            expressions: &mut Default::default(),
            buttons: &mut Default::default(),
        };
        viewer.connect(&from, &to, &mut canvas.snarl);
        viewer.disconnect(&from, &to, &mut canvas.snarl);
        viewer.drop_inputs(&to, &mut canvas.snarl);
        viewer.drop_outputs(&from, &mut canvas.snarl);
        assert_eq!(canvas.snarl.wires().count(), before);
        assert_eq!(viewer.commands.len(), 4);
        assert!(viewer.commands.iter().all(|c| matches!(
            c,
            EditorCommand::Edit {
                document_id: DocumentId(1),
                revision: 9,
                ..
            }
        )));
    }
    fn key(key: egui::Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }
    }
    #[test]
    fn picker_shortcuts_respect_text_focus_and_keep_rejected_drafts() {
        let (_, mut canvas) = fixture();
        let ctx = egui::Context::default();
        let mut text = String::new();
        let mut output = ctx.run_ui(Default::default(), |ui| {
            ui.text_edit_singleline(&mut text).request_focus();
            canvas.show(ui);
        });
        output.textures_delta.clear();
        let input = egui::RawInput {
            events: vec![key(egui::Key::N), key(egui::Key::Delete)],
            ..Default::default()
        };
        let mut commands = vec![];
        let mut output = ctx.run_ui(input, |ui| {
            ui.text_edit_singleline(&mut text);
            commands = canvas.show(ui);
        });
        output.textures_delta.clear();
        assert!(canvas.picker.is_none());
        assert!(
            commands
                .iter()
                .all(|c| !matches!(c, EditorCommand::Edit { .. }))
        );
        canvas.picker = Some((egui::Vec2::ZERO, None));
        canvas.draft = "= (".into();
        canvas.edit_result(false);
        assert!(canvas.picker.is_some());
        assert_eq!(canvas.draft, "= (");
        canvas.edit_result(true);
        assert!(canvas.picker.is_none());
    }
    #[test]
    fn n_opens_picker_and_group_snap_preserves_offsets() {
        let (_, mut canvas) = fixture();
        let ctx = egui::Context::default();
        let input = |events| egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200., 900.),
            )),
            events,
            ..Default::default()
        };
        for _ in 0..3 {
            let mut out = ctx.run_ui(
                input(vec![egui::Event::PointerMoved(egui::pos2(1100., 800.))]),
                |ui| {
                    canvas.show(ui);
                },
            );
            out.textures_delta.clear();
        }
        let mut out = ctx.run_ui(input(vec![key(egui::Key::N)]), |ui| {
            canvas.show(ui);
        });
        out.textures_delta.clear();
        assert!(canvas.picker.is_some());
        canvas.picker = None;
        let ids: Vec<_> = canvas
            .ids
            .iter()
            .take(2)
            .map(|(id, native)| (*id, *native))
            .collect();
        let positions = [egui::pos2(31., 42.), egui::pos2(94., 115.)];
        for ((_, native), position) in ids.iter().zip(positions) {
            canvas.snarl.get_node_info_mut(*native).unwrap().pos = position;
        }
        canvas.snap = true;
        let mut commands = vec![];
        let mut out = ctx.run_ui(input(vec![]), |ui| commands = canvas.show(ui));
        out.textures_delta.clear();
        let moves = commands
            .into_iter()
            .find_map(|c| match c {
                EditorCommand::Edit {
                    edit: Edit::Move(m),
                    ..
                } => Some(m),
                _ => None,
            })
            .unwrap();
        assert_eq!(moves.len(), 2);
        assert_eq!(moves[0].1, egui::vec2(40., 40.));
        assert_eq!(moves[1].1 - moves[0].1, positions[1] - positions[0]);
        for scale in [0.5, 2.0] {
            let transform = egui::emath::TSTransform {
                scaling: scale,
                translation: egui::vec2(50., 100.),
            };
            let global = transform.mul_pos(egui::pos2(31., 42.));
            assert_eq!(transform.inverse().mul_pos(global), egui::pos2(31., 42.));
        }
    }
    #[test]
    fn sync_preserves_identity_and_restores_all_wires() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
        let doc = EditorDocument::load(
            &root.join("wisp_gui/control.twf"),
            &root.join("wisp/core"),
            DocumentId(1),
        )
        .unwrap();
        let mut canvas = GraphCanvas::default();
        let s = doc.snapshot();
        canvas.sync(&s);
        let ids = canvas.ids.clone();
        canvas.sync(&s);
        assert_eq!(canvas.ids, ids);
        assert_eq!(canvas.snarl.node_ids().count(), 7);
        assert_eq!(canvas.snarl.wires().count(), 7);
        assert_ne!(canvas.ids[&s.nodes[0].0].0, s.nodes[0].0.0);
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(Default::default(), |ui| {
            canvas.show(ui);
        });
        output.textures_delta.clear();
    }
    #[test]
    fn geometry_fences_allow_undo_to_reconcile_the_canvas() {
        let (mut doc, mut canvas) = fixture();
        let snapshot = doc.snapshot();
        let id = snapshot.nodes[0].0;
        let native = canvas.ids[&id];
        let original = snapshot.nodes[0].1.position;
        canvas.track_geometry(
            snapshot.id,
            id,
            egui::vec2(42., 55.),
            egui::vec2(180., 120.),
            RequestId(7),
        );
        canvas.sync(&snapshot);
        assert_eq!(
            canvas.snarl.get_node_info(native).unwrap().pos,
            egui::pos2(42., 55.)
        );
        assert!(!canvas.acknowledge_control(RequestId(6)));
        assert!(canvas.acknowledge_control(RequestId(7)));
        doc.set_geometry(id, egui::vec2(42., 55.), egui::vec2(180., 120.))
            .unwrap();
        canvas.sync(&doc.snapshot());
        doc.edit(doc.snapshot().revision, Edit::Undo).unwrap();
        canvas.sync(&doc.snapshot());
        assert_eq!(
            canvas.snarl.get_node_info(native).unwrap().pos,
            original.to_pos2()
        );
    }
}
