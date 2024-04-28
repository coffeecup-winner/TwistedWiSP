use std::{cell::RefCell, collections::HashMap};

use petgraph::{
    graph::{EdgeIndex, NodeIndex},
    stable_graph::{EdgeIndices, NodeIndices, StableGraph},
    visit::{EdgeFiltered, EdgeRef, NodeRef, Topo, Walker},
    Directed, Direction,
};

use crate::{context::WispContext, DefaultInputValue, FunctionInput, FunctionOutput, WispFunction};

use twisted_wisp_ir::{CallId, IRFunction, Instruction, Operand, SourceLocation, VarRef};

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
pub struct FlowConnection {
    pub output_index: u32,
    pub input_index: u32,
}

pub type FlowNodeIndex = NodeIndex;
pub type FlowConnectionIndex = EdgeIndex;
type FlowGraph = StableGraph<FlowNode, FlowConnection, Directed>;

#[derive(Debug)]
pub struct FlowFunction {
    name: String,
    graph: FlowGraph,
    ir: RefCell<Vec<Instruction>>,
}

impl WispFunction for FlowFunction {
    fn name(&self) -> &str {
        &self.name
    }

    fn inputs_count(&self) -> u32 {
        0
    }

    fn input(&self, _idx: u32) -> Option<&FunctionInput> {
        None
    }

    fn outputs_count(&self) -> u32 {
        0
    }

    fn output(&self, _idx: u32) -> Option<&FunctionOutput> {
        None
    }

    fn get_ir_function(&self, ctx: &WispContext) -> IRFunction {
        // TODO: Only do this if the flow has changed
        *self.ir.borrow_mut() = self.compile_to_ir(ctx);
        IRFunction {
            name: self.name.clone(),
            inputs: vec![],
            outputs: vec![],
            data: vec![],
            ir: self.ir.borrow().clone(),
        }
    }

    fn as_flow(&self) -> Option<&FlowFunction> {
        Some(self)
    }

    fn as_flow_mut(&mut self) -> Option<&mut FlowFunction> {
        Some(self)
    }

    fn load(s: &str) -> Option<Box<dyn WispFunction>>
    where
        Self: Sized,
    {
        let mut lines = s.lines();
        let mut first_line = lines.next()?;
        first_line = first_line.strip_prefix("flow:")?;
        let mut parts = first_line.split(' ');
        let name = parts.next()?;
        let node_count = parts.next()?.parse::<u32>().ok()?;
        let edge_count = parts.next()?.parse::<u32>().ok()?;
        let mut graph = FlowGraph::new();
        for idx in 0..node_count {
            let line = lines.next()?;
            let mut parts = line.split(' ');
            let name = parts.next()?;
            let x = parts.next()?.parse::<i32>().ok()?;
            let y = parts.next()?.parse::<i32>().ok()?;
            let w = parts.next()?.parse::<u32>().ok()?;
            let h = parts.next()?.parse::<u32>().ok()?;
            let node_idx = graph.add_node(FlowNode {
                name: name.into(),
                data: FlowNodeData { x, y, w, h },
            });
            assert_eq!(node_idx.index().id() as u32, idx);
        }
        for idx in 0..edge_count {
            let line = lines.next()?;
            let mut parts = line.split(' ');
            let from = parts.next()?.parse::<u32>().ok()?;
            let output_index = parts.next()?.parse::<u32>().ok()?;
            let to = parts.next()?.parse::<u32>().ok()?;
            let input_index = parts.next()?.parse::<u32>().ok()?;
            let edge_idx = graph.add_edge(
                from.into(),
                to.into(),
                FlowConnection {
                    input_index,
                    output_index,
                },
            );
            assert_eq!(edge_idx.index().id() as u32, idx);
        }
        Some(Box::new(FlowFunction {
            name: name.into(),
            graph,
            ir: RefCell::new(vec![]),
        }))
    }

    fn save(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!(
            "flow:{} {} {}\n",
            self.name,
            self.graph.node_count(),
            self.graph.edge_count()
        ));
        for idx in self.graph.node_indices() {
            let n = self.graph.node_weight(idx).unwrap();
            s.push_str(&format!(
                "{} {} {} {} {}\n",
                n.name, n.data.x, n.data.y, n.data.w, n.data.h
            ));
        }
        for idx in self.graph.edge_indices() {
            let endpoints = self.graph.edge_endpoints(idx).unwrap();
            let e = self.graph.edge_weight(idx).unwrap();
            s.push_str(&format!(
                "{} {} {} {}\n",
                endpoints.0.index(),
                e.output_index,
                endpoints.1.index(),
                e.input_index
            ))
        }
        s
    }
}

impl FlowFunction {
    pub fn new(name: String) -> Self {
        FlowFunction {
            name,
            graph: Default::default(),
            ir: Default::default(),
        }
    }

    pub fn add_node(&mut self, name: String) -> FlowNodeIndex {
        self.graph.add_node(FlowNode {
            name,
            data: Default::default(),
        })
    }

    pub fn node_indices(&self) -> NodeIndices<FlowNode> {
        self.graph.node_indices()
    }

    pub fn get_node(&self, idx: FlowNodeIndex) -> Option<&FlowNode> {
        self.graph.node_weight(idx)
    }

    pub fn get_node_mut(&mut self, idx: FlowNodeIndex) -> Option<&mut FlowNode> {
        self.graph.node_weight_mut(idx)
    }

    pub fn edge_indices(&self) -> EdgeIndices<FlowConnection> {
        self.graph.edge_indices()
    }

    pub fn get_connection(
        &self,
        idx: FlowConnectionIndex,
    ) -> Option<(FlowNodeIndex, FlowNodeIndex, &FlowConnection)> {
        let (from, to) = self.graph.edge_endpoints(idx)?;
        let conn = self.graph.edge_weight(idx)?;
        Some((from, to, conn))
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
            for idx in 0..func.inputs_count() {
                for e in self.graph.edges_directed(n, Direction::Incoming) {
                    if e.weight().input_index != idx {
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
                if (inputs.len() as u32) < idx + 1 {
                    match func.input(idx).unwrap().fallback {
                        DefaultInputValue::Value(v) => {
                            inputs.push(Operand::Literal(v));
                        }
                        DefaultInputValue::Normal => {
                            inputs.push(inputs[(idx - 1) as usize]);
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
            for idx in 0..func.outputs_count() {
                let vref = VarRef(vref_id);
                outputs.push(vref);
                vref_id += 1;
                output_vrefs.insert((n, idx), vref);
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
