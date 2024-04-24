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
    pub data_arg: Option<PointerValue<'ctx>>,
    pub outputs: Vec<Option<BasicValueEnum<'ctx>>>,
    pub vars: HashMap<VarRef, BasicValueEnum<'ctx>>,
    pub locals: HashMap<LocalRef, PointerValue<'ctx>>,
}

impl<'ctx, 'temp> FunctionContext<'ctx, 'temp> {
    pub fn new(
        func: &'temp Function,
        function: FunctionValue<'ctx>,
        data_arg: Option<PointerValue<'ctx>>,
        num_outputs: usize,
    ) -> Self {
        FunctionContext {
            func,
            function,
            data_arg,
            outputs: vec![None; num_outputs],
            vars: HashMap::new(),
            locals: HashMap::new(),
        }
    }

    pub fn get_data_argument(&self) -> Result<PointerValue<'ctx>, SignalProcessCreationError> {
        self.data_arg
            .ok_or_else(|| SignalProcessCreationError::InvalidDataLayout(self.func.name().into()))
    }

    pub fn get_argument(
        &self,
        nth: u32,
    ) -> Result<BasicValueEnum<'ctx>, SignalProcessCreationError> {
        let idx = if self.data_arg.is_some() {
            nth + 1
        } else {
            nth
        };
        self.function.get_nth_param(idx).ok_or_else(|| {
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
