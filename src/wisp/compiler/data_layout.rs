use std::collections::{BTreeMap, HashMap};

use crate::wisp::{
    function::Function,
    ir::{CallId, DataRef, Instruction},
    runtime::Runtime,
};

#[derive(Debug)]
pub struct FunctionDataLayout {
    pub own_data_offsets: BTreeMap<DataRef, u32>,
    pub children_data_offsets: BTreeMap<CallId, u32>,
    pub total_size: u32,
}

pub fn calculate_data_layout(
    top_level_func: &Function,
    runtime: &Runtime,
) -> HashMap<String, FunctionDataLayout> {
    let mut data_layout = HashMap::new();
    if let Some(function_data_layout) =
        calculate_function_data_layout(top_level_func, runtime, &mut data_layout)
    {
        data_layout.insert(top_level_func.name().into(), function_data_layout);
    }
    data_layout
}

fn calculate_function_data_layout(
    func: &Function,
    runtime: &Runtime,
    data_layout: &mut HashMap<String, FunctionDataLayout>,
) -> Option<FunctionDataLayout> {
    let mut children_data_sizes = BTreeMap::new();
    for insn in func.instructions() {
        if let Instruction::Call(id, name, _, _) = insn {
            if let Some(child_data_layout) = data_layout.get(name) {
                children_data_sizes.insert(id, child_data_layout.total_size);
            } else if let Some(child_data_layout) = calculate_function_data_layout(
                runtime.get_function(name).unwrap(),
                runtime,
                data_layout,
            ) {
                children_data_sizes.insert(id, child_data_layout.total_size);
                data_layout.insert(name.into(), child_data_layout);
            }
        }
    }

    let mut own_data_offsets = BTreeMap::new();
    let mut total_size = 0;
    for (idx, _) in func.data().iter().enumerate() {
        own_data_offsets.insert(DataRef(idx as u32), total_size);
        // All data is f32 at the moment
        total_size += 4;
    }

    if total_size == 0 && children_data_sizes.is_empty() {
        return None;
    }

    let mut children_data_offsets = BTreeMap::new();
    for (id, size) in children_data_sizes {
        children_data_offsets.insert(*id, total_size);
        total_size += size;
    }

    Some(FunctionDataLayout {
        own_data_offsets,
        children_data_offsets,
        total_size,
    })
}
