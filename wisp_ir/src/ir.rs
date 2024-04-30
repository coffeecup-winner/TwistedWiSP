use serde::{Deserialize, Serialize};

#[derive(Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Serialize, Deserialize)]
pub struct VarRef(pub u32);

#[derive(Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Serialize, Deserialize)]
pub struct LocalRef(pub u32);

#[derive(Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Serialize, Deserialize)]
pub struct DataRef(pub u32);

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Serialize, Deserialize)]
pub struct FunctionOutputIndex(pub u32);

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Serialize, Deserialize)]
pub struct SignalOutputIndex(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Operand {
    Constant(Constant),
    Literal(f32),
    Var(VarRef),
    Arg(u32),
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize)]
pub enum Constant {
    SampleRate,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize)]
pub enum BinaryOpType {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize)]
pub enum ComparisonOpType {
    Equal,
    NotEqual,
    Less,
    LessOrEqual,
    Greater,
    GreaterOrEqual,
}

#[derive(Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Serialize, Deserialize)]
pub struct CallId(pub u32);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SourceLocation {
    Local(LocalRef),
    Data(DataRef),
    LastValue(CallId, String, DataRef),
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum TargetLocation {
    Local(LocalRef),
    Data(DataRef),
    FunctionOutput(FunctionOutputIndex),
    SignalOutput(SignalOutputIndex),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Instruction {
    AllocLocal(LocalRef),

    Load(VarRef, SourceLocation),
    Store(TargetLocation, Operand),

    BinaryOp(VarRef, BinaryOpType, Operand, Operand),
    ComparisonOp(VarRef, ComparisonOpType, Operand, Operand),

    Conditional(VarRef, Vec<Instruction>, Vec<Instruction>),

    Call(CallId, String, Vec<Operand>, Vec<VarRef>),

    Debug(VarRef),
}
