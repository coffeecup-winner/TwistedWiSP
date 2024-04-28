use std::collections::HashMap;

use petgraph::{
    graph::{EdgeIndex, NodeIndex},
    stable_graph::StableGraph,
    visit::{EdgeFiltered, EdgeRef, Topo, Walker},
    Directed, Direction,
};

use crate::context::WispContext;

use super::function::DefaultInputValue;

use twisted_wisp_ir::{CallId, Instruction, Operand, SourceLocation, VarRef};

#[derive(Debug, Clone)]
pub struct FlowNode {
    pub name: String,
    pub data: FlowNodeData,
}

#[derive(Debug, Default, Clone)]
pub struct FlowNodeData {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

#[derive(Debug, Clone, Copy)]
struct FlowConnection {
    output_index: u32,
    input_index: u32,
}

pub type FlowNodeIndex = NodeIndex;
type FlowGraph = StableGraph<FlowNode, FlowConnection, Directed>;

#[derive(Debug, Default)]
pub struct Flow {
    graph: FlowGraph,
}

impl Flow {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn add_node(&mut self, name: String) -> FlowNodeIndex {
        self.graph.add_node(FlowNode {
            name,
            data: Default::default(),
        })
    }

    pub fn get_node_mut(&mut self, idx: FlowNodeIndex) -> Option<&mut FlowNode> {
        self.graph.node_weight_mut(idx)
    }

    pub fn connect(
        &mut self,
        from: FlowNodeIndex,
        output_index: u32,
        to: FlowNodeIndex,
        input_index: u32,
    ) {
        if self
            .find_connection(from, output_index, to, input_index)
            .is_some()
        {
            // Connection already exists
            return;
        }

        self.graph.add_edge(
            from,
            to,
            FlowConnection {
                output_index,
                input_index,
            },
        );
    }

    pub fn disconnect(
        &mut self,
        from: FlowNodeIndex,
        output_index: u32,
        to: FlowNodeIndex,
        input_index: u32,
    ) {
        if let Some(e) = self.find_connection(from, output_index, to, input_index) {
            self.graph.remove_edge(e);
        }
    }

    fn find_connection(
        &self,
        from: FlowNodeIndex,
        output_index: u32,
        to: FlowNodeIndex,
        input_index: u32,
    ) -> Option<EdgeIndex> {
        for e in self.graph.edges_connecting(from, to) {
            if e.weight().output_index == output_index && e.weight().input_index == input_index {
                return Some(e.id());
            }
        }
        None
    }

    pub fn compile_to_ir(&self, ctx: &WispContext) -> Vec<Instruction> {
        // This function walks the graph in topological order, so all producing nodes
        // are visited before all consuming nodes. To break graph cycles, lag outputs
        // are ignored and lagged values are used instead. Since topological sort
        // visits all nodes, the lag nodes are visited and updated later.
        // This allows compiling the signal flow as a series of function calls
        // (and lag value fetches).

        let filtered_graph = EdgeFiltered::from_fn(&self.graph, |e| {
            ctx.get_function(&self.graph.node_weight(e.source()).unwrap().name)
                .unwrap()
                .lag_value()
                .is_none()
        });

        let mut vref_id = 0;
        let mut output_vrefs = HashMap::new();
        let mut instructions = vec![];

        let topo = Topo::new(&filtered_graph);
        'nodes: for n in topo.iter(&filtered_graph) {
            let func = ctx
                .get_function(&self.graph.node_weight(n).unwrap().name)
                .expect("Failed to find function");

            let mut inputs = vec![];
            for (idx, _) in func.inputs().iter().enumerate() {
                for e in self.graph.edges_directed(n, Direction::Incoming) {
                    if e.weight().input_index != idx as u32 {
                        continue;
                    }
                    let source_func = ctx
                        .get_function(&self.graph.node_weight(e.source()).unwrap().name)
                        .unwrap();
                    if let Some(dref) = source_func.lag_value() {
                        let vref = VarRef(vref_id);
                        instructions.push(Instruction::Load(
                            vref,
                            SourceLocation::LastValue(
                                CallId(e.source().index() as u32),
                                source_func.name().into(),
                                dref,
                            ),
                        ));
                        vref_id += 1;
                        inputs.push(Operand::Var(vref));
                    } else {
                        let vref = *output_vrefs
                            .get(&(e.source(), e.weight().output_index))
                            .expect("Failed to find incoming signal's var ref");
                        inputs.push(Operand::Var(vref));
                    }
                }
                if inputs.len() < idx + 1 {
                    match func.inputs()[idx].fallback {
                        DefaultInputValue::Value(v) => {
                            inputs.push(Operand::Literal(v));
                        }
                        DefaultInputValue::Normal => {
                            inputs.push(inputs[idx - 1]);
                        }
                        DefaultInputValue::Skip => {
                            assert!(
                                func.lag_value().is_some(),
                                "Input skip mode must not be used on non-lag functions"
                            );
                            // Skip this call
                            continue 'nodes;
                        }
                    }
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
                CallId(n.index() as u32),
                self.graph.node_weight(n).unwrap().name.clone(),
                inputs,
                outputs,
            ));
        }

        instructions
    }
}
