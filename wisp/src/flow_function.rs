use std::{cell::RefCell, collections::HashMap, path::PathBuf, vec};

use petgraph::{
    graph::{EdgeIndex, NodeIndex},
    stable_graph::{EdgeIndices, NodeIndices, StableGraph},
    visit::{EdgeFiltered, EdgeRef, Topo, Walker},
    Directed, Direction,
};
use serde::{Deserialize, Serialize};

use crate::{
    context::WispContext, CodeFunction, DataType, DefaultInputValue, FunctionInput, FunctionOutput,
    MathFunctionParser, WispFunction,
};

use twisted_wisp_ir::{
    BinaryOpType, CallId, Constant, FunctionOutputIndex, IRFunction, IRFunctionInput,
    IRFunctionOutput, Instruction, Operand, SourceLocation, TargetLocation, VarRef,
};

#[derive(Debug, Clone)]
pub struct FlowNode {
    pub name: String,
    pub display_text: String,
    pub coords: FlowNodeCoords,
    pub buffer: Option<String>,
    pub value: Option<f32>,
}

#[derive(Debug, Default, Clone)]
pub struct FlowNodeCoords {
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
    inputs: Vec<FunctionInput>,
    outputs: Vec<FunctionOutput>,
    graph: FlowGraph,
    ir_function: RefCell<Option<IRFunction>>,
    math_function_id_gen: u32,
    math_functions: HashMap<String, Box<dyn WispFunction>>,
    buffers: HashMap<String, PathBuf>,
}

#[derive(Debug, Serialize, Deserialize)]
struct FileFormat {
    flow: FileFormatFlow,
    buffers: Option<Vec<FileFormatBuffer>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct FileFormatFlow {
    name: String,
    nodes: Vec<FileFormatNode>,
    edges: Vec<FileFormatEdge>,
}

#[derive(Debug, Serialize, Deserialize)]
struct FileFormatNode {
    text: String,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    buffer: Option<String>,
    value: Option<f32>,
}

#[derive(Debug, Serialize, Deserialize)]
struct FileFormatEdge {
    from: u32,
    output_index: u32,
    to: u32,
    input_index: u32,
}

#[derive(Debug, Serialize, Deserialize)]
struct FileFormatBuffer {
    name: String,
    path: PathBuf,
}

impl WispFunction for FlowFunction {
    fn name(&self) -> &str {
        &self.name
    }

    fn name_mut(&mut self) -> &mut String {
        &mut self.name
    }

    fn inputs(&self) -> &[FunctionInput] {
        &self.inputs
    }

    fn outputs(&self) -> &[FunctionOutput] {
        &self.outputs
    }

    fn get_ir_functions(&self, ctx: &WispContext) -> Vec<IRFunction> {
        // TODO: Only do this if the flow has changed
        *self.ir_function.borrow_mut() = Some(self.compile_to_ir(ctx));
        let mut result = vec![self.ir_function.borrow().clone().unwrap()];
        for math_function in self.math_functions.values() {
            result.extend(math_function.get_ir_functions(ctx));
        }
        result
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
        let format = toml::from_str::<FileFormat>(s).ok()?;
        let mut flow = FlowFunction::new(format.flow.name);
        for n in format.flow.nodes {
            let node_idx = flow.add_node(&n.text);
            let node = flow.get_node_mut(node_idx).unwrap();
            node.coords = FlowNodeCoords {
                x: n.x,
                y: n.y,
                w: n.w,
                h: n.h,
            };
            node.buffer = n.buffer;
            node.value = n.value;
        }
        for e in format.flow.edges {
            flow.graph.add_edge(
                e.from.into(),
                e.to.into(),
                FlowConnection {
                    output_index: e.output_index,
                    input_index: e.input_index,
                },
            );
        }
        if let Some(buffers) = format.buffers {
            for b in buffers {
                flow.buffers.insert(b.name, b.path);
            }
        }
        Some(Box::new(flow))
    }

    fn save(&self) -> String {
        let mut nodes = vec![];
        let mut node_idx_map = HashMap::new();
        for (sequential_idx, idx) in self.graph.node_indices().enumerate() {
            let n = self.graph.node_weight(idx).unwrap();
            nodes.push(FileFormatNode {
                text: n.display_text.clone(),
                x: n.coords.x,
                y: n.coords.y,
                w: n.coords.w,
                h: n.coords.h,
                buffer: n.buffer.clone(),
                value: n.value,
            });
            node_idx_map.insert(idx.index(), sequential_idx);
        }
        let mut edges = vec![];
        for idx in self.graph.edge_indices() {
            let endpoints = self.graph.edge_endpoints(idx).unwrap();
            let e = self.graph.edge_weight(idx).unwrap();
            edges.push(FileFormatEdge {
                from: node_idx_map[&endpoints.0.index()] as u32,
                output_index: e.output_index,
                to: node_idx_map[&endpoints.1.index()] as u32,
                input_index: e.input_index,
            });
        }
        let buffers = self
            .buffers
            .iter()
            .map(|(name, path)| FileFormatBuffer {
                name: name.clone(),
                path: path.clone(),
            })
            .collect::<Vec<_>>();
        let buffers = if buffers.is_empty() {
            None
        } else {
            Some(buffers)
        };
        toml::to_string_pretty(&FileFormat {
            flow: FileFormatFlow {
                name: self.name.clone(),
                nodes,
                edges,
            },
            buffers,
        })
        .expect("Failed to serialize the flow function")
    }

    fn clone(&self) -> Box<dyn WispFunction> {
        // Manually clone the math functions since WispFunction doesn't inherit Clone
        let math_functions = self
            .math_functions
            .iter()
            .map(|(k, v)| (k.clone(), (*v).clone()))
            .collect::<HashMap<_, _>>();
        Box::new(FlowFunction {
            name: self.name.clone(),
            inputs: self.inputs.clone(),
            outputs: self.outputs.clone(),
            graph: self.graph.clone(),
            ir_function: RefCell::new(None),
            math_function_id_gen: self.math_function_id_gen,
            math_functions,
            buffers: self.buffers.clone(),
        })
    }
}

impl FlowFunction {
    pub fn new(name: String) -> Self {
        FlowFunction {
            name,
            inputs: Default::default(),
            outputs: Default::default(),
            graph: Default::default(),
            ir_function: Default::default(),
            math_function_id_gen: 0,
            math_functions: Default::default(),
            buffers: Default::default(),
        }
    }

    pub fn add_buffer(&mut self, name: &str, path: PathBuf) {
        self.buffers.insert(name.into(), path);
    }

    pub fn buffers(&self) -> &HashMap<String, PathBuf> {
        &self.buffers
    }

    pub fn add_node(&mut self, display_text: &str) -> FlowNodeIndex {
        // TODO: Support several inputs/outputs
        if display_text == "inputs" {
            self.inputs = vec![FunctionInput::new(
                "in".into(),
                DataType::Float,
                DefaultInputValue::Value(0.0),
            )];
        } else if display_text == "outputs" {
            self.outputs = vec![FunctionOutput::new("out".into(), DataType::Float)];
        }

        let name = if display_text.starts_with('=') {
            let math_function =
                if let Some(mut func) = MathFunctionParser::parse_function(display_text) {
                    *func.name_mut() = format!("{}:math${}", self.name, self.math_function_id_gen);
                    Box::new(func) as Box<dyn WispFunction>
                } else {
                    Box::new(CodeFunction::new(
                        format!("{}:stub${}", self.name(), self.math_function_id_gen),
                        vec![],
                        vec![],
                        vec![],
                        vec![],
                        None,
                    ))
                };
            self.math_function_id_gen += 1;
            let name = math_function.name().to_owned();
            self.math_functions
                .insert(math_function.name().to_owned(), math_function);
            name
        } else {
            display_text.to_owned()
        };

        self.graph.add_node(FlowNode {
            name,
            display_text: display_text.to_owned(),
            coords: Default::default(),
            buffer: None,
            value: None,
        })
    }

    pub fn remove_node(&mut self, idx: FlowNodeIndex) -> Option<FlowNode> {
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

    pub fn get_function(&self, name: &str) -> Option<&dyn WispFunction> {
        self.math_functions.get(name).map(|f| &**f)
    }

    pub fn get_function_mut(&mut self, name: &str) -> Option<&mut Box<dyn WispFunction>> {
        self.math_functions.get_mut(name)
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

    // TODO: Return Option/Result
    pub fn compile_to_ir(&self, ctx: &WispContext) -> IRFunction {
        // This function walks the graph in topological order, so all producing nodes
        // are visited before all consuming nodes. To break graph cycles, lag outputs
        // are ignored and lagged values are used instead. Since topological sort
        // visits all nodes, the lag nodes are visited and updated later.
        // This allows compiling the signal flow as a series of function calls
        // (and lag value fetches).

        let filtered_graph = EdgeFiltered::from_fn(&self.graph, |e| {
            let name = &self.graph.node_weight(e.source()).unwrap().name;
            ctx.get_function(name).unwrap().lag_value().is_none()
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
            for (idx, input) in func.inputs().iter().enumerate() {
                // Add all incoming signals to the input list
                for e in self.graph.edges_directed(n, Direction::Incoming) {
                    if e.weight().input_index != idx as u32 {
                        continue;
                    }
                    let source_func = ctx
                        .get_function(&self.graph.node_weight(e.source()).unwrap().name)
                        .unwrap();
                    // Custom flow input processing
                    if source_func.name() == "inputs" {
                        inputs.push(Operand::Arg(e.weight().output_index));
                        continue;
                    }
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
                while inputs.len() > idx + 1 {
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
                if inputs.len() < idx + 1 {
                    match input.fallback {
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
                        DefaultInputValue::EmptyArray => {
                            // Empty array
                            inputs.push(Operand::Constant(Constant::EmptyArray));
                        }
                    }
                }
            }

            if func.lag_value().is_some() {
                // For lag calls we don't need any outputs
                lag_calls.push(Instruction::Call(
                    CallId(n.index() as u32),
                    self.graph.node_weight(n).unwrap().name.clone(),
                    inputs,
                    vec![],
                ))
            } else if func.name() == "inputs" {
                // Custom flow input processing
                // Do nothing - inputs are already handled above
            } else if func.name() == "outputs" {
                // Custom flow output processing
                for (idx, input) in inputs.into_iter().enumerate() {
                    instructions.push(Instruction::Store(
                        TargetLocation::FunctionOutput(FunctionOutputIndex(idx as u32)),
                        input,
                    ));
                }
            } else {
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
        }

        instructions.append(&mut lag_calls);

        IRFunction {
            name: self.name.clone(),
            inputs: self
                .inputs
                .iter()
                .map(|i| IRFunctionInput {
                    type_: i.type_.into(),
                })
                .collect(),
            outputs: self
                .outputs
                .iter()
                .map(|o| IRFunctionOutput {
                    type_: o.type_.into(),
                })
                .collect(),
            data: vec![],
            ir: instructions,
        }
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

    #[test]
    fn test_empty_flow() {
        let ctx = test_ctx();
        let f = FlowFunction::new("test".into());

        let ir = f.compile_to_ir(&ctx).ir;
        assert_eq!(Vec::<Instruction>::new(), ir,);
    }

    #[test]
    fn test_empty_output() {
        let ctx = test_ctx();
        let mut f = FlowFunction::new("test".into());
        f.add_node("out");

        let ir = f.compile_to_ir(&ctx).ir;
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
        let idx_test = f.add_node("test");
        let idx_out = f.add_node("out");
        f.connect(idx_test, 0, idx_out, 0);

        let ir = f.compile_to_ir(&ctx).ir;
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
        let idx_test = f.add_node("test");
        let idx_out = f.add_node("out");
        f.connect(idx_test, 0, idx_out, 1);

        let ir = f.compile_to_ir(&ctx).ir;
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
        let idx_test0 = f.add_node("test");
        let idx_test1 = f.add_node("test");
        let idx_out = f.add_node("out");
        f.connect(idx_test0, 0, idx_out, 0);
        f.connect(idx_test1, 0, idx_out, 0);

        let ir = f.compile_to_ir(&ctx).ir;
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
        let idx_src = f.add_node("src");
        let idx_1to1 = f.add_node("1to1");
        let idx_2to1_0 = f.add_node("2to1");
        let idx_2to1_1 = f.add_node("2to1");
        let idx_out = f.add_node("out");
        f.connect(idx_src, 0, idx_2to1_0, 0);
        f.connect(idx_src, 0, idx_1to1, 0);
        f.connect(idx_1to1, 0, idx_2to1_0, 1);
        f.connect(idx_2to1_0, 0, idx_2to1_1, 0);
        f.connect(idx_src, 0, idx_2to1_1, 1);
        f.connect(idx_2to1_1, 0, idx_out, 0);
        //  src -> 2to1 -> 2to1 -> out
        //   \-1to1-/      /
        //    \-----------/

        let ir = f.compile_to_ir(&ctx).ir;
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
        let idx_lag = f.add_node("lag");
        let idx_test = f.add_node("test");
        let idx_out = f.add_node("out");
        f.connect(idx_lag, 0, idx_test, 0);
        f.connect(idx_test, 0, idx_lag, 0);
        f.connect(idx_test, 0, idx_out, 0);
        //   lag
        //  /   \
        //  test -> out

        let ir = f.compile_to_ir(&ctx).ir;
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
        let idx_lag0 = f.add_node("lag");
        let idx_lag1 = f.add_node("lag");
        let idx_test = f.add_node("test");
        let idx_out = f.add_node("out");
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

        let ir = f.compile_to_ir(&ctx).ir;
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
        let idx_lag0 = f.add_node("lag");
        let idx_lag1 = f.add_node("lag");
        let idx_test = f.add_node("test");
        let idx_out = f.add_node("out");
        f.connect(idx_lag0, 0, idx_lag1, 0);
        f.connect(idx_lag0, 0, idx_test, 0);
        f.connect(idx_test, 0, idx_lag0, 0);
        f.connect(idx_lag1, 0, idx_test, 0);
        f.connect(idx_test, 0, idx_lag1, 0);
        f.connect(idx_test, 0, idx_out, 0);
        //  lag0 - lag1
        //    \\  //
        //     test -> out

        let ir = f.compile_to_ir(&ctx).ir;
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
        let idx_lag0 = f.add_node("lag");
        let idx_lag1 = f.add_node("lag");
        let idx_test = f.add_node("test");
        let idx_out = f.add_node("out");
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

        let ir = f.compile_to_ir(&ctx).ir;
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
