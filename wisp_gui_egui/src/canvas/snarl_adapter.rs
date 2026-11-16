// SPDX-License-Identifier: MIT OR Apache-2.0
//! Snarl IDs are confined to this presentation adapter and never used as document IDs.
use super::node_content;
use crate::editor::{
    commands::EditorCommand,
    document::{DocumentId, DocumentSnapshot, EditorNodeId, NodeSnapshot},
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
    ids: BTreeMap<EditorNodeId, NodeId>,
    document_id: Option<DocumentId>,
}
impl GraphCanvas {
    pub fn sync(&mut self, snapshot: &DocumentSnapshot) {
        if self.document_id != Some(snapshot.id) {
            self.snarl = Snarl::new();
            self.ids.clear();
            self.document_id = Some(snapshot.id);
        }
        let desired: BTreeSet<_> = snapshot.nodes.iter().map(|(id, _)| *id).collect();
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
                // Preserve the canvas position while an input gesture/snapshot is in flight.
                self.snarl[*native] = node.clone();
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
    }
    pub fn show(&mut self, ui: &mut Ui) -> Vec<EditorCommand> {
        let Some(document_id) = self.document_id else {
            ui.label("Open a .twf flow to begin.");
            return vec![];
        };
        let mut viewer = SnarlAdapter {
            document_id,
            commands: vec![],
        };
        SnarlWidget::new()
            .id_salt(("flow", document_id.0))
            .show(&mut self.snarl, &mut viewer, ui);
        // Flush on release or focus loss, never on every animation frame.
        if ui.input(|i| !i.pointer.any_down()) {
            for (_, pos, node) in self.snarl.nodes_pos_ids_mut() {
                let current = pos.to_vec2().round();
                if node.position != current {
                    node.position = current;
                    viewer.commands.push(EditorCommand::MoveNode {
                        document_id,
                        node_id: node.id,
                        position: current,
                    });
                }
            }
        }
        viewer.commands
    }
}
pub struct SnarlAdapter {
    document_id: DocumentId,
    commands: Vec<EditorCommand>,
}
impl SnarlViewer<NodeSnapshot> for SnarlAdapter {
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
        if let Some(value) = node_content::show_node_body(&mut snarl[node], ui) {
            self.commands.push(EditorCommand::SetControlValue {
                document_id: self.document_id,
                node_id: snarl[node].id,
                value,
            });
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
    // Topology editing is milestone 2. Never mutate only the canvas.
    fn connect(&mut self, _: &OutPin, _: &InPin, _: &mut Snarl<NodeSnapshot>) {}
    fn disconnect(&mut self, _: &OutPin, _: &InPin, _: &mut Snarl<NodeSnapshot>) {}
    fn drop_inputs(&mut self, _: &InPin, _: &mut Snarl<NodeSnapshot>) {}
    fn drop_outputs(&mut self, _: &OutPin, _: &mut Snarl<NodeSnapshot>) {}
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
}
