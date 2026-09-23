use super::*;
use crate::{
    model::{AssetCoord, TransferEndpoint},
    simulation_state::SimulationState,
};

/// The current simulation and effect references. Account-wide liquidation has
/// a source account but no single cash or asset endpoint.
pub struct EvaluationContext<'a> {
    pub state: &'a SimulationState,
    pub source: TransferEndpoint,
    pub target: TransferEndpoint,
    source_account: Option<AccountId>,
}

impl<'a> EvaluationContext<'a> {
    pub fn new(state: &'a SimulationState) -> Self {
        Self {
            state,
            source: TransferEndpoint::External,
            target: TransferEndpoint::External,
            source_account: None,
        }
    }

    pub fn with_endpoints(mut self, source: TransferEndpoint, target: TransferEndpoint) -> Self {
        self.source = source;
        self.target = target;
        self.source_account = None;
        self
    }

    /// Bind an account-wide source without inventing a transfer endpoint.
    /// `balance(source)` and `cash(source)` work; `source_balance()` is undefined.
    pub fn with_source_account(mut self, account_id: AccountId) -> Self {
        self.source = TransferEndpoint::External;
        self.source_account = Some(account_id);
        self
    }

    fn endpoint(&self, reference: AccountRef) -> TransferEndpoint {
        match reference {
            AccountRef::Source => self.source,
            AccountRef::Target => self.target,
            AccountRef::Account(account_id) => TransferEndpoint::Cash { account_id },
        }
    }

    fn account(&self, reference: AccountRef) -> Result<AccountId, String> {
        if let AccountRef::Source = reference
            && let Some(account_id) = self.source_account
        {
            return Ok(account_id);
        }
        match self.endpoint(reference) {
            TransferEndpoint::Cash { account_id } => Ok(account_id),
            TransferEndpoint::Asset { asset_coord } => Ok(asset_coord.account_id),
            TransferEndpoint::External => Err(format!(
                "{reference:?} has no single account in this effect"
            )),
        }
    }

    fn endpoint_balance(&self, reference: AccountRef) -> Result<f64, String> {
        match self.endpoint(reference) {
            TransferEndpoint::Cash { account_id } => self
                .state
                .account_cash_balance(account_id)
                .map_err(|e| e.to_string()),
            TransferEndpoint::Asset { asset_coord } => self
                .state
                .asset_balance(asset_coord)
                .map_err(|e| e.to_string()),
            TransferEndpoint::External => Err(format!(
                "{reference:?} has no single endpoint balance in this effect"
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Value {
    Number(f64),
    Boolean(bool),
}

impl Value {
    fn number(self) -> Result<f64, String> {
        match self {
            Self::Number(value) => Ok(value),
            Self::Boolean(_) => Err("expected a numeric value".into()),
        }
    }

    fn boolean(self) -> Result<bool, String> {
        match self {
            Self::Boolean(value) => Ok(value),
            Self::Number(_) => Err("expected a Boolean value".into()),
        }
    }
}

impl Expression {
    /// Evaluate a bound numeric expression without modifying simulation state.
    /// `if`, `and`, and `or` only evaluate the branch or operand they need.
    /// Boolean results require `evaluate_bool`; they never become zero or one.
    pub fn evaluate(&self, context: &EvaluationContext<'_>) -> Result<f64, ExpressionError> {
        match self.evaluate_value(context, NUMERIC)? {
            Value::Number(value) => Ok(value),
            Value::Boolean(_) => unreachable!("numeric result checked before evaluation"),
        }
    }

    /// Preview a condition. Numbers do not implicitly convert to booleans.
    pub fn evaluate_bool(&self, context: &EvaluationContext<'_>) -> Result<bool, ExpressionError> {
        match self.evaluate_value(context, BOOLEAN)? {
            Value::Boolean(value) => Ok(value),
            Value::Number(_) => unreachable!("Boolean result checked before evaluation"),
        }
    }

    fn evaluate_value(
        &self,
        context: &EvaluationContext<'_>,
        expected: u8,
    ) -> Result<Value, ExpressionError> {
        // Analyze the entire program (including untaken branches) before use.
        // Operand indexes are derived from validated postfix instructions, never
        // trusted jump offsets from serialized input. The work stack is iterative.
        let analysis = self.analyze()?;
        let root = self.program.len() - 1;
        if analysis.result_type & expected == 0 {
            return Err(ExpressionError::new(
                self.program[root].span.clone(),
                if expected == BOOLEAN {
                    "expected a Boolean expression"
                } else {
                    "expected a numeric expression; use evaluate_bool for a condition"
                },
            ));
        }
        let mut values: Vec<Option<Value>> = vec![None; self.program.len()];
        let mut pending = vec![root];
        let state = context.state;
        while let Some(&index) = pending.last() {
            let instruction = &self.program[index];
            let roots = analysis.operands[index];
            let error = |message| ExpressionError::new(instruction.span.clone(), message);
            let dependency = (|| -> Result<Option<usize>, String> {
                match instruction.op {
                    Op::If => {
                        let Some(condition) = values[roots[0]] else {
                            return Ok(Some(roots[0]));
                        };
                        let selected = roots[if condition.boolean()? { 1 } else { 2 }];
                        Ok(values[selected].is_none().then_some(selected))
                    }
                    Op::And | Op::Or => {
                        let Some(left) = values[roots[0]] else {
                            return Ok(Some(roots[0]));
                        };
                        let left = left.boolean()?;
                        if matches!(instruction.op, Op::And) && !left
                            || matches!(instruction.op, Op::Or) && left
                        {
                            Ok(None)
                        } else {
                            Ok(values[roots[1]].is_none().then_some(roots[1]))
                        }
                    }
                    _ => Ok(roots[..instruction.op.arity()]
                        .iter()
                        .copied()
                        .find(|&child| values[child].is_none())),
                }
            })()
            .map_err(&error)?;
            if let Some(child) = dependency {
                pending.push(child);
                continue;
            }
            let result = (|| -> Result<Value, String> {
                let value = |position: usize| {
                    values[roots[position]].ok_or_else(|| "missing operand value".to_string())
                };
                let number = |position| value(position)?.number();
                let boolean = |position| value(position)?.boolean();
                let number = match instruction.op {
                    Op::Literal(value, _) => value,
                    Op::Bool(value) => return Ok(Value::Boolean(value)),
                    Op::If => return value(if boolean(0)? { 1 } else { 2 }),
                    Op::And => return Ok(Value::Boolean(boolean(0)? && boolean(1)?)),
                    Op::Or => return Ok(Value::Boolean(boolean(0)? || boolean(1)?)),
                    Op::Not => return Ok(Value::Boolean(!boolean(0)?)),
                    Op::Equal => return Ok(Value::Boolean(value(0)? == value(1)?)),
                    Op::NotEqual => return Ok(Value::Boolean(value(0)? != value(1)?)),
                    Op::Less => return Ok(Value::Boolean(number(0)? < number(1)?)),
                    Op::LessEqual => return Ok(Value::Boolean(number(0)? <= number(1)?)),
                    Op::Greater => return Ok(Value::Boolean(number(0)? > number(1)?)),
                    Op::GreaterEqual => return Ok(Value::Boolean(number(0)? >= number(1)?)),
                    Op::Parameter(id, _)
                    | Op::AgeYears(id)
                    | Op::DaysUntil(DateRef::Parameter(id))
                    | Op::YearsUntil(DateRef::Parameter(id)) => {
                        return Err(format!("parameter {id:?} was not bound before evaluation"));
                    }
                    Op::Balance(account) => state
                        .account_balance(context.account(account)?)
                        .map_err(|e| e.to_string())?,
                    Op::Cash(account) => state
                        .account_cash_balance(context.account(account)?)
                        .map_err(|e| e.to_string())?,
                    Op::Holding(account, asset_id) => state
                        .asset_balance(AssetCoord {
                            account_id: context.account(account)?,
                            asset_id,
                        })
                        .map_err(|e| e.to_string())?,
                    Op::EndpointBalance(reference) => context.endpoint_balance(reference)?,
                    Op::NetWorth => state.net_worth(),
                    Op::Age => {
                        let birth = state.timeline.birth_date;
                        let today = state.timeline.current_date;
                        if today < birth {
                            return Err("age() is undefined before birth_date".into());
                        }
                        let mut months = (i32::from(today.year()) - i32::from(birth.year())) * 12
                            + i32::from(today.month())
                            - i32::from(birth.month());
                        let anniversary_day = birth
                            .day()
                            .min(crate::date_math::days_in_month(today.year(), today.month()));
                        if today.day() < anniversary_day {
                            months -= 1;
                        }
                        f64::from(months) / 12.0
                    }
                    Op::Year => f64::from(state.timeline.current_date.year()),
                    Op::Month => f64::from(state.timeline.current_date.month()),
                    Op::YearsSinceStart => {
                        days_between(state.timeline.start_date, state.timeline.current_date)?
                            / 365.2425
                    }
                    Op::DaysUntil(DateRef::Literal(date)) => {
                        days_between(state.timeline.current_date, date)?
                    }
                    Op::YearsUntil(DateRef::Literal(date)) => {
                        days_between(state.timeline.current_date, date)? / 365.2425
                    }
                    Op::Positive => number(0)?,
                    Op::Negate => -number(0)?,
                    Op::Percent => number(0)? / 100.0,
                    Op::Abs => number(0)?.abs(),
                    Op::Inflate => state
                        .portfolio
                        .market
                        .get_inflation_adjusted_value(
                            state.timeline.start_date,
                            state.timeline.current_date,
                            number(0)?,
                        )
                        .map_err(|e| e.to_string())?,
                    Op::Clamp => {
                        let lower = number(1)?;
                        let upper = number(2)?;
                        if lower > upper {
                            return Err("clamp minimum exceeds maximum".into());
                        }
                        number(0)?.clamp(lower, upper)
                    }
                    Op::Add => number(0)? + number(1)?,
                    Op::Sub => number(0)? - number(1)?,
                    Op::Mul => number(0)? * number(1)?,
                    Op::Div => {
                        let denominator = number(1)?;
                        if denominator == 0.0 {
                            return Err("division by zero".into());
                        }
                        number(0)? / denominator
                    }
                    Op::Min => number(0)?.min(number(1)?),
                    Op::Max => number(0)?.max(number(1)?),
                };
                if !number.is_finite() {
                    return Err("expression produced a non-finite value".into());
                }
                Ok(Value::Number(number))
            })()
            .map_err(error)?;
            values[index] = Some(result);
            pending.pop();
        }
        values[root].ok_or_else(|| {
            ExpressionError::new(self.program[root].span.clone(), "missing expression result")
        })
    }
}

fn days_between(start: Date, end: Date) -> Result<f64, String> {
    start
        .until(end)
        .map(|span| f64::from(span.get_days()))
        .map_err(|e| e.to_string())
}
