use std::{cell::RefCell, collections::HashMap};

use petgraph::{
    graph::{EdgeIndex, NodeIndex},
    stable_graph::{EdgeIndices, NodeIndices, StableGraph},
    visit::{EdgeFiltered, EdgeRef, NodeRef, Topo, Walker},
    Directed, Direction,
};

use crate::{context::WispContext, DefaultInputValue, FunctionInput, FunctionOutput, WispFunction};

use twisted_wisp_ir::{
    BinaryOpType, CallId, Constant, IRFunction, Instruction, Operand, SourceLocation, VarRef,
};

#[derive(Debug, Clone)]
pub struct FlowNode {
    pub name: String,
    pub display_text: String,
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
    watch_idx_map: HashMap<u32, FlowNodeIndex>,
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

    fn load(s: &str, _ctx: &WispContext) -> Option<Box<dyn WispFunction>>
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
            let display_text = lines.next()?.into();
            let node_idx = graph.add_node(FlowNode {
                name: name.into(),
                data: FlowNodeData { x, y, w, h },
                display_text,
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
            watch_idx_map: Default::default(),
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
        let mut node_idx_map = HashMap::new();
        let mut sequential_index = 0;
        for idx in self.graph.node_indices() {
            let n = self.graph.node_weight(idx).unwrap();
            s.push_str(&format!(
                "{} {} {} {} {}\n",
                n.name, n.data.x, n.data.y, n.data.w, n.data.h
            ));
            s.push_str(&format!("{}\n", n.display_text));
            node_idx_map.insert(idx.index(), sequential_index);
            sequential_index += 1;
        }
        for idx in self.graph.edge_indices() {
            let endpoints = self.graph.edge_endpoints(idx).unwrap();
            let e = self.graph.edge_weight(idx).unwrap();
            s.push_str(&format!(
                "{} {} {} {}\n",
                node_idx_map[&endpoints.0.index()],
                e.output_index,
                node_idx_map[&endpoints.1.index()],
                e.input_index
            ))
        }
        s
    }

    fn create_alias(&self, name: String) -> Box<dyn WispFunction> {
        Box::new(FlowFunction {
            name,
            graph: self.graph.clone(),
            ir: RefCell::new(vec![]),
            watch_idx_map: self.watch_idx_map.clone(),
        })
    }
}

impl FlowFunction {
    pub fn new(name: String) -> Self {
        FlowFunction {
            name,
            graph: Default::default(),
            ir: Default::default(),
            watch_idx_map: Default::default(),
        }
    }

    pub fn add_node(&mut self, name: &str, display_text: &str) -> FlowNodeIndex {
        self.graph.add_node(FlowNode {
            name: name.to_owned(),
            data: Default::default(),
            display_text: display_text.to_owned(),
        })
    }

    pub fn remove_node(&mut self, idx: FlowNodeIndex) -> Option<FlowNode> {
        let watch_idx = self
            .watch_idx_map
            .iter()
            .find(|(_k, v)| **v == idx)
            .map(|(k, _)| *k);
        if let Some(idx) = watch_idx {
            self.watch_idx_map.remove(&idx);
        }
        self.graph.remove_node(idx)
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

    // TODO: Should it be stored here at all?
    pub fn add_watch_idx(&mut self, node_idx: FlowNodeIndex, idx: u32) {
        self.watch_idx_map.insert(idx, node_idx);
    }

    pub fn watch_idx_to_node_idx(&self, idx: u32) -> FlowNodeIndex {
        self.watch_idx_map[&idx]
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
        let mut lag_calls = vec![];

        let topo = Topo::new(&filtered_graph);
        'nodes: for n in topo.iter(&filtered_graph) {
            let func = ctx
                .get_function(&self.graph.node_weight(n).unwrap().name)
                .expect("Failed to find function");

            let mut inputs = vec![];
            for idx in 0..func.inputs_count() {
                // Add all incoming signals to the input list
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
                // If there are several signals for this input, combine them
                while (inputs.len() as u32) > idx + 1 {
                    let vref0 = inputs.pop().unwrap();
                    let vref1 = inputs.pop().unwrap();
                    let vref_result = VarRef(vref_id);
                    vref_id += 1;
                    instructions.push(Instruction::BinaryOp(
                        vref_result,
                        BinaryOpType::Add,
                        vref0,
                        vref1,
                    ));
                    inputs.push(Operand::Var(vref_result));
                }
                // If there are not enough inputs, add a fallback value
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
                        DefaultInputValue::EmptyArray => {
                            // Empty array
                            inputs.push(Operand::Constant(Constant::EmptyArray));
                        }
                    }
                }
            }
            // For lag calls we don't need any outputs
            if func.lag_value().is_some() {
                lag_calls.push(Instruction::Call(
                    CallId(n.index() as u32),
                    self.graph.node_weight(n).unwrap().name.clone(),
                    inputs,
                    vec![],
                ))
            } else {
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
        }

        instructions.append(&mut lag_calls);

        instructions
    }
}

#[cfg(test)]
mod tests {
    use twisted_wisp_ir::DataRef;

    use crate::{CodeFunction, DataType, FunctionDataItem};

    use super::*;

    fn test_ctx() -> WispContext {
        let mut ctx = WispContext::new(2);
        ctx.add_builtin_functions();
        ctx
    }

    fn lag_function() -> Box<CodeFunction> {
        Box::new(CodeFunction::new(
            "lag".into(),
            vec![FunctionInput::new(
                "in".into(),
                DataType::Float,
                DefaultInputValue::Value(0.0),
            )],
            vec![],
            vec![FunctionDataItem::new("lag".into(), DataType::Float)],
            vec![],
            Some(DataRef(0)),
        ))
    }

    fn create_function(name: &str, num_inputs: u32, num_outputs: u32) -> Box<CodeFunction> {
        Box::new(CodeFunction::new(
            name.into(),
            (0..num_inputs)
                .map(|i| {
                    FunctionInput::new(
                        format!("in{}", i),
                        DataType::Float,
                        DefaultInputValue::Value(0.0),
                    )
                })
                .collect(),
            (0..num_outputs)
                .map(|i| FunctionOutput::new(format!("out{}", i), DataType::Float))
                .collect(),
            vec![],
            vec![],
            None,
        ))
    }

    fn add_node(f: &mut FlowFunction, name: &str) -> FlowNodeIndex {
        f.add_node(name, name)
    }

    #[test]
    fn test_empty_flow() {
        let ctx = test_ctx();
        let f = FlowFunction::new("test".into());

        let ir = f.compile_to_ir(&ctx);
        assert_eq!(Vec::<Instruction>::new(), ir,);
    }

    #[test]
    fn test_empty_output() {
        let ctx = test_ctx();
        let mut f = FlowFunction::new("test".into());
        add_node(&mut f, "out");

        let ir = f.compile_to_ir(&ctx);
        assert_eq!(
            vec![Instruction::Call(
                CallId(0),
                "out".into(),
                vec![Operand::Literal(0.0), Operand::Literal(0.0)],
                vec![]
            ),],
            ir,
        );
    }

    #[test]
    fn test_single_node_into_out() {
        let mut ctx = test_ctx();
        ctx.add_function(create_function("test", 0, 1));

        let mut f = FlowFunction::new("test".into());
        let idx_test = add_node(&mut f, "test");
        let idx_out = add_node(&mut f, "out");
        f.connect(idx_test, 0, idx_out, 0);

        let ir = f.compile_to_ir(&ctx);
        assert_eq!(
            vec![
                Instruction::Call(CallId(0), "test".into(), vec![], vec![VarRef(0)]),
                Instruction::Call(
                    CallId(1),
                    "out".into(),
                    vec![Operand::Var(VarRef(0)), Operand::Var(VarRef(0))],
                    vec![]
                ),
            ],
            ir,
        );
    }

    #[test]
    fn test_single_node_into_out_normalization() {
        let mut ctx = test_ctx();
        ctx.add_function(create_function("test", 0, 1));

        let mut f = FlowFunction::new("test".into());
        let idx_test = add_node(&mut f, "test");
        let idx_out = add_node(&mut f, "out");
        f.connect(idx_test, 0, idx_out, 1);

        let ir = f.compile_to_ir(&ctx);
        assert_eq!(
            vec![
                Instruction::Call(CallId(0), "test".into(), vec![], vec![VarRef(0)]),
                Instruction::Call(
                    CallId(1),
                    "out".into(),
                    vec![Operand::Literal(0.0), Operand::Var(VarRef(0))],
                    vec![]
                ),
            ],
            ir,
        );
    }

    #[test]
    fn test_node_input_summation() {
        let mut ctx = test_ctx();
        ctx.add_function(create_function("test", 0, 1));

        let mut f = FlowFunction::new("test".into());
        let idx_test0 = add_node(&mut f, "test");
        let idx_test1 = add_node(&mut f, "test");
        let idx_out = add_node(&mut f, "out");
        f.connect(idx_test0, 0, idx_out, 0);
        f.connect(idx_test1, 0, idx_out, 0);

        let ir = f.compile_to_ir(&ctx);
        assert_eq!(
            vec![
                Instruction::Call(CallId(1), "test".into(), vec![], vec![VarRef(0)]),
                Instruction::Call(CallId(0), "test".into(), vec![], vec![VarRef(1)]),
                Instruction::BinaryOp(
                    VarRef(2),
                    BinaryOpType::Add,
                    Operand::Var(VarRef(1)),
                    Operand::Var(VarRef(0)),
                ),
                Instruction::Call(
                    CallId(2),
                    "out".into(),
                    vec![Operand::Var(VarRef(2)), Operand::Var(VarRef(2))],
                    vec![]
                ),
            ],
            ir,
        );
    }

    #[test]
    fn test_call_graph() {
        let mut ctx = test_ctx();
        ctx.add_function(create_function("src", 0, 1));
        ctx.add_function(create_function("1to1", 1, 1));
        ctx.add_function(create_function("2to1", 2, 1));

        let mut f = FlowFunction::new("test".into());
        let idx_src = add_node(&mut f, "src");
        let idx_1to1 = add_node(&mut f, "1to1");
        let idx_2to1_0 = add_node(&mut f, "2to1");
        let idx_2to1_1 = add_node(&mut f, "2to1");
        let idx_out = add_node(&mut f, "out");
        f.connect(idx_src, 0, idx_2to1_0, 0);
        f.connect(idx_src, 0, idx_1to1, 0);
        f.connect(idx_1to1, 0, idx_2to1_0, 1);
        f.connect(idx_2to1_0, 0, idx_2to1_1, 0);
        f.connect(idx_src, 0, idx_2to1_1, 1);
        f.connect(idx_2to1_1, 0, idx_out, 0);
        //  src -> 2to1 -> 2to1 -> out
        //   \-1to1-/      /
        //    \-----------/

        let ir = f.compile_to_ir(&ctx);
        assert_eq!(
            vec![
                Instruction::Call(CallId(0), "src".into(), vec![], vec![VarRef(0)]),
                Instruction::Call(
                    CallId(1),
                    "1to1".into(),
                    vec![Operand::Var(VarRef(0))],
                    vec![VarRef(1)]
                ),
                Instruction::Call(
                    CallId(2),
                    "2to1".into(),
                    vec![Operand::Var(VarRef(0)), Operand::Var(VarRef(1))],
                    vec![VarRef(2)]
                ),
                Instruction::Call(
                    CallId(3),
                    "2to1".into(),
                    vec![Operand::Var(VarRef(2)), Operand::Var(VarRef(0))],
                    vec![VarRef(3)]
                ),
                Instruction::Call(
                    CallId(4),
                    "out".into(),
                    vec![Operand::Var(VarRef(3)), Operand::Var(VarRef(3))],
                    vec![]
                ),
            ],
            ir,
        );
    }

    #[test]
    fn test_lag_function() {
        let mut ctx = test_ctx();
        ctx.add_function(lag_function());
        ctx.add_function(create_function("test", 1, 1));

        let mut f = FlowFunction::new("test".into());
        let idx_lag = add_node(&mut f, "lag");
        let idx_test = add_node(&mut f, "test");
        let idx_out = add_node(&mut f, "out");
        f.connect(idx_lag, 0, idx_test, 0);
        f.connect(idx_test, 0, idx_lag, 0);
        f.connect(idx_test, 0, idx_out, 0);
        //   lag
        //  /   \
        //  test -> out

        let ir = f.compile_to_ir(&ctx);
        assert_eq!(
            vec![
                Instruction::Load(
                    VarRef(0),
                    SourceLocation::LastValue(CallId(0), "lag".into(), DataRef(0)),
                ),
                Instruction::Call(
                    CallId(1),
                    "test".into(),
                    vec![Operand::Var(VarRef(0))],
                    vec![VarRef(1)]
                ),
                Instruction::Call(
                    CallId(2),
                    "out".into(),
                    vec![Operand::Var(VarRef(1)), Operand::Var(VarRef(1))],
                    vec![]
                ),
                Instruction::Call(
                    CallId(0),
                    "lag".into(),
                    vec![Operand::Var(VarRef(1))],
                    vec![]
                ),
            ],
            ir,
        );
    }

    #[test]
    fn test_two_lag_nodes() {
        let mut ctx = test_ctx();
        ctx.add_function(lag_function());
        ctx.add_function(create_function("test", 1, 1));

        let mut f = FlowFunction::new("test".into());
        let idx_lag0 = add_node(&mut f, "lag");
        let idx_lag1 = add_node(&mut f, "lag");
        let idx_test = add_node(&mut f, "test");
        let idx_out = add_node(&mut f, "out");
        f.connect(idx_lag0, 0, idx_test, 0);
        f.connect(idx_test, 0, idx_lag0, 0);
        f.connect(idx_lag1, 0, idx_test, 0);
        f.connect(idx_test, 0, idx_lag1, 0);
        f.connect(idx_test, 0, idx_out, 0);
        //   lag0
        //  /   \
        //  test -> out
        //  \   /
        //   lag1

        let ir = f.compile_to_ir(&ctx);
        assert_eq!(
            vec![
                Instruction::Load(
                    VarRef(0),
                    SourceLocation::LastValue(CallId(1), "lag".into(), DataRef(0)),
                ),
                Instruction::Load(
                    VarRef(1),
                    SourceLocation::LastValue(CallId(0), "lag".into(), DataRef(0)),
                ),
                Instruction::BinaryOp(
                    VarRef(2),
                    BinaryOpType::Add,
                    Operand::Var(VarRef(1)),
                    Operand::Var(VarRef(0)),
                ),
                Instruction::Call(
                    CallId(2),
                    "test".into(),
                    vec![Operand::Var(VarRef(2))],
                    vec![VarRef(3)]
                ),
                Instruction::Call(
                    CallId(3),
                    "out".into(),
                    vec![Operand::Var(VarRef(3)), Operand::Var(VarRef(3))],
                    vec![]
                ),
                Instruction::Call(
                    CallId(0),
                    "lag".into(),
                    vec![Operand::Var(VarRef(3))],
                    vec![]
                ),
                Instruction::Call(
                    CallId(1),
                    "lag".into(),
                    vec![Operand::Var(VarRef(3))],
                    vec![]
                ),
            ],
            ir,
        );
    }

    #[test]
    fn test_two_lag_nodes_chained() {
        let mut ctx = test_ctx();
        ctx.add_function(lag_function());
        ctx.add_function(create_function("test", 1, 1));

        let mut f = FlowFunction::new("test".into());
        let idx_lag0 = add_node(&mut f, "lag");
        let idx_lag1 = add_node(&mut f, "lag");
        let idx_test = add_node(&mut f, "test");
        let idx_out = add_node(&mut f, "out");
        f.connect(idx_lag0, 0, idx_lag1, 0);
        f.connect(idx_lag0, 0, idx_test, 0);
        f.connect(idx_test, 0, idx_lag0, 0);
        f.connect(idx_lag1, 0, idx_test, 0);
        f.connect(idx_test, 0, idx_lag1, 0);
        f.connect(idx_test, 0, idx_out, 0);
        //  lag0 - lag1
        //    \\  //
        //     test -> out

        let ir = f.compile_to_ir(&ctx);
        assert_eq!(
            vec![
                Instruction::Load(
                    VarRef(0),
                    SourceLocation::LastValue(CallId(1), "lag".into(), DataRef(0)),
                ),
                Instruction::Load(
                    VarRef(1),
                    SourceLocation::LastValue(CallId(0), "lag".into(), DataRef(0)),
                ),
                Instruction::BinaryOp(
                    VarRef(2),
                    BinaryOpType::Add,
                    Operand::Var(VarRef(1)),
                    Operand::Var(VarRef(0)),
                ),
                Instruction::Call(
                    CallId(2),
                    "test".into(),
                    vec![Operand::Var(VarRef(2))],
                    vec![VarRef(3)]
                ),
                Instruction::Load(
                    VarRef(4),
                    SourceLocation::LastValue(CallId(0), "lag".into(), DataRef(0)),
                ),
                Instruction::BinaryOp(
                    VarRef(5),
                    BinaryOpType::Add,
                    Operand::Var(VarRef(4)),
                    Operand::Var(VarRef(3)),
                ),
                Instruction::Call(
                    CallId(3),
                    "out".into(),
                    vec![Operand::Var(VarRef(3)), Operand::Var(VarRef(3))],
                    vec![]
                ),
                Instruction::Call(
                    CallId(0),
                    "lag".into(),
                    vec![Operand::Var(VarRef(3))],
                    vec![]
                ),
                Instruction::Call(
                    CallId(1),
                    "lag".into(),
                    vec![Operand::Var(VarRef(5))],
                    vec![]
                ),
            ],
            ir,
        );
    }

    #[test]
    fn test_two_lag_nodes_crosslinked() {
        let mut ctx = test_ctx();
        ctx.add_function(lag_function());
        ctx.add_function(create_function("test", 1, 1));

        let mut f = FlowFunction::new("test".into());
        let idx_lag0 = add_node(&mut f, "lag");
        let idx_lag1 = add_node(&mut f, "lag");
        let idx_test = add_node(&mut f, "test");
        let idx_out = add_node(&mut f, "out");
        f.connect(idx_lag0, 0, idx_lag1, 0);
        f.connect(idx_lag1, 0, idx_lag0, 0);
        f.connect(idx_lag0, 0, idx_test, 0);
        f.connect(idx_test, 0, idx_lag0, 0);
        f.connect(idx_lag1, 0, idx_test, 0);
        f.connect(idx_test, 0, idx_lag1, 0);
        f.connect(idx_test, 0, idx_out, 0);
        //  lag0 = lag1
        //    \\  //
        //     test -> out

        let ir = f.compile_to_ir(&ctx);
        assert_eq!(
            vec![
                Instruction::Load(
                    VarRef(0),
                    SourceLocation::LastValue(CallId(1), "lag".into(), DataRef(0)),
                ),
                Instruction::Load(
                    VarRef(1),
                    SourceLocation::LastValue(CallId(0), "lag".into(), DataRef(0)),
                ),
                Instruction::BinaryOp(
                    VarRef(2),
                    BinaryOpType::Add,
                    Operand::Var(VarRef(1)),
                    Operand::Var(VarRef(0)),
                ),
                Instruction::Call(
                    CallId(2),
                    "test".into(),
                    vec![Operand::Var(VarRef(2))],
                    vec![VarRef(3)]
                ),
                Instruction::Load(
                    VarRef(4),
                    SourceLocation::LastValue(CallId(1), "lag".into(), DataRef(0)),
                ),
                Instruction::BinaryOp(
                    VarRef(5),
                    BinaryOpType::Add,
                    Operand::Var(VarRef(4)),
                    Operand::Var(VarRef(3)),
                ),
                Instruction::Load(
                    VarRef(6),
                    SourceLocation::LastValue(CallId(0), "lag".into(), DataRef(0)),
                ),
                Instruction::BinaryOp(
                    VarRef(7),
                    BinaryOpType::Add,
                    Operand::Var(VarRef(6)),
                    Operand::Var(VarRef(3)),
                ),
                Instruction::Call(
                    CallId(3),
                    "out".into(),
                    vec![Operand::Var(VarRef(3)), Operand::Var(VarRef(3))],
                    vec![]
                ),
                Instruction::Call(
                    CallId(0),
                    "lag".into(),
                    vec![Operand::Var(VarRef(5))],
                    vec![]
                ),
                Instruction::Call(
                    CallId(1),
                    "lag".into(),
                    vec![Operand::Var(VarRef(7))],
                    vec![]
                ),
            ],
            ir,
        );
    }
}
