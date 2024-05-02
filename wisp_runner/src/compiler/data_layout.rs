use std::collections::{BTreeMap, HashMap, HashSet};

use twisted_wisp_ir::{CallId, DataRef, IRFunction, Instruction, SourceLocation};

use crate::context::WispContext;

#[derive(Debug)]
pub struct FunctionDataLayout {
    pub own_data_offsets: BTreeMap<DataRef, u32>,
    pub children_data_offsets: BTreeMap<CallId, (String, u32)>,
    pub total_size: u32,
}

pub fn calculate_data_layout(
    top_level_func: &IRFunction,
    wctx: &WispContext,
) -> (HashMap<String, FunctionDataLayout>, HashSet<String>) {
    let mut data_layout = HashMap::new();
    let mut called_functions = HashSet::new();
    if let Some(function_data_layout) = calculate_function_data_layout(
        top_level_func,
        wctx,
        &mut data_layout,
        &mut called_functions,
    ) {
        data_layout.insert(top_level_func.name().into(), function_data_layout);
        called_functions.insert(top_level_func.name().into());
    }
    (data_layout, called_functions)
}

fn calculate_function_data_layout(
    func: &IRFunction,
    wctx: &WispContext,
    data_layout: &mut HashMap<String, FunctionDataLayout>,
    called_functions: &mut HashSet<String>,
) -> Option<FunctionDataLayout> {
    let mut children_data_sizes = BTreeMap::new();
    for insn in func.instructions().iter() {
        match insn {
            Instruction::Call(id, name, _, _)
            | Instruction::Load(_, SourceLocation::LastValue(id, name, _)) => {
                called_functions.insert(name.into());
                if let Some(child_data_layout) = data_layout.get(name) {
                    children_data_sizes.insert(*id, (name.into(), child_data_layout.total_size));
                } else if let Some(child_data_layout) = calculate_function_data_layout(
                    wctx.get_function(name).unwrap(),
                    wctx,
                    data_layout,
                    called_functions,
                ) {
                    children_data_sizes.insert(*id, (name.into(), child_data_layout.total_size));
                    data_layout.insert(name.into(), child_data_layout);
                }
            }
            _ => (),
        }
    }

    let mut own_data_offsets = BTreeMap::new();
    let mut total_size = 0;
    for (idx, _) in func.data().iter().enumerate() {
        own_data_offsets.insert(DataRef(idx as u32), total_size);
        // All data is f32 at the moment, size is in elements
        total_size += 1;
    }

    if total_size == 0 && children_data_sizes.is_empty() {
        return None;
    }

    let mut children_data_offsets = BTreeMap::new();
    for (id, (name, size)) in children_data_sizes {
        children_data_offsets.insert(id, (name, total_size));
        total_size += size;
    }

    Some(FunctionDataLayout {
        own_data_offsets,
        children_data_offsets,
        total_size,
    })
}
