use std::collections::HashMap;

use inkwell::{
    builder::{Builder, BuilderError},
    context::Context,
    module::Module,
    types::{FloatType, IntType, PointerType, VoidType},
    AddressSpace,
};

use crate::wisp::{function::Function, runtime::Runtime};

use super::{data_layout::FunctionDataLayout, error::SignalProcessCreationError};

#[derive(Debug)]
pub(super) struct ModuleTypes<'ctx> {
    pub void: VoidType<'ctx>,
    pub i32: IntType<'ctx>,
    pub f32: FloatType<'ctx>,
    pub pf32: PointerType<'ctx>,
}

impl<'ctx> ModuleTypes<'ctx> {
    pub fn new(context: &'ctx Context) -> Self {
        ModuleTypes {
            void: context.void_type(),
            i32: context.i32_type(),
            f32: context.f32_type(),
            pf32: context.f32_type().ptr_type(AddressSpace::default()),
        }
    }
}

#[derive(Debug)]
pub(super) struct ModuleContext<'ctx, 'temp> {
    pub runtime: &'temp Runtime,
    pub types: ModuleTypes<'ctx>,
    pub module: &'temp Module<'ctx>,
    pub builder: Builder<'ctx>,
    pub data_layout: HashMap<String, FunctionDataLayout>,
}

impl<'ctx, 'temp> ModuleContext<'ctx, 'temp> {
    pub fn new(
        context: &'ctx Context,
        runtime: &'temp Runtime,
        module: &'temp Module<'ctx>,
        data_layout: HashMap<String, FunctionDataLayout>,
    ) -> Self {
        ModuleContext {
            runtime,
            types: ModuleTypes::new(context),
            module,
            builder: context.create_builder(),
            data_layout,
        }
    }

    pub fn get_function(&self, name: &str) -> Result<&'temp Function, SignalProcessCreationError> {
        self.runtime
            .get_function(name)
            .ok_or_else(|| SignalProcessCreationError::UnknownFunction(name.into()))
    }

    pub fn build<F, R>(&self, name: &str, func: F) -> Result<R, SignalProcessCreationError>
    where
        F: FnOnce(&Builder<'ctx>, &str) -> Result<R, BuilderError>,
    {
        func(&self.builder, &format!("tmp_{}", name))
            .map_err(|_| SignalProcessCreationError::BuildInstruction(name.into()))
    }
}
