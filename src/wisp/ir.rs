#![allow(dead_code)]

#[derive(Debug, Hash, PartialEq, Eq, Clone, Copy)]
pub struct VarRef(pub u32);

#[derive(Debug, Hash, PartialEq, Eq, Clone, Copy)]
pub struct LocalRef(pub u32);

#[derive(Debug, Hash, PartialEq, Eq, Clone, Copy)]
pub enum GlobalRef {
    Data,
    Output,
}

#[derive(Debug, Hash, PartialEq, Eq, Clone, Copy)]
pub struct DataRef(pub u32);

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct FunctionOutputIndex(pub u32);

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct SignalOutputIndex(pub u32);

#[derive(Debug, Clone, Copy)]
pub enum Operand {
    Constant(Constant),
    Literal(f32),
    Var(VarRef),
    Arg(u32),
}

#[derive(Debug, Clone, Copy)]
pub enum Constant {
    SampleRate,
}

#[derive(Debug, Clone, Copy)]
pub enum BinaryOpType {
    Add,
    Subtract,
    Multiply,
    Divide,
}

#[derive(Debug, Clone, Copy)]
pub enum ComparisonOpType {
    Equal,
    NotEqual,
    Less,
    LessOrEqual,
    Greater,
    GreaterOrEqual,
}

#[derive(Debug, Hash, PartialEq, Eq, Clone, Copy)]
pub struct CallId(pub u32);

#[derive(Debug, Clone, Copy)]
pub enum SourceLocation {
    Local(LocalRef),
    Global(GlobalRef),
    Data(DataRef),
    LastValue(CallId, DataRef),
}

#[derive(Debug, Clone, Copy)]
pub enum TargetLocation {
    Local(LocalRef),
    Global(GlobalRef),
    Data(DataRef),
    FunctionOutput(FunctionOutputIndex),
    SignalOutput(SignalOutputIndex),
}

#[derive(Debug, Clone)]
pub enum Instruction {
    AllocLocal(LocalRef),

    Load(VarRef, SourceLocation),
    Store(TargetLocation, Operand),

    BinaryOp(VarRef, BinaryOpType, Operand, Operand),
    ComparisonOp(VarRef, ComparisonOpType, Operand, Operand),

    Conditional(VarRef, Vec<Instruction>, Vec<Instruction>),

    Call(CallId, String, Vec<Option<Operand>>, Vec<VarRef>),

    Debug(VarRef),
}
