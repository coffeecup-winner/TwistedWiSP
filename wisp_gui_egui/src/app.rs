pub struct TwistedWispApp {}

impl eframe::App for TwistedWispApp {
    fn update(&mut self, ctx: &eframe::egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.label("Top panel");
        });
    }
}
