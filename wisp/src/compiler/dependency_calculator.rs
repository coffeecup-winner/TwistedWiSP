use std::collections::BTreeSet;

use crate::{
    ir::{IRFunction, Instruction, SourceLocation},
    runner::context::WispRuntimeContext,
    utils::dep_prop::DependencyHandle,
};

pub fn calculate_dependencies(ir_func: &IRFunction) -> BTreeSet<String> {
    let mut dependencies = BTreeSet::new();
    calculate_dependencies_core(&ir_func.ir, &mut dependencies);
    dependencies
}

fn calculate_dependencies_core(ir: &[Instruction], dependencies: &mut BTreeSet<String>) {
    for instr in ir {
        match instr {
            Instruction::Call(_, name, ..)
            | Instruction::Load(_, SourceLocation::LastValue(_, name, _)) => {
                dependencies.insert(name.clone());
            }
            Instruction::Conditional(_, true_branch, false_branch) => {
                calculate_dependencies_core(true_branch, dependencies);
                calculate_dependencies_core(false_branch, dependencies);
            }
            _ => {}
        }
    }
}

pub fn calculate_active_set(
    rctx: &WispRuntimeContext,
    top_level: &str,
    dep_handle: DependencyHandle,
) -> Result<Vec<String>, super::SignalProcessCreationError> {
    fn visit(
        rctx: &WispRuntimeContext,
        name: &str,
        handle: DependencyHandle,
        visiting: &mut BTreeSet<String>,
        done: &mut BTreeSet<String>,
        result: &mut Vec<String>,
    ) -> Result<(), super::SignalProcessCreationError> {
        if done.contains(name) {
            return Ok(());
        }
        if !visiting.insert(name.to_owned()) {
            return Err(super::SignalProcessCreationError::CustomLogicalError(
                format!("Recursive dependency: {name}"),
            ));
        }
        let f = rctx
            .get_function(name)
            .ok_or_else(|| super::SignalProcessCreationError::UnknownFunction(name.into()))?;
        for child in f.dependencies().get(handle.clone()).iter() {
            visit(rctx, child, handle.clone(), visiting, done, result)?;
        }
        visiting.remove(name);
        done.insert(name.into());
        result.push(name.into());
        Ok(())
    }
    let mut result = vec![];
    visit(
        rctx,
        top_level,
        dep_handle,
        &mut BTreeSet::new(),
        &mut BTreeSet::new(),
        &mut result,
    )?;
    result.reverse(); // builder walks in reverse to calculate children first
    Ok(result)
}
