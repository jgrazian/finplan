# Drawdown: which accounts pay for retirement

Status: implemented (2026-10-04, branch `drawdown-funding-policy`); see
"Implementation notes" at the end for where it departs from this text. Design:
claude.ai/design project `ddf1acc9-…`, file `Finplan Drawdown Analysis.dc.html`
("Analysis · Drawdown", panel 1a).

A fourth Analysis mode. For each retirement year it draws the plan's spending
as a line and, under it, stacked bars of where the money came from: income
(Social Security, pensions), each investment account, and cash balances. The
user can switch between withdrawal strategies, compare what each one costs in
taxes and ending balance, and apply the one they pick to the plan.

Getting there takes two pieces of work, and the first one is worth doing even
without the second:

1. **A plan-level funding policy** in the engine: "when cash runs short, sell
   investments in this order". Today the engine only sells investments when an
   event tells it to, so a plan without a Sweep just records a shortfall.
2. **The Drawdown projection**: re-simulate the run's median market path once
   per strategy and break each year into sources.

## Why a funding policy

The engine does not move money by itself. Its shortfall warning says so:
*"Other assets do not automatically fund spending; add a withdrawal or transfer
rule."* Plans fund spending in three ways today:

| How the plan was made | Funding |
|---|---|
| Guided setup, `fund_from_investments: true` | Every recurring expense event carries its own `Sweep` (`top_up`, `TaxEfficientEarly`) before its `Expense` (`templates::recurring_expense`). N expenses, N sweeps, each with its own strategy. One-off expenses get none. |
| Guided setup, `fund_from_investments: false` | No sweep; cash runs out and the run records a `CashShortfall`. |
| Hand-built or AI-drafted | Anything: one standalone sweep, `Custom`/`SingleAccount` sources, or none. |

Preflight only warns (`funding_intent`). A drawdown view that "overrides the
plan's Sweep" has nothing to override in the second row, an ambiguous set in
the first, and no say over one-off expenses, loan payments or tax bills in any
of them. A single policy that covers every deficit fixes all three.

## Part 1: the funding policy

### Engine (`finplan_core`)

```rust
/// Cover cash deficits by selling investments.
pub struct FundingPolicy {
    pub order: WithdrawalOrder,
    /// Investment accounts never sold to cover a deficit.
    pub exclude_accounts: Vec<AccountId>,
    /// First date the policy acts; None = the whole plan.
    pub from: Option<jiff::civil::Date>,
}

// SimulationConfig
#[serde(default)]
pub funding: Option<FundingPolicy>,
```

**The settle step.** `simulate_inner` already waits for a date's events to
settle before it tests for a shortfall (`record_cash_shortfall`). Just before
that test, when a policy is set and `current_date >= from`:

- For each **bank account** (`AccountFlavor::Bank`) whose balance is below
  `-0.005`, in account-id order, evaluate and apply
  `EventEffect::Sweep { sources: Strategy { order, exclude_accounts }, to: that
  account, amount: fixed(deficit), amount_mode: Net, lot_method: Fifo,
  income_type: TaxFree }` through the same `evaluate_effect_into`/apply path
  events use. Liquidation, lots, tax withholding, early-withdrawal penalties
  and bracket ceilings are all reused unchanged.
- Ledger entries it writes have `source_event: None`. The sales are ordinary
  `AssetSale`s and the credit is `LiquidationProceeds`, so the cash-flow
  summaries and the Results ledger need no change. The ledger renders them as
  "Funding policy".
- If the investments cannot cover the deficit, what they can cover is sold and
  the rest stays a shortfall, recorded as today.
- An error from the sweep (nothing to sell) is not a warning: the shortfall
  check that follows records the problem.

Event-level sweeps fire during the event passes and so always run first. The
policy is a backstop: it only covers what is still short after them.

`None` (the default) leaves a run byte-for-byte as it is today.

**`ProRata`, for real.** `strategy_sources` documents "Proportional draws are not
implemented; this is account order." Implement it: a `ProRata` sweep splits
the net amount it needs across the eligible accounts in proportion to their
current market value, then makes a second pass in account order for any amount
a capped account could not supply. Both event sweeps and the policy get it.

**Tax payments get their own kind.** Add `CashFlowKind::Tax`. The RSU vest
sell-to-cover in `evaluate.rs` debits the tax as `Expense`; it becomes `Tax`.
`build_yearly_cash_flows` counts `Tax` debits in `expenses` exactly as before,
so stored figures do not move, but anything that wants "spending" can now
leave taxes out.

**Percentile seeds.** `MonteCarloSummary` gains
`percentile_seeds: Vec<(f64, u64)>` (`finish_inner` already computes them). A
run's paths can then be re-simulated exactly: `simulate(config, seed)`.

### Plan model (`finplan_plan`, server, wasm)

Storage, migration `0022_funding_policy.sql`:

```sql
ALTER TABLE scenarios ADD COLUMN funding_strategy TEXT CHECK (funding_strategy IS NULL OR
    funding_strategy IN ('TaxEfficientEarly','TaxDeferredFirst','TaxFreeFirst','ProRata',
                         'PenaltyAware','BracketFilling'));
ALTER TABLE scenarios ADD COLUMN funding_bracket_ceiling REAL CHECK (funding_bracket_ceiling IS NULL
    OR (funding_bracket_ceiling >= 0 AND funding_bracket_ceiling < 1));
CREATE TABLE scenario_funding_excludes (
    scenario_id INTEGER NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
    account_id  INTEGER NOT NULL REFERENCES accounts(id)  ON DELETE CASCADE,
    PRIMARY KEY (scenario_id, account_id)
);
```

`funding_strategy IS NULL` means off. A stored policy covers the whole plan;
`from` is only set by Drawdown's overlay (below).

- `ScenarioRow` gains `funding_strategy: Option<String>` and
  `funding_bracket_ceiling: Option<f64>`; `ScenarioGraph` gains
  `funding_excludes: Vec<i64>`. All `#[serde(default)]`, so plans already in
  IndexedDB and older archives and snapshots load as "off".
- `specs::FundingPolicySpec { strategy: WithdrawalStrategy, bracket_ceiling:
  Option<f64>, exclude_accounts: Vec<i64> }`, ts-rs exported. Read as part of
  the scenario detail (`funding: Option<FundingPolicySpec>`).
- Write: `PUT /scenarios/{id}/funding` with `{ funding: FundingPolicySpec | null }`.
  It is an `EditOp` like every other write route, held equal to its edit in
  `domain/edit_route_tests.rs`, and the local store answers it through
  `edit::apply`. Validation: excluded accounts must exist and be investment
  accounts; the bracket ceiling only with `BracketFilling`.
- Compile maps it to `SimulationConfig::funding` through the id map.
- `PlanArchive` and the snapshot carry it; `MODEL_VERSION` bumps because the
  config changed.
- Deleting an account drops it from the excludes (the FK cascades on the
  server; the in-memory delete edit must do the same).
- Duplicate copies it.

Behaviour changes:

- **Preflight**: `funding_intent` is satisfied by an enabled policy as well as
  by a Sweep or CashTransfer.
- **Guided setup**: `fund_from_investments: true` turns the policy on
  (`TaxEfficientEarly`) and no longer adds a Sweep to each expense. False
  leaves it off. The drafting agent's templates keep their own behaviour (out
  of scope).
- **Shortfall warning text**: when a policy is on, the message says the
  investments it may sell ran out, instead of telling the user to add a rule.

UI: a "When cash runs short" setting on the Plan tab: *Sell investments using
[strategy ▾] (bracket ceiling when Bracket filling) · Never sell [accounts]* or
*Don't sell; record a shortfall*.

## Part 2: the Drawdown projection

### Re-run, don't fork

A run's market path is sampled from its seed before the first event
(`SimulationState::from_parameters` → `Market::from_profiles`); events use a
separate RNG. Re-simulating the same seed with a different withdrawal order
therefore sees exactly the same returns and inflation: only the withdrawals
differ. That gives a fair comparison without engine support for saving and
resuming state mid-run. Forking at the retirement date was rejected: it would
need `SimulationState` to be serializable (lots, YTD tax, event state, RNG
position) only to skip the cheap working years, and the order can matter
before retirement (bridge years, `PenaltyAware` before 59½).

Drawdown re-runs against the **run's input snapshot**, not the live plan, so
the view matches "Last run Oct 3", and with the **median path's seed**.

Caveat shown in the UI: the median is chosen by final net worth under the
plan's own strategy. For the other strategies it is the same market, not their
own median, so the subtitle reads "median market path".

### Normalizing the plan for a strategy

Given the run's compiled `SimulationConfig` and a strategy `S`:

1. Every `Sweep` whose sources are `Strategy { .. }` gets `order = S` (its
   `exclude_accounts` kept). Sweeps nested in `Conditional`/`Sequence` effects
   too.
2. `SingleAsset`, `SingleAccount` and `Custom` sweeps are left alone and
   listed in the response as `fixed_sweeps` (event id and name), so the UI can
   say "Roof fund always sells from Brokerage".
3. The funding policy: if the plan has one, its order becomes `S`. If it has
   none, one is installed as an **overlay** with `from` = the retirement date,
   and the response says `overlay: true`.

Plus one extra row, **As planned**, which runs the snapshot unchanged (no
overlay). For a plan with neither policy nor strategy sweeps it shows the cash
running out.

### Retirement date

The engine has no notion of retirement. In order:

1. `request.retirement_year`, when the user has set it.
2. The plan's Age parameter whose name contains "retire"
   (`what_if::retirement_ages` already finds it), at the birth date.
3. The first year after which no `Income` credit from an event whose
   `IncomeType` is wage income lands. If none, the plan's start year.

The response says which one it used (`retirement_source`).

### The projection (`finplan_plan::drawdown`)

Pure, no I/O, builds for wasm:

```rust
pub fn project(
    snapshot: &ScenarioGraph,       // the run's input snapshot, compiled inside
    seed: u64,
    request: &DrawdownRequest,
) -> PlanResult<DrawdownBody>;
```

`DrawdownRequest { strategies: Vec<StrategyChoice>, retirement_year: Option<i64> }`,
where `StrategyChoice` is `AsPlanned` or `Strategy { strategy, bracket_ceiling }`.
Default: As planned, then the six engine strategies (Bracket filling at 12%).

For each choice it simulates once (with the ledger) and folds that path's
ledger into per-year rows from the retirement year to the end:

| Field | Source |
|---|---|
| `spending` (the line) | Σ `CashDebit { kind: Expense }` |
| `income[]` per event | Σ `CashCredit { kind: Income }` by `source_event` |
| `withdrawals[]` per account | Σ `AssetSale.proceeds` (gross) by `account_id` |
| `rmd[]` per account | Σ `RmdWithdrawal.actual_amount` by `account_id` (a subset of withdrawals, for the marker and panel) |
| `withdrawal_taxes` | gross sales − the `LiquidationProceeds` credits they produced (taxes and penalties withheld) |
| `cash` | drawn from bank balances: the residual below, when positive |
| `surplus` | money withdrawn beyond need (RMDs): the residual, when negative |
| `shortfall` | increase over the year in the total of negative bank balances |
| `balances[]` per account | year-end snapshot |
| `inflation` | the path's cumulative factor (Today's $) |
| `total_tax` | `yearly_taxes` total + penalties |

The bars must add up, and a test holds them to it each year:

```
Σincome + Σwithdrawals + cash + shortfall = spending + withdrawal_taxes + surplus  (± $1)
```

where `cash`/`surplus` absorb whatever else moved through the bank accounts
(interest, transfers, contributions). Per choice it also reports: lifetime
spending, lifetime total tax, ending balance (nominal and real), first
shortfall year, and markers (first year of each income event, first RMD year
per account).

Accounts are labelled from the snapshot (as `account_labels` is today);
income sources from event names.

### Comparison across many markets

One path cannot say which strategy is better. `compare(snapshot, request,
iterations)` runs a small Monte Carlo per choice on the **same base seed**
(common random numbers, `analysis::ANALYSIS_SEED`), so differences come from
the strategy and not from sampling noise, and returns per choice: success
rate, funding success rate, median final net worth, and median lifetime tax.
Iterations default to 200 per choice and are capped like a quick what-if, so a
full comparison stays inside one request on the server and one job on the
local compute pool.

### Endpoints

| | Cloud | Local |
|---|---|---|
| Projection | `POST /runs/{run_id}/drawdown` → `DrawdownBody` | `drawdown(snapshot, seed, request)` wasm export via the store worker |
| Comparison | `POST /runs/{run_id}/drawdown/compare` → `DrawdownComparison` | `drawdown_compare(...)` on a compute worker |
| Apply | `PUT /scenarios/{id}/funding` (Part 1), plus updating the strategy sweeps | same `EditOp` through the store |

Both read the run's stored snapshot and its median seed. A run saved before
seeds were stored answers 409 "Run the plan again to see drawdown."

Seed storage: `run_percentiles` gains `seed TEXT` (decimal string, a `u64` like
`worst_seed`); `PathResults` gains `seed: Option<String>`; the local store keeps
it with the run.

### Apply to plan

Writes the chosen strategy to the funding policy (turning it on when it was
off) and, in the same edit, sets every `Strategy`-sourced Sweep to the same
order, so the plan has one strategy everywhere. It reports which sweeps it
changed and that the plan needs a new run.

## Part 3: the screen

Follows the design (panel 1a) with these changes:

- **Chips**: As planned, Tax-efficient early, Tax-deferred first, Tax-free
  first, Pro rata, Penalty aware, Bracket filling (with a 10/12/22% ceiling
  picker). The design's "cash first" ordering does not exist: strategies order
  investment accounts only, and cash is whatever the bank balances supply.
- **Taxes hang below zero.** Above the axis, sources (accounts net of the
  tax withheld on them) meet the spending line, with any surplus (RMDs beyond
  need, outlined) above it and a hatched cap below it for a shortfall. Below
  the axis, the tax withheld on withdrawals grows downward, so a strategy's tax
  cost is visible without reading as overspending. Share mode leaves it out.
- **A comparison strip** under the chips: success rate, median ending balance
  and median lifetime tax for each strategy, from `compare`, with the best of
  each highlighted.
- **Markers come from the data**: first year of each income event, first RMD.
- **Today's $** uses the path's own inflation factors, not a fixed rate.
- **Overlay banner** when the plan has no funding policy: "Your plan doesn't
  sell investments to pay for spending. This view assumes it does from 2041."
  with a button that turns the policy on.
- **Fixed sweeps** listed under the side panel.
- Selected year side panel, Dollars/Share and Nominal/Today's $ toggles,
  Export CSV, and "Over retirement" as designed.

The view model lives in `web/lib/view/drawdown.ts`; the URL keeps the
selected strategy and pinned year (`lib/nav`).

## Build order

1. Core: `FundingPolicy` and the settle step, `CashFlowKind::Tax`, real
   `ProRata`, `percentile_seeds` on the summary.
2. Plan, server and wasm: the funding policy as plan data (storage, edit,
   route, compile, archive, preflight, guided setup), seed storage.
3. `finplan_plan::drawdown` (`project`, `compare`), its routes and wasm
   exports, bindings.
4. Web: the Plan-tab funding setting, then the Drawdown mode.

## Open questions

1. Should the policy's floor be configurable ("keep $10k in checking")? Not
   now: `Option<f64>` later, default 0.
2. Should guided setup's old per-expense sweeps be migrated to the policy for
   existing plans? No: they keep working, and Apply offers to align them.
3. Does the drafting agent learn the policy? Later, through its tool schema.

## Implementation notes

What shipped differs from the text above in these places:

- **Engine.** `ProRata` takes shares from market values when the sweep starts.
  Its second pass, in account order, has no test yet: a Net sale grosses up for
  tax, which makes a capped account hard to build. `RmdWithdrawal.actual_amount`
  counted the transfer to a different destination as well as the sale. It now
  counts only `LiquidationProceeds`.
- **Plan data.** The scenario row carries the policy fields only when they are
  set, so existing snapshot hashes and goldens are unchanged. `MODEL_VERSION`
  is `finplan-0.9.0/snapshot-1`. The server selects the policy as one JSON
  column on `Scenario`. Changing an account's flavor is already refused, so
  excludes cannot go stale through `UpdateAccount`.
- **Projection.** `project` and `compare` take the snapshot graph. The core has
  no `Conditional` or `Sequence` effect; `Random` is the only nested effect that
  `normalize` walks. With no wage notion in the engine, retirement "from income"
  is the year after the last `Income` credit from an event not named like a
  benefit or investment income. Lifetime spending and tax cover the retirement
  years only.
- **Comparison.** The Monte Carlo keeps no tax per iteration, so the tax column
  is the tax on each strategy's median path: one extra simulation per strategy.
  Iterations are 25 to 500 per strategy, default 200, on `ANALYSIS_SEED` with
  four batches, so the server and the browser agree.
- **Apply.** `SetFunding.align_sweeps` sets the policy and every Strategy-mode
  withdrawal source in one edit. The response does not list the sweeps it
  changed.
- **Screen.** Taxes and surplus come off each account's gross sales in
  proportion, so account amounts are after tax. The tax was first stacked on
  top of the bar, past the spending line, where it read as overspending; it
  now hangs below zero. The side panel shows it outside the funded total, as a
  share of spending. The bracket ceiling is part of the strategy key in `sel`, and the
  pinned year is `yr`. With no `sel`, the screen opens on the plan's current
  strategy. The app has no toast, so Apply confirms with an inline notice.
  `FundingRuleDialog` was removed, since nothing used it.
- **Open.** An investment account's balance includes its uninvested cash, which
  the policy never sells, so a year can show a balance and a shortfall
  together. A snapshot from an older `MODEL_VERSION` that no longer compiles
  gets an error rather than the "run again" 409.
