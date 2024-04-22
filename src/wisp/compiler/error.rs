use thiserror::Error;

#[derive(Debug, Error)]
pub enum SignalProcessCreationError {
    #[error("Failed to initialize the evaluation engine")]
    InitEE,

    #[error("Failed to load the function")]
    LoadFunction,

    #[error("Failed to build instruction: {0}")]
    BuildInstruction(String),

    #[error("Var ref {0} is uninitialized")]
    UninitializedVar(u32),

    #[error("Local ref {0} is uninitialized")]
    UninitializedLocal(u32),

    #[error("Function {0} is not found")]
    UnknownFunction(String),

    #[error("Invalid number of arguments for function {0}: expected {1}, found {2}")]
    InvalidNumberOfInputs(String, u32, u32),

    #[error("Required input {1} for function {0} was not initialized")]
    UninitializedInput(String, u32),

    #[error("Invalid number of outputs for function {0}: expected at most {1}, found {2}")]
    InvalidNumberOfOutputs(String, u32, u32),

    #[error("Output {1} for function {0} was not initialized")]
    UninitializedOutput(String, u32),

    #[error("Invalid data layout for function {0}")]
    InvalidDataLayout(String),

    #[error("Logical error: {0}")]
    CustomLogicalError(String),
}
