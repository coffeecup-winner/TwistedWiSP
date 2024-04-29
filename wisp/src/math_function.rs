use pest::{
    iterators::Pairs,
    pratt_parser::{Assoc, Op, PrattParser},
    Parser,
};
use pest_derive::Parser;
use twisted_wisp_ir::{BinaryOpType, Constant};

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

    fn get_ir_function(&self, _ctx: &WispContext) -> twisted_wisp_ir::IRFunction {
        todo!()
    }

    fn load(_s: &str) -> Option<Box<dyn WispFunction>>
    where
        Self: Sized,
    {
        todo!()
    }

    fn save(&self) -> String {
        todo!()
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
}

#[derive(Parser)]
#[grammar = "math_function.pest"]
pub struct MathFunctionParser;

#[derive(Debug)]
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
        let pairs = Self::parse(Rule::math_function, &expr_string).ok()?;
        let expr = Self::parse_expr(pairs)?;
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
