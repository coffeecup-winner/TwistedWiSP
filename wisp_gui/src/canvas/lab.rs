// SPDX-License-Identifier: MIT OR Apache-2.0
//! Synthetic content only: no runtime watches or document mutations.
use egui::{Color32, Vec2};
use egui_snarl::{
    InPin, InPinId, NodeId, OutPin, OutPinId, Snarl,
    ui::{PinInfo, SnarlStyle, SnarlViewer, SnarlWidget},
};

#[derive(Clone, Copy)]
enum LabNode {
    Waveform,
    Text,
    Controls,
    Nested,
    Child,
}
struct Content {
    samples: [f32; 2048],
    head: usize,
    tick: u64,
    text: String,
    name: String,
    value: f32,
    toggle: bool,
    pressed: bool,
    width: f32,
    child: Snarl<LabNode>,
    child_rect: egui::Rect,
    child_transform: egui::emath::TSTransform,
    parent_transform: egui::emath::TSTransform,
    interactive_child: bool,
}
pub struct CanvasLab {
    graph: Snarl<LabNode>,
    content: Content,
    scale: Option<f32>,
}
impl Default for CanvasLab {
    fn default() -> Self {
        let mut graph = Snarl::new();
        for (kind, pos) in [
            (LabNode::Waveform, (0., 0.)),
            (LabNode::Text, (430., 0.)),
            (LabNode::Controls, (0., 300.)),
            (LabNode::Nested, (430., 300.)),
        ] {
            graph.insert_node(egui::pos2(pos.0, pos.1), kind);
        }
        let mut child = Snarl::new();
        let a = child.insert_node(egui::pos2(0., 0.), LabNode::Child);
        let b = child.insert_node(egui::pos2(180., 0.), LabNode::Child);
        child.connect(
            OutPinId { node: a, output: 0 },
            InPinId { node: b, input: 0 },
        );
        Self {
            graph,
            scale: None,
            content: Content {
                samples: std::array::from_fn(|i| signal(i as u64)),
                head: 0,
                tick: 0,
                text: "$0 * 440.0\n+ sin(time)".into(),
                name: "Synthetic control".into(),
                value: 0.5,
                toggle: false,
                pressed: false,
                width: 256.,
                child,
                child_rect: egui::Rect::NOTHING,
                child_transform: Default::default(),
                parent_transform: Default::default(),
                interactive_child: false,
            },
        }
    }
}
fn signal(i: u64) -> f32 {
    let x = i as f32 * 0.035;
    x.sin() * 0.65 + (x * 3.17).sin() * 0.2
}
impl CanvasLab {
    pub fn show(&mut self, ui: &mut egui::Ui) {
        ui.label("Synthetic content · vanilla egui-snarl · edits stay in this lab");
        ui.horizontal(|ui| {
            ui.label("Test scale:");
            for scale in [0.25, 0.5, 1., 2.] {
                if ui.button(format!("{scale}×")).clicked() {
                    self.scale = Some(scale);
                }
            }
            if ui.button("Free zoom").clicked() {
                self.scale = None;
            }
        });
        let tick = (ui.input(|i| i.time) * 30.).max(0.) as u64;
        if tick != self.content.tick {
            // Bounded update, even after the lab has been hidden for a long time.
            let count = (tick.saturating_sub(self.content.tick) * 32).min(2048);
            for n in 0..count {
                self.content.samples[self.content.head] = signal(tick * 32 + n);
                self.content.head = (self.content.head + 1) % 2048;
            }
            self.content.tick = tick;
        }
        let mut viewer = LabViewer {
            content: &mut self.content,
        };
        let style = SnarlStyle {
            min_scale: self.scale.or(Some(0.25)),
            max_scale: self.scale.or(Some(2.)),
            ..Default::default()
        };
        SnarlWidget::new()
            .id_salt("custom_content_lab")
            .style(style)
            .show(&mut self.graph, &mut viewer, ui);
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(33));
    }
}
struct LabViewer<'a> {
    content: &'a mut Content,
}
impl SnarlViewer<LabNode> for LabViewer<'_> {
    fn current_transform(
        &mut self,
        transform: &mut egui::emath::TSTransform,
        _: &mut Snarl<LabNode>,
    ) {
        self.content.parent_transform = *transform;
    }
    fn title(&mut self, node: &LabNode) -> String {
        match node {
            LabNode::Waveform => "Synthetic waveform",
            LabNode::Text => "Highlighted editor",
            LabNode::Controls => "Mixed controls",
            LabNode::Nested => "Child graph",
            LabNode::Child => "Child",
        }
        .into()
    }
    fn inputs(&mut self, _: &LabNode) -> usize {
        1
    }
    fn outputs(&mut self, _: &LabNode) -> usize {
        1
    }
    fn show_input(
        &mut self,
        _: &InPin,
        ui: &mut egui::Ui,
        _: &mut Snarl<LabNode>,
    ) -> impl egui_snarl::ui::SnarlPin + 'static {
        ui.label("in");
        PinInfo::circle()
    }
    fn show_output(
        &mut self,
        _: &OutPin,
        ui: &mut egui::Ui,
        _: &mut Snarl<LabNode>,
    ) -> impl egui_snarl::ui::SnarlPin + 'static {
        ui.label("out");
        PinInfo::circle()
    }
    fn has_body(&mut self, _: &LabNode) -> bool {
        true
    }
    fn show_header(
        &mut self,
        node: NodeId,
        _: &[InPin],
        _: &[OutPin],
        ui: &mut egui::Ui,
        snarl: &mut Snarl<LabNode>,
    ) {
        ui.colored_label(Color32::LIGHT_BLUE, self.title(&snarl[node]));
    }
    fn has_footer(&mut self, _: &LabNode) -> bool {
        true
    }
    fn show_footer(
        &mut self,
        _: NodeId,
        _: &[InPin],
        _: &[OutPin],
        ui: &mut egui::Ui,
        _: &mut Snarl<LabNode>,
    ) {
        ui.small("Application-owned node content");
    }
    fn show_body(
        &mut self,
        node: NodeId,
        _: &[InPin],
        _: &[OutPin],
        ui: &mut egui::Ui,
        snarl: &mut Snarl<LabNode>,
    ) {
        ui.push_id(node.0, |ui| match snarl[node] {
            LabNode::Waveform => {
                ui.add(egui::Slider::new(&mut self.content.width, 160.0..=400.0).text("width"));
                let (rect, _) = ui
                    .allocate_exact_size(Vec2::new(self.content.width, 96.), egui::Sense::hover());
                let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
                painter.line_segment(
                    [
                        egui::pos2(rect.left(), rect.center().y),
                        egui::pos2(rect.right(), rect.center().y),
                    ],
                    egui::Stroke::new(1., Color32::DARK_GRAY),
                );
                let points = (0..256)
                    .map(|i| {
                        egui::pos2(
                            rect.left() + i as f32 / 255. * rect.width(),
                            rect.center().y
                                - self.content.samples[(self.content.head + i * 8) % 2048] * 40.,
                        )
                    })
                    .collect();
                painter.add(egui::Shape::line(
                    points,
                    egui::Stroke::new(1., Color32::LIGHT_GREEN),
                ));
            }
            LabNode::Text => {
                let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, width: f32| {
                    let mut job = highlight(text.as_str());
                    job.wrap.max_width = width;
                    ui.fonts_mut(|fonts| fonts.layout_job(job))
                };
                ui.add(
                    egui::TextEdit::multiline(&mut self.content.text)
                        .id_salt("expression")
                        .desired_width(250.)
                        .desired_rows(4)
                        .layouter(&mut layouter),
                );
            }
            LabNode::Controls => {
                ui.add(egui::Slider::new(&mut self.content.value, 0.0..=1.0).text("value"));
                ui.checkbox(&mut self.content.toggle, "Toggle");
                self.content.pressed = ui.button("Hold").is_pointer_button_down_on();
                ui.label(format!("Momentary: {}", self.content.pressed));
                ui.add(
                    egui::TextEdit::singleline(&mut self.content.name)
                        .id_salt("name")
                        .desired_width(180.),
                );
            }
            LabNode::Nested => {
                ui.label("Read-only child preview · interactive nesting unavailable");
                let (rect, _) = ui.allocate_exact_size(Vec2::new(320., 200.), egui::Sense::hover());
                self.content.child_rect = ui
                    .ctx()
                    .layer_transform_to_global(ui.layer_id())
                    .unwrap_or_default()
                    * rect;
                let mut child =
                    ui.new_child(egui::UiBuilder::new().id_salt("nested_body").max_rect(rect));
                child.set_clip_rect(rect.intersect(ui.clip_rect()));
                if !self.content.interactive_child {
                    child.disable();
                }
                let mut viewer = ChildViewer {
                    transform: &mut self.content.child_transform,
                    interactive: self.content.interactive_child,
                };
                SnarlWidget::new().id_salt("inline_child").show(
                    &mut self.content.child,
                    &mut viewer,
                    &mut child,
                );
            }
            LabNode::Child => {
                ui.label("Child content");
            }
        });
    }
    fn connect(&mut self, _: &OutPin, _: &InPin, _: &mut Snarl<LabNode>) {}
    fn disconnect(&mut self, _: &OutPin, _: &InPin, _: &mut Snarl<LabNode>) {}
    fn drop_inputs(&mut self, _: &InPin, _: &mut Snarl<LabNode>) {}
    fn drop_outputs(&mut self, _: &OutPin, _: &mut Snarl<LabNode>) {}
}
struct ChildViewer<'a> {
    transform: &'a mut egui::emath::TSTransform,
    interactive: bool,
}
impl SnarlViewer<LabNode> for ChildViewer<'_> {
    fn current_transform(
        &mut self,
        transform: &mut egui::emath::TSTransform,
        _: &mut Snarl<LabNode>,
    ) {
        if self.interactive
            || (self.transform.scaling == 1.0 && self.transform.translation == egui::Vec2::ZERO)
        {
            *self.transform = *transform;
        } else {
            *transform = *self.transform;
        }
    }
    fn title(&mut self, _: &LabNode) -> String {
        "Child".into()
    }
    fn inputs(&mut self, _: &LabNode) -> usize {
        1
    }
    fn outputs(&mut self, _: &LabNode) -> usize {
        1
    }
    fn show_input(
        &mut self,
        _: &InPin,
        ui: &mut egui::Ui,
        _: &mut Snarl<LabNode>,
    ) -> impl egui_snarl::ui::SnarlPin + 'static {
        ui.label("in");
        PinInfo::circle()
    }
    fn show_output(
        &mut self,
        _: &OutPin,
        ui: &mut egui::Ui,
        _: &mut Snarl<LabNode>,
    ) -> impl egui_snarl::ui::SnarlPin + 'static {
        ui.label("out");
        PinInfo::circle()
    }
    fn connect(&mut self, _: &OutPin, _: &InPin, _: &mut Snarl<LabNode>) {}
    fn disconnect(&mut self, _: &OutPin, _: &InPin, _: &mut Snarl<LabNode>) {}
    fn drop_inputs(&mut self, _: &InPin, _: &mut Snarl<LabNode>) {}
    fn drop_outputs(&mut self, _: &OutPin, _: &mut Snarl<LabNode>) {}
}
fn highlight(text: &str) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    for token in text.split_inclusive(|c: char| c.is_whitespace() || "+-*/()".contains(c)) {
        let color = if token.trim().parse::<f32>().is_ok() {
            Color32::LIGHT_BLUE
        } else if token.contains(['+', '-', '*', '/']) {
            Color32::LIGHT_YELLOW
        } else {
            Color32::LIGHT_GREEN
        };
        job.append(
            token,
            0.,
            egui::TextFormat {
                font_id: egui::FontId::monospace(14.),
                color,
                ..Default::default()
            },
        );
    }
    job
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_input_routing_reproduction() {
        let ctx = egui::Context::default();
        let mut lab = CanvasLab {
            scale: Some(1.),
            ..Default::default()
        };
        lab.content.interactive_child = true;
        let frame = |lab: &mut CanvasLab, events| {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(1800., 1400.),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| lab.show(ui),
            );
            out.textures_delta.clear();
        };
        for _ in 0..4 {
            frame(&mut lab, vec![]);
        }
        let position = lab.content.child_rect.center();
        frame(&mut lab, vec![egui::Event::PointerMoved(position)]);
        let parent = lab.content.parent_transform;
        let child = lab.content.child_transform;
        frame(
            &mut lab,
            vec![egui::Event::MouseWheel {
                phase: egui::TouchPhase::Move,
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0., 80.),
                modifiers: Default::default(),
            }],
        );
        for _ in 0..5 {
            frame(&mut lab, vec![]);
        }
        // Vanilla nested Scene routes this child's wheel to the parent. Keep production lab read-only.
        assert_ne!(parent.translation, lab.content.parent_transform.translation);
        assert_eq!(child, lab.content.child_transform);
        assert!(lab.content.child_rect.is_finite());
    }
    #[test]
    fn custom_bodies_render_at_zoom_and_display_scales() {
        for dpi in [1., 2.] {
            for scale in [0.25, 0.5, 1., 2.] {
                let ctx = egui::Context::default();
                ctx.set_pixels_per_point(dpi);
                let mut lab = CanvasLab {
                    scale: Some(scale),
                    ..Default::default()
                };
                for _ in 0..3 {
                    let mut output = ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                Vec2::new(1800., 1400.),
                            )),
                            ..Default::default()
                        },
                        |ui| lab.show(ui),
                    );
                    output.textures_delta.clear();
                }
                assert_eq!(lab.content.child.wires().count(), 1);
            }
        }
    }
}
