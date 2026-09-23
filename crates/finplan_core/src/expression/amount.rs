use super::*;
use crate::model::AssetCoord;

/// An expression whose result is a transfer amount, rather than a scalar rate.
///
/// All amount calculations use the expression evaluator. Constructors build the
/// same flat program as the DSL compiler; no parsing is needed for Rust literals.
/// Numeric inputs are checked for finiteness when a run initializes. Parameters
/// retain their IDs until binding, so a configuration can be reused across runs.
#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct TransferAmount(Expression);

impl<'de> Deserialize<'de> for TransferAmount {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Expression::deserialize(deserializer)?
            .into_amount()
            .map_err(serde::de::Error::custom)
    }
}

impl TryFrom<Expression> for TransferAmount {
    type Error = ExpressionError;

    fn try_from(expression: Expression) -> Result<Self, Self::Error> {
        if expression.result_types()? & MONEY == 0 {
            return Err(ExpressionError::new(
                0..0,
                "transfer amount must have Money type",
            ));
        }
        Ok(Self(expression))
    }
}

impl TransferAmount {
    fn leaf(op: Op) -> Self {
        Self(Expression {
            program: vec![Instruction { op, span: 0..0 }],
        })
    }

    fn unary(mut self, op: Op) -> Self {
        self.0.program.push(Instruction { op, span: 0..0 });
        self
    }

    fn binary(mut self, other: Self, op: Op) -> Self {
        self.0.program.extend(other.0.program);
        self.unary(op)
    }

    /// A dollar literal. Non-finite values are rejected at run initialization.
    #[must_use]
    pub fn fixed(value: f64) -> Self {
        Self::leaf(Op::Literal(value, MONEY))
    }

    /// A Money parameter. To multiply by a Rate parameter use `scaled_rate`.
    #[must_use]
    pub fn parameter(id: ParameterId) -> Self {
        Self::leaf(Op::Parameter(id, MONEY))
    }

    #[must_use]
    pub fn account_balance(account: impl Into<AccountRef>) -> Self {
        Self::leaf(Op::Balance(account.into()))
    }

    #[must_use]
    pub fn cash_balance(account: impl Into<AccountRef>) -> Self {
        Self::leaf(Op::Cash(account.into()))
    }

    #[must_use]
    pub fn holding_balance(coord: AssetCoord) -> Self {
        Self::leaf(Op::Holding(
            AccountRef::Account(coord.account_id),
            coord.asset_id,
        ))
    }

    /// Read a specific source cash or asset endpoint. For an account-wide sale,
    /// use account balance minus cash with `AccountRef::Source` instead.
    #[must_use]
    pub fn source_balance() -> Self {
        Self::leaf(Op::EndpointBalance(AccountRef::Source))
    }

    #[must_use]
    pub fn target_balance() -> Self {
        Self::leaf(Op::EndpointBalance(AccountRef::Target))
    }

    #[must_use]
    pub fn net_worth() -> Self {
        Self::leaf(Op::NetWorth)
    }

    /// Adjust simulation-start dollars for cumulative inflation.
    #[must_use]
    pub fn inflated(self) -> Self {
        self.unary(Op::Inflate)
    }

    #[must_use]
    pub fn inflation_adjusted(value: f64) -> Self {
        Self::fixed(value).inflated()
    }

    /// Multiply a money amount by a scalar factor.
    #[must_use]
    pub fn scaled(factor: f64, amount: Self) -> Self {
        Self::leaf(Op::Literal(factor, SCALAR)).binary(amount, Op::Mul)
    }

    /// Multiply a money amount by a Rate parameter.
    #[must_use]
    pub fn scaled_rate(id: ParameterId, amount: Self) -> Self {
        Self::leaf(Op::Parameter(id, SCALAR)).binary(amount, Op::Mul)
    }

    #[must_use]
    pub fn percent_of_account(rate: f64, account_id: AccountId) -> Self {
        Self::scaled(rate, Self::account_balance(account_id))
    }

    #[must_use]
    pub fn plus(self, other: Self) -> Self {
        self.binary(other, Op::Add)
    }

    #[must_use]
    pub fn minus(self, other: Self) -> Self {
        self.binary(other, Op::Sub)
    }

    #[must_use]
    pub fn min(self, other: Self) -> Self {
        self.binary(other, Op::Min)
    }

    #[must_use]
    pub fn max(self, other: Self) -> Self {
        self.binary(other, Op::Max)
    }

    #[must_use]
    pub fn top_up(target: f64) -> Self {
        Self::fixed(target)
            .minus(Self::target_balance())
            .max(Self::fixed(0.0))
    }

    /// Pay off a liability using its negative account balance.
    #[must_use]
    pub fn payoff() -> Self {
        Self::leaf(Op::Balance(AccountRef::Target))
            .unary(Op::Negate)
            .max(Self::fixed(0.0))
    }

    #[must_use]
    pub fn up_to(amount: f64) -> Self {
        Self::fixed(amount).min(Self::source_balance())
    }

    #[must_use]
    pub fn excess_above(reserve: f64) -> Self {
        Self::source_balance()
            .minus(Self::fixed(reserve))
            .max(Self::fixed(0.0))
    }

    #[must_use]
    pub fn expression(&self) -> &Expression {
        &self.0
    }

    #[must_use]
    pub fn into_expression(self) -> Expression {
        self.0
    }

    pub fn to_source(&self, metadata: &SimulationMetadata) -> Result<String, ExpressionError> {
        self.0.to_source(metadata)
    }

    pub fn bind_parameters(
        &self,
        parameters: &HashMap<ParameterId, ParameterValue>,
        birth_date: Option<Date>,
    ) -> Result<Self, ExpressionError> {
        self.0
            .bind_parameters(parameters, birth_date)?
            .into_amount()
    }

    /// Replace the base value of a literal or `inflation(literal)` expression.
    /// Calculated and parameterized amounts are deliberately not rewritten.
    pub fn with_fixed_value(&self, value: f64) -> Result<Self, ExpressionError> {
        self.0.result_types()?;
        let mut program = self.0.program.as_slice();
        let inflated = program.last().is_some_and(|i| matches!(i.op, Op::Inflate));
        if inflated {
            program = &program[..program.len() - 1];
        }
        if !value.is_finite()
            || !matches!(program.first().map(|i| i.op), Some(Op::Literal(_, _)))
            || !program[1..]
                .iter()
                .all(|i| matches!(i.op, Op::Positive | Op::Negate))
        {
            return Err(ExpressionError::new(
                0..0,
                "value sweep requires a finite value and a literal or inflation(literal) amount",
            ));
        }
        let amount = Self::fixed(value);
        Ok(if inflated { amount.inflated() } else { amount })
    }

    /// Replace the single literal scalar factor in a root multiplication.
    /// Percent literals retain percent syntax; the supplied factor is fractional.
    pub fn with_scale_factor(&self, factor: f64) -> Result<Self, ExpressionError> {
        self.0.result_types()?;
        let error = || {
            ExpressionError::new(
                0..0,
                "multiplier sweep requires a finite factor and one unambiguous literal scalar in a root multiplication",
            )
        };
        if !factor.is_finite() || !matches!(self.0.program.last().map(|i| i.op), Some(Op::Mul)) {
            return Err(error());
        }
        let end = self.0.program.len() - 1;
        let split = operand_start(&self.0.program, end);
        let left = &self.0.program[..split];
        let right = &self.0.program[split..end];
        let kind = |program: &[Instruction]| {
            Expression {
                program: program.to_vec(),
            }
            .result_types()
        };
        let left_kind = kind(left)?;
        let right_kind = kind(right)?;
        let literal = |program: &[Instruction]| {
            let mut ops = program
                .iter()
                .map(|i| i.op)
                .filter(|op| !matches!(op, Op::Positive));
            if !matches!(ops.next(), Some(Op::Literal(..))) {
                return None;
            }
            match (ops.next(), ops.next()) {
                (None, None) => Some(false),
                (Some(Op::Percent), None) => Some(true),
                _ => None,
            }
        };
        let left_factor = (left_kind & SCALAR != 0 && right_kind & MONEY != 0)
            .then(|| literal(left))
            .flatten();
        let right_factor = (right_kind & SCALAR != 0 && left_kind & MONEY != 0)
            .then(|| literal(right))
            .flatten();
        let (index, percent) = match (left_factor, right_factor) {
            (Some(percent), None) => (0, percent),
            (None, Some(percent)) => (split, percent),
            _ => return Err(error()),
        };
        let replacement = if percent { factor * 100.0 } else { factor };
        if !replacement.is_finite() {
            return Err(error());
        }
        let mut updated = self.clone();
        if let Op::Literal(value, _) = &mut updated.0.program[index].op {
            *value = replacement;
        }
        Ok(updated)
    }
}

/// Find a root operand in an already validated stack program.
fn operand_start(program: &[Instruction], end: usize) -> usize {
    let mut needed = 1;
    let mut start = end;
    while needed > 0 {
        start -= 1;
        needed -= 1;
        needed += program[start].op.arity();
    }
    start
}
