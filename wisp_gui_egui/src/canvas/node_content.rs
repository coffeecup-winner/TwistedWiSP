// SPDX-License-Identifier: MIT OR Apache-2.0
use crate::editor::document::{NodeSnapshot, NodeType};
use egui::{Color32, RichText, Ui};
pub fn node_type_color(kind: &NodeType) -> Color32 {
    match kind {
        NodeType::Control => Color32::LIGHT_BLUE,
        NodeType::Flow => Color32::LIGHT_GREEN,
        NodeType::IO => Color32::LIGHT_RED,
        NodeType::Subflow => Color32::LIGHT_YELLOW,
    }
}
/// Return a scalar edit intent; this function knows nothing about snarl or the backend.
pub fn show_node_body(node: &mut NodeSnapshot, ui: &mut Ui) -> Option<f64> {
    if let Some(size) = node.custom_size {
        ui.set_min_width((size.x - 48.0).max(60.0));
        ui.set_min_height((size.y - 48.0).max(16.0));
    }
    if let Some(expr) = &node.expression {
        ui.monospace(expr);
    }
    if node.node_type == NodeType::Control {
        let value = node.control_values.entry(0).or_insert(0.0);
        if ui
            .add(
                egui::Slider::new(value, 0.0..=1.0)
                    .step_by(0.001)
                    // Backend f32 snapshots need not fall exactly on f64 slider steps.
                    // Painting must not round them and emit another edit.
                    .clamping(egui::SliderClamping::Edits),
            )
            .changed()
        {
            return Some(*value);
        }
    } else if matches!(
        node.function_name.as_str(),
        "button" | "toggle" | "watch" | "graph" | "buffer"
    ) {
        ui.label(RichText::new("Specialized UI: milestone 4").weak());
    }
    None
}
