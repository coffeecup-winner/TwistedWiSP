use pest::{
    iterators::Pairs,
    pratt_parser::{Assoc, Op, PrattParser},
    Parser,
};
use pest_derive::Parser;

use crate::{
    core::{DataType, DefaultInputValue, FunctionInput, FunctionOutput, WispContext, WispFunction},
    ir::{
        BinaryOpType, ComparisonOpType, Constant, FunctionOutputIndex, IRFunction, IRFunctionInput,
        IRFunctionOutput, Instruction, Operand, TargetLocation, VarRef,
    },
};

#[derive(Debug, Clone)]
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

    fn name_mut(&mut self) -> &mut String {
        &mut self.name
    }

    fn inputs(&self) -> &[FunctionInput] {
        &self.inputs
    }

    fn outputs(&self) -> &[FunctionOutput] {
        &self.outputs
    }

    fn get_ir_functions(&self, _ctx: &WispContext) -> Vec<IRFunction> {
        vec![self.compile_ir_function()]
    }

    fn save(&self) -> String {
        self.expr_string.clone()
    }
}

impl MathFunction {
    pub fn new(expr_string: String, expr: MathExpression) -> Self {
        let mut inputs = vec![];
        for i in 0..Self::get_inputs_count(&expr) {
            inputs.push(FunctionInput::new(
                i.to_string(),
                DataType::Float,
                DefaultInputValue::Value(0.0),
            ));
        }
        MathFunction {
            name: "$math".to_owned(),
            expr_string,
            expr,
            inputs,
            outputs: vec![FunctionOutput::new("out".to_owned(), DataType::Float)],
        }
    }

    fn get_inputs_count(expr: &MathExpression) -> u32 {
        match expr {
            MathExpression::Argument(idx) => idx + 1,
            MathExpression::BinaryOp(_, lhs, rhs) => {
                Self::get_inputs_count(lhs).max(Self::get_inputs_count(rhs))
            }
            MathExpression::ComparisonOp(_, lhs, rhs) => {
                Self::get_inputs_count(lhs).max(Self::get_inputs_count(rhs))
            }
            _ => 0,
        }
    }

    fn compile_ir_function(&self) -> IRFunction {
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
                let op0 = Self::compile(lhs, result, vref_id_gen);
                let op1 = Self::compile(rhs, result, vref_id_gen);
                let vref = VarRef(*vref_id_gen);
                *vref_id_gen += 1;
                result.push(Instruction::BinaryOp(vref, *type_, op0, op1));
                Operand::Var(vref)
            }
            MathExpression::ComparisonOp(type_, lhs, rhs) => {
                let op0 = Self::compile(lhs, result, vref_id_gen);
                let op1 = Self::compile(rhs, result, vref_id_gen);
                let vref = VarRef(*vref_id_gen);
                *vref_id_gen += 1;
                result.push(Instruction::ComparisonOp(vref, *type_, op0, op1));
                *vref_id_gen += 1;
                result.push(Instruction::BoolToFloat(vref, Operand::Var(vref)));
                Operand::Var(vref)
            }
        }
    }
}

#[derive(Parser)]
#[grammar = "core/math_function.pest"]
pub struct MathFunctionParser;

#[derive(Debug, Clone, PartialEq)]
pub enum MathExpression {
    Number(f32),
    Argument(u32),
    Constant(Constant),
    BinaryOp(BinaryOpType, Box<MathExpression>, Box<MathExpression>),
    ComparisonOp(ComparisonOpType, Box<MathExpression>, Box<MathExpression>),
}

lazy_static::lazy_static! {
    static ref PARSER: PrattParser<Rule> = {
        PrattParser::new()
        .op(Op::infix(Rule::less_than, Assoc::Left) | Op::infix(Rule::less_or_equal, Assoc::Left) | Op::infix(Rule::greater_than, Assoc::Left) | Op::infix(Rule::greater_or_equal, Assoc::Left) | Op::infix(Rule::equal, Assoc::Left) | Op::infix(Rule::not_equal, Assoc::Left))
        .op(Op::infix(Rule::add, Assoc::Left) | Op::infix(Rule::subtract, Assoc::Left))
        .op(Op::infix(Rule::multiply, Assoc::Left) | Op::infix(Rule::divide, Assoc::Left) | Op::infix(Rule::remainder, Assoc::Left))
        .op(Op::prefix(Rule::unary_minus))
    };
}

impl MathFunctionParser {
    pub fn parse_function(expr_string: &str) -> Option<MathFunction> {
        let mut pairs = Self::parse(Rule::math_function, expr_string).expect("X"); //.ok()?;
        let expr = Self::parse_expr(pairs.next().unwrap().into_inner())?;
        Some(MathFunction::new(expr_string.to_owned(), expr))
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
                enum Result {
                    BinaryOp(BinaryOpType),
                    ComparisonOp(ComparisonOpType),
                }
                let result = match op.as_rule() {
                    Rule::add => Result::BinaryOp(BinaryOpType::Add),
                    Rule::subtract => Result::BinaryOp(BinaryOpType::Subtract),
                    Rule::multiply => Result::BinaryOp(BinaryOpType::Multiply),
                    Rule::divide => Result::BinaryOp(BinaryOpType::Divide),
                    Rule::remainder => Result::BinaryOp(BinaryOpType::Remainder),
                    Rule::less_than => Result::ComparisonOp(ComparisonOpType::Less),
                    Rule::less_or_equal => Result::ComparisonOp(ComparisonOpType::LessOrEqual),
                    Rule::greater_than => Result::ComparisonOp(ComparisonOpType::Greater),
                    Rule::greater_or_equal => {
                        Result::ComparisonOp(ComparisonOpType::GreaterOrEqual)
                    }
                    Rule::equal => Result::ComparisonOp(ComparisonOpType::Equal),
                    Rule::not_equal => Result::ComparisonOp(ComparisonOpType::NotEqual),
                    _ => unreachable!(),
                };
                match result {
                    Result::BinaryOp(type_) => Some(MathExpression::BinaryOp(
                        type_,
                        Box::new(lhs?),
                        Box::new(rhs?),
                    )),
                    Result::ComparisonOp(type_) => Some(MathExpression::ComparisonOp(
                        type_,
                        Box::new(lhs?),
                        Box::new(rhs?),
                    )),
                }
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

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // MathExpression tests
    // ========================================================================

    fn parse_function(s: &str, expected_inputs_count: u32) -> MathFunction {
        let func = MathFunctionParser::parse_function(s).unwrap();
        assert_eq!("$math", func.name());
        assert_eq!(expected_inputs_count, func.inputs().len() as u32);
        assert_eq!(1, func.outputs().len());
        func
    }

    #[test]
    fn test_parse_number() {
        let func = parse_function("= 42", 0);
        assert_eq!(MathExpression::Number(42.0), func.expr);
    }

    #[test]
    fn test_parse_argument() {
        let func = parse_function("= $0", 1);
        assert_eq!(MathExpression::Argument(0), func.expr);
    }

    #[test]
    fn test_parse_constant() {
        let func = parse_function("= SampleRate", 0);
        assert_eq!(MathExpression::Constant(Constant::SampleRate), func.expr);
    }

    #[test]
    fn test_parse_addition() {
        let func = parse_function("= 1 + $0", 1);
        assert_eq!(
            MathExpression::BinaryOp(
                BinaryOpType::Add,
                Box::new(MathExpression::Number(1.0)),
                Box::new(MathExpression::Argument(0))
            ),
            func.expr
        );
    }

    #[test]
    fn test_parse_subtraction() {
        let func = parse_function("= $0 - $1", 2);
        assert_eq!(
            MathExpression::BinaryOp(
                BinaryOpType::Subtract,
                Box::new(MathExpression::Argument(0)),
                Box::new(MathExpression::Argument(1))
            ),
            func.expr
        );
    }

    #[test]
    fn test_parse_multiplication() {
        let func = parse_function("= $0 * 3", 1);
        assert_eq!(
            MathExpression::BinaryOp(
                BinaryOpType::Multiply,
                Box::new(MathExpression::Argument(0)),
                Box::new(MathExpression::Number(3.0))
            ),
            func.expr
        );
    }

    #[test]
    fn test_parse_division() {
        let func = parse_function("= 6 / 2.5", 0);
        assert_eq!(
            MathExpression::BinaryOp(
                BinaryOpType::Divide,
                Box::new(MathExpression::Number(6.0)),
                Box::new(MathExpression::Number(2.5))
            ),
            func.expr
        );
    }

    #[test]
    fn test_parse_remainder() {
        let func = parse_function("= 7.42 % -0.3", 0);
        assert_eq!(
            MathExpression::BinaryOp(
                BinaryOpType::Remainder,
                Box::new(MathExpression::Number(7.42)),
                Box::new(MathExpression::Number(-0.3))
            ),
            func.expr
        );
    }

    #[test]
    fn test_parse_unary_minus() {
        let func = parse_function("= -(-$0) * -SampleRate", 1);
        assert_eq!(
            MathExpression::BinaryOp(
                BinaryOpType::Multiply,
                Box::new(MathExpression::BinaryOp(
                    BinaryOpType::Subtract,
                    Box::new(MathExpression::Number(0.0)),
                    Box::new(MathExpression::BinaryOp(
                        BinaryOpType::Subtract,
                        Box::new(MathExpression::Number(0.0)),
                        Box::new(MathExpression::Argument(0))
                    ))
                )),
                Box::new(MathExpression::BinaryOp(
                    BinaryOpType::Subtract,
                    Box::new(MathExpression::Number(0.0)),
                    Box::new(MathExpression::Constant(Constant::SampleRate))
                ))
            ),
            func.expr
        );
    }

    #[test]
    fn test_parse_complex_expression() {
        let func = parse_function("= 1 + 2 * $0 - 3 / 4.78 % 0.1e-10", 1);
        assert_eq!(
            MathExpression::BinaryOp(
                BinaryOpType::Subtract,
                Box::new(MathExpression::BinaryOp(
                    BinaryOpType::Add,
                    Box::new(MathExpression::Number(1.0)),
                    Box::new(MathExpression::BinaryOp(
                        BinaryOpType::Multiply,
                        Box::new(MathExpression::Number(2.0)),
                        Box::new(MathExpression::Argument(0))
                    ))
                )),
                Box::new(MathExpression::BinaryOp(
                    BinaryOpType::Remainder,
                    Box::new(MathExpression::BinaryOp(
                        BinaryOpType::Divide,
                        Box::new(MathExpression::Number(3.0)),
                        Box::new(MathExpression::Number(4.78))
                    )),
                    Box::new(MathExpression::Number(0.1e-10))
                ))
            ),
            func.expr
        );
    }

    #[test]
    fn test_parse_complex_expression_with_parentheses() {
        let func = parse_function("= (1 + 2) * ($0 - 3) / (4.78 % 0.1e-10)", 1);
        assert_eq!(
            MathExpression::BinaryOp(
                BinaryOpType::Divide,
                Box::new(MathExpression::BinaryOp(
                    BinaryOpType::Multiply,
                    Box::new(MathExpression::BinaryOp(
                        BinaryOpType::Add,
                        Box::new(MathExpression::Number(1.0)),
                        Box::new(MathExpression::Number(2.0))
                    )),
                    Box::new(MathExpression::BinaryOp(
                        BinaryOpType::Subtract,
                        Box::new(MathExpression::Argument(0)),
                        Box::new(MathExpression::Number(3.0))
                    ))
                )),
                Box::new(MathExpression::BinaryOp(
                    BinaryOpType::Remainder,
                    Box::new(MathExpression::Number(4.78)),
                    Box::new(MathExpression::Number(0.1e-10))
                ))
            ),
            func.expr
        );
    }

    // ========================================================================
    // IRFunction tests
    // ========================================================================

    fn get_ir_function(f: MathFunction) -> IRFunction {
        let ctx = WispContext::new(2, 44100);
        let ir_func = f.get_ir_functions(&ctx)[0].clone();
        assert_eq!(f.name(), ir_func.name);
        assert_eq!(f.inputs().len(), ir_func.inputs.len());
        assert_eq!(f.outputs().len(), ir_func.outputs.len());
        ir_func
    }

    #[test]
    fn test_ir_number() {
        let func = get_ir_function(parse_function("= 42", 0));
        assert_eq!(
            vec![Instruction::Store(
                TargetLocation::FunctionOutput(FunctionOutputIndex(0)),
                Operand::Literal(42.0)
            )],
            func.ir
        );
    }

    #[test]
    fn test_ir_argument() {
        let func = get_ir_function(parse_function("= $0", 1));
        assert_eq!(
            vec![Instruction::Store(
                TargetLocation::FunctionOutput(FunctionOutputIndex(0)),
                Operand::Arg(0)
            )],
            func.ir
        );
    }

    #[test]
    fn test_ir_constant() {
        let func = get_ir_function(parse_function("= SampleRate", 0));
        assert_eq!(
            vec![Instruction::Store(
                TargetLocation::FunctionOutput(FunctionOutputIndex(0)),
                Operand::Constant(Constant::SampleRate)
            )],
            func.ir
        );
    }

    #[test]
    fn test_ir_addition() {
        let func = get_ir_function(parse_function("= 1 + $0", 1));
        assert_eq!(
            vec![
                Instruction::BinaryOp(
                    VarRef(0),
                    BinaryOpType::Add,
                    Operand::Literal(1.0),
                    Operand::Arg(0)
                ),
                Instruction::Store(
                    TargetLocation::FunctionOutput(FunctionOutputIndex(0)),
                    Operand::Var(VarRef(0))
                )
            ],
            func.ir
        );
    }

    #[test]
    fn test_ir_subtraction() {
        let func = get_ir_function(parse_function("= $0 - $1", 2));
        assert_eq!(
            vec![
                Instruction::BinaryOp(
                    VarRef(0),
                    BinaryOpType::Subtract,
                    Operand::Arg(0),
                    Operand::Arg(1)
                ),
                Instruction::Store(
                    TargetLocation::FunctionOutput(FunctionOutputIndex(0)),
                    Operand::Var(VarRef(0))
                )
            ],
            func.ir
        );
    }

    #[test]
    fn test_ir_multiplication() {
        let func = get_ir_function(parse_function("= $0 * 3", 1));
        assert_eq!(
            vec![
                Instruction::BinaryOp(
                    VarRef(0),
                    BinaryOpType::Multiply,
                    Operand::Arg(0),
                    Operand::Literal(3.0)
                ),
                Instruction::Store(
                    TargetLocation::FunctionOutput(FunctionOutputIndex(0)),
                    Operand::Var(VarRef(0))
                )
            ],
            func.ir
        );
    }

    #[test]
    fn test_ir_division() {
        let func = get_ir_function(parse_function("= 6 / 2.5", 0));
        assert_eq!(
            vec![
                Instruction::BinaryOp(
                    VarRef(0),
                    BinaryOpType::Divide,
                    Operand::Literal(6.0),
                    Operand::Literal(2.5)
                ),
                Instruction::Store(
                    TargetLocation::FunctionOutput(FunctionOutputIndex(0)),
                    Operand::Var(VarRef(0))
                )
            ],
            func.ir
        );
    }

    #[test]
    fn test_ir_remainder() {
        let func = get_ir_function(parse_function("= 7.42 % -0.3", 0));
        assert_eq!(
            vec![
                Instruction::BinaryOp(
                    VarRef(0),
                    BinaryOpType::Remainder,
                    Operand::Literal(7.42),
                    Operand::Literal(-0.3)
                ),
                Instruction::Store(
                    TargetLocation::FunctionOutput(FunctionOutputIndex(0)),
                    Operand::Var(VarRef(0))
                )
            ],
            func.ir
        );
    }

    #[test]
    fn test_ir_unary_minus() {
        let func = get_ir_function(parse_function("= -(-$0) * -SampleRate", 1));
        assert_eq!(
            vec![
                Instruction::BinaryOp(
                    VarRef(0),
                    BinaryOpType::Subtract,
                    Operand::Literal(0.0),
                    Operand::Arg(0)
                ),
                Instruction::BinaryOp(
                    VarRef(1),
                    BinaryOpType::Subtract,
                    Operand::Literal(0.0),
                    Operand::Var(VarRef(0)),
                ),
                Instruction::BinaryOp(
                    VarRef(2),
                    BinaryOpType::Subtract,
                    Operand::Literal(0.0),
                    Operand::Constant(Constant::SampleRate)
                ),
                Instruction::BinaryOp(
                    VarRef(3),
                    BinaryOpType::Multiply,
                    Operand::Var(VarRef(1)),
                    Operand::Var(VarRef(2))
                ),
                Instruction::Store(
                    TargetLocation::FunctionOutput(FunctionOutputIndex(0)),
                    Operand::Var(VarRef(3))
                )
            ],
            func.ir
        );
    }

    #[test]
    fn test_ir_complex_expression() {
        let func = get_ir_function(parse_function("= 1 + 2 * $0 - 3 / 4.78 % 0.1e-10", 1));
        assert_eq!(
            vec![
                Instruction::BinaryOp(
                    VarRef(0),
                    BinaryOpType::Multiply,
                    Operand::Literal(2.0),
                    Operand::Arg(0)
                ),
                Instruction::BinaryOp(
                    VarRef(1),
                    BinaryOpType::Add,
                    Operand::Literal(1.0),
                    Operand::Var(VarRef(0))
                ),
                Instruction::BinaryOp(
                    VarRef(2),
                    BinaryOpType::Divide,
                    Operand::Literal(3.0),
                    Operand::Literal(4.78)
                ),
                Instruction::BinaryOp(
                    VarRef(3),
                    BinaryOpType::Remainder,
                    Operand::Var(VarRef(2)),
                    Operand::Literal(0.1e-10)
                ),
                Instruction::BinaryOp(
                    VarRef(4),
                    BinaryOpType::Subtract,
                    Operand::Var(VarRef(1)),
                    Operand::Var(VarRef(3))
                ),
                Instruction::Store(
                    TargetLocation::FunctionOutput(FunctionOutputIndex(0)),
                    Operand::Var(VarRef(4))
                )
            ],
            func.ir
        );
    }

    #[test]
    fn test_ir_complex_expression_with_parentheses() {
        let func = get_ir_function(parse_function("= (1 + 2) * ($0 - 3) / (4.78 % 0.1e-10)", 1));
        assert_eq!(
            vec![
                Instruction::BinaryOp(
                    VarRef(0),
                    BinaryOpType::Add,
                    Operand::Literal(1.0),
                    Operand::Literal(2.0)
                ),
                Instruction::BinaryOp(
                    VarRef(1),
                    BinaryOpType::Subtract,
                    Operand::Arg(0),
                    Operand::Literal(3.0)
                ),
                Instruction::BinaryOp(
                    VarRef(2),
                    BinaryOpType::Multiply,
                    Operand::Var(VarRef(0)),
                    Operand::Var(VarRef(1))
                ),
                Instruction::BinaryOp(
                    VarRef(3),
                    BinaryOpType::Remainder,
                    Operand::Literal(4.78),
                    Operand::Literal(0.1e-10)
                ),
                Instruction::BinaryOp(
                    VarRef(4),
                    BinaryOpType::Divide,
                    Operand::Var(VarRef(2)),
                    Operand::Var(VarRef(3))
                ),
                Instruction::Store(
                    TargetLocation::FunctionOutput(FunctionOutputIndex(0)),
                    Operand::Var(VarRef(4))
                )
            ],
            func.ir
        );
    }

    #[test]
    fn test_ir_less_than() {
        let func = get_ir_function(parse_function("= $0 < 1.0", 1));
        assert_eq!(
            vec![
                Instruction::ComparisonOp(
                    VarRef(0),
                    ComparisonOpType::Less,
                    Operand::Arg(0),
                    Operand::Literal(1.0)
                ),
                Instruction::BoolToFloat(VarRef(0), Operand::Var(VarRef(0))),
                Instruction::Store(
                    TargetLocation::FunctionOutput(FunctionOutputIndex(0)),
                    Operand::Var(VarRef(0))
                ),
            ],
            func.ir
        );
    }

    #[test]
    fn test_ir_less_or_equal() {
        let func = get_ir_function(parse_function("= 1.0 <= $0", 1));
        assert_eq!(
            vec![
                Instruction::ComparisonOp(
                    VarRef(0),
                    ComparisonOpType::LessOrEqual,
                    Operand::Literal(1.0),
                    Operand::Arg(0)
                ),
                Instruction::BoolToFloat(VarRef(0), Operand::Var(VarRef(0))),
                Instruction::Store(
                    TargetLocation::FunctionOutput(FunctionOutputIndex(0)),
                    Operand::Var(VarRef(0))
                ),
            ],
            func.ir
        );
    }

    #[test]
    fn test_ir_greater_than() {
        let func = get_ir_function(parse_function("= $0 > $1", 2));
        assert_eq!(
            vec![
                Instruction::ComparisonOp(
                    VarRef(0),
                    ComparisonOpType::Greater,
                    Operand::Arg(0),
                    Operand::Arg(1)
                ),
                Instruction::BoolToFloat(VarRef(0), Operand::Var(VarRef(0))),
                Instruction::Store(
                    TargetLocation::FunctionOutput(FunctionOutputIndex(0)),
                    Operand::Var(VarRef(0))
                ),
            ],
            func.ir
        );
    }

    #[test]
    fn test_ir_greater_or_equal() {
        let func = get_ir_function(parse_function("= $0 >= $0", 1));
        assert_eq!(
            vec![
                Instruction::ComparisonOp(
                    VarRef(0),
                    ComparisonOpType::GreaterOrEqual,
                    Operand::Arg(0),
                    Operand::Arg(0)
                ),
                Instruction::BoolToFloat(VarRef(0), Operand::Var(VarRef(0))),
                Instruction::Store(
                    TargetLocation::FunctionOutput(FunctionOutputIndex(0)),
                    Operand::Var(VarRef(0))
                ),
            ],
            func.ir
        );
    }

    #[test]
    fn test_ir_equal() {
        let func = get_ir_function(parse_function("= $0 == -1.0", 1));
        assert_eq!(
            vec![
                Instruction::ComparisonOp(
                    VarRef(0),
                    ComparisonOpType::Equal,
                    Operand::Arg(0),
                    Operand::Literal(-1.0)
                ),
                Instruction::BoolToFloat(VarRef(0), Operand::Var(VarRef(0))),
                Instruction::Store(
                    TargetLocation::FunctionOutput(FunctionOutputIndex(0)),
                    Operand::Var(VarRef(0))
                ),
            ],
            func.ir
        );
    }

    #[test]
    fn test_ir_not_equal() {
        let func = get_ir_function(parse_function("= 1.0 != $0", 1));
        assert_eq!(
            vec![
                Instruction::ComparisonOp(
                    VarRef(0),
                    ComparisonOpType::NotEqual,
                    Operand::Literal(1.0),
                    Operand::Arg(0)
                ),
                Instruction::BoolToFloat(VarRef(0), Operand::Var(VarRef(0))),
                Instruction::Store(
                    TargetLocation::FunctionOutput(FunctionOutputIndex(0)),
                    Operand::Var(VarRef(0))
                ),
            ],
            func.ir
        );
    }
}
