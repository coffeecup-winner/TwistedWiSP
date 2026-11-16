mod builder;
mod data_layout;
mod dependency_calculator;
mod error;
mod function_context;
mod module_context;
mod processor;

pub use builder::SignalProcessorBuilder;
pub use data_layout::DataArrayHandle;
pub use error::SignalProcessCreationError;
pub use processor::SignalProcessor;
