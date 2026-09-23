use super::*;

fn missing(kind: &str, id: impl fmt::Debug) -> ExpressionError {
    ExpressionError::new(0..0, format!("missing {kind} name for {id:?}"))
}

fn quote(value: &str) -> String {
    let mut result = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            _ => result.push(ch),
        }
    }
    result.push('"');
    result
}

fn account(
    reference: AccountRef,
    metadata: &SimulationMetadata,
) -> Result<String, ExpressionError> {
    Ok(match reference {
        AccountRef::Source => "source".into(),
        AccountRef::Target => "target".into(),
        AccountRef::Account(id) => quote(
            metadata
                .account_name(id)
                .ok_or_else(|| missing("account", id))?,
        ),
    })
}

fn parameter(id: ParameterId, metadata: &SimulationMetadata) -> Result<String, ExpressionError> {
    let name = metadata
        .parameter_name(id)
        .ok_or_else(|| missing("parameter", id))?;
    let mut chars = name.chars();
    let simple = chars
        .next()
        .is_some_and(|ch| ch.is_alphabetic() || ch == '_')
        && chars.all(|ch| ch.is_alphanumeric() || ch == '_');
    Ok(format!(
        "${}",
        if simple {
            name.to_string()
        } else {
            quote(name)
        }
    ))
}

fn number(value: f64) -> Result<String, ExpressionError> {
    if !value.is_finite() {
        return Err(ExpressionError::new(
            0..0,
            "cannot format a non-finite value",
        ));
    }
    Ok(value.to_string())
}

fn date(reference: DateRef, metadata: &SimulationMetadata) -> Result<String, ExpressionError> {
    match reference {
        DateRef::Literal(value) => Ok(quote(&value.to_string())),
        DateRef::Parameter(id) => parameter(id, metadata),
    }
}

impl Expression {
    /// Render valid DSL using the current names for the expression's stable IDs.
    /// Use the original unbound expression when preparing editable text.
    pub fn to_source(&self, metadata: &SimulationMetadata) -> Result<String, ExpressionError> {
        self.result_types()?;
        let mut stack: Vec<String> = Vec::new();
        for instruction in &self.program {
            let mut pop = || {
                stack.pop().ok_or_else(|| {
                    ExpressionError::new(instruction.span.clone(), "missing operand")
                })
            };
            let source = match instruction.op {
                Op::Literal(value, _) => number(value)?,
                Op::Bool(value) => value.to_string(),
                Op::Parameter(id, _) => parameter(id, metadata)?,
                Op::Balance(reference) => format!("balance({})", account(reference, metadata)?),
                Op::Cash(reference) => format!("cash({})", account(reference, metadata)?),
                Op::Holding(reference, id) => format!(
                    "holding({}, {})",
                    account(reference, metadata)?,
                    quote(
                        metadata
                            .asset_name(id)
                            .ok_or_else(|| missing("asset", id))?
                    )
                ),
                Op::EndpointBalance(reference) => {
                    format!("endpoint_balance({})", account(reference, metadata)?)
                }
                Op::NetWorth => "net_worth()".into(),
                Op::Age => "age()".into(),
                Op::Year => "year()".into(),
                Op::Month => "month()".into(),
                Op::YearsSinceStart => "years_since_start()".into(),
                Op::DaysUntil(reference) => format!("days_until({})", date(reference, metadata)?),
                Op::YearsUntil(reference) => format!("years_until({})", date(reference, metadata)?),
                Op::AgeYears(id) => format!("age_years({})", parameter(id, metadata)?),
                Op::Positive => format!("(+{})", pop()?),
                Op::Negate => format!("(-{})", pop()?),
                Op::Not => format!("(not {})", pop()?),
                Op::Percent => format!("({})%", pop()?),
                Op::Abs => format!("abs({})", pop()?),
                Op::Inflate => format!("inflation({})", pop()?),
                Op::Clamp => {
                    let upper = pop()?;
                    let lower = pop()?;
                    format!("clamp({}, {lower}, {upper})", pop()?)
                }
                Op::If => {
                    let otherwise = pop()?;
                    let then = pop()?;
                    format!("if({}, {then}, {otherwise})", pop()?)
                }
                op @ (Op::Add
                | Op::Sub
                | Op::Mul
                | Op::Div
                | Op::Min
                | Op::Max
                | Op::Less
                | Op::LessEqual
                | Op::Greater
                | Op::GreaterEqual
                | Op::Equal
                | Op::NotEqual
                | Op::And
                | Op::Or) => {
                    let right = pop()?;
                    let left = pop()?;
                    match op {
                        Op::Min => format!("min({left}, {right})"),
                        Op::Max => format!("max({left}, {right})"),
                        _ => {
                            let symbol = match op {
                                Op::Add => "+",
                                Op::Sub => "-",
                                Op::Mul => "*",
                                Op::Div => "/",
                                Op::Less => "<",
                                Op::LessEqual => "<=",
                                Op::Greater => ">",
                                Op::GreaterEqual => ">=",
                                Op::Equal => "==",
                                Op::NotEqual => "!=",
                                Op::And => "and",
                                Op::Or => "or",
                                _ => unreachable!(),
                            };
                            format!("({left} {symbol} {right})")
                        }
                    }
                }
            };
            stack.push(source);
        }
        Ok(stack.remove(0))
    }
}
