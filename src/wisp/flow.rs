use std::{
    cell::{Ref, RefCell},
    collections::HashMap,
};

use petgraph::{
    graph::NodeIndex,
    stable_graph::StableGraph,
    visit::{EdgeFiltered, EdgeRef, Topo, Walker},
    Directed, Direction,
};

use super::{
    function::Function,
    ir::{CallId, Instruction, Operand, SourceLocation, VarRef},
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
    name: String,
    graph: FlowGraph,
    ir: RefCell<Option<Vec<Instruction>>>,
}

impl Flow {
    pub fn new(name: String) -> Self {
        Flow {
            name,
            ..Default::default()
        }
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

    pub fn compile_function(&self, runtime: &Runtime) -> Function {
        let process_func_instructions = self
            .get_compiled_flow(runtime)
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        // TODO: Support inputs/outputs for flows
        Function::new(
            self.name.clone(),
            vec![],
            vec![],
            vec![],
            process_func_instructions,
            None,
        )
    }

    fn get_compiled_flow(&self, runtime: &Runtime) -> Ref<'_, Vec<Instruction>> {
        if self.ir.borrow().is_none() {
            let ir = self.compile(runtime);
            *self.ir.borrow_mut() = Some(ir);
        }
        Ref::map(self.ir.borrow(), |is| is.as_ref().unwrap())
    }

    fn compile(&self, runtime: &Runtime) -> Vec<Instruction> {
        // This function walks the graph in topological order, so all producing nodes
        // are visited before all consuming nodes. To break graph cycles, lag outputs
        // are ignored and lagged values are used instead. Since topological sort
        // visits all nodes, the lag nodes are visited and updated later.
        // This allows compiling the signal flow as a series of function calls
        // (and lag value fetches).

        let filtered_graph = EdgeFiltered::from_fn(&self.graph, |e| {
            runtime
                .get_function(self.graph.node_weight(e.source()).unwrap())
                .unwrap()
                .lag_value()
                .is_none()
        });

        let mut vref_id = 0;
        let mut output_vrefs = HashMap::new();
        let mut instructions = vec![];

        let topo = Topo::new(&filtered_graph);
        for n in topo.iter(&filtered_graph) {
            let func = runtime
                .get_function(self.graph.node_weight(n).unwrap())
                .expect("Failed to find function");

            let mut inputs = vec![];
            for (idx, _) in func.inputs().iter().enumerate() {
                for e in self.graph.edges_directed(n, Direction::Incoming) {
                    let source_func = runtime
                        .get_function(self.graph.node_weight(e.source()).unwrap())
                        .unwrap();
                    if let Some(dref) = source_func.lag_value() {
                        let vref = VarRef(vref_id);
                        instructions.push(Instruction::Load(
                            vref,
                            SourceLocation::LastValue(CallId(e.source().index() as u32), dref),
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
