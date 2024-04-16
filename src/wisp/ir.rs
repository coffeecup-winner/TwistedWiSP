#![allow(dead_code)]

#[derive(Debug, Hash, PartialEq, Eq)]
pub struct VarRef(pub u32);

#[derive(Debug, PartialEq, Eq)]
pub struct OutputIndex(pub u32);

#[derive(Debug)]
pub enum Operand {
    Constant(f32),
    Var(VarRef),
}

#[derive(Debug)]
pub enum BinaryOpType {
    Add,
    Subtract,
    Multiply,
    Divide,
}

#[derive(Debug)]
pub enum Instruction {
    LoadPrev(VarRef),
    StoreNext(VarRef),
    BinaryOp(VarRef, BinaryOpType, Operand, Operand),
    Output(OutputIndex, VarRef),
}
