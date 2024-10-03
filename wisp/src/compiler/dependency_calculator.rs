use std::collections::BTreeSet;

use crate::{
    ir::{IRFunction, Instruction},
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
            Instruction::Call(_, name, ..) => {
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
    rtcx: &WispRuntimeContext,
    top_level: &str,
    dep_handle: Option<DependencyHandle>,
) -> Vec<String> {
    let mut visited = BTreeSet::new();
    let mut stack = Vec::new();
    let mut result = Vec::new();

    stack.push(top_level.to_owned());

    while let Some(name) = stack.pop() {
        if visited.contains(&name) {
            continue;
        }
        visited.insert(name.clone());

        let func = rtcx.get_function(&name).unwrap();
        let dependencies = func.dependencies().get(dep_handle.clone());
        for dependency in dependencies.iter() {
            stack.push(dependency.clone());
        }

        result.push(name);
    }

    result
}
