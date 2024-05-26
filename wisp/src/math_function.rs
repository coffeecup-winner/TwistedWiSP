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

use crate::{
    DataType, DefaultInputValue, FunctionInput, FunctionOutput, WispContext, WispFunction,
};

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
                DataType::Float,
                DefaultInputValue::Value(0.0),
            ));
        }
        MathFunction {
            name: format!("$math${}${}", flow_name, id),
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
                let op0 = Self::compile(lhs, result, vref_id_gen);
                let op1 = Self::compile(rhs, result, vref_id_gen);
                let vref = VarRef(*vref_id_gen);
                *vref_id_gen += 1;
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
    pub fn parse_function(flow_name: &str, id: u32, expr_string: &str) -> Option<MathFunction> {
        let mut pairs = Self::parse(Rule::math_function, expr_string).ok()?;
        let expr = Self::parse_expr(pairs.next().unwrap().into_inner())?;
        Some(MathFunction::new(
            flow_name,
            id,
            expr_string.to_owned(),
            expr,
        ))
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

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // MathExpression tests
    // ========================================================================

    fn parse_function(s: &str, expected_inputs_count: u32) -> MathFunction {
        let func = MathFunctionParser::parse_function("test", 0, s).unwrap();
        assert_eq!("$math$test$0", func.name());
        assert_eq!(expected_inputs_count, func.inputs_count());
        assert_eq!(1, func.outputs_count());
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
        let ctx = WispContext::new(2);
        let ir_func = f.get_ir_function(&ctx);
        assert_eq!(f.name(), ir_func.name);
        assert_eq!(f.inputs_count(), ir_func.inputs.len() as u32);
        assert_eq!(f.outputs_count(), ir_func.outputs.len() as u32);
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
}
