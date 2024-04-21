use std::cell::{Ref, RefCell};

use petgraph::{graph::NodeIndex, stable_graph::StableGraph};

use super::{
    ir::{Instruction, VarRef},
    runtime::Runtime,
};

type FlowGraph = StableGraph<String, FlowConnection>;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct FlowFunctionIndex(NodeIndex);

#[derive(Debug, Clone, Copy)]
struct FlowConnection {
    #[allow(dead_code)]
    output_index: u32,
    #[allow(dead_code)]
    input_index: u32,
}

#[derive(Debug, Default)]
pub struct Flow {
    graph: FlowGraph,
    ir: RefCell<Option<Vec<Instruction>>>,
}

impl Flow {
    pub fn new() -> Self {
        Flow::default()
    }

    pub fn add_function(&mut self, name: String) -> FlowFunctionIndex {
        FlowFunctionIndex(self.graph.add_node(name))
    }

    pub fn connect(
        &mut self,
        from: FlowFunctionIndex,
        output_index: u32,
        to: FlowFunctionIndex,
        input_index: u32,
    ) {
        self.graph.add_edge(
            from.0,
            to.0,
            FlowConnection {
                output_index,
                input_index,
            },
        );
    }

    pub fn get_compiled_flow(&self, runtime: &Runtime) -> Ref<'_, Vec<Instruction>> {
        if self.ir.borrow().is_none() {
            let ir = self.compile(runtime);
            *self.ir.borrow_mut() = Some(ir);
        }
        Ref::map(self.ir.borrow(), |is| is.as_ref().unwrap())
    }

    fn compile(&self, _runtime: &Runtime) -> Vec<Instruction> {
        // TODO: Implement this function
        // For now, return a hard-coded flow
        vec![
            Instruction::Call("test".into(), vec![], vec![VarRef(0)]),
            Instruction::Call("out".into(), vec![VarRef(0)], vec![]),
        ]
    }
}
