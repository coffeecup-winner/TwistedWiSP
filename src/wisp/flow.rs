use std::{
    cell::{Ref, RefCell},
    collections::HashMap,
};

use petgraph::{
    graph::NodeIndex,
    stable_graph::StableGraph,
    visit::{DfsPostOrder, EdgeRef, Reversed, Walker},
    Directed, Direction,
};

use super::{
    ir::{Instruction, Operand, VarRef},
    runtime::Runtime,
};

type FlowGraph = StableGraph<String, FlowConnection, Directed>;

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

    fn compile(&self, runtime: &Runtime) -> Vec<Instruction> {
        // This function walks the graph from `out` to the nodes leading into it
        // in post order, so all producing nodes are visited before all consuming
        // nodes. This allows compiling the signal flow as a series of function calls.
        let out_idx = self
            .graph
            .node_indices()
            .find(|n| self.graph.node_weight(*n).unwrap() == "out");
        if out_idx.is_none() {
            return vec![];
        }
        let out_idx = out_idx.unwrap();

        let rev_graph = Reversed(&self.graph);
        let dfs_post_order = DfsPostOrder::new(&rev_graph, out_idx);

        let mut instructions = vec![];

        let mut vref_id = 0;
        let mut output_vrefs = HashMap::new();
        for n in dfs_post_order.iter(&rev_graph) {
            let func = runtime
                .get_function(self.graph.node_weight(n).unwrap())
                .expect("Failed to find function");
            let mut inputs = vec![];
            for (idx, _) in func.inputs().iter().enumerate() {
                for e in self.graph.edges_directed(n, Direction::Incoming) {
                    if e.weight().input_index == idx as u32 {
                        let vref = *output_vrefs
                            .get(&(e.source(), e.weight().output_index))
                            .expect("Failed to find incoming signal's var ref");
                        inputs.push(Some(Operand::Var(vref)));
                    }
                }
                if inputs.len() < idx + 1 {
                    inputs.push(None);
                }
            }
            let mut outputs = vec![];
            for (idx, _) in func.outputs().iter().enumerate() {
                let vref = VarRef(vref_id);
                outputs.push(vref);
                vref_id += 1;
                output_vrefs.insert((n, idx as u32), vref);
            }
            instructions.push(Instruction::Call(
                self.graph.node_weight(n).unwrap().clone(),
                inputs,
                outputs,
            ));
        }

        instructions
    }
}
