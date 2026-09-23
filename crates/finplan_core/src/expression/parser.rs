use super::*;

#[derive(Debug, Clone, PartialEq)]
enum TokenKind {
    Number(f64),
    Name(String),
    String(String),
    Symbol(char),
    Comparison(&'static str),
    End,
}

#[derive(Debug, Clone)]
struct Token {
    kind: TokenKind,
    span: Range<usize>,
}

fn lex(source: &str) -> Result<Vec<Token>, ExpressionError> {
    if source.len() > 16_384 {
        return Err(ExpressionError::new(
            0..source.len(),
            "expression exceeds 16384 bytes",
        ));
    }
    let mut tokens = Vec::new();
    let mut chars = source.char_indices().peekable();
    while let Some((start, ch)) = chars.next() {
        if ch.is_whitespace() {
            continue;
        }
        let kind = if ch == '"' {
            let mut value = String::new();
            let mut closed = false;
            while let Some((at, ch)) = chars.next() {
                match ch {
                    '"' => {
                        closed = true;
                        break;
                    }
                    '\\' => {
                        let Some((_, escaped)) = chars.next() else {
                            break;
                        };
                        value.push(match escaped {
                            '"' => '"',
                            '\\' => '\\',
                            'n' => '\n',
                            'r' => '\r',
                            't' => '\t',
                            _ => {
                                return Err(ExpressionError::new(
                                    at..at + 1 + escaped.len_utf8(),
                                    "unsupported string escape",
                                ));
                            }
                        });
                    }
                    _ => value.push(ch),
                }
            }
            if !closed {
                return Err(ExpressionError::new(
                    start..source.len(),
                    "unterminated string",
                ));
            }
            TokenKind::String(value)
        } else if ch.is_ascii_digit() || ch == '.' {
            while chars.peek().is_some_and(|(_, c)| c.is_ascii_digit()) {
                chars.next();
            }
            if ch != '.' && chars.peek().is_some_and(|(_, c)| *c == '.') {
                chars.next();
                while chars.peek().is_some_and(|(_, c)| c.is_ascii_digit()) {
                    chars.next();
                }
            }
            if chars.peek().is_some_and(|(_, c)| *c == 'e' || *c == 'E') {
                chars.next();
                if chars.peek().is_some_and(|(_, c)| *c == '+' || *c == '-') {
                    chars.next();
                }
                while chars.peek().is_some_and(|(_, c)| c.is_ascii_digit()) {
                    chars.next();
                }
            }
            let end = chars.peek().map_or(source.len(), |(i, _)| *i);
            let value = source[start..end]
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite())
                .ok_or_else(|| ExpressionError::new(start..end, "expected a finite number"))?;
            TokenKind::Number(value)
        } else if ch.is_alphabetic() || ch == '_' {
            while chars
                .peek()
                .is_some_and(|(_, c)| c.is_alphanumeric() || *c == '_')
            {
                chars.next();
            }
            let end = chars.peek().map_or(source.len(), |(i, _)| *i);
            TokenKind::Name(source[start..end].into())
        } else if "<>!=".contains(ch) {
            let paired = chars.peek().is_some_and(|(_, next)| *next == '=');
            if paired {
                chars.next();
            }
            match (ch, paired) {
                ('<', false) => TokenKind::Comparison("<"),
                ('<', true) => TokenKind::Comparison("<="),
                ('>', false) => TokenKind::Comparison(">"),
                ('>', true) => TokenKind::Comparison(">="),
                ('=', true) => TokenKind::Comparison("=="),
                ('!', true) => TokenKind::Comparison("!="),
                _ => {
                    return Err(ExpressionError::new(
                        start..start + ch.len_utf8(),
                        format!("unexpected character {ch:?}"),
                    ));
                }
            }
        } else if "+-*/%(),$".contains(ch) {
            TokenKind::Symbol(ch)
        } else {
            return Err(ExpressionError::new(
                start..start + ch.len_utf8(),
                format!("unexpected character {ch:?}"),
            ));
        };
        let end = chars.peek().map_or(source.len(), |(i, _)| *i);
        tokens.push(Token {
            kind,
            span: start..end,
        });
    }
    tokens.push(Token {
        kind: TokenKind::End,
        span: source.len()..source.len(),
    });
    Ok(tokens)
}

pub(super) fn parse(
    source: &str,
    metadata: &SimulationMetadata,
    parameters: &HashMap<ParameterId, ParameterValue>,
) -> Result<(Expression, Option<AmountMode>), ExpressionError> {
    let mut parser = Parser {
        tokens: lex(source)?,
        position: 0,
        program: Vec::new(),
        metadata,
        parameters,
    };
    let mode = match &parser.peek().kind {
        TokenKind::Name(name) if name == "gross" => Some(AmountMode::Gross),
        TokenKind::Name(name) if name == "net" => Some(AmountMode::Net),
        _ => None,
    };
    if mode.is_some() {
        parser.take();
        parser.expect('(')?;
    }
    parser.expression(0, 0)?;
    if mode.is_some() {
        parser.expect(')')?;
    }
    if parser.peek().kind != TokenKind::End {
        return Err(
            parser.error("unexpected trailing input; gross/net must wrap the entire amount")
        );
    }
    let expression = Expression {
        program: parser.program,
    };
    expression.result_types()?;
    Ok((expression, mode))
}

struct Parser<'a> {
    tokens: Vec<Token>,
    position: usize,
    program: Vec<Instruction>,
    metadata: &'a SimulationMetadata,
    parameters: &'a HashMap<ParameterId, ParameterValue>,
}

impl Parser<'_> {
    fn peek(&self) -> &Token {
        &self.tokens[self.position]
    }

    fn take(&mut self) -> Token {
        let token = self.peek().clone();
        if token.kind != TokenKind::End {
            self.position += 1;
        }
        token
    }

    fn error(&self, message: impl Into<String>) -> ExpressionError {
        ExpressionError::new(self.peek().span.clone(), message)
    }

    fn eat(&mut self, ch: char) -> bool {
        if self.peek().kind == TokenKind::Symbol(ch) {
            self.take();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, ch: char) -> Result<(), ExpressionError> {
        if self.eat(ch) {
            Ok(())
        } else {
            Err(self.error(format!("expected '{ch}'")))
        }
    }

    fn emit(&mut self, op: Op, span: Range<usize>) -> Result<(), ExpressionError> {
        if self.program.len() >= MAX_INSTRUCTIONS {
            return Err(ExpressionError::new(span, "expression is too complex"));
        }
        self.program.push(Instruction { op, span });
        Ok(())
    }

    fn expression(&mut self, min_precedence: u8, depth: usize) -> Result<(), ExpressionError> {
        if depth >= 64 {
            return Err(self.error("expression nesting exceeds 64 levels"));
        }
        let token = self.take();
        match token.kind {
            TokenKind::Number(value) => {
                self.emit(Op::Literal(value, MONEY | SCALAR), token.span)?
            }
            TokenKind::Name(ref name) if name == "true" || name == "false" => {
                self.emit(Op::Bool(name == "true"), token.span)?;
            }
            TokenKind::Name(ref name) if name == "not" => {
                self.expression(3, depth + 1)?;
                self.emit(Op::Not, token.span)?;
            }
            TokenKind::Symbol('$') => {
                let (id, value) = self.parameter()?;
                let kind = match value {
                    ParameterValue::Money(_) => MONEY,
                    ParameterValue::Rate(_) => SCALAR,
                    _ => {
                        return Err(ExpressionError::new(
                            token.span,
                            "Date/Age parameters require days_until, years_until, or age_years",
                        ));
                    }
                };
                self.emit(Op::Parameter(id, kind), token.span)?;
            }
            TokenKind::Symbol('(') => {
                self.expression(0, depth + 1)?;
                self.expect(')')?;
            }
            TokenKind::Symbol(sign @ ('+' | '-')) => {
                self.expression(7, depth + 1)?;
                self.emit(
                    if sign == '-' {
                        Op::Negate
                    } else {
                        Op::Positive
                    },
                    token.span,
                )?;
            }
            TokenKind::Name(name) => self.function(&name, token.span, depth + 1)?,
            _ => {
                return Err(ExpressionError::new(
                    token.span,
                    "expected a number, boolean, $parameter, function, or '('",
                ));
            }
        }
        let mut saw_comparison = false;
        loop {
            if self.peek().kind == TokenKind::Symbol('%') {
                let span = self.take().span;
                self.emit(Op::Percent, span)?;
                continue;
            }
            let (precedence, op) = match &self.peek().kind {
                TokenKind::Name(name) if name == "or" => (1, Op::Or),
                TokenKind::Name(name) if name == "and" => (2, Op::And),
                TokenKind::Comparison("<") => (4, Op::Less),
                TokenKind::Comparison("<=") => (4, Op::LessEqual),
                TokenKind::Comparison(">") => (4, Op::Greater),
                TokenKind::Comparison(">=") => (4, Op::GreaterEqual),
                TokenKind::Comparison("==") => (4, Op::Equal),
                TokenKind::Comparison("!=") => (4, Op::NotEqual),
                TokenKind::Symbol('+') => (5, Op::Add),
                TokenKind::Symbol('-') => (5, Op::Sub),
                TokenKind::Symbol('*') => (6, Op::Mul),
                TokenKind::Symbol('/') => (6, Op::Div),
                _ => break,
            };
            if precedence < min_precedence {
                break;
            }
            if precedence == 4 {
                if saw_comparison {
                    return Err(self.error("comparison operators cannot be chained"));
                }
                saw_comparison = true;
            }
            let span = self.take().span;
            self.expression(precedence + 1, depth + 1)?;
            self.emit(op, span)?;
        }
        Ok(())
    }

    fn parameter(&mut self) -> Result<(ParameterId, ParameterValue), ExpressionError> {
        let token = self.take();
        let name = match token.kind {
            TokenKind::Name(name) | TokenKind::String(name) => name,
            _ => {
                return Err(ExpressionError::new(
                    token.span,
                    "expected parameter name after '$'",
                ));
            }
        };
        let id = self.metadata.parameter_id(&name).ok_or_else(|| {
            ExpressionError::new(token.span.clone(), format!("unknown parameter {name:?}"))
        })?;
        let value = self
            .parameters
            .get(&id)
            .copied()
            .filter(|v| v.is_valid())
            .ok_or_else(|| {
                ExpressionError::new(token.span, format!("missing or invalid parameter {name:?}"))
            })?;
        Ok((id, value))
    }

    fn account(&mut self) -> Result<AccountRef, ExpressionError> {
        let token = self.take();
        match token.kind {
            TokenKind::Name(name) if name == "source" => Ok(AccountRef::Source),
            TokenKind::Name(name) if name == "target" => Ok(AccountRef::Target),
            TokenKind::String(name) => self
                .metadata
                .account_id(&name)
                .map(AccountRef::Account)
                .ok_or_else(|| {
                    ExpressionError::new(token.span, format!("unknown account {name:?}"))
                }),
            _ => Err(ExpressionError::new(
                token.span,
                "expected quoted account name, source, or target",
            )),
        }
    }

    fn date(&mut self) -> Result<DateRef, ExpressionError> {
        if self.eat('$') {
            let span = self.peek().span.clone();
            let (id, value) = self.parameter()?;
            if matches!(value, ParameterValue::Date(_)) {
                return Ok(DateRef::Parameter(id));
            }
            return Err(ExpressionError::new(span, "expected a Date parameter"));
        }
        let token = self.take();
        if let TokenKind::String(value) = token.kind {
            return value.parse::<Date>().map(DateRef::Literal).map_err(|_| {
                ExpressionError::new(token.span, "expected a valid date: YYYY-MM-DD")
            });
        }
        Err(ExpressionError::new(
            token.span,
            "expected a quoted date or $DateParameter",
        ))
    }

    fn function(
        &mut self,
        name: &str,
        span: Range<usize>,
        depth: usize,
    ) -> Result<(), ExpressionError> {
        self.expect('(')?;
        let op = match name {
            "balance" => Op::Balance(self.account()?),
            "cash" => Op::Cash(self.account()?),
            "holding" => {
                let account = self.account()?;
                self.expect(',')?;
                let token = self.take();
                let TokenKind::String(name) = token.kind else {
                    return Err(ExpressionError::new(
                        token.span,
                        "expected quoted holding name",
                    ));
                };
                let asset = self.metadata.asset_id(&name).ok_or_else(|| {
                    ExpressionError::new(token.span, format!("unknown holding {name:?}"))
                })?;
                Op::Holding(account, asset)
            }
            "endpoint_balance" => {
                let account = self.account()?;
                if matches!(account, AccountRef::Account(_)) {
                    return Err(ExpressionError::new(
                        span,
                        "endpoint_balance requires source or target",
                    ));
                }
                Op::EndpointBalance(account)
            }
            "source_balance" => Op::EndpointBalance(AccountRef::Source),
            "target_balance" => Op::EndpointBalance(AccountRef::Target),
            "net_worth" => Op::NetWorth,
            "age" => Op::Age,
            "year" => Op::Year,
            "month" => Op::Month,
            "years_since_start" => Op::YearsSinceStart,
            "days_until" => Op::DaysUntil(self.date()?),
            "years_until" => Op::YearsUntil(self.date()?),
            "age_years" => {
                self.expect('$')?;
                let (id, value) = self.parameter()?;
                if !matches!(value, ParameterValue::Age(_)) {
                    return Err(ExpressionError::new(
                        span,
                        "age_years requires an Age parameter",
                    ));
                }
                Op::AgeYears(id)
            }
            "if" => {
                self.expression(0, depth)?;
                self.expect(',')?;
                self.expression(0, depth)?;
                self.expect(',')?;
                self.expression(0, depth)?;
                Op::If
            }
            "min" | "max" | "clamp" | "abs" | "inflation" | "top_up" => {
                self.expression(0, depth)?;
                if matches!(name, "min" | "max" | "clamp") {
                    self.expect(',')?;
                    self.expression(0, depth)?;
                }
                if name == "clamp" {
                    self.expect(',')?;
                    self.expression(0, depth)?;
                }
                match name {
                    "min" => Op::Min,
                    "max" => Op::Max,
                    "clamp" => Op::Clamp,
                    "abs" => Op::Abs,
                    "inflation" => Op::Inflate,
                    "top_up" => {
                        self.emit(Op::EndpointBalance(AccountRef::Target), span.clone())?;
                        self.emit(Op::Sub, span.clone())?;
                        self.emit(Op::Literal(0.0, MONEY), span.clone())?;
                        Op::Max
                    }
                    _ => unreachable!(),
                }
            }
            "payoff" => {
                self.emit(Op::Balance(AccountRef::Target), span.clone())?;
                self.emit(Op::Negate, span.clone())?;
                self.emit(Op::Literal(0.0, MONEY), span.clone())?;
                Op::Max
            }
            "gross" | "net" => {
                return Err(ExpressionError::new(
                    span,
                    "gross/net must wrap the entire amount and cannot be nested",
                ));
            }
            _ => {
                return Err(ExpressionError::new(
                    span,
                    format!("unknown function {name:?}"),
                ));
            }
        };
        self.expect(')')?;
        self.emit(op, span)
    }
}
