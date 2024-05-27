use serde::{Deserialize, Serialize};

use crate::Instruction;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum IRFunctionDataType {
    Float,
    Array,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct IRFunctionInput {
    pub type_: IRFunctionDataType,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct IRFunctionOutput {
    pub type_: IRFunctionDataType,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct IRFunctionDataItem {
    pub type_: IRFunctionDataType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IRFunction {
    pub name: String,
    pub inputs: Vec<IRFunctionInput>,
    pub outputs: Vec<IRFunctionOutput>,
    pub data: Vec<IRFunctionDataItem>,
    pub ir: Vec<Instruction>,
}

impl IRFunction {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn inputs(&self) -> &[IRFunctionInput] {
        &self.inputs
    }

    pub fn outputs(&self) -> &[IRFunctionOutput] {
        &self.outputs
    }

    pub fn data(&self) -> &[IRFunctionDataItem] {
        &self.data
    }

    pub fn instructions(&self) -> &[Instruction] {
        &self.ir
    }
}
