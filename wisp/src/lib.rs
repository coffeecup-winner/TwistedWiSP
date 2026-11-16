mod audio;
mod compiler;
pub mod core;
mod ir;
mod midi;
mod runner;
mod utils;

pub use runner::engine::*;

mod offline;
pub use compiler::{SignalProcessCreationError, SignalProcessor};
pub use offline::OfflineSession;

pub mod buffer;
pub use compiler::DataArrayHandle;

pub use midi::mailbox::{MidiMailbox, MidiObservation};

#[cfg(test)]
mod allocation_probe;
