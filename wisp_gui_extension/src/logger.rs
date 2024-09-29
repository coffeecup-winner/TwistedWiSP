use godot::global::{godot_error, godot_print, godot_warn};
use log::Log;

pub struct GodotLogger;

impl GodotLogger {
    pub fn init() -> std::result::Result<(), log::SetLoggerError> {
        log::set_max_level(log::LevelFilter::Debug);
        log::set_boxed_logger(Box::new(GodotLogger))
    }
}

impl Log for GodotLogger {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        match record.level() {
            log::Level::Error => godot_error!("{}", record.args()),
            log::Level::Warn => godot_warn!("{}", record.args()),
            log::Level::Info | log::Level::Debug | log::Level::Trace => {
                godot_print!("{}", record.args())
            }
        }
    }

    fn flush(&self) {}
}
