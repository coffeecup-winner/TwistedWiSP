// SPDX-License-Identifier: MIT OR Apache-2.0
//! Acknowledged selection, with numeric drafts owned only while focused/dragging.
use crate::editor::{
    commands::{EditorCommand, RequestId},
    document::{DocumentId, DocumentSnapshot, EditorNodeId, NodeSnapshot, NodeType},
};
use std::collections::BTreeSet;
#[derive(Default)]
pub struct Inspector {
    identity: Option<(DocumentId, EditorNodeId)>,
    values: [f64; 5],
    focused: bool,
    dragging: bool,
    pending: Option<RequestId>,
    canceled_frame: Option<u64>,
}
impl Inspector {
    pub fn track(&mut self, document: DocumentId, node: EditorNodeId, request: RequestId) {
        if self.identity == Some((document, node)) {
            self.pending = Some(request);
        }
    }
    pub fn acknowledge(&mut self, request: RequestId) {
        if self.pending == Some(request) {
            self.pending = None;
        }
    }
    fn read(node: &NodeSnapshot) -> [f64; 5] {
        let size = node.custom_size.unwrap_or(egui::vec2(160., 100.));
        [
            node.position.x as f64,
            node.position.y as f64,
            size.x as f64,
            size.y as f64,
            node.control_values.get(&0).copied().unwrap_or(0.),
        ]
    }
    pub fn clear(&mut self) {
        *self = Self::default();
    }
    pub fn finish_gesture(&mut self) -> Option<EditorCommand> {
        if !self.dragging {
            return None;
        }
        self.dragging = false;
        self.identity
            .map(|(document_id, _)| EditorCommand::Gesture {
                document_id,
                begin: false,
            })
    }
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: Option<&DocumentSnapshot>,
        selection: &BTreeSet<EditorNodeId>,
        enabled: bool,
    ) -> Vec<EditorCommand> {
        let mut commands = vec![];
        ui.heading("Inspector");
        let Some(s) = snapshot else {
            if let Some(command) = self.finish_gesture() {
                commands.push(command);
            }
            self.clear();
            ui.label("No document");
            return commands;
        };
        if selection.len() != 1 {
            if let Some(command) = self.finish_gesture() {
                commands.push(command);
            }
            self.clear();
            ui.label(if selection.is_empty() {
                "Select one node".into()
            } else {
                format!("{} nodes selected", selection.len())
            });
            return commands;
        }
        let id = *selection.first().unwrap();
        let Some((_, node)) = s.nodes.iter().find(|(n, _)| *n == id) else {
            if let Some(command) = self.finish_gesture() {
                commands.push(command);
            }
            self.clear();
            return commands;
        };
        let identity = (s.id, id);
        if self.identity != Some(identity) {
            if let Some(command) = self.finish_gesture() {
                commands.push(command);
            }
            self.clear();
            self.identity = Some(identity);
            self.values = Self::read(node);
        }
        if !self.focused && !self.dragging && self.pending.is_none() {
            self.values = Self::read(node);
        }
        ui.label(&node.label);
        ui.small("Enter/focus loss commits; Escape cancels typing.");
        let previous_values = self.values;
        let cancel_typing = self.focused && ui.input(|i| i.key_pressed(egui::Key::Escape));
        let frame = ui.ctx().cumulative_frame_nr();
        if cancel_typing {
            self.canceled_frame = Some(frame);
        }
        let cancel_value = self
            .canceled_frame
            .is_some_and(|canceled| frame <= canceled + 1);
        let mut focused = false;
        let mut started = false;
        let mut geometry = false;
        let mut scalar = false;
        ui.add_enabled_ui(enabled, |ui| {
            for (index, label) in ["x", "y", "width", "height", "value"]
                .into_iter()
                .enumerate()
            {
                if index == 4 && node.node_type != NodeType::Control {
                    continue;
                }
                ui.horizontal(|ui| {
                    ui.label(label);
                    let previous = self.values[index];
                    let mut drag =
                        egui::DragValue::new(&mut self.values[index]).update_while_editing(false);
                    drag = if index == 4 {
                        drag.speed(0.001).range(0.0..=1.0).fixed_decimals(3)
                    } else if index >= 2 {
                        drag.speed(1.)
                            .range(if index == 2 { 80.0 } else { 60.0 }..=2_000_000_000.)
                            .fixed_decimals(0)
                    } else {
                        drag.speed(1.)
                            .range(-2_000_000_000.0..=2_000_000_000.)
                            .fixed_decimals(0)
                    };
                    let response = ui.add(drag);
                    focused |= response.has_focus();
                    if response.drag_started() {
                        started = true;
                    }
                    if self.values[index] != previous {
                        if index == 4 {
                            scalar = true;
                        } else {
                            geometry = true;
                        }
                    }
                });
            }
        });
        // egui 0.36 can parse a canceled DragValue draft again on the next
        // frame's lost-focus path. Consume that transition without an edit.
        if cancel_value {
            self.values = previous_values;
            geometry = false;
            scalar = false;
        }
        if started && !self.dragging {
            self.dragging = true;
            commands.push(EditorCommand::Gesture {
                document_id: s.id,
                begin: true,
            });
        }
        if geometry {
            commands.push(EditorCommand::SetGeometry {
                document_id: s.id,
                node_id: id,
                position: egui::vec2(self.values[0] as f32, self.values[1] as f32),
                size: egui::vec2(self.values[2] as f32, self.values[3] as f32),
            });
        }
        if scalar {
            self.values[4] = (self.values[4] * 1000.).round() / 1000.;
            commands.push(EditorCommand::SetControlValue {
                document_id: s.id,
                node_id: id,
                value: self.values[4],
            });
        }
        self.focused = focused;
        if self.dragging
            && ui.input(|i| !i.pointer.any_down() || !i.focused)
            && let Some(command) = self.finish_gesture()
        {
            commands.push(command);
        }
        if !focused && !self.dragging && !geometry && !scalar && self.pending.is_none() {
            self.values = Self::read(node);
        }
        commands
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::document::EditorDocument;
    fn render(
        inspector: &mut Inspector,
        snapshot: &DocumentSnapshot,
        selection: &BTreeSet<EditorNodeId>,
    ) -> Vec<EditorCommand> {
        let ctx = egui::Context::default();
        let mut commands = vec![];
        let mut output = ctx.run_ui(Default::default(), |ui| {
            commands = inspector.show(ui, Some(snapshot), selection, true);
        });
        output.textures_delta.clear();
        commands
    }
    #[test]
    fn selection_drafts_acknowledgements_and_gesture_cleanup() {
        let core = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../wisp/core");
        let mut doc = EditorDocument::from_text(
            "[flow]\nname='inspector'\n[[flow.nodes]]\ntext='control'\n",
            &core,
            DocumentId(1),
            None,
        )
        .unwrap();
        let id = doc.snapshot().nodes[0].0;
        let selected = BTreeSet::from([id]);
        let mut inspector = Inspector::default();
        assert!(render(&mut inspector, &doc.snapshot(), &selected).is_empty());
        inspector.values[0] = 42.;
        inspector.track(DocumentId(1), id, RequestId(3));
        render(&mut inspector, &doc.snapshot(), &selected);
        assert_eq!(inspector.values[0], 42.); // Don't replace a pending draft with old snapshots.
        inspector.acknowledge(RequestId(2));
        assert!(inspector.pending.is_some());
        doc.set_geometry(id, egui::vec2(42., 5.), egui::vec2(80., 60.))
            .unwrap();
        inspector.acknowledge(RequestId(3));
        render(&mut inspector, &doc.snapshot(), &selected);
        assert_eq!(&inspector.values[..4], &[42., 5., 80., 60.]);
        doc.edit(doc.snapshot().revision, crate::editor::document::Edit::Undo)
            .unwrap();
        render(&mut inspector, &doc.snapshot(), &selected);
        assert_eq!(inspector.values[0], 0.);
        inspector.dragging = true;
        let commands = render(&mut inspector, &doc.snapshot(), &BTreeSet::new());
        assert!(matches!(
            commands.as_slice(),
            [EditorCommand::Gesture { begin: false, .. }]
        ));
        assert!(inspector.identity.is_none());
        render(
            &mut inspector,
            &doc.snapshot(),
            &BTreeSet::from([id, EditorNodeId(99)]),
        );
        assert!(inspector.identity.is_none());
    }
    #[test]
    fn typing_escape_enter_and_save_focus_loss() {
        let core = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../wisp/core");
        let doc = EditorDocument::from_text(
            "[flow]\nname='typing'\n[[flow.nodes]]\ntext='control'\n",
            &core,
            DocumentId(1),
            None,
        )
        .unwrap();
        let snapshot = doc.snapshot();
        let selected = BTreeSet::from([snapshot.nodes[0].0]);
        let mut inspector = Inspector::default();
        let ctx = egui::Context::default();
        let mut frame = |events: Vec<egui::Event>| {
            let mut commands = vec![];
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(600., 500.),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    commands = inspector.show(ui, Some(&snapshot), &selected, true);
                },
            );
            let mut position = None;
            for shape in &output.shapes {
                if let egui::Shape::Text(text) = &shape.shape
                    && text.galley.text() == "0"
                    && position.is_none()
                {
                    position = Some(text.pos + text.galley.size() * 0.5);
                }
            }
            output.textures_delta.clear();
            (commands, position)
        };
        let (_, position) = frame(vec![]);
        let pos = position.expect("x field text");
        let click = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        let key = |key| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        };
        assert!(
            frame(vec![egui::Event::PointerMoved(pos), click(true)])
                .0
                .is_empty()
        );
        assert!(frame(vec![click(false)]).0.is_empty());
        let typed = frame(vec![egui::Event::Text("42".into())]).0;
        assert!(typed.is_empty(), "{typed:?}");
        assert!(frame(vec![key(egui::Key::Escape)]).0.is_empty());
        assert!(frame(vec![]).0.is_empty());
        frame(vec![click(true)]);
        frame(vec![click(false)]);
        assert!(frame(vec![egui::Event::Text("17".into())]).0.is_empty());
        let commands = frame(vec![key(egui::Key::Enter)]).0;
        assert!(
            matches!(commands.as_slice(), [EditorCommand::SetGeometry { position, .. }] if position.x == 17.),
            "{commands:?}"
        );
        frame(vec![]);
        frame(vec![click(true)]);
        frame(vec![click(false)]);
        assert!(frame(vec![egui::Event::Text("23".into())]).0.is_empty());
        ctx.memory_mut(|memory| {
            if let Some(id) = memory.focused() {
                memory.surrender_focus(id);
            }
        });
        let commands = frame(vec![]).0;
        assert!(
            matches!(commands.as_slice(), [EditorCommand::SetGeometry { position, .. }] if position.x == 23.),
            "{commands:?}"
        );
    }
}
