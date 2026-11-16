//! Reproducible UI CPU benchmark; never claims GPU/presentation timings.
use std::{path::Path, time::Instant};
use wisp_gui_egui::{
    canvas::snarl_adapter::GraphCanvas,
    editor::{
        document::{DocumentId, EditorDocument},
        telemetry::SpecialSnapshot,
    },
};
fn fixture(nodes: usize, wires: usize, scopes: bool) -> String {
    let mut text = "[flow]\nname='benchmark'\n".to_owned();
    for i in 0..nodes {
        let kind = if scopes && i > 0 {
            "graph"
        } else if i < nodes / 2 {
            "control"
        } else {
            "= $0"
        };
        text += &format!(
            "[[flow.nodes]]\ntext='{kind}'\nx={}\ny={}\nw=180\nh=120\n",
            (i % 10) * 240,
            (i / 10) * 160
        );
    }
    if scopes {
        for i in 1..nodes {
            text += &format!("[[flow.edges]]\nfrom=0\nto={i}\noutput_index=0\ninput_index=0\n");
        }
    } else {
        let half = nodes / 2;
        for i in 0..wires {
            let from = i % half;
            let to = half + (i / half) % (nodes - half);
            text +=
                &format!("[[flow.edges]]\nfrom={from}\nto={to}\noutput_index=0\ninput_index=0\n");
        }
    }
    text
}
fn main() {
    let core = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "wisp/core".into());
    for (nodes, wires, scopes) in [(100, 200, false), (1000, 2000, false), (17, 16, true)] {
        let doc = EditorDocument::from_text(
            &fixture(nodes, wires, scopes),
            Path::new(&core),
            DocumentId(1),
            None,
        )
        .unwrap();
        for motion in [false, true] {
            let ctx = egui::Context::default();
            let mut canvas = GraphCanvas::default();
            canvas.sync(&doc.snapshot());
            let mut telemetry = SpecialSnapshot {
                document: Some(DocumentId(1)),
                running: scopes,
                ..Default::default()
            };
            for (id, node) in doc.snapshot().nodes {
                if node.function_name == "graph" {
                    telemetry
                        .traces
                        .insert(id, (0..4096).map(|i| (i as f32 * 0.03).sin()).collect());
                }
            }
            canvas.set_special(telemetry);
            let mut times = Vec::new();
            for frame in 0..350 {
                let events = if motion {
                    vec![
                        egui::Event::PointerMoved(egui::pos2(1200., 750.)),
                        egui::Event::MouseWheel {
                            unit: egui::MouseWheelUnit::Point,
                            phase: egui::TouchPhase::Move,
                            delta: egui::vec2(if frame % 2 == 0 { 2. } else { -2. }, 0.),
                            modifiers: Default::default(),
                        },
                    ]
                } else {
                    vec![]
                };
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280., 800.),
                    )),
                    time: Some(frame as f64 / 60.),
                    events,
                    ..Default::default()
                };
                let start = Instant::now();
                let mut output = ctx.run_ui(input, |ui| {
                    let _ = canvas.show(ui);
                });
                let elapsed = start.elapsed().as_secs_f64() * 1000.;
                output.textures_delta.clear();
                if frame >= 50 {
                    times.push(elapsed);
                }
            }
            times.sort_by(f64::total_cmp);
            println!(
                "nodes={nodes} wires={wires} scopes={scopes} wheel={motion} warmup=50 frames=300 dpi=1 size=1280x800 UI_CPU_p50_ms={:.3} UI_CPU_p95_ms={:.3}",
                times[150], times[285]
            );
        }
    }
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        for line in status
            .lines()
            .filter(|line| line.starts_with("VmRSS:") || line.starts_with("VmHWM:"))
        {
            println!("{line}");
        }
    }
}
