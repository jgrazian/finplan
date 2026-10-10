# Dynamic spending: rules that respond to the market

Status: implemented (2026-10-08, branch `tax-config-editor`); see
"Implementation notes" at the end for where it departs from this text.
Follows [20](20_drawdown.md) and
[21](21_cash_and_conversions.md): those decide *which accounts* pay for
retirement; this decides *how much* they pay, year by year.

## Why

Retirement spending today is a recurring `Expense`: a fixed amount in today's
dollars, inflated every year (`templates::recurring_expense`, reached from the
Retirement template's `spending`). The funding policy sells whatever it takes
to cover it. That is the 4% rule's assumption: the same real spending after a
40% crash as after a 30% boom.

Nobody retires that way, and every tool this one is compared with
(ProjectionLab, Boldin, FI Calc, cFIREsim) models the alternative. It changes
the answer more than any tax refinement does:

- A plan that "fails" in 25% of markets at fixed spending usually never runs
  out under guardrails. What is left is a question about **how deep and how
  long the cuts are**, which the results cannot answer today.
- The success rate answers "did a rigid plan survive?" when the user's real
  question is "how much can I spend, and what would I have to give up in a bad
  decade?"
- Goal-seek already finds "the largest fixed spend at 95%". That number is
  conservative by construction, because it has to survive the worst markets
  without ever adjusting.

### What the engine can do now

An amount is an `Expression` with `net_worth()`, `balance()`, `min`, `max`,
`clamp` and `if`, so "4% of net worth, between $40k and $80k" can almost be
written by hand. Three things stop it from being done properly:

1. **No memory between years.** Guardrails (Guyton-Klinger) change *last
   year's* spending by a step, and skip the inflation raise after a losing
   year. An expression sees the present only. `SimEventState::event_flow_ytd`
   resets each January and is never read.
2. **The wrong portfolio.** `net_worth()` counts home equity and subtracts the
   mortgage, so a rule written on it reacts to house prices.
3. **No spending in the results.** The summary reports balances and success
   rates. With variable spending, the output that matters is the distribution
   of what was spent, which only exists today as one representative path's
   `yearly_cash_flows[].expenses`.

## The model

### A new effect, not a field on `Expense`

```rust
EventEffect::Spending {
    from: AccountId,
    rule: SpendingRule,
    portfolio: SpendingPortfolio,
    /// Spend the plan's other net cash flows through `from` as well (below).
    pass_through: bool,
}
```

`Expense.amount` is a free expression evaluated on each occurrence. A rule
needs state across years, a yearly decision and a starting point. Putting a
rule on `Expense` would leave `amount` meaning one thing with a rule and
another without. A plain `Expense` stays exactly as it is: fixed spending is
"no rule", and the snapshot of every existing plan is unchanged.

A `Spending` effect belongs to a `Repeating` event with a `Monthly`,
`Quarterly` or `Yearly` interval (validation refuses the others). Its start
and end triggers are the event's own, usually the retirement date and the end
of the plan.

### The draw

Each rule decides one number per calendar year: **the draw `W`**, the year's
spending from the portfolio, in the dollars of that year. It is decided at
the event's first occurrence in the year. Each occurrence pays `W / n`, where
`n` is the interval's occurrences per year (12, 4 or 1). A first year that
starts in July pays six twelfths.

The rule's state is kept per event and per path, in real terms (deflated by
the path's cumulative inflation, the same factor Results uses):

```rust
pub struct SpendingRuleState {
    pub initial_rate: f64,     // W₀ / P₀, set the first year
    pub initial_real: f64,     // W₀ in plan-start dollars
    pub last_real: f64,        // last year's W in plan-start dollars
    pub last_year: i16,
}
```

It lives on `SimEventState` beside the other per-event vectors and is reset
with them for each iteration.

### The portfolio

```rust
pub enum SpendingPortfolio {
    /// Every Bank and Investment account: everything but Property and Liability.
    Liquid,
    /// Exactly these accounts.
    Accounts(Vec<AccountId>),
}
```

`P` is the portfolio's balance at the year's first occurrence. Tax-deferred
accounts count gross (see Open questions). `Liquid` is the default and matches
`PathDiagnostics::liquid_depleted`'s definition of "money to spend".

**Last year's return** `r` is the portfolio's market growth over the previous
calendar year divided by its January 1 balance. Market growth is the
appreciation the cash-flow ledger already reports as Growth (commit 36005dc),
restricted to the portfolio's accounts. The engine records it per year in
`SimPortfolio` while it applies returns. There is no `r` in the first year.

### Pass-through

With `pass_through` (the default), each occurrence spends `W / n` **plus the
net of every other cash flow through `from` since the previous occurrence**:
`Income` credits in, other `Expense` debits out, never below zero in total.

This is what makes the draw mean what the rules assume. A retiree with a
$40k draw and $30k of Social Security spends $70k, and the withdrawal rate
that guardrails watch is the $40k against the portfolio, not the $70k. A
mortgage or health-care `Expense` from the same account comes out of the
budget rather than on top of it, so the portfolio still gives up about `W`.
When fixed costs exceed income plus `W`, the occurrence spends nothing extra
and the deficit falls to the funding policy as today. That shows up as a
shortfall, which is the honest outcome.

Without `pass_through`, the effect spends exactly `W / n`, and other flows are
independent of it (for a plan that models discretionary spending alone).

`W` is net cash. Tax on the sales that fund it is paid on top, as for every
expense today. Rules therefore watch a net withdrawal rate; most published
rules are stated gross. See Open questions.

## The rules

```rust
pub enum SpendingRule {
    PercentOfPortfolio { rate: f64, floor: Option<f64>, ceiling: Option<f64> },
    Guardrails {
        initial: Initial,
        band: f64,              // 0.20
        step: f64,              // 0.10
        skip_inflation_after_loss: bool,   // true
        protect_final_years: u8,           // 15
        floor: Option<f64>,
        ceiling: Option<f64>,
    },
    CappedChange { rate: f64, max_cut: f64, max_raise: f64 },   // 0.025, 0.05
    Vpw { real_return: f64, final_age: u8 },                    // from allocation, 100
}

pub enum Initial { Amount(f64), Rate(f64) }
```

`floor` and `ceiling` are in plan-start dollars and are applied last, as
clamps. Every rule produces a nominal `W ≥ 0`.

### Percent of portfolio

`W = rate × P`, then clamped. Without clamps it can never deplete the
portfolio and swings with every market. With them it is Bengen's floor and
ceiling: the floor can deplete the portfolio, and that is the trade-off the
results show.

### Guardrails (Guyton-Klinger)

The first year: `W₀ = Initial::Amount` inflated to that year, or
`Initial::Rate × P₀`. The year's rate `W₀ / P₀` becomes `initial_rate`.

Each later year, in order:

1. **Inflation.** `W = last_real`, carried to this year's dollars. With
   `skip_inflation_after_loss`, when `r < 0` and the resulting rate `W / P`
   exceeds `initial_rate`, `W` stays at last year's *nominal* draw instead
   (Guyton's modified withdrawal rule; the skipped raise is never made up).
2. **Capital preservation.** If `W / P > initial_rate × (1 + band)`, then
   `W ×= 1 − step`. This does not apply in the plan's last
   `protect_final_years` years.
3. **Prosperity.** If `W / P < initial_rate × (1 − band)`, then `W ×= 1 + step`.
4. Clamp to `floor` and `ceiling`.

One adjustment per year, as published: no repeating until back inside the
band.

### Capped change (Vanguard-style dynamic spending)

The target is `rate × P`. Compared with `last_real` in real terms, the change
is held to `[−max_cut, +max_raise]`. In the first year the draw is just the
target. The result is a smooth draw that follows the market slowly.

### VPW (variable percentage withdrawal)

`W = P × pmt(real_return, years_left)`, where `years_left = final_age − age`
(at least 1), `pmt(r, n) = r / (1 − (1 + r)^−n)`, and `pmt(0, n) = 1 / n`. It
spends the portfolio down by `final_age` by design. The UI fills
`real_return` from the allocation (Open questions) and explains that a high
`final_age` is the safety margin.

## Results

### Per path (`finplan_core`)

`PathDiagnostics` gains, when the plan has a `Spending` effect:

```rust
pub struct SpendingDiagnostics {
    pub first_real: f64,         // year one's draw, plan-start $
    pub lowest_real: f64,
    pub largest_cut: f64,        // largest real year-over-year fall, as a fraction
    pub years_below_first: u16,  // years with real draw < 90% of first_real
    pub lifetime_real: f64,      // Σ real draws
}
```

Draws from several `Spending` events in one year are summed. Validation warns
on more than one, since the usual plan has exactly one.

### Across paths

`MonteCarloSummary` gains `real_spending: Option<RealSpendingSummary>`. It
uses the same date grid and quantiles (p5 to p95, type 7) as `real_net_worth`,
over **total spending**: each year's `Expense` and `Spending` debits, deflated.
That is the same quantity as Drawdown's `spending` line (spec 20), so the two
views agree. Collected by a `RealSpendingAccumulator` that mirrors
`RealAccumulator`, mergeable across batches and from offloaded runs.

`MonteCarloStats` gains, from the diagnostics:

| Field | Meaning |
|---|---|
| `median_lifetime_spending` | median of `lifetime_real` |
| `p10_lowest_draw` | 10th percentile of `lowest_real`: "1 in 10 markets, your draw dips to" |
| `cut_rate` | share of paths whose `largest_cut` exceeds 10% |
| `median_years_below_first` | median of `years_below_first` |

Plans without a `Spending` effect produce none of these, and their summaries
serialize exactly as today.

### Success

With a rule and no floor, the portfolio almost never empties, so "positive
final net worth" says nothing. For a plan with a `Spending` effect the
headline becomes **funding success** (cash never short, already computed), and
the spending tiles sit beside it. The net-worth success rate is still served
for comparisons, labelled as such. This is the first step toward making
funding success the default headline for every plan (spec 06 §F).

## Plan model (`finplan_plan`, server, wasm)

- `EffectSpec::Spending { from_account_id, rule: SpendingRuleSpec, portfolio:
  PortfolioSpec, pass_through }`, ts-rs exported. Validation:
  - `0 < rate ≤ 0.2`, `0 < band < 1`, `0 < step < 1`;
  - `floor ≤ ceiling`;
  - `final_age` above the age at the event's start;
  - accounts in `Accounts` exist and are not Property or Liability;
  - the event repeats at a supported interval.
- Compile lowers it to `EventEffect::Spending`. `snapshot::MODEL_VERSION`
  bumps. The canonical snapshot of a plan without the effect, and its hash,
  are unchanged (a test holds `testdata/default.snapshot.sha256`).
- Edits go through the existing effect edit ops; no new route. The edit route
  tests cover it as for every effect.
- **Templates:** `RetirementSpending` gains `rule: Option<SpendingRuleSpec>`.
  `None` emits the `Expense` it does today; `Some` emits a `Spending` effect
  instead.
- **Results bodies** (`results::project`): a `spending` block with the real
  fan and the four stats, absent when the run has none.
- **AI** (spec 16): the draft prompt and the plan facts tool describe the
  effect, so a draft can propose "guardrails starting at $80k".

## Analysis

- **Compare rules.** `spending::compare(snapshot, choices, iterations)`, built
  like `drawdown::compare`: the same base seed for every choice (common random
  numbers), 200 iterations by default.
  - Default choices: As planned, Fixed (an `Expense` at the plan's first-year
    draw), Percent of portfolio at the plan's initial rate, Guardrails, Capped
    change, VPW.
  - Per choice: funding success, median lifetime spending, `p10_lowest_draw`,
    `cut_rate` and median ending balance.
  - Replacing the plan's spending event with a choice is the same kind of
    normalization `drawdown::normalize` does for sweeps.
- **Sweeps and sensitivity.** `EffectParam::SpendingRule(SpendingField)` with
  fields `Rate`, `InitialAmount`, `Band`, `Step` and `Floor`, so a sweep or a
  sensitivity row can vary them.
- **Solve.** "The largest starting draw with funding success ≥ 95% and a P10
  lowest draw ≥ $X." It needs a second constraint metric:
  `SolveConstraintMetric::LowestDrawP10 { min_real }`. Constraints are
  fractions today, so this one carries its own floor. Bisection still applies,
  because the draw is monotone in the starting amount for every rule here.
- **Metrics.** `AnalysisMetric::{MedianLifetimeSpending, LowestDrawP10, CutRate}`.

## Review checks

Two catalogue entries (spec 21 Part 4):

| id | Layer | Looks for | Offers |
|---|---|---|---|
| `fixed_spending_stress` | Rule | A fixed retirement `Expense` with a first-year withdrawal rate above 4.5% and funding success below 85%. | A Change that replaces it with Guardrails at the same starting amount. |
| `spending_rule_no_floor` | Rule | A `Spending` rule without a floor whose `p10_lowest_draw` is under half its first draw. | Set a floor, shown with the funding success it costs. |

`retirement_spending_shape` (a Reviewer check) also counts a `Spending`
effect as retirement spending.

## The screens

- **Plan tab.** `EffectSentence` gets a Spending sentence, for example:
  "Spend from **Checking** using **Guardrails ▾**, starting at **$80,000** a
  year in today's dollars; cut **10%** when the withdrawal rate rises **20%**
  above where it started; raise **10%** when it falls **20%** below." Optional
  fields are a disclosure: floor and ceiling, the portfolio, pass-through, the
  final-years protection. The rule dropdown switches between rules while
  keeping the starting amount. Choosing "Fixed" turns the effect back into an
  `Expense`.
- **New scenario / Retirement template.** The spending step asks "Fixed, or
  adjust with the market?", with Guardrails as the suggested choice and a
  one-line explanation of each.
- **Results.** When the run has `spending`, a Spending card sits beside net
  worth:
  - a fan of real spending by year (the same chart and year focus as net
    worth);
  - tiles for median lifetime spending, "1 in 10 markets your draw dips to",
    and "cuts deeper than 10%";
  - funding success as the headline.

  The cash-flow ledger's Spending column already shows the representative
  path's total.
- **Analysis.** A "Spending rules" comparison next to Drawdown's strategy
  comparison, reusing `StrategyComparison`'s layout. Apply writes the chosen
  rule to the plan, as Drawdown's Apply does.
- **TUI.** It lists the effect read-only, with an "edit on the web" hint.
  Editing comes later.

## Build order

1. **Engine.** The `Spending` effect, `SpendingPortfolio`, per-year portfolio
   growth, `SpendingRuleState`, and the four rules with clamps and
   pass-through. Then `SpendingDiagnostics`, `RealSpendingAccumulator` and
   the new stats.
2. **Plan model.** The spec type, validation, compile, the `MODEL_VERSION`
   bump, the template option and the results block. Then `gen-bindings.sh`
   and the wasm rebuild.
3. **Web.** The Spending sentence and editor, the Results card, and the
   template step.
4. **Analysis.** Compare rules with Apply, sweep and sensitivity fields, the
   solve constraint and the metrics.
5. **Review checks.** `fixed_spending_stress` and `spending_rule_no_floor`.

Steps 1 to 3 are the useful core: a plan can use guardrails and see what they
cost. Step 4 makes it easy to choose a rule.

## Tests

- **Each rule on a fixed-return profile** (no volatility, known inflation):
  - percent of portfolio equals `rate × P` each year, and the clamps hold;
  - VPW spends to near zero at `final_age`;
  - capped change never moves more than its caps in real terms.
- **Guardrails** with a `MarketCrash` at year 3:
  - capital preservation cuts by `step` exactly once;
  - the inflation raise is skipped after the loss year only when the rate
    exceeds the initial rate;
  - nothing is cut within `protect_final_years`;
  - after a boom, prosperity raises.
- **Pass-through:**
  - $40k draw plus $30k income spends $70k, and the portfolio gives up $40k
    plus tax;
  - a $20k fixed `Expense` from the same account leaves the draw at $40k;
  - fixed costs above income plus the draw fall to the funding policy;
  - without pass-through the effect spends exactly `W`.
- **Cadence:** monthly and yearly events with the same rule give the same
  annual draw; a July start pays six twelfths.
- **State:** each iteration starts clean, and a re-simulated seed reproduces
  the same draws (Drawdown relies on this).
- **Results:**
  - `real_spending` quantiles match a direct computation from per-path
    results;
  - batches merge to the same summary as one run;
  - a plan without the effect serializes byte-for-byte as before, with the
    same snapshot hash;
  - Drawdown's balance identity still holds with a `Spending` effect.
- **Compare:** every choice shares the base seed; Fixed reproduces the
  plain-`Expense` plan's funding success.

## Open questions

1. **Gross or net withdrawal rate?** Published guardrails are stated on gross
   withdrawals. `W` is net cash, and its tax depends on which accounts fund it.
   Start net, so the draw is the number users spend. Revisit if comparisons
   against FI Calc disagree.
2. **Tax-deferred balances at face value?** Counting a 401(k) at its gross
   value overstates what it can spend. Counting it net of the tax it still owes
   (`deferred_exit_tax`, spec 21 Part 3) is more honest, but makes a rule's rate differ from the
   published one. Start gross; offer a "count pre-tax at after-tax value"
   option if users ask.
3. **Floors on total spending.** Users think "I need $60k to live", not "my
   draw must stay above $30k". A total-spending floor needs the year's income
   before it has arrived. Pass-through only knows it afterwards. Start with
   floors on the draw, worded as such.
4. **VPW's `real_return`.** It could be derived from the portfolio's return
   profiles (their mean real return, weighted by holdings) or come from the
   published VPW table by stock share. Deriving it ties the rule to the
   plan's own assumptions. Start derived, with the value shown and editable.
5. **Withdrawal order.** The draw is funded by the funding policy and its
   order (spec 20), which is unchanged. Should Drawdown's strategy rows and
   this spec's rule rows become one two-dimensional comparison? Not until
   both have been used separately.
6. **Couples and stochastic longevity.** VPW's `final_age` and Guardrails'
   final-years protection assume one person and a known horizon. They will
   need revisiting when those are modeled.

## Implementation notes

Where the build departs from the text above.

**Engine.**
- `SpendingRuleState` also keeps `last_nominal`, which the skipped inflation
  raise needs. Real figures are deflated by the path's inflation on the day
  the year's draw is decided, so a rule that does not move reads as flat.
- The draw is decided by `finplan_core::spending` (a `SpendingTracker` on the
  path, `None` without a `Spending` effect). A `Spending` effect inside a
  `Random`, or on an event that does not repeat monthly, quarterly or yearly,
  is an `EvaluationFailed` warning.
- Pass-through counts loan payments with the expenses, as Drawdown's
  spending line does. Its first occurrence spends only what came in that
  day, so savings built up before spending starts are not spent at once.
- Last year's return `r` is market growth (price moves, interest and market
  shocks) on the portfolio's accounts over their balance when the year
  opened, the previous December 31 after its events.
- `SpendingDiagnostics` use each year's decided draw; `lifetime_real` sums
  the draws paid, so a part year counts as paid. `MonteCarloStats.spending`
  carries the four stats plus `median_first_draw`. The fan's points are dated
  where net worth's grid closes each year; Results draws full calendar years
  only.

**Plan model.** The rule is stored whole, as JSON, in a new `effects` column
(`spending_rule`, with `pass_through`), and a named portfolio as withdrawal
items with the role `portfolio` (migration 0031, which also adds
`run_stats.spending`). VPW's `real_return` defaults to the portfolio's
expected compound return less expected inflation from the plan's own
profiles, filled in at compile. The interval and VPW's final age are checked
when the event is written and when the plan compiles. A preflight warning
(`spending_rules`) covers more than one rule. `RecurringExpenseParams` takes
the `rule` too, which is what `RetirementSpending.rule` and guided setup
(`SetupPlan.retirement_spending_rule`, Guardrails suggested) go through.

**Analysis.** Compare rules is `POST /runs/{id}/spending/compare` (and a
wasm export), a "Spending rules" mode in Analysis. It targets the plan's
`Spending` event, or else an `Expense` event named for retirement. A rule's
terms are analysis parameters (`spending:<event>:<term>`) for sweeps,
sensitivity and solve; What-if lists named parameters only. The solve floor
is `lowest_draw_floor` on the request, a second constraint in
`SolveConfig::also`: bisection steers by the main constraint, and the floor
is checked at its answer (where, for the largest draw, it is highest). The
web has no Solve screen, so the floor is reachable through the API only.

**Review checks.** `fixed_spending_stress` looks at enabled expense events
named for retirement. `spending_rule_no_floor` offers a floor at two-thirds of
the median first draw, for Guardrails and Percent of portfolio (Capped change
and VPW have no floor); the preview shows what it does to funding success.
