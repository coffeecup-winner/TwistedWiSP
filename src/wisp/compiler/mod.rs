mod builder;
mod error;
mod function_context;
mod module_context;
mod processor;

pub use builder::SignalProcessorBuilder;
pub use error::SignalProcessCreationError;
pub use processor::SignalProcessor;
