use std::collections::HashMap;

use inkwell::values::{BasicValueEnum, FunctionValue, PointerValue};

use crate::wisp::{
    function::Function,
    ir::{LocalRef, VarRef},
};

use super::error::SignalProcessCreationError;

#[derive(Debug)]
pub(super) struct FunctionContext<'ctx, 'temp> {
    pub func: &'temp Function,
    pub function: FunctionValue<'ctx>,
    pub outputs: Vec<Option<BasicValueEnum<'ctx>>>,
    pub vars: HashMap<VarRef, BasicValueEnum<'ctx>>,
    pub locals: HashMap<LocalRef, PointerValue<'ctx>>,
}

impl<'ctx, 'temp> FunctionContext<'ctx, 'temp> {
    pub fn new(func: &'temp Function, function: FunctionValue<'ctx>, num_outputs: usize) -> Self {
        FunctionContext {
            func,
            function,
            outputs: vec![None; num_outputs],
            vars: HashMap::new(),
            locals: HashMap::new(),
        }
    }

    pub fn get_argument(
        &self,
        nth: u32,
    ) -> Result<BasicValueEnum<'ctx>, SignalProcessCreationError> {
        self.function.get_nth_param(nth).ok_or_else(|| {
            SignalProcessCreationError::CustomLogicalError(
                "Invalid number of function arguments".into(),
            )
        })
    }

    pub fn get_var(
        &self,
        vref: &VarRef,
    ) -> Result<BasicValueEnum<'ctx>, SignalProcessCreationError> {
        Ok(*self
            .vars
            .get(vref)
            .ok_or(SignalProcessCreationError::UninitializedVar(vref.0))?)
    }

    pub fn get_local(
        &self,
        lref: &LocalRef,
    ) -> Result<PointerValue<'ctx>, SignalProcessCreationError> {
        Ok(*self
            .locals
            .get(lref)
            .ok_or(SignalProcessCreationError::UninitializedLocal(lref.0))?)
    }
}
