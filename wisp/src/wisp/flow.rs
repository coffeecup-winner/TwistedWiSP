use std::collections::HashMap;

use petgraph::{
    graph::{EdgeIndex, NodeIndex},
    stable_graph::StableGraph,
    visit::{EdgeFiltered, EdgeRef, Topo, Walker},
    Directed, Direction,
};

use super::{
    ir::{CallId, Instruction, Operand, SourceLocation, VarRef},
    WispContext,
};

type FlowGraph = StableGraph<String, FlowConnection, Directed>;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct FlowNodeIndex(pub NodeIndex);

#[derive(Debug, Clone, Copy)]
struct FlowConnection {
    output_index: u32,
    input_index: u32,
}

#[derive(Debug, Default)]
pub struct Flow {
    graph: FlowGraph,
}

impl Flow {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn add_node(&mut self, name: String) -> FlowNodeIndex {
        FlowNodeIndex(self.graph.add_node(name))
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
            from.0,
            to.0,
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
        for e in self.graph.edges_connecting(from.0, to.0) {
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
            ctx.get_function(self.graph.node_weight(e.source()).unwrap())
                .unwrap()
                .lag_value()
                .is_none()
        });

        let mut vref_id = 0;
        let mut output_vrefs = HashMap::new();
        let mut instructions = vec![];

        let topo = Topo::new(&filtered_graph);
        for n in topo.iter(&filtered_graph) {
            let func = ctx
                .get_function(self.graph.node_weight(n).unwrap())
                .expect("Failed to find function");

            let mut inputs = vec![];
            for (idx, _) in func.inputs().iter().enumerate() {
                for e in self.graph.edges_directed(n, Direction::Incoming) {
                    let source_func = ctx
                        .get_function(self.graph.node_weight(e.source()).unwrap())
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
                        inputs.push(Some(Operand::Var(vref)));
                    } else if e.weight().input_index == idx as u32 {
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
                CallId(n.index() as u32),
                self.graph.node_weight(n).unwrap().clone(),
                inputs,
                outputs,
            ));
        }

        instructions
    }
}
