mod logger;
mod twisted_wisp;
mod twisted_wisp_flow;

pub use twisted_wisp::*;
pub use twisted_wisp_flow::*;

use godot::prelude::*;

struct TwistedWispExtension;

#[gdextension]
unsafe impl ExtensionLibrary for TwistedWispExtension {}
