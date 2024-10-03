use std::collections::{BTreeMap, HashMap};

use crate::{
    ir::{DataRef, IRFunction, IRFunctionDataType, Instruction, SourceLocation},
    runner::context::WispRuntimeContext,
    utils::dep_prop::DependencyHandle,
    CallIndex,
};

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)] // Only used by the JIT-compiled code
struct DataArray {
    length: u32,
    data: *mut f32,
}

#[derive(Debug, Clone, Copy)]
#[repr(transparent)]
pub struct DataArrayHandle(*mut DataArray);
// Safety is guaranteed by never removing or modifying the data array while it's still in use
unsafe impl Send for DataArrayHandle {}

impl<'a> From<&'a [f32]> for DataArrayHandle {
    fn from(value: &'a [f32]) -> Self {
        DataArrayHandle(value.as_ptr() as *mut DataArray)
    }
}

#[derive(Clone, Copy)]
#[repr(C)]
pub union DataValue {
    float: f32,
    array: DataArrayHandle,
}
unsafe impl Send for DataValue {}

impl DataValue {
    pub fn new_float(value: f32) -> DataValue {
        DataValue { float: value }
    }

    pub fn new_array(array: DataArrayHandle) -> DataValue {
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

#[derive(Debug, Default, Clone)]
pub struct FunctionDataLayout {
    pub own_data_items: BTreeMap<DataRef, DataItem>,
    pub children_data_items: BTreeMap<CallIndex, (String, u32)>,
    pub total_size: u32,
}

#[derive(Debug, Default, Clone)]
pub struct DataLayout {
    data_layout: HashMap<String, FunctionDataLayout>,
}

impl DataLayout {
    pub fn new(rctx: &WispRuntimeContext, dep_handle: DependencyHandle) -> Self {
        let mut data_layout = HashMap::new();
        let active_set = rctx.active_set().get(dep_handle.clone());
        for name in active_set.iter() {
            let func = rctx.get_function(name).unwrap();
            let func_layout = func.data_layout().get(dep_handle.clone());
            if let Some(func_layout) = func_layout.as_ref() {
                data_layout.insert(name.clone(), func_layout.clone());
            } else {
                data_layout.insert(name.clone(), FunctionDataLayout::default());
            }
        }
        DataLayout { data_layout }
    }

    pub fn calculate_function_data_layout(
        func: &IRFunction,
        rctx: &WispRuntimeContext,
        dep_handle: DependencyHandle,
    ) -> Option<FunctionDataLayout> {
        let mut children_data_sizes = BTreeMap::new();

        Self::calculate_children_data_sizes(
            func.instructions(),
            rctx,
            dep_handle,
            &mut children_data_sizes,
        );

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

    fn calculate_children_data_sizes(
        insns: &[Instruction],
        rctx: &WispRuntimeContext,
        dep_handle: DependencyHandle,
        sizes: &mut BTreeMap<CallIndex, (String, u32)>,
    ) {
        for insn in insns {
            match insn {
                Instruction::Call(id, name, _, _)
                | Instruction::Load(_, SourceLocation::LastValue(id, name, _)) => {
                    let child_data_layout = rctx.get_function(name).unwrap().data_layout();
                    assert!(child_data_layout.is_valid(), "Programmer error - wrong calculation order, data layout for {} is not valid", name);
                    if let Some(child_data_layout) =
                        child_data_layout.get(dep_handle.clone()).as_ref()
                    {
                        sizes.insert(CallIndex(id.0), (name.into(), child_data_layout.total_size));
                    }
                }
                Instruction::Conditional(_, then, else_) => {
                    Self::calculate_children_data_sizes(then, rctx, dep_handle.clone(), sizes);
                    Self::calculate_children_data_sizes(else_, rctx, dep_handle.clone(), sizes);
                }
                _ => (),
            }
        }
    }

    pub fn get(&self, name: &str) -> Option<&FunctionDataLayout> {
        self.data_layout.get(name)
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
                    DataValue::new_array(DataArrayHandle::from(&[0f32][..]))
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
