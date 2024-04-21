#![allow(dead_code)]

#[derive(Debug, Hash, PartialEq, Eq, Clone, Copy)]
pub struct VarRef(pub u32);

#[derive(Debug, Hash, PartialEq, Eq, Clone, Copy)]
pub struct LocalRef(pub u32);

#[derive(Debug, Hash, PartialEq, Eq, Clone, Copy)]
pub enum GlobalRef {
    Output,
    Prev,
    Next,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct FunctionInputIndex(pub u32);

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct FunctionOutputIndex(pub u32);

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct OutputIndex(pub u32);

#[derive(Debug, Clone, Copy)]
pub enum Operand {
    Constant(Constant),
    Literal(f32),
    Var(VarRef),
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

#[derive(Debug, Clone)]
pub enum Instruction {
    LoadPrev(VarRef),
    StoreNext(VarRef),

    AllocLocal(LocalRef),
    LoadLocal(VarRef, LocalRef),
    StoreLocal(LocalRef, VarRef),

    LoadGlobal(VarRef, GlobalRef),
    StoreGlobal(GlobalRef, VarRef),

    LoadFunctionInput(VarRef, FunctionInputIndex),
    StoreFunctionOutput(FunctionOutputIndex, VarRef),

    BinaryOp(VarRef, BinaryOpType, Operand, Operand),
    ComparisonOp(VarRef, ComparisonOpType, Operand, Operand),

    Conditional(VarRef, Vec<Instruction>, Vec<Instruction>),

    Call(String, Vec<VarRef>, Vec<VarRef>),

    Output(OutputIndex, VarRef),
    Debug(VarRef),
}
