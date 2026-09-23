# Core amount expression DSL

Implemented in `finplan_core::expression`, the sole amount calculation system in
the core. `TransferAmount` is a Money expression wrapper, not a recursive enum.
Core builders, analysis, optimization, examples, and benchmarks use it directly.
The old amount enum and its loading format have been removed; no compatibility
loader is provided. No new dependencies are required.

The TUI supports static/expression amount fields, parameter completion, and
compilation of saved source (see `04_tui_application.md`). The server still uses
the removed API and needs a separate migration, including the website's
request/storage shapes and editors. The TUI and core can be tested with
`cargo test -p finplan -p finplan_core`; the server migration is required for a
working whole-workspace build.

## Syntax

```text
5000
$MonthlySpending
$WithdrawalRate * balance("Vanguard")
0.5 * holding("Vanguard", "VTI")
10% * net_worth()
net(top_up(inflation($MonthlySpending)))
min(cash(source), 5000)
max(0, balance(source) - inflation(10000))
payoff()
if(age() >= 65, inflation($"Retirement spending"), inflation($MonthlySpending))
if(cash(source) > 0 and not year() < 2035, min(cash(source), 5000), 0)
```

Arithmetic supports `+`, `-`, `*`, `/`, unary signs, parentheses, decimal and
scientific literals, and postfix `%` (`10%` is `0.1`). Multiplication/division bind
more tightly than addition/subtraction; operators of equal precedence associate
left. Percent binds more tightly than arithmetic. `%` is not modulo.

Comparisons support `<`, `<=`, `>`, `>=`, `==`, and `!=`. Conditions combine with
`and`, `or`, and prefix `not`; `true` and `false` are Boolean literals. Precedence,
from strongest to weakest, is `%`, unary signs, `*`/`/`, `+`/`-`, comparisons,
`not`, `and`, `or`. Thus `not age() >= 65` means `not (age() >= 65)`.
Parentheses override precedence. Comparisons cannot be chained: write
`50 <= age() and age() < 65`, not `50 <= age() < 65`.

`if(condition, then_amount, else_amount)` selects one branch. It evaluates only
the selected branch; `and` and `or` likewise skip an unnecessary right operand.
This allows guards such as `if(cash(source) > 0, 500 / cash(source) * 100, 0)`
without evaluating division by zero. Every branch is still checked for valid
syntax, names, and types at compilation, and all parameters are checked and bound
at run initialization, even when a branch will not be selected.

Parameters use `$Name`, or `$"Monthly spending"` for names containing spaces or
punctuation. Identifiers support Unicode letters, digits after the first
character, and underscores. Account/asset names are double-quoted strings with
`\"`, `\\`, `\n`, `\r`, and `\t` escapes. Names and function names are case-sensitive.
`"source"` names an actual account; bare `source` is the effect's contextual account.

The compiler resolves names through `SimulationMetadata` to type-safe IDs. An
already compiled expression survives renames. `Expression::to_source` renders
using current metadata names. Frontends should retain/edit source text and compile
on save, and regenerate text from IDs when an entity is renamed. Metadata must
provide unambiguous names for the compiler.

## Functions

| Syntax | Meaning |
| --- | --- |
| `net_worth()` | Sum of all current account values, including property and negative liabilities |
| `balance(account)` | Total account value, including cash and holdings |
| `cash(account)` | Cash only; errors for accounts without cash |
| `holding(account, "VTI")` | Current value of that asset in that account, across all lots |
| `endpoint_balance(source)` / `endpoint_balance(target)` | Balance of a specific cash or asset endpoint |
| `source_balance()` / `target_balance()` | Short forms of the endpoint functions |
| `inflation(amount)` | Convert simulation-start dollars to current nominal dollars using this run's cumulative inflation |
| `min(a, b)` / `max(a, b)` | Lower/upper value; two arguments |
| `clamp(value, lower, upper)` | Limit a value to a range; errors if lower exceeds upper |
| `abs(value)` | Absolute value |
| `if(condition, then_value, else_value)` | Lazily select between two values of compatible types |
| `top_up(desired)` | `max(0, desired - target_balance())` |
| `payoff()` | `max(0, -balance(target))`; debt balances are negative |
| `age()` | Completed calendar months since the scenario birth date, divided by 12 |
| `age_years($AgeParameter)` | Convert an existing typed Age parameter to years plus months / 12 |
| `year()` / `month()` | Current simulated calendar year / month (1–12) |
| `years_since_start()` | Elapsed simulated days / 365.2425 |
| `days_until("2035-01-01")` / `days_until($DateParameter)` | Signed days from the current simulation date to the given date |
| `years_until(date)` | Signed days until the date / 365.2425 |

Balance functions use the engine's existing valuation rules. In particular, a
known asset with no lots in an investment account has value zero. Inflation is
explicit: balances are already nominal, so inflating them again is generally not
the intended calculation.

`age()` requires an explicit scenario birth date at run initialization. Calendar
anniversaries clamp to month end (a February 29 birthday reaches its anniversary
on February 28 in a non-leap year). Day-based year fractions deliberately differ
from calendar ages. These helpers return scalar numbers; date and age parameters
cannot silently become dollar amounts. Existing typed age/date triggers continue
to handle scheduling. Date construction and trigger expressions remain outside
the amount DSL.

## Source, target, and tax interpretation

Bind context per **effect**, not per event: an event can contain several effects
with different accounts. This avoids a second set of event metadata that can
disagree with the actual transfer.

| Effect | `source` | `target` |
| --- | --- | --- |
| Income | Unavailable | Destination cash account |
| Expense | Source cash account | Unavailable |
| Cash transfer | Source cash account | Destination account |
| Asset purchase | Source cash account | Purchased holding |
| Asset sale | Source account; asset endpoint only when an asset is selected | Cash in the selling account |
| Sweep from one account/asset | Selected account; asset endpoint only for single-asset selection | Destination cash account |
| Sweep using a strategy/custom list | Unavailable (no single source) | Destination cash account |
| Balance adjustment | Unavailable | Adjusted account |

`balance(source)` always means the whole containing account. For an asset sale,
`source_balance()` means the selected holding if one is specified. An account-wide
sale or single-account sweep has an account reference but no single source
endpoint: calculate the invested balance as `balance(source) - cash(source)`.
`EventBuilder::full_balance()` uses this expression for sales. Aggregate holdings
are a balance calculation, not a `TransferEndpoint` variant. `cash(source)` always means the containing account's
cash. The endpoint of a cash transfer to a liability has no cash balance; use
`payoff()` or `balance(target)` for liabilities. Missing/ambiguous context is an
error, never an implicit zero or arbitrarily selected account.

`EvaluationContext::with_source_account` binds account identity independently of
transfer endpoints. `source_balance()` errors for such account-wide operations;
it does not silently switch between cash and an aggregate of holdings.

`gross(amount)` and `net(amount)` are allowed only around the **entire amount**.
They set the existing `AmountMode` on Income, AssetSale, or Sweep effects through
`CompiledAmount::apply_to`. A bare expression preserves the effect's current mode.
Nested annotations and annotations on effects that do not support them are errors.
They do not compute tax conversions inside arithmetic: the existing effect
machinery has the account, lots, tax status, year-to-date income, and withdrawal
context required to do so.

## Rust API and execution

```rust
use finplan_core::expression::compile_amount;

// config and metadata come from SimulationBuilder or the application compiler.
let parsed = compile_amount(
    "net(top_up(inflation($MonthlySpending)))",
    &metadata,
    &config.parameters,
)?;
parsed.apply_to(&mut effect)?;
// Install effect in config.events, then simulate normally.
```

For numeric previews, use `Expression::compile`, `bind_parameters`, and
`evaluate(&EvaluationContext)`; use `evaluate_bool` to preview a condition.
`Expression::into_amount` checks that the result
can be Money. `compile_amount` does this check and additionally accepts gross/net.
The core stores the compiled program in `pub struct TransferAmount(Expression)`;
its field is private. `TryFrom<Expression>` and amount deserialization validate
that the result is Money. The wrapper serializes directly as an expression
program, without enum tags. `TransferAmount::to_source` renders editable DSL.

Rust callers can construct common amounts without parsing:

```rust
TransferAmount::fixed(5000.0)
TransferAmount::parameter(spending_id).inflated()
TransferAmount::scaled_rate(rate_id, TransferAmount::account_balance(account_id))
TransferAmount::percent_of_account(0.04, account_id)
TransferAmount::fixed(500.0).plus(TransferAmount::cash_balance(account_id))
```

These helpers emit flat instructions, not another expression tree. `parameter`
expects Money; `scaled_rate` expects a Rate parameter and a Money amount. Their
numeric inputs must be finite and are checked at run initialization. Builder
methods `amount`, `full_balance`, `transfer_amount`, and `parameter_amount` all
store the same wrapper directly; there is no separate builder amount enum.

Compilation emits a flat postfix program. Evaluation derives operand indexes from
the validated program and uses an iterative work stack, skipping unused branches.
Simulation initialization checks and
binds parameters into a per-run copy, including expressions in random branches.
Original configurations keep IDs, so changing
parameter values for optimization or another run takes effect without reparsing.
Balances, inflation, and calendar helpers read live state on each evaluation.
Compiled expressions support serde, and loaded programs are checked before use.
Every amount goes through the same expression evaluator and type checker; the
recursive amount evaluator and duplicate parameter/type traversal are removed.

Named-parameter optimization continues to change the run's parameter map and
rebind the original expression. Direct effect sweeps use `with_fixed_value` for
literal or `inflation(literal)` expressions, and `with_scale_factor` for a root
multiplication with one unambiguous literal scalar (including a percent). They
reject computed or parameterized factors and ambiguous forms such as `500 * 4`
without changing the original amount. Use named parameters for more complex rules.

Money and Rate parameters retain their types. Unsuffixed numeric literals can be
money or scalars; percent expressions are scalars. Money × Rate and Money / Rate
produce Money; Money / Money produces a scalar. Money × Money, Money + Rate,
inflation of a Rate, and using a pure Rate as an event amount are rejected.
Comparisons require compatible numeric types; equality also accepts two Booleans.
`not`, `and`, `or`, and the condition of `if` require Booleans, without numeric
truthiness or conversion to zero/one. Both `if` branches must share a type; they
may return Money, Rate, or Boolean, but an event amount must ultimately be Money.
Numeric equality compares the underlying floating-point values exactly.
No rounding is introduced, and negative values remain available for balance
adjustments. Functions such as `top_up` explicitly floor their result at zero.

Errors include UTF-8 byte spans for editor highlighting. Unknown names, wrong
arity, incompatible parameter types, malformed dates, division by zero, invalid
clamp bounds, non-finite results, missing run parameters, and unavailable context
produce errors. The existing simulation error/warning handling applies to runtime
effect errors. Inputs are bounded to 16 KiB, 64 nested parser levels, and 1024
instructions. There is no arbitrary code execution or external data access.

## UI migration reference

Use these DSL equivalents when replacing frontend amount builders. The core no
longer exposes or accepts the old calculation variants, and the legacy formatter
has been removed. New rules should use the intended semantics directly.

| Existing calculation | Expression |
| --- | --- |
| Fixed | `5000` |
| Parameter | `$MonthlySpending` |
| TUI RateTimes | `$WithdrawalRate * balance("Vanguard")` |
| InflationAdjusted | `inflation(...)` |
| Scale / percentage of account | `4% * balance("Vanguard")` |
| SourceBalance | `source_balance()` |
| TargetToBalance | `top_up(5000)` |
| AccountTotalBalance / TUI AccountBalance | `balance("Vanguard")` |
| AccountCashBalance | `cash("Vanguard")` |
| AssetBalance | `holding("Vanguard", "VTI")` |
| Min / Max | `min(a, b)` / `max(a, b)` |
| Add / Sub / Mul | `a + b` / `a - b` / `a * b` |
| Intended debt payoff | `payoff()` |

Use `payoff()` for debt payoff; `target_balance()` reads an endpoint and does not
negate it. This removes the old `ZeroTargetBalance` variant's misleading semantics.

Remaining server/web integration work:

1. Add expression source to the server's request/storage schema. Compile with
   shared name/ID metadata and typed parameters.
2. Replace the recursive editors with a text field, reference/function completion,
   syntax help, and span-based errors. Use `to_source` to render compiled amounts.
3. Route gross/net annotations through `apply_to`, avoiding duplicated/conflicting
   form controls. Derive source/target help from the selected effect.
4. Update reference tracking, rename/delete behavior, analysis parameter discovery,
   and bindings together. Legacy amount loading is not required.

Further candidates are rounding, contribution room, previous-year balances, and explicit
real-dollar conversion. RMD tables, lot selection, contribution limits, recurring
schedules, and tax computation remain responsibilities of the existing effects
and triggers; they are not simple amount operations to replace mechanically.
