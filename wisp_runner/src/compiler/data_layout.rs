use std::collections::{BTreeMap, HashMap, HashSet};

use twisted_wisp_ir::{
    CallId, DataRef, IRFunction, IRFunctionDataType, Instruction, SourceLocation,
};

use crate::context::WispContext;

#[derive(Debug, Clone, Copy)]
pub struct DataArray {
    pub length: u32,
    pub data: *mut f32,
}

#[derive(Clone, Copy)]
#[repr(C)]
pub union DataValue {
    float: f32,
    array: *mut DataArray,
}
unsafe impl Send for DataValue {}

impl DataValue {
    pub fn new_float(value: f32) -> DataValue {
        DataValue { float: value }
    }

    pub fn new_array(array: *mut DataArray) -> DataValue {
        DataValue { array }
    }

    pub fn as_float(&self) -> f32 {
        unsafe { self.float }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DataItem {
    pub offset: u32,
    pub type_: IRFunctionDataType,
}

#[derive(Debug, Default)]
pub struct FunctionDataLayout {
    pub own_data_items: BTreeMap<DataRef, DataItem>,
    pub children_data_items: BTreeMap<CallId, (String, u32)>,
    pub total_size: u32,
}

#[derive(Debug)]
pub struct DataLayout {
    data_layout: HashMap<String, FunctionDataLayout>,
    called_functions: HashSet<String>,
}

impl DataLayout {
    pub fn calculate(top_level_func: &IRFunction, wctx: &WispContext) -> Self {
        let mut data_layout = HashMap::new();
        let mut called_functions = HashSet::new();
        if let Some(function_data_layout) = Self::calculate_function_data_layout(
            top_level_func,
            wctx,
            &mut data_layout,
            &mut called_functions,
        ) {
            data_layout.insert(top_level_func.name().into(), function_data_layout);
            called_functions.insert(top_level_func.name().into());
        } else {
            data_layout.insert(top_level_func.name().into(), FunctionDataLayout::default());
            called_functions.insert(top_level_func.name().into());
        }
        DataLayout {
            data_layout,
            called_functions,
        }
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
                        children_data_sizes
                            .insert(*id, (name.into(), child_data_layout.total_size));
                    } else if let Some(child_data_layout) = Self::calculate_function_data_layout(
                        wctx.get_function(name).unwrap(),
                        wctx,
                        data_layout,
                        called_functions,
                    ) {
                        children_data_sizes
                            .insert(*id, (name.into(), child_data_layout.total_size));
                        data_layout.insert(name.into(), child_data_layout);
                    }
                }
                _ => (),
            }
        }

        let mut own_data_offsets = BTreeMap::new();
        let mut total_size = 0;
        for (idx, d) in func.data().iter().enumerate() {
            own_data_offsets.insert(
                DataRef(idx as u32),
                DataItem {
                    offset: total_size,
                    type_: d.type_,
                },
            );
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
            own_data_items: own_data_offsets,
            children_data_items: children_data_offsets,
            total_size,
        })
    }

    pub fn get(&self, name: &str) -> Option<&FunctionDataLayout> {
        self.data_layout.get(name)
    }

    pub fn was_called(&self, name: &str) -> bool {
        self.called_functions.contains(name)
    }

    pub fn create_data(&self, name: &str) -> Vec<DataValue> {
        if let Some(layout) = self.data_layout.get(name) {
            let mut data = vec![DataValue::new_float(0.0); layout.total_size as usize];
            self.create_data_recursive(&mut data, layout);
            data
        } else {
            vec![]
        }
    }

    fn create_data_recursive(&self, data: &mut [DataValue], layout: &FunctionDataLayout) {
        let mut offset = 0;
        for (_, item) in layout.own_data_items.iter() {
            data[item.offset as usize] = match item.type_ {
                IRFunctionDataType::Float => DataValue::new_float(0.0),
                IRFunctionDataType::Array => {
                    DataValue::new_array([0u32].as_ptr() as *mut DataArray)
                }
            };
        }
        offset += layout.own_data_items.len() as u32;
        for (_, (name, _)) in layout.children_data_items.iter() {
            if let Some(layout) = self.data_layout.get(name).as_ref() {
                self.create_data_recursive(&mut data[offset as usize..], layout);
                offset += layout.total_size;
            }
        }
    }
}
