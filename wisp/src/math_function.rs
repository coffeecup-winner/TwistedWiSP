use pest::{
    iterators::Pairs,
    pratt_parser::{Assoc, Op, PrattParser},
    Parser,
};
use pest_derive::Parser;
use twisted_wisp_ir::{
    BinaryOpType, Constant, FunctionOutputIndex, IRFunction, IRFunctionInput, IRFunctionOutput,
    Instruction, Operand, TargetLocation, VarRef,
};

use crate::{DefaultInputValue, FunctionInput, FunctionOutput, WispContext, WispFunction};

#[derive(Debug)]
pub struct MathFunction {
    name: String,
    #[allow(dead_code)]
    expr_string: String,
    #[allow(dead_code)]
    expr: MathExpression,
    inputs: Vec<FunctionInput>,
    outputs: Vec<FunctionOutput>,
}

impl WispFunction for MathFunction {
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
        self.compile_ir_function()
    }

    fn load(_s: &str) -> Option<Box<dyn WispFunction>>
    where
        Self: Sized,
    {
        panic!("Must not be called, use MathFunctionParser::parse_function instead");
    }

    fn save(&self) -> String {
        self.expr_string.clone()
    }
}

impl MathFunction {
    pub fn new(flow_name: &str, id: u32, expr_string: String, expr: MathExpression) -> Self {
        let mut inputs = vec![];
        for i in 0..Self::get_inputs_count(&expr) {
            inputs.push(FunctionInput::new(
                i.to_string(),
                DefaultInputValue::Value(0.0),
            ));
        }
        MathFunction {
            name: format!("math${}${}", flow_name, id),
            expr_string,
            expr,
            inputs,
            outputs: vec![FunctionOutput],
        }
    }

    fn get_inputs_count(expr: &MathExpression) -> u32 {
        match expr {
            MathExpression::Argument(idx) => idx + 1,
            MathExpression::BinaryOp(_, lhs, rhs) => {
                Self::get_inputs_count(lhs).max(Self::get_inputs_count(rhs))
            }
            _ => 0,
        }
    }

    fn compile_ir_function(&self) -> IRFunction {
        IRFunction {
            name: self.name.clone(),
            inputs: self.inputs.iter().map(|_| IRFunctionInput).collect(),
            outputs: self.outputs.iter().map(|_| IRFunctionOutput).collect(),
            data: vec![],
            ir: self.compile_ir(),
        }
    }

    fn compile_ir(&self) -> Vec<Instruction> {
        let mut result = vec![];
        let mut vref_id_gen = 0;
        let op = Self::compile(&self.expr, &mut result, &mut vref_id_gen);
        result.push(Instruction::Store(
            TargetLocation::FunctionOutput(FunctionOutputIndex(0)),
            op,
        ));
        result
    }

    fn compile(
        expr: &MathExpression,
        result: &mut Vec<Instruction>,
        vref_id_gen: &mut u32,
    ) -> Operand {
        match expr {
            MathExpression::Number(v) => Operand::Literal(*v),
            MathExpression::Argument(idx) => Operand::Arg(*idx),
            MathExpression::Constant(c) => Operand::Constant(*c),
            MathExpression::BinaryOp(type_, lhs, rhs) => {
                let vref = VarRef(*vref_id_gen);
                *vref_id_gen += 1;
                let op0 = Self::compile(lhs, result, vref_id_gen);
                let op1 = Self::compile(rhs, result, vref_id_gen);
                result.push(Instruction::BinaryOp(vref, *type_, op0, op1));
                Operand::Var(vref)
            }
        }
    }
}

#[derive(Parser)]
#[grammar = "math_function.pest"]
pub struct MathFunctionParser;

#[derive(Debug, Clone, PartialEq)]
pub enum MathExpression {
    Number(f32),
    Argument(u32),
    Constant(Constant),
    BinaryOp(BinaryOpType, Box<MathExpression>, Box<MathExpression>),
}

lazy_static::lazy_static! {
    static ref PARSER: PrattParser<Rule> = {
        PrattParser::new()
        .op(Op::infix(Rule::add, Assoc::Left) | Op::infix(Rule::subtract, Assoc::Left))
        .op(Op::infix(Rule::multiply, Assoc::Left) | Op::infix(Rule::divide, Assoc::Left) | Op::infix(Rule::remainder, Assoc::Left))
        .op(Op::prefix(Rule::unary_minus))
    };
}

impl MathFunctionParser {
    pub fn parse_function(flow_name: &str, id: u32, expr_string: String) -> Option<MathFunction> {
        let mut pairs = Self::parse(Rule::math_function, &expr_string).ok()?;
        let expr = Self::parse_expr(pairs.next().unwrap().into_inner())?;
        Some(MathFunction::new(flow_name, id, expr_string, expr))
    }

    fn parse_expr(pairs: Pairs<Rule>) -> Option<MathExpression> {
        PARSER
            .map_primary(|p| match p.as_rule() {
                Rule::f32 => Some(MathExpression::Number(p.as_str().parse::<f32>().unwrap())),
                Rule::arg => Some(MathExpression::Argument(
                    p.as_str()
                        .strip_prefix('$')
                        .unwrap()
                        .parse::<u32>()
                        .unwrap(),
                )),
                Rule::id => match p.as_str() {
                    "SampleRate" => Some(MathExpression::Constant(Constant::SampleRate)),
                    _ => None,
                },
                Rule::expr => Self::parse_expr(p.into_inner()),
                _ => None,
            })
            .map_infix(|lhs, op, rhs| {
                let type_ = match op.as_rule() {
                    Rule::add => BinaryOpType::Add,
                    Rule::subtract => BinaryOpType::Subtract,
                    Rule::multiply => BinaryOpType::Multiply,
                    Rule::divide => BinaryOpType::Divide,
                    Rule::remainder => BinaryOpType::Remainder,
                    _ => unreachable!(),
                };
                Some(MathExpression::BinaryOp(
                    type_,
                    Box::new(lhs?),
                    Box::new(rhs?),
                ))
            })
            .map_prefix(|op, rhs| match op.as_rule() {
                Rule::unary_minus => Some(MathExpression::BinaryOp(
                    BinaryOpType::Subtract,
                    Box::new(MathExpression::Number(0.0)),
                    Box::new(rhs?),
                )),
                _ => unreachable!(),
            })
            .parse(pairs)
    }
}
