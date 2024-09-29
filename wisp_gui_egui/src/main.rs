mod app;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        ..Default::default()
    };
    eframe::run_native(
        "Twisted WiSP",
        options,
        Box::new(|_cc| Ok(Box::new(crate::app::TwistedWispApp {}))),
    )
}
