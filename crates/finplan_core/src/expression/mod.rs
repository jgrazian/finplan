//! Text expressions for event amounts. See `spec/14_expression_dsl.md` for syntax.
//!
//! Compile names once, bind parameters per simulation run, then evaluate against
//! live state. `gross(...)` and `net(...)` are outer amount annotations, handled
//! by [`compile_amount`]; they do not perform a context-free tax conversion.
//!
//! ```
//! use finplan_core::{SimulationBuilder, expression::compile_amount};
//! use finplan_core::model::{EventEffect, TransferAmount};
//!
//! # fn main() -> Result<(), finplan_core::expression::ExpressionError> {
//! let (config, metadata) = SimulationBuilder::new()
//!     .bank("Checking", 10_000.0)
//!     .parameter("MonthlySpending", 500.0)
//!     .build();
//! let mut expense = EventEffect::Expense {
//!     from: metadata.account_id("Checking").unwrap(),
//!     amount: TransferAmount::fixed(0.0),
//! };
//! compile_amount("inflation($MonthlySpending)", &metadata, &config.parameters)?
//!     .apply_to(&mut expense)?;
//! // Add expense to an event; simulation initialization binds its parameters.
//! # Ok(())
//! # }
//! ```

mod amount;
mod parser;
mod render;
mod runtime;

#[cfg(test)]
mod tests;

use std::{collections::HashMap, fmt, ops::Range};

use jiff::civil::Date;
use serde::{Deserialize, Serialize};

use crate::{
    config::SimulationMetadata,
    model::{AccountId, AmountMode, AssetId, EventEffect, ParameterId, ParameterValue},
};

pub use amount::TransferAmount;
pub use runtime::EvaluationContext;

pub(crate) const MONEY: u8 = 1;
pub(crate) const SCALAR: u8 = 2;
const BOOLEAN: u8 = 4;
const NUMERIC: u8 = MONEY | SCALAR;
pub(crate) const MAX_INSTRUCTIONS: usize = 1024;

/// An error with a UTF-8 byte range suitable for highlighting in an editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpressionError {
    pub span: Range<usize>,
    pub message: String,
}

impl ExpressionError {
    pub(crate) fn new(span: Range<usize>, message: impl Into<String>) -> Self {
        Self {
            span,
            message: message.into(),
        }
    }
}

impl fmt::Display for ExpressionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} at bytes {}..{}",
            self.message, self.span.start, self.span.end
        )
    }
}

impl std::error::Error for ExpressionError {}

/// A reference to an account, resolved by name at compilation or by effect at evaluation.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum AccountRef {
    Account(AccountId),
    Source,
    Target,
}

impl From<AccountId> for AccountRef {
    fn from(id: AccountId) -> Self {
        Self::Account(id)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
enum DateRef {
    Literal(Date),
    Parameter(ParameterId),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
enum Op {
    Literal(f64, u8),
    Bool(bool),
    Parameter(ParameterId, u8),
    Balance(AccountRef),
    Cash(AccountRef),
    Holding(AccountRef, AssetId),
    EndpointBalance(AccountRef),
    NetWorth,
    Age,
    Year,
    Month,
    YearsSinceStart,
    DaysUntil(DateRef),
    YearsUntil(DateRef),
    AgeYears(ParameterId),
    Positive,
    Negate,
    Percent,
    Abs,
    Inflate,
    Add,
    Sub,
    Mul,
    Div,
    Min,
    Max,
    Clamp,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Equal,
    NotEqual,
    Not,
    And,
    Or,
    If,
}

impl Op {
    fn arity(self) -> usize {
        match self {
            Self::Positive
            | Self::Negate
            | Self::Percent
            | Self::Abs
            | Self::Inflate
            | Self::Not => 1,
            Self::Add
            | Self::Sub
            | Self::Mul
            | Self::Div
            | Self::Min
            | Self::Max
            | Self::Less
            | Self::LessEqual
            | Self::Greater
            | Self::GreaterEqual
            | Self::Equal
            | Self::NotEqual
            | Self::And
            | Self::Or => 2,
            Self::Clamp | Self::If => 3,
            _ => 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Instruction {
    op: Op,
    span: Range<usize>,
}

/// A bounded stack program. References use stable IDs, so renaming an account
/// does not change an already compiled expression. Persist source separately
/// for editing; compile it again when the user changes the text.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Expression {
    program: Vec<Instruction>,
}

/// Operand roots in the flat postfix program, derived while checking its types.
/// Runtime evaluation uses these to skip unselected branches without recursion.
struct ProgramAnalysis {
    result_type: u8,
    operands: Vec<[usize; 3]>,
}

impl Expression {
    /// Compile arithmetic, conditions, and functions, checking names and types.
    pub fn compile(
        source: &str,
        metadata: &SimulationMetadata,
        parameters: &HashMap<ParameterId, ParameterValue>,
    ) -> Result<Self, ExpressionError> {
        let (expression, mode) = parser::parse(source, metadata, parameters)?;
        if mode.is_some() {
            return Err(ExpressionError::new(
                0..source.len(),
                "use compile_amount for gross/net annotations",
            ));
        }
        Ok(expression)
    }

    /// Validate a numeric expression as an event amount (Money rather than Rate).
    pub fn into_amount(self) -> Result<TransferAmount, ExpressionError> {
        self.try_into()
    }

    /// Validate and substitute the current run's parameter values in a copy.
    /// The original keeps IDs, allowing optimizers to rebind each run.
    pub fn bind_parameters(
        &self,
        parameters: &HashMap<ParameterId, ParameterValue>,
        birth_date: Option<Date>,
    ) -> Result<Self, ExpressionError> {
        self.result_types()?;
        let mut bound = self.clone();
        for instruction in &mut bound.program {
            let error = |message| ExpressionError::new(instruction.span.clone(), message);
            let parameter = |id| {
                parameters
                    .get(&id)
                    .copied()
                    .filter(|p| p.is_valid())
                    .ok_or_else(|| error(format!("missing or invalid parameter {id:?}")))
            };
            instruction.op = match instruction.op {
                Op::Parameter(id, kind) => {
                    let value = match (kind, parameter(id)?) {
                        (MONEY, ParameterValue::Money(value))
                        | (SCALAR, ParameterValue::Rate(value)) => value,
                        _ => return Err(error(format!("parameter {id:?} changed type"))),
                    };
                    Op::Literal(value, kind)
                }
                Op::AgeYears(id) => match parameter(id)? {
                    ParameterValue::Age(age) => {
                        Op::Literal(f64::from(age.years) + f64::from(age.months) / 12.0, SCALAR)
                    }
                    _ => return Err(error(format!("parameter {id:?} must be Age"))),
                },
                Op::DaysUntil(DateRef::Parameter(id)) | Op::YearsUntil(DateRef::Parameter(id)) => {
                    let ParameterValue::Date(date) = parameter(id)? else {
                        return Err(error(format!("parameter {id:?} must be Date")));
                    };
                    if matches!(instruction.op, Op::DaysUntil(_)) {
                        Op::DaysUntil(DateRef::Literal(date))
                    } else {
                        Op::YearsUntil(DateRef::Literal(date))
                    }
                }
                Op::Age if birth_date.is_none() => {
                    return Err(error("age() requires birth_date".into()));
                }
                op => op,
            };
        }
        Ok(bound)
    }

    pub(crate) fn result_types(&self) -> Result<u8, ExpressionError> {
        Ok(self.analyze()?.result_type)
    }

    fn analyze(&self) -> Result<ProgramAnalysis, ExpressionError> {
        if self.program.is_empty() || self.program.len() > MAX_INSTRUCTIONS {
            return Err(ExpressionError::new(
                0..0,
                "invalid expression program size",
            ));
        }
        let mut stack = Vec::new();
        let mut operands = Vec::with_capacity(self.program.len());
        for (index, instruction) in self.program.iter().enumerate() {
            let error = |message| ExpressionError::new(instruction.span.clone(), message);
            let mut roots = [0; 3];
            let mut kinds = [0; 3];
            for position in (0..instruction.op.arity()).rev() {
                let (kind, root) = stack.pop().ok_or_else(|| error("missing operand"))?;
                roots[position] = root;
                kinds[position] = kind;
            }
            let [left, right, third] = kinds;
            let kind = match instruction.op {
                Op::Literal(value, kind) if value.is_finite() && (1..=3).contains(&kind) => kind,
                Op::Parameter(_, kind) if kind == MONEY || kind == SCALAR => kind,
                Op::Literal(..) | Op::Parameter(..) => {
                    return Err(error("invalid numeric value or type"));
                }
                Op::Bool(_) => BOOLEAN,
                Op::Balance(_)
                | Op::Cash(_)
                | Op::Holding(..)
                | Op::EndpointBalance(_)
                | Op::NetWorth => MONEY,
                Op::Age
                | Op::Year
                | Op::Month
                | Op::YearsSinceStart
                | Op::DaysUntil(_)
                | Op::YearsUntil(_)
                | Op::AgeYears(_) => SCALAR,
                Op::Positive | Op::Negate | Op::Abs => left & NUMERIC,
                Op::Percent => left & SCALAR,
                Op::Inflate => left & MONEY,
                Op::Add | Op::Sub | Op::Min | Op::Max => left & right & NUMERIC,
                Op::Clamp => left & right & third & NUMERIC,
                Op::Mul | Op::Div => binary_types(instruction.op, left, right),
                Op::Less | Op::LessEqual | Op::Greater | Op::GreaterEqual => {
                    if left & right & NUMERIC == 0 {
                        return Err(error("comparison requires compatible numeric types"));
                    }
                    BOOLEAN
                }
                Op::Equal | Op::NotEqual => {
                    if left & right == 0 {
                        return Err(error("equality requires compatible types"));
                    }
                    BOOLEAN
                }
                Op::Not => left & BOOLEAN,
                Op::And | Op::Or => left & right & BOOLEAN,
                Op::If => {
                    if left != BOOLEAN {
                        return Err(error("if condition must be Boolean"));
                    }
                    if right & third == 0 {
                        return Err(error("if branches must have compatible types"));
                    }
                    right & third
                }
            };
            if kind == 0 {
                return Err(error("incompatible Money, Rate, or Boolean types"));
            }
            stack.push((kind, index));
            operands.push(roots);
        }
        if stack.len() != 1 {
            return Err(ExpressionError::new(0..0, "invalid expression stack"));
        }
        Ok(ProgramAnalysis {
            result_type: stack[0].0,
            operands,
        })
    }
}

fn binary_types(op: Op, left: u8, right: u8) -> u8 {
    let mut result = 0;
    if left & MONEY != 0 && right & SCALAR != 0 {
        result |= MONEY;
    }
    if matches!(op, Op::Mul) && left & SCALAR != 0 && right & MONEY != 0 {
        result |= MONEY;
    }
    if left & SCALAR != 0 && right & SCALAR != 0
        || matches!(op, Op::Div) && left & MONEY != 0 && right & MONEY != 0
    {
        result |= SCALAR;
    }
    result
}

/// A compiled amount and optional outer gross/net annotation.
#[derive(Debug, Clone)]
pub struct CompiledAmount {
    pub amount: TransferAmount,
    pub amount_mode: Option<AmountMode>,
}

/// Compile a money expression, optionally wrapped in `gross(...)` or `net(...)`.
pub fn compile_amount(
    source: &str,
    metadata: &SimulationMetadata,
    parameters: &HashMap<ParameterId, ParameterValue>,
) -> Result<CompiledAmount, ExpressionError> {
    let (expression, amount_mode) = parser::parse(source, metadata, parameters)?;
    Ok(CompiledAmount {
        amount: expression.into_amount()?,
        amount_mode,
    })
}

impl CompiledAmount {
    /// Install the amount on an effect. Mode annotations are accepted only by
    /// effects whose existing tax machinery supports them. Failure is atomic.
    pub fn apply_to(self, effect: &mut EventEffect) -> Result<(), ExpressionError> {
        match effect {
            EventEffect::Income {
                amount,
                amount_mode,
                ..
            }
            | EventEffect::AssetSale {
                amount,
                amount_mode,
                ..
            }
            | EventEffect::Sweep {
                amount,
                amount_mode,
                ..
            } => {
                *amount = self.amount;
                if let Some(mode) = self.amount_mode {
                    *amount_mode = mode;
                }
            }
            EventEffect::Expense { amount, .. }
            | EventEffect::AssetPurchase { amount, .. }
            | EventEffect::AdjustBalance { amount, .. }
            | EventEffect::CashTransfer { amount, .. }
                if self.amount_mode.is_none() =>
            {
                *amount = self.amount
            }
            _ => {
                return Err(ExpressionError::new(
                    0..0,
                    "effect does not support this amount or gross/net annotation",
                ));
            }
        }
        Ok(())
    }
}
