// SPDX-License-Identifier: MIT OR Apache-2.0
use clap::Parser;
use wisp_gui_egui::app::{Options, WispGuiApp};
fn main() -> eframe::Result {
    let options = Options::parse();
    let _ = env_logger::Builder::from_env(
        env_logger::Env::default().filter_or("TWISTED_WISP_LOG", "info"),
    )
    .try_init();
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([400.0, 300.0])
            .with_title("TwistedWiSP Editor"),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    eframe::run_native(
        "TwistedWiSP Editor",
        native_options,
        Box::new(move |cc| Ok(Box::new(WispGuiApp::new(cc, options)))),
    )
}
