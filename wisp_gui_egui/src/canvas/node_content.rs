// SPDX-License-Identifier: MIT OR Apache-2.0
use crate::editor::{
    document::{NodeSnapshot, NodeType},
    telemetry::SpecialSnapshot,
};
use egui::{Color32, Ui};
pub fn node_type_color(kind: &NodeType) -> Color32 {
    match kind {
        NodeType::Control => Color32::LIGHT_BLUE,
        NodeType::Flow => Color32::LIGHT_GREEN,
        NodeType::IO => Color32::LIGHT_RED,
        NodeType::Subflow => Color32::LIGHT_YELLOW,
    }
}
pub enum NodeAction {
    Scalar(f64),
    Momentary(bool),
    ChooseBuffer,
    Sine,
    Retry(std::path::PathBuf),
}
/// Application-owned controls; document and runtime state never live in widgets.
pub fn show_node_body(
    node: &mut NodeSnapshot,
    ui: &mut Ui,
    special: &SpecialSnapshot,
) -> Option<NodeAction> {
    if let Some(size) = node.custom_size {
        ui.set_min_width((size.x - 48.).max(60.));
        ui.set_min_height((size.y - 48.).max(16.));
    }
    if let Some(expr) = &node.expression {
        ui.monospace(expr);
    }
    match node.function_name.as_str() {
        "control" => {
            let value = node.control_values.entry(0).or_insert(0.);
            if ui
                .add(
                    egui::Slider::new(value, 0.0..=1.0)
                        .step_by(0.001)
                        .clamping(egui::SliderClamping::Edits),
                )
                .changed()
            {
                return Some(NodeAction::Scalar(*value));
            }
        }
        "toggle" => {
            let mut on = node.control_values.get(&0).copied().unwrap_or(0.) > 0.;
            if ui.checkbox(&mut on, "On").changed() {
                let value = f64::from(on);
                node.control_values.insert(0, value);
                return Some(NodeAction::Scalar(value));
            }
        }
        "button" => {
            let r = ui.add(egui::Button::new("Hold").sense(egui::Sense::click_and_drag()));
            let has_focus = r.has_focus();
            let pointer_down = r.is_pointer_button_down_on();
            return Some(NodeAction::Momentary(
                ui.is_enabled()
                    && ui.input(|i| {
                        i.focused
                            && ((pointer_down && i.pointer.primary_down())
                                || (has_focus
                                    && (i.key_down(egui::Key::Space)
                                        || i.key_down(egui::Key::Enter))))
                    }),
            ));
        }
        "watch" | "graph" => {
            let trace = special.traces.get(&node.id);
            if let Some(error) = &special.watch_error {
                ui.weak(format!("Watch unavailable: {error}"));
            }
            match trace.and_then(|t| t.back()) {
                Some(value) => {
                    ui.monospace(format!(
                        "{value:.4}{}",
                        if special.running { "" } else { " (stopped)" }
                    ));
                }
                None => {
                    ui.weak(if special.running {
                        "Waiting for samples"
                    } else {
                        "Unavailable / stopped"
                    });
                }
            }
            if node.function_name == "graph" {
                scope(
                    ui,
                    trace
                        .into_iter()
                        .flat_map(|t| t.iter().copied())
                        .collect::<Vec<_>>()
                        .as_slice(),
                );
            }
        }
        "buffer" => {
            ui.label(node.buffer_name.as_deref().unwrap_or("No source"));
            if let Some(buffer) = special.buffers.get(&node.id) {
                ui.label(&buffer.status);
                if let Some(path) = &buffer.path {
                    ui.small(path.to_string_lossy());
                }
                waveform(ui, &buffer.preview);
                if let Some(path) = &buffer.path
                    && ui.button("Reload / retry").clicked()
                {
                    return Some(NodeAction::Retry(path.clone()));
                }
            }
            if ui.button("Choose WAV…").clicked() {
                return Some(NodeAction::ChooseBuffer);
            }
            if ui.button("Builtin sine").clicked() {
                return Some(NodeAction::Sine);
            }
        }
        _ => {}
    }
    None
}
fn physical_scale(ui: &Ui) -> f32 {
    let zoom = ui
        .ctx()
        .layer_transform_to_global(ui.layer_id())
        .map_or(1., |t| t.scaling.abs());
    (ui.ctx().pixels_per_point() * zoom).max(0.001)
}
fn scope(ui: &mut Ui, samples: &[f32]) {
    let columns = ((ui.available_width() * physical_scale(ui)).ceil() as usize).clamp(1, 2048);
    let chunk = samples.len().div_ceil(columns).max(1);
    let peaks = samples
        .chunks(chunk)
        .map(|c| {
            c.iter()
                .fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), v| {
                    (a.min(*v), b.max(*v))
                })
        })
        .collect::<Vec<_>>();
    waveform(ui, &peaks);
}
fn waveform(ui: &mut Ui, peaks: &[(f32, f32)]) {
    let size = egui::vec2(ui.available_width().clamp(60., 1024.), 70.);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter().with_clip_rect(ui.clip_rect().intersect(rect));
    painter.rect_filled(rect, 2., Color32::from_gray(20));
    let columns = ((rect.width() * physical_scale(ui)).ceil() as usize).clamp(1, 2048);
    let chunk = peaks.len().div_ceil(columns).max(1);
    let count = peaks.len().div_ceil(chunk).max(1);
    let mut points = Vec::with_capacity(count * 2);
    for (i, c) in peaks.chunks(chunk).enumerate() {
        let (lo, hi) = c
            .iter()
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), (lo, hi)| {
                (a.min(*lo), b.max(*hi))
            });
        let x = rect.left() + rect.width() * (i as f32 + 0.5) / count as f32;
        let y = |v: f32| rect.center().y - v.clamp(-1., 1.) * rect.height() * 0.5;
        let low = egui::pos2(x, y(lo));
        let high = egui::pos2(x, y(hi));
        points.push(low);
        if high != low {
            points.push(high);
        }
    }
    // A connected min/max path preserves peaks when dense and joins individual
    // samples when sparse, rather than leaving isolated vertical strokes/dots.
    let stroke = egui::Stroke::new(1., Color32::LIGHT_GREEN);
    if points.len() == 1 {
        painter.circle_filled(points[0], 0.5, stroke.color);
    } else if !points.is_empty() {
        painter.add(egui::Shape::line(points, stroke));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::document::{DocumentId, EditorDocument};
    #[test]
    fn toggle_render_does_not_emit_feedback_and_scopes_are_bounded() {
        let core = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../wisp/core");
        let doc=EditorDocument::from_text("[flow]\nname='render'\n[[flow.nodes]]\ntext='toggle'\nvalue=1.0\n[[flow.nodes]]\ntext='graph'\n",&core,DocumentId(1),None).unwrap();
        let mut special = SpecialSnapshot::default();
        let graph = doc.snapshot().nodes[1].0;
        crate::editor::telemetry::append(
            special.traces.entry(graph).or_default(),
            (0..10000).map(|i| (i as f32).sin()),
        );
        special.running = true;
        for dpi in [0.25, 1., 4.] {
            let ctx = egui::Context::default();
            ctx.set_pixels_per_point(dpi);
            let mut nodes = doc.snapshot().nodes;
            let mut output = ctx.run_ui(Default::default(), |ui| {
                for (_, node) in &mut nodes {
                    assert!(show_node_body(node, ui, &special).is_none());
                }
            });
            assert!(output.shapes.len() < 2200);
            output.textures_delta.clear();
            assert_eq!(nodes[0].1.control_values[&0], 1.);
        }
    }
    #[test]
    fn sparse_and_dense_waveforms_have_connected_traces() {
        for peaks in [
            vec![(0., 0.), (0.5, 0.5), (-0.5, -0.5)],
            vec![(-0.8, 0.9); 10_000],
            vec![(0.25, 0.25); 10_000],
        ] {
            let ctx = egui::Context::default();
            let mut output = ctx.run_ui(Default::default(), |ui| waveform(ui, &peaks));
            output.textures_delta.clear();
            let paths = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Path(path)
                        if path.stroke.color
                            == egui::epaint::ColorMode::Solid(Color32::LIGHT_GREEN) =>
                    {
                        Some(path)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(paths.len(), 1, "Trace must be one continuous path");
            let path = paths[0];
            assert!(path.points.len() >= 3);
            assert!(path.points.len() <= 4096);
            assert!(path.points.windows(2).all(|p| p[0].x <= p[1].x));
            if peaks.len() == 3 {
                assert_eq!(path.points.len(), 3);
                assert!(path.points[1].y < path.points[0].y);
                assert!(path.points[2].y > path.points[0].y);
            }
        }
    }

    #[test]
    fn button_pointer_release_and_focus_loss() {
        let core = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../wisp/core");
        let doc = EditorDocument::from_text(
            "[flow]\nname='button_render'\n[[flow.nodes]]\ntext='button'\n",
            &core,
            DocumentId(1),
            None,
        )
        .unwrap();
        let mut node = doc.snapshot().nodes[0].1.clone();
        let ctx = egui::Context::default();
        let mut frame = |events: Vec<egui::Event>, focused: bool| {
            let mut down = false;
            let mut position = None;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    events,
                    focused,
                    ..Default::default()
                },
                |ui| {
                    if let Some(NodeAction::Momentary(value)) =
                        show_node_body(&mut node, ui, &SpecialSnapshot::default())
                    {
                        down = value;
                    }
                },
            );
            for shape in &output.shapes {
                if let egui::Shape::Text(text) = &shape.shape
                    && text.galley.text() == "Hold"
                {
                    position = Some(text.pos + text.galley.size() * 0.5);
                }
            }
            output.textures_delta.clear();
            (down, position)
        };
        let (down, pos) = frame(vec![], true);
        assert!(!down);
        let pos = pos.unwrap();
        let pointer = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        assert!(frame(vec![egui::Event::PointerMoved(pos), pointer(true)], true).0);
        assert!(!frame(vec![pointer(false)], true).0);
        assert!(frame(vec![pointer(true)], true).0);
        assert!(!frame(vec![], false).0);
    }
}
