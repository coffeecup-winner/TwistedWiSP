use std::{
    collections::{HashMap, HashSet},
    iter::Peekable,
};

use crate::{
    DataType, DefaultInputValue, FunctionDataItem, FunctionInput, FunctionOutput, WispContext,
    WispFunction,
};

use log::error;
use logos::{Lexer, Logos};
use twisted_wisp_ir::{
    BinaryOpType, CallId, ComparisonOpType, Constant, DataRef, FunctionOutputIndex, IRFunction,
    IRFunctionDataItem, IRFunctionInput, IRFunctionOutput, Instruction, LocalRef, Operand,
    SignalOutputIndex, SourceLocation, TargetLocation, VarRef,
};

#[derive(Debug, PartialEq, Clone)]
pub struct CodeFunction {
    name: String,
    inputs: Vec<FunctionInput>,
    outputs: Vec<FunctionOutput>,
    data: Vec<FunctionDataItem>,
    ir: Vec<Instruction>,
    lag_value: Option<DataRef>,
}

impl WispFunction for CodeFunction {
    fn name(&self) -> &str {
        &self.name
    }

    fn inputs_count(&self) -> u32 {
        self.inputs.len() as u32
    }

    fn input(&self, idx: u32) -> Option<&FunctionInput> {
        self.inputs.get(idx as usize)
    }

    fn outputs_count(&self) -> u32 {
        self.outputs.len() as u32
    }

    fn output(&self, idx: u32) -> Option<&FunctionOutput> {
        self.outputs.get(idx as usize)
    }

    fn get_ir_function(&self, _ctx: &WispContext) -> IRFunction {
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
            data: self
                .data
                .iter()
                .map(|d| IRFunctionDataItem {
                    type_: d.type_.into(),
                })
                .collect(),
            ir: self.ir.clone(),
        }
    }

    fn lag_value(&self) -> Option<DataRef> {
        self.lag_value
    }

    fn load(s: &str, ctx: &WispContext) -> Option<Box<dyn WispFunction>>
    where
        Self: Sized,
    {
        CodeFunctionParser::new(s)
            .parse_function()
            .and_then(|r| match r {
                CodeFunctionParseResult::Function(f) => Some(Box::new(f) as Box<dyn WispFunction>),
                CodeFunctionParseResult::Alias(alias, name) => {
                    let func = ctx.get_function(&name)?;
                    Some(func.create_alias(alias))
                }
            })
    }

    fn save(&self) -> String {
        let mut s = String::new();
        if let Some(lag) = self.lag_value {
            s.push_str(&format!(
                "[lag_value: {}]\n",
                self.data[lag.0 as usize].name
            ));
        }
        s.push_str(&format!("func {}(", self.name));
        for (idx, input) in self.inputs.iter().enumerate() {
            if input.type_ == DataType::Float && input.fallback != DefaultInputValue::Value(0.0) {
                let fallback = format!(
                    "[default: {}] ",
                    match input.fallback {
                        DefaultInputValue::Value(v) => format!("{}", v),
                        DefaultInputValue::Normal => "normal".into(),
                        DefaultInputValue::Skip => "skip".into(),
                        DefaultInputValue::EmptyArray => unreachable!(),
                    }
                );
                s.push_str(fallback.as_str());
            }
            s.push_str(&format!("{}: {}", input.name, input.type_.to_str()));
            if idx < self.inputs.len() - 1 {
                s.push_str(", ");
            }
        }
        s.push_str(") -> (");
        for (idx, output) in self.outputs.iter().enumerate() {
            s.push_str(&format!("{}: {}", output.name, output.type_.to_str()));
            if idx < self.outputs.len() - 1 {
                s.push_str(", ");
            }
        }
        s.push_str(")\n");
        if !self.data.is_empty() {
            s.push_str("data\n");
            for item in &self.data {
                s.push_str(&format!("  {}: {}\n", item.name, item.type_.to_str()));
            }
        }
        s.push_str("begin\n");
        for insn in &self.ir {
            fn format_insn(
                insn: &Instruction,
                inputs: &[FunctionInput],
                outputs: &[FunctionOutput],
                data: &[FunctionDataItem],
            ) -> Vec<String> {
                let format_operand = |op: &Operand| match op {
                    Operand::Constant(c) => match c {
                        Constant::SampleRate => "SampleRate".to_owned(),
                        Constant::EmptyArray => "EmptyArray".to_owned(),
                    },
                    Operand::Literal(value) => format!("{}", value),
                    Operand::Var(vref) => format!("%{}", vref.0),
                    Operand::Arg(arg) => format!("${}", inputs[*arg as usize].name),
                };
                match insn {
                    Instruction::AllocLocal(lref) => vec![format!("alloc !{}", lref.0)],
                    Instruction::Load(vref, sloc) => vec![format!(
                        "load %{}, {}",
                        vref.0,
                        match sloc {
                            SourceLocation::Local(lref) => format!("!{}", lref.0),
                            SourceLocation::Data(dref) =>
                                format!("@{}", data[dref.0 as usize].name),
                            SourceLocation::LastValue(id, name, dref) =>
                                format!("last#{}({}@{})", id.0, name, dref.0),
                        }
                    )],
                    Instruction::Store(tloc, op) => vec![format!(
                        "store {}, {}",
                        match tloc {
                            TargetLocation::Local(lref) => format!("!{}", lref.0),
                            TargetLocation::Data(dref) =>
                                format!("@{}", data[dref.0 as usize].name),
                            TargetLocation::FunctionOutput(idx) =>
                                format!("#{}", outputs[idx.0 as usize].name),
                            TargetLocation::SignalOutput(idx) => format!(">{}", idx.0),
                        },
                        format_operand(op)
                    )],
                    Instruction::ILoad(vref, op_array, op_idx) => {
                        vec![format!(
                            "iload %{}, {}, {}",
                            vref.0,
                            format_operand(op_array),
                            format_operand(op_idx)
                        )]
                    }
                    Instruction::IStore(op_array, op_idx, op_value) => {
                        vec![format!(
                            "istore {}, {}, {}",
                            format_operand(op_array),
                            format_operand(op_idx),
                            format_operand(op_value)
                        )]
                    }
                    Instruction::Len(vref, op_array) => {
                        vec![format!("len %{}, {}", vref.0, format_operand(op_array))]
                    }
                    Instruction::BinaryOp(vref, type_, op0, op1) => vec![format!(
                        "{} %{}, {}, {}",
                        match type_ {
                            BinaryOpType::Add => "add",
                            BinaryOpType::Subtract => "sub",
                            BinaryOpType::Multiply => "mul",
                            BinaryOpType::Divide => "div",
                            BinaryOpType::Remainder => "rem",
                        },
                        vref.0,
                        format_operand(op0),
                        format_operand(op1)
                    )],
                    Instruction::ComparisonOp(vref, type_, op0, op1) => vec![format!(
                        "cmp.{} %{}, {}, {}",
                        match type_ {
                            ComparisonOpType::Equal => "eq",
                            ComparisonOpType::NotEqual => "ne",
                            ComparisonOpType::Less => "lt",
                            ComparisonOpType::LessOrEqual => "le",
                            ComparisonOpType::Greater => "gt",
                            ComparisonOpType::GreaterOrEqual => "ge",
                        },
                        vref.0,
                        format_operand(op0),
                        format_operand(op1)
                    )],
                    Instruction::Conditional(vref, then_branch, else_branch) => {
                        let mut result = vec![format!("if %{}", vref.0)];
                        for i in then_branch {
                            result.extend(
                                format_insn(i, inputs, outputs, data)
                                    .into_iter()
                                    .map(|s| "  ".to_owned() + &s),
                            );
                        }
                        if !else_branch.is_empty() {
                            result.push("else".into());
                            for i in else_branch {
                                result.extend(
                                    format_insn(i, inputs, outputs, data)
                                        .into_iter()
                                        .map(|s| "  ".to_owned() + &s),
                                );
                            }
                        }
                        result.push("end".into());
                        result
                    }
                    Instruction::Call(id, name, inputs, outputs) => vec![format!(
                        "{}#{}({}) -> ({})",
                        name,
                        id.0,
                        inputs
                            .iter()
                            .map(format_operand)
                            .collect::<Vec<_>>()
                            .join(", "),
                        outputs
                            .iter()
                            .map(|vref| format!("%{}", vref.0))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )],
                    Instruction::Debug(op) => vec![format!("debug {}", format_operand(op))],
                }
            }
            for line in format_insn(insn, &self.inputs, &self.outputs, &self.data) {
                s.push_str(&format!("  {}\n", line));
            }
        }
        s.push_str("end\n");
        s
    }

    fn create_alias(&self, name: String) -> Box<dyn WispFunction> {
        Box::new(CodeFunction {
            name,
            inputs: self.inputs.clone(),
            outputs: self.outputs.clone(),
            data: self.data.clone(),
            ir: self.ir.clone(),
            lag_value: self.lag_value,
        })
    }
}

impl CodeFunction {
    pub fn new(
        name: String,
        inputs: Vec<FunctionInput>,
        outputs: Vec<FunctionOutput>,
        data: Vec<FunctionDataItem>,
        instructions: Vec<Instruction>,
        lag_value: Option<DataRef>,
    ) -> Self {
        CodeFunction {
            name,
            inputs,
            outputs,
            data,
            ir: instructions,
            lag_value,
        }
    }
}

#[derive(Debug, PartialEq, Clone)]
pub enum CodeFunctionParseResult {
    Function(CodeFunction),
    Alias(String, String),
}

#[derive(Logos, Debug, PartialEq, Clone)]
#[logos(skip r"[ \t\r\n\f]+")]
#[logos(skip r"//.*\n")]
#[logos(skip r"/\*([^*]|\*[^/])*\*/")]
enum Token {
    #[token("func")]
    Func,
    #[token("data")]
    Data,
    #[token("begin")]
    Begin,
    #[token("end")]
    End,
    #[token("alias")]
    Alias,
    #[token("if")]
    If,
    #[token("else")]
    Else,
    #[token("alloc")]
    Alloc,
    #[token("load")]
    Load,
    #[token("store")]
    Store,
    #[token("istore")]
    IStore,
    #[token("iload")]
    ILoad,
    #[token("len")]
    Len,
    #[token("add")]
    Add,
    #[token("sub")]
    Sub,
    #[token("mul")]
    Mul,
    #[token("div")]
    Div,
    #[token("rem")]
    Rem,
    #[token("cmp.eq")]
    Equal,
    #[token("cmp.ne")]
    NotEqual,
    #[token("cmp.lt")]
    Less,
    #[token("cmp.le")]
    LessOrEqual,
    #[token("cmp.gt")]
    Greater,
    #[token("cmp.ge")]
    GreaterOrEqual,
    #[token("debug")]
    Debug,

    #[token(":")]
    Colon,
    #[token(",")]
    Comma,
    #[token("%")]
    VarPrefix,
    #[token("$")]
    ArgPrefix,
    #[token("#")]
    OutputPrefix,
    #[token("@")]
    DataPrefix,
    #[token("!")]
    LocalPrefix,
    #[token(">")]
    SignalPrefix,
    #[token("(")]
    OpenParen,
    #[token(")")]
    CloseParen,
    #[token("->")]
    Arrow,
    #[token("[")]
    OpenBracket,
    #[token("]")]
    CloseBracket,
    #[token("last")]
    Last,

    #[regex("[a-zA-Z_.]+", |lex| lex.slice().to_owned())]
    Identifier(String),
    #[regex("[0-9]+", priority = 3, callback = |lex| lex.slice().parse::<u32>().unwrap())]
    U32(u32),
    #[regex(r"-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?", |lex| lex.slice().parse::<f32>().unwrap())]
    F32(f32),
}

pub struct CodeFunctionParser<'source> {
    lex: Peekable<Lexer<'source, Token>>,
}

impl<'source> CodeFunctionParser<'source> {
    pub fn new(s: &'source str) -> CodeFunctionParser<'source> {
        CodeFunctionParser {
            lex: Token::lexer(s).peekable(),
        }
    }

    pub fn parse_function(&mut self) -> Option<CodeFunctionParseResult> {
        if self.peek_token()? == Token::Alias {
            self.next_token()?;
            let alias_name = self.parse_identifier()?;
            self.expect_token(Token::Colon)?;
            let target_name = self.parse_identifier()?;
            return Some(CodeFunctionParseResult::Alias(alias_name, target_name));
        }

        let mut symbols = Symbols::new();

        let mut func_attrs = self.parse_attributes()?;
        self.expect_token(Token::Func)?;
        let name = match self.next_token()? {
            Token::Identifier(id) => id.to_owned(),
            _ => return None,
        };

        self.expect_token(Token::OpenParen)?;
        let mut inputs = vec![];
        let mut input_attrs = None;
        loop {
            match self.peek_token()? {
                Token::CloseParen => {
                    self.next_token()?;
                    break;
                }
                Token::OpenBracket => {
                    input_attrs = Some(self.parse_attributes()?);
                }
                Token::Identifier(id) => {
                    let input_name = id.to_owned();
                    self.next_token()?;
                    self.expect_token(Token::Colon)?;
                    let input_type = match self.next_token()? {
                        Token::Identifier(id) => match id.as_str() {
                            "float" => DataType::Float,
                            "array" => DataType::Array,
                            _ => return None,
                        },
                        _ => return None,
                    };
                    let mut fallback = if input_type == DataType::Float {
                        DefaultInputValue::Value(0.0)
                    } else {
                        DefaultInputValue::EmptyArray
                    };
                    if let Some(attrs) = input_attrs.as_mut() {
                        if let Some(v) = attrs.remove("default") {
                            fallback = match v {
                                Some(Token::Identifier(id)) => match id.as_str() {
                                    "skip" => DefaultInputValue::Skip,
                                    "normal" => DefaultInputValue::Normal,
                                    _ => return None,
                                },
                                Some(Token::F32(v)) => DefaultInputValue::Value(v),
                                Some(Token::U32(v)) => DefaultInputValue::Value(v as f32),
                                None => {
                                    // TODO: Remove duplication
                                    if input_type == DataType::Float {
                                        DefaultInputValue::Value(0.0)
                                    } else {
                                        DefaultInputValue::EmptyArray
                                    }
                                }
                                _ => return None,
                            }
                        }
                        if !attrs.is_empty() {
                            return None;
                        }
                    }
                    if !symbols.insert(input_name.clone(), Symbol::Arg(inputs.len() as u32)) {
                        return None;
                    }
                    inputs.push(FunctionInput::new(input_name, input_type, fallback));
                    match self.peek_token()? {
                        Token::Comma => {
                            self.next_token()?;
                            continue;
                        }
                        Token::CloseParen => {}
                        _ => return None,
                    }
                }
                _ => return None,
            }
        }

        self.expect_token(Token::Arrow)?;

        self.expect_token(Token::OpenParen)?;
        let mut outputs = vec![];
        loop {
            match self.next_token()? {
                Token::CloseParen => {
                    break;
                }
                Token::Identifier(id) => {
                    self.expect_token(Token::Colon)?;
                    let output_type = match self.next_token()? {
                        Token::Identifier(id) => match id.as_str() {
                            "float" => DataType::Float,
                            "array" => DataType::Array,
                            _ => return None,
                        },
                        _ => return None,
                    };
                    if !symbols.insert(
                        id.clone(),
                        Symbol::FunctionOutput(FunctionOutputIndex(outputs.len() as u32)),
                    ) {
                        return None;
                    }
                    outputs.push(FunctionOutput::new(id, output_type));
                    match self.peek_token()? {
                        Token::Comma => {
                            self.next_token()?;
                            continue;
                        }
                        Token::CloseParen => {}
                        _ => return None,
                    }
                }
                _ => return None,
            }
        }

        let mut data = vec![];
        if self.peek_token()? == Token::Data {
            self.next_token()?;
            let mut token = self.peek_token()?;
            while let Token::Identifier(_) = token {
                let data_item_name = self.parse_identifier()?;
                self.expect_token(Token::Colon)?;
                let data_type = match self.next_token()? {
                    Token::Identifier(id) => match id.as_str() {
                        "float" => DataType::Float,
                        "array" => DataType::Array,
                        _ => return None,
                    },
                    _ => return None,
                };
                if !symbols.insert(
                    data_item_name.clone(),
                    Symbol::Data(DataRef(data.len() as u32)),
                ) {
                    return None;
                }
                data.push(FunctionDataItem {
                    name: data_item_name,
                    type_: data_type,
                });
                token = self.peek_token()?;
            }
        }

        let mut lag_value = None;
        if let Some(v) = func_attrs.remove("lag_value") {
            let dref = match v {
                Some(Token::U32(v)) => DataRef(v),
                Some(Token::Identifier(id)) => match symbols.get(&id) {
                    Some(Symbol::Data(d)) => d,
                    _ => return None,
                },
                _ => return None,
            };
            lag_value = Some(dref);
        }

        if !func_attrs.is_empty() {
            return None;
        }

        self.expect_token(Token::Begin)?;

        Some(CodeFunctionParseResult::Function(CodeFunction::new(
            name,
            inputs,
            outputs,
            data,
            self.parse_instructions(&mut symbols)?,
            lag_value,
        )))
    }

    fn parse_attributes(&mut self) -> Option<HashMap<String, Option<Token>>> {
        let mut attributes = HashMap::new();
        if let Some(Token::OpenBracket) = self.peek_token() {
            self.next_token()?;
        } else {
            return Some(attributes);
        }
        loop {
            match self.next_token()? {
                Token::CloseBracket => break,
                Token::Identifier(name) => {
                    let value = if let Some(Token::Colon) = self.peek_token() {
                        self.next_token()?;
                        let token = match self.next_token()? {
                            t @ Token::Identifier(_) | t @ Token::F32(_) | t @ Token::U32(_) => t,
                            _ => return None,
                        };
                        Some(token)
                    } else {
                        None
                    };
                    attributes.insert(name, value);
                }
                _ => return None,
            }
        }
        Some(attributes)
    }

    fn parse_instructions(&mut self, symbols: &mut Symbols) -> Option<Vec<Instruction>> {
        let mut instructions = vec![];
        loop {
            match self.next_token()? {
                Token::End => break,
                Token::If => {
                    let vref = self.parse_vref(symbols, false)?;
                    let then_branch = self.parse_instructions(symbols)?;
                    let else_branch = self.parse_instructions(symbols)?;
                    instructions.push(Instruction::Conditional(vref, then_branch, else_branch));
                }
                Token::Else => break,
                Token::Alloc => {
                    instructions.push(Instruction::AllocLocal(self.parse_lref(symbols, true)?))
                }
                Token::Load => {
                    let var_ref = self.parse_vref(symbols, true)?;
                    self.expect_token(Token::Comma)?;
                    let source_location = self.parse_sloc(symbols)?;
                    instructions.push(Instruction::Load(var_ref, source_location));
                }
                Token::Store => {
                    let target_location = self.parse_tloc(symbols)?;
                    self.expect_token(Token::Comma)?;
                    let operand = self.parse_op(symbols)?;
                    instructions.push(Instruction::Store(target_location, operand));
                }
                Token::ILoad => {
                    let vref = self.parse_vref(symbols, true)?;
                    self.expect_token(Token::Comma)?;
                    let op_array = self.parse_op(symbols)?;
                    self.expect_token(Token::Comma)?;
                    let op_idx = self.parse_op(symbols)?;
                    instructions.push(Instruction::ILoad(vref, op_array, op_idx));
                }
                Token::IStore => {
                    let op_array = self.parse_op(symbols)?;
                    self.expect_token(Token::Comma)?;
                    let op_idx = self.parse_op(symbols)?;
                    self.expect_token(Token::Comma)?;
                    let op_value = self.parse_op(symbols)?;
                    instructions.push(Instruction::IStore(op_array, op_idx, op_value));
                }
                Token::Len => {
                    let vref = self.parse_vref(symbols, true)?;
                    self.expect_token(Token::Comma)?;
                    let op_array = self.parse_op(symbols)?;
                    instructions.push(Instruction::Len(vref, op_array));
                }
                t @ Token::Add
                | t @ Token::Sub
                | t @ Token::Mul
                | t @ Token::Div
                | t @ Token::Rem => {
                    let vref = self.parse_vref(symbols, true)?;
                    self.expect_token(Token::Comma)?;
                    let op0 = self.parse_op(symbols)?;
                    self.expect_token(Token::Comma)?;
                    let op1 = self.parse_op(symbols)?;
                    let type_ = match t {
                        Token::Add => BinaryOpType::Add,
                        Token::Sub => BinaryOpType::Subtract,
                        Token::Mul => BinaryOpType::Multiply,
                        Token::Div => BinaryOpType::Divide,
                        Token::Rem => BinaryOpType::Remainder,
                        _ => unreachable!(),
                    };
                    instructions.push(Instruction::BinaryOp(vref, type_, op0, op1))
                }
                t @ Token::Equal
                | t @ Token::NotEqual
                | t @ Token::Less
                | t @ Token::LessOrEqual
                | t @ Token::Greater
                | t @ Token::GreaterOrEqual => {
                    let vref = self.parse_vref(symbols, true)?;
                    self.expect_token(Token::Comma)?;
                    let op0 = self.parse_op(symbols)?;
                    self.expect_token(Token::Comma)?;
                    let op1 = self.parse_op(symbols)?;
                    let type_ = match t {
                        Token::Equal => ComparisonOpType::Equal,
                        Token::NotEqual => ComparisonOpType::NotEqual,
                        Token::Less => ComparisonOpType::Less,
                        Token::LessOrEqual => ComparisonOpType::LessOrEqual,
                        Token::Greater => ComparisonOpType::Greater,
                        Token::GreaterOrEqual => ComparisonOpType::GreaterOrEqual,
                        _ => unreachable!(),
                    };
                    instructions.push(Instruction::ComparisonOp(vref, type_, op0, op1))
                }
                Token::Identifier(name) => {
                    let id = match self.next_token()? {
                        Token::OutputPrefix => {
                            let id = self.parse_u32()?;
                            self.expect_token(Token::OpenParen)?;
                            id
                        }
                        Token::OpenParen => 0,
                        _ => return None,
                    };
                    self.expect_token(Token::OpenParen)?;
                    let mut inputs = vec![];
                    loop {
                        inputs.push(self.parse_op(symbols)?);
                        match self.next_token()? {
                            Token::Comma => continue,
                            Token::CloseParen => break,
                            _ => return None,
                        }
                    }
                    let mut outputs = vec![];
                    self.expect_token(Token::Arrow)?;
                    match self.next_token()? {
                        Token::OpenParen => loop {
                            match self.next_token()? {
                                Token::VarPrefix => outputs.push(VarRef(self.parse_u32()?)),
                                Token::Comma => continue,
                                Token::CloseParen => break,
                                _ => return None,
                            }
                        },
                        _ => outputs.push(self.parse_vref(symbols, false)?),
                    }
                    instructions.push(Instruction::Call(CallId(id), name, inputs, outputs));
                }
                Token::Debug => instructions.push(Instruction::Debug(self.parse_op(symbols)?)),
                _ => return None,
            }
        }
        Some(instructions)
    }

    fn parse_vref(&mut self, symbols: &mut Symbols, allow_create: bool) -> Option<VarRef> {
        self.expect_token(Token::VarPrefix)?;
        match self.next_token()? {
            Token::U32(v) => Some(VarRef(v)),
            Token::Identifier(id) => match symbols.get(&id) {
                Some(Symbol::Var(v)) => Some(v),
                Some(_) => None,
                None => {
                    if allow_create {
                        let v = VarRef(
                            symbols
                                .known_symbols
                                .iter()
                                .filter(|s| matches!(s, Symbol::Var(_)))
                                .count() as u32,
                        );
                        symbols.insert(id, Symbol::Var(v));
                        Some(v)
                    } else {
                        None
                    }
                }
            },
            _ => None,
        }
    }

    fn parse_lref(&mut self, symbols: &mut Symbols, allow_create: bool) -> Option<LocalRef> {
        self.expect_token(Token::LocalPrefix)?;
        match self.next_token()? {
            Token::U32(v) => Some(LocalRef(v)),
            Token::Identifier(id) => match symbols.get(&id) {
                Some(Symbol::Local(l)) => Some(l),
                _ => {
                    if allow_create {
                        let l = LocalRef(
                            symbols
                                .known_symbols
                                .iter()
                                .filter(|s| matches!(s, Symbol::Local(_)))
                                .count() as u32,
                        );
                        symbols.insert(id, Symbol::Local(l));
                        Some(l)
                    } else {
                        None
                    }
                }
            },
            _ => None,
        }
    }

    fn parse_dref(&mut self, symbols: &Symbols) -> Option<DataRef> {
        self.expect_token(Token::DataPrefix)?;
        match self.next_token()? {
            Token::U32(v) => Some(DataRef(v)),
            Token::Identifier(id) => match symbols.get(&id) {
                Some(Symbol::Data(d)) => Some(d),
                _ => None,
            },
            _ => None,
        }
    }

    fn parse_tloc(&mut self, symbols: &Symbols) -> Option<TargetLocation> {
        match self.next_token()? {
            Token::LocalPrefix => match self.next_token()? {
                Token::U32(v) => Some(TargetLocation::Local(LocalRef(v))),
                Token::Identifier(id) => match symbols.get(&id) {
                    Some(Symbol::Local(l)) => Some(TargetLocation::Local(l)),
                    _ => None,
                },
                _ => None,
            },
            Token::DataPrefix => match self.next_token()? {
                Token::U32(v) => Some(TargetLocation::Data(DataRef(v))),
                Token::Identifier(id) => match symbols.get(&id) {
                    Some(Symbol::Data(d)) => Some(TargetLocation::Data(d)),
                    _ => None,
                },
                _ => None,
            },
            Token::OutputPrefix => match self.next_token()? {
                Token::U32(v) => Some(TargetLocation::FunctionOutput(FunctionOutputIndex(v))),
                Token::Identifier(id) => match symbols.get(&id) {
                    Some(Symbol::FunctionOutput(f)) => Some(TargetLocation::FunctionOutput(f)),
                    _ => None,
                },
                _ => None,
            },
            Token::SignalPrefix => match self.next_token()? {
                Token::U32(v) => Some(TargetLocation::SignalOutput(SignalOutputIndex(v))),
                _ => None,
            },
            _ => None,
        }
    }

    fn parse_sloc(&mut self, symbols: &Symbols) -> Option<SourceLocation> {
        match self.next_token()? {
            Token::LocalPrefix => match self.next_token()? {
                Token::U32(v) => Some(SourceLocation::Local(LocalRef(v))),
                Token::Identifier(id) => match symbols.get(&id) {
                    Some(Symbol::Local(l)) => Some(SourceLocation::Local(l)),
                    _ => None,
                },
                _ => None,
            },
            Token::DataPrefix => match self.next_token()? {
                Token::U32(v) => Some(SourceLocation::Data(DataRef(v))),
                Token::Identifier(id) => match symbols.get(&id) {
                    Some(Symbol::Data(d)) => Some(SourceLocation::Data(d)),
                    _ => None,
                },
                _ => None,
            },
            Token::Last => {
                self.expect_token(Token::OutputPrefix)?;
                let id = self.parse_u32()?;
                self.expect_token(Token::OpenParen)?;
                let name = self.parse_identifier()?;
                let dref = self.parse_dref(symbols)?;
                self.expect_token(Token::CloseParen)?;
                Some(SourceLocation::LastValue(CallId(id), name, dref))
            }
            _ => None,
        }
    }

    fn parse_op(&mut self, symbols: &Symbols) -> Option<Operand> {
        match self.next_token()? {
            Token::VarPrefix => match self.next_token()? {
                Token::U32(v) => Some(Operand::Var(VarRef(v))),
                Token::Identifier(id) => match symbols.get(&id) {
                    Some(Symbol::Var(v)) => Some(Operand::Var(v)),
                    _ => None,
                },
                _ => None,
            },
            Token::ArgPrefix => match self.next_token()? {
                Token::U32(v) => Some(Operand::Arg(v)),
                Token::Identifier(id) => match symbols.get(&id) {
                    Some(Symbol::Arg(a)) => Some(Operand::Arg(a)),
                    _ => None,
                },
                _ => None,
            },
            Token::F32(v) => Some(Operand::Literal(v)),
            Token::U32(v) => Some(Operand::Literal(v as f32)),
            Token::Identifier(id) => Some(Operand::Constant(match &id[..] {
                "SampleRate" => Constant::SampleRate,
                _ => return None,
            })),
            _ => None,
        }
    }

    fn parse_u32(&mut self) -> Option<u32> {
        if let Token::U32(v) = self.next_token()? {
            Some(v)
        } else {
            None
        }
    }

    fn parse_identifier(&mut self) -> Option<String> {
        if let Token::Identifier(id) = self.next_token()? {
            Some(id)
        } else {
            None
        }
    }

    fn next_token(&mut self) -> Option<Token> {
        self.lex.next()?.ok()
    }

    fn peek_token(&mut self) -> Option<Token> {
        self.lex.peek().map(|t| t.clone().ok())?
    }

    #[must_use]
    fn expect_token(&mut self, expected: Token) -> Option<()> {
        if let Ok(curr) = self.lex.next()? {
            if curr == expected {
                return Some(());
            } else {
                error!("Expected {:?}, got {:?}", expected, curr);
            }
        }
        None
    }
}

#[derive(Debug, Hash, PartialEq, Eq, Clone, Copy)]
enum Symbol {
    Var(VarRef),
    Local(LocalRef),
    Arg(u32),
    Data(DataRef),
    FunctionOutput(FunctionOutputIndex),
}

struct Symbols {
    symbols: HashMap<String, Symbol>,
    known_symbols: HashSet<Symbol>,
}

impl Symbols {
    pub fn new() -> Self {
        Symbols {
            symbols: HashMap::new(),
            known_symbols: HashSet::new(),
        }
    }

    pub fn insert(&mut self, name: String, symbol: Symbol) -> bool {
        if self.known_symbols.contains(&symbol) {
            return false;
        }
        self.known_symbols.insert(symbol);
        self.symbols.insert(name, symbol).is_none()
    }

    pub fn get(&self, name: &str) -> Option<Symbol> {
        self.symbols.get(name).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_function_lag() -> CodeFunction {
        CodeFunction {
            name: "lag".to_owned(),
            inputs: vec![FunctionInput {
                name: "value".to_owned(),
                type_: DataType::Float,
                fallback: DefaultInputValue::Skip,
            }],
            outputs: vec![FunctionOutput::new("out".to_owned(), DataType::Float)],
            data: vec![FunctionDataItem {
                name: "prev".to_owned(),
                type_: DataType::Float,
            }],
            ir: vec![
                Instruction::Load(VarRef(0), SourceLocation::Data(DataRef(0))),
                Instruction::Store(
                    TargetLocation::FunctionOutput(FunctionOutputIndex(0)),
                    Operand::Var(VarRef(0)),
                ),
                Instruction::Store(TargetLocation::Data(DataRef(0)), Operand::Arg(0)),
            ],
            lag_value: Some(DataRef(0)),
        }
    }

    #[test]
    fn test_parse_function() {
        let mut parser = CodeFunctionParser::new(
            r#"[lag_value: prev]
            func lag([default: skip] value: float) -> (out: float)
            data
              prev: float
            begin
              load %temp, @prev
              store #out, %temp
              store @prev, $value
            end"#,
        );
        assert_eq!(
            CodeFunctionParseResult::Function(create_test_function_lag()),
            parser.parse_function().unwrap()
        );
    }

    #[test]
    fn test_save_function() {
        assert_eq!(
            r#"[lag_value: prev]
func lag([default: skip] value: float) -> (out: float)
data
  prev: float
begin
  load %0, @prev
  store #out, %0
  store @prev, $value
end
"#,
            create_test_function_lag().save()
        );
    }
}
