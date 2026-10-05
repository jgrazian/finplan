# Cash in investment accounts and Roth conversions

Status: proposed (2026-10-04). Follows [20](20_drawdown.md): the funding
policy, the Drawdown view and its comparison. Two engine changes, one metric
and a catalogue of review checks, each worth having alone, that together let
Drawdown and Review answer the question a large pre-tax balance raises: what
to do about the RMDs.

## Why

Drawdown on a plan that retires at 40 with a large taxable account and a
smaller 401(k) shows the pattern this spec is for:

- For thirty-odd years the plan lives on taxable sales. Its ordinary income is
  close to zero, and the low brackets go unused.
- The 401(k) compounds untouched. From 75 its RMDs force out more than the plan
  spends (by 87, $1.3M against $750k of spending), taxed at high rates. Tax on
  withdrawals reaches a third of spending.
- The excess lands as cash in the RMD's destination account. If that is an
  investment account, no sweep will ever spend it.

Four pieces address it:

1. **Cash first.** A withdrawal uses an investment account's own cash before it
   sells that account's holdings. Today that cash is stranded.
2. **Roth conversions**, as an event effect. Each year in a window, move pre-tax
   money to a Roth up to the top of a chosen bracket, filling the brackets the
   plan would otherwise waste.
3. **After-tax ending balance.** The metric that lets the comparison judge
   conversions fairly.
4. **Standard plan checks.** One catalogue of what a review looks for, with
   new checks that point a plan like this at Roth conversions and at
   reinvesting the cash its RMDs pile up.

Reinvesting surplus cash (an RMD larger than the year's spending) is left to
plan events: a yearly `CashTransfer` into a brokerage and an `AssetPurchase`
already model it, and a plan can choose the account, the buffer and the date.

## Part 1: cash first in investment accounts

### Today

An `InvestmentContainer` holds `cash` alongside its `positions`. That cash
grows at its cash return profile, and `Account::cash_balance` reports it, but
nothing withdraws it: a `Sweep` liquidates positions (`AssetSale`) and moves
only the proceeds (`evaluate.rs`, Sweep step 3). Money that reaches an
investment account as cash (an RMD paid into a brokerage, an `Income` deposited
there, a sale's leftovers) can only leave through an explicit `CashTransfer`.
The funding policy only covers bank deficits, so it does not reach it either.

### The rule

Within each account a sweep visits, take the account's cash before selling its
holdings. The order of accounts does not change. Cash is taxed by the
account's status, exactly as a sale of that account would be, minus the gain:

| Account | Cash withdrawn is | Tax |
|---|---|---|
| Taxable | Already-taxed money | None. No sale, no gain, no lot |
| Tax-deferred | A distribution | Ordinary income, plus the early-withdrawal penalty before 59½. The same treatment as `liquidate_tax_deferred_into` |
| Tax-free | A qualified distribution | None, as for a tax-free sale (but see the five-year rule in Part 2) |

The tax-deferred row is the trap: moving 401(k) cash as a plain transfer would
take it out untaxed. Cash leaves a tax-deferred account only through the same
tax path as a sale.

Mechanics:

- New `liquidation::withdraw_cash_into(params, amount, out)`, beside the three
  `liquidate_*_into` functions. It debits the account's cash and, for
  tax-deferred, pushes `IncomeTax` (and `EarlyWithdrawalPenalty`) on the gross
  amount and credits the net, exactly as a tax-deferred sale does with zero
  basis. Net mode grosses up with `calculate_gross_from_net`, as sales do.
- The Sweep loop calls it for each source account before building that
  account's `AssetSale`, for the smaller of the account's cash and what remains.
  `SingleAsset` sources skip it: they name a holding to sell.
- Bracket filling's per-step income ceiling applies to tax-deferred cash the
  same way it applies to sales.
- `ApplyRmd` gets it for free, since it is a `SingleAccount` sweep: an RMD
  counts the account's cash first. `actual_amount` sums cash withdrawn plus
  gross sale proceeds.
- Ledger: a new `StateEvent::CashWithdrawal { account_id, amount }` for each
  cash draw (gross), so Drawdown and the Results ledger can credit it to its
  account. Its tax events follow it, as a sale's do.
- Drawdown's projection adds `CashWithdrawal` amounts to the account's
  `withdrawals` alongside `AssetSale` proceeds; `withdrawal_taxes` already
  takes them in as gross minus net credited.

Results change for every plan whose investment accounts hold cash, so
`MODEL_VERSION` bumps. A plan with no investment cash runs byte-for-byte as
before; a test holds that.

## Part 2: Roth conversions

### What a conversion is (for the model)

- Moving pre-tax money (401(k), traditional IRA) into a Roth. There is **no
  annual limit** and no income limit. The ~$7k figure people quote is the IRA
  *contribution* limit, which needs earned income and is unrelated.
- The converted amount is ordinary income in the year of conversion. It cannot
  be undone.
- In a year an RMD is due, the RMD comes out first and cannot itself be
  converted.
- Before 59½, each conversion must sit in the Roth five years before it can be
  withdrawn without the 10% penalty. Retiring early, this is the **conversion
  ladder**: convert each year, spend each conversion five years later.
- Roth accounts owe no RMDs during the owner's life (Roth 401(k)s too, from
  2024).

### Why an event, not a plan policy

The funding policy (spec 20) lives at the plan level because it reacts to any
deficit the moment it appears, which no schedule can say. A conversion is the
opposite: a deliberate yearly action with a start, an end and an amount, which
is what an event is. RMDs are already modeled the same way (`ApplyRmd` on a
yearly repeating event). As an event, a conversion gets for free:

- **Its window** from the trigger: `Repeating` with start and end conditions by
  date, age or age parameter.
- **Analysis**: a plan parameter in the trigger or the amount is a sweep axis
  and a solve target (convert until what age? up to which bracket?).
- **Ladders and several sources**: several events.
- **Editing**: the Plan tab's event editor and the TUI already handle events.
  A template adds a ready-made one.

What events lack is "fill to the top of the bracket", and that is an
expression function, not a new kind of amount.

### Engine

**`bracket_room(rate)`**, an expression function: the top of the highest
federal bracket taxed at or below `rate`, less the year's ordinary income so
far, floored at zero. It reads the same indexed brackets, with the standard
deduction folded in, and the same `ytd_tax.ordinary_income` that
`BracketFilling` uses (`evaluate::bracket_ceiling`). It works in any amount,
including a Sweep's, and with a parameter (`bracket_room($ConversionCeiling)`)
it is sweepable.

**`EventEffect::RothConversion`**:

```rust
RothConversion {
    /// Tax-deferred account converted from.
    from: AccountId,
    /// Tax-free account converted into.
    to: AccountId,
    /// Gross amount to convert, e.g. `bracket_room(0.22)` or a fixed sum.
    amount: TransferAmount,
    /// Account whose cash or holdings pay the tax; None = withhold it from
    /// the conversion.
    pay_tax_from: Option<AccountId>,
}
```

- **What moves.** The account's cash first (Part 1), then holdings **in kind**:
  lots leave the pre-tax account and land in the Roth at the same units, oldest
  first, so the money stays invested and no sale is simulated. Basis is
  irrelevant inside a Roth. The amount is capped at what the account holds.
- **Tax.** The market value converted is ordinary income for the year: push
  `IncomeTax`, with no early-withdrawal penalty (a conversion is not a
  distribution for the penalty).
- **Paying it.** With `pay_tax_from`, the tax is debited from that account's
  cash, selling its holdings if needed (as a Net sweep would), and the funding
  policy covers a bank that runs short. That is the better choice: the whole
  conversion reaches the Roth. Withheld instead, less is converted and, before
  59½, the withheld part is an early distribution and pays the penalty.
- **RMD years.** If `from` owes an RMD this year that has not been taken, the
  effect is skipped with an `EffectSkipped` warning naming the order to fix.
  An RMD cannot be converted, and converting first would understate the next
  year's RMD base.
- **Ledger.** `StateEvent::RothConversion { from, to, amount, tax }`, plus the
  lot moves (a subtract/add pair per lot, or a new `AssetLotMoved`). Cash-flow
  summaries leave conversions out of withdrawals and expenses: no money left
  the plan.
- **Validation** (compile and the edit): `from` is tax-deferred, `to` is
  tax-free, `pay_tax_from` is taxable or a bank.

**Timing.** `bracket_room` is only right once the year's other ordinary income
has landed, so a conversion belongs at year-end: the template schedules it
yearly on Dec 30, and last in event order. (Not Dec 31: the engine captures
year-end balances, the next RMD's base, as Dec 31 begins and before that
day's events, so a Dec 31 conversion would stay in next year's RMD base.) A conversion scheduled mid-year
still works; it just fills against the income so far. The event editor shows
a hint when a `bracket_room` amount fires before December.

**The five-year rule.** Each conversion is recorded on the Roth as a
`ConversionTranche { year, amount }`. A withdrawal from a tax-free account
before 59½ consumes basis first: contributions (not modeled; zero), then
tranches oldest first. Any part drawn from a tranche younger than five years
pays the early-withdrawal penalty. Earnings come out last and, before 59½, are
penalized and taxed. v1 can ship with the tranches recorded and only the
tranche penalty enforced; the tests say which is modeled.

**Not modeled** (said in the event editor's help, and in the open questions):
IRMAA Medicare surcharges, ACA premium credits, and the taxable share of Social
Security, all of which a real conversion plan watches. The engine taxes Social
Security as the plan's `IncomeType` says, and gains at a flat
`capital_gains_rate`. In law, long-term gains stack on top of ordinary income,
so each converted dollar can push a dollar of gains out of the 0% band. For a
plan that lives on taxable sales while converting, that interaction is a large
part of the answer, and this engine does not yet see it.

### Plan model

- `RothConversion` joins the effect kinds: a migration widens the `effects.kind`
  CHECK, and the effect stores `from` (`account_id`), `to` (`to_account_id`),
  its amount through the existing amount rows, and `pay_tax_from` in a new
  nullable column. `EffectSpec::RothConversion` (ts-rs), compile, read, archive.
- `bracket_room` joins the expression functions in the parser, renderer and the
  expression reference the UI shows.
- **Template** `RothConversions { from, to, ceiling_rate, start, end, pay_tax_from }`
  creates a yearly Dec 30 event from `start` (default: the retirement
  parameter, else now) until `end` (default: the year before the RMD age),
  amount `bracket_room(<rate>)`. The Plan tab offers it as "Add Roth
  conversions"; the drafting agent can use it.
- Preflight: a note when a plan has a tax-deferred balance and a Roth account
  but no conversion, and RMDs would begin inside the plan. Informational only.

## Part 3: after-tax ending balance, and conversions in Drawdown

A dollar in a 401(k) is not a dollar in a Roth: the heir, or the owner's later
withdrawals, pay tax on it. Compared on pre-tax ending balance, a conversion
always looks worse, since it pays tax now and shrinks the total. So:

- **Plan setting** `deferred_tax_rate` (default 24%): the rate applied to
  tax-deferred balances when valuing the plan after tax. Stored on the scenario,
  edited beside the tax config.
- **`after_tax_ending_balance`** = taxable + tax-free + bank + property − debt +
  tax-deferred × (1 − rate). Reported per path, in the Drawdown summary and
  comparison rows (median over iterations, nominal and real), and as an
  `AnalysisMetric` so sweeps and solve can target it.
- **The comparison strip** shows after-tax ending balance as its headline
  balance column. Pre-tax stays available on hover.

Drawdown:

- A **conversion toggle** beside the strategy chips: *As planned · None · up
  to 12% / 22% / 24%*. `StrategyChoice` gains `conversion: Option<ConversionChoice>`.
  Normalizing for a rate sets every `RothConversion` effect's amount to
  `bracket_room(rate)`; when the plan has none, it adds an overlay event (from
  the retirement date until the year before RMDs, Dec 31, from the largest
  tax-deferred account into the largest tax-free one, tax paid from the largest
  taxable account) and reports `conversion_overlay`. *None* disables the plan's
  conversions. The toggle is disabled, with the reason, when the plan has no
  tax-free account to convert into.
- The comparison runs each strategy with the toggle's setting, so
  "Tax-efficient early + convert to 22%" competes with "Bracket filling +
  convert to 22%" on the same markets.
- Conversions are not spending, so they are not in the bars. Each year that
  converts gets a **hollow marker above the axis** at its amount, and its tax
  joins the year's tax below zero, marked apart from tax on withdrawals.
- `DrawdownYear` gains `conversion` (gross) and `conversion_tax`; the side
  panel shows both and the Roth's balance; the CSV gains the columns.
- Apply to plan writes the conversion choice: it retargets the plan's
  conversion events, or adds the template's event when there are none.

## Part 4: standard plan checks

### Today

A plan is checked in three places, and nothing lists them together:

| Layer | Where | When | What it produces |
|---|---|---|---|
| Preflight | `finplan_plan::preflight` | Before a run | Warnings on the Run button and the run's review |
| Rules | `finplan_plan::rules` | Every review, deterministic | Review notes with evidence, and paths where the fix is unambiguous |
| AI reviewer | `finplan_server::suggest::ai` (`prompt.rs`) | A review the user asks for | Notes it writes and previews itself, by the prompt's priorities (correctness, realism, risk, optimization), never repeating a rule's note |

The AI reviewer is told what the rules wrote but not what they *checked*, so it
cannot tell "the rule looked and found nothing" from "no rule covers this". A
user cannot see either.

### The catalogue

`finplan_plan::rules::catalogue()` returns every standard check as data:

```rust
pub struct PlanCheck {
    pub id: &'static str,          // the rule or preflight code
    pub layer: Layer,              // Preflight | Rule | Reviewer
    pub section: Section,          // portfolio | plan | results
    pub kind: Kind,                // fix | check | stress | read
    pub looks_for: &'static str,   // one sentence
    pub offers: &'static str,      // the path it proposes, or "none"
}
```

- Every rule and preflight code has an entry, and a test fails when a rule or
  code exists without one (or an entry without its rule).
- `Layer::Reviewer` entries are checks with no deterministic rule: the AI
  reviewer is responsible for them. They are how a check that needs judgment
  or a preview gets onto the list.
- The review prompt gets the catalogue, with each Rule entry marked *ran,
  wrote N notes* or *ran, found nothing*, and the Reviewer entries as its own
  checklist. The prompt's line "do not repeat a note the rules already wrote"
  becomes "do not redo a check marked ran".
- The Review tab shows it as "What Review checks", collapsed under the board,
  with each check's state for the current review.

### The standard checks

Existing (unchanged, catalogued):

| Id | Layer | Looks for |
|---|---|---|
| `invalid_plan` | Preflight | The plan does not compile |
| `unmapped_asset` | Preflight | A held asset with no return model |
| `empty_event` | Preflight | An enabled event with no effects |
| `missing_spending` | Preflight | No expense anywhere in the plan |
| `funding_intent` | Preflight | Spending, but no sweep, transfer or funding policy |
| `missing_birth` | Preflight | No birth date, so ages and RMDs are guesses |
| `horizon` | Preflight | The plan ends before a reasonable life expectancy |
| `no_inflation` | Preflight | No inflation profile |
| `assumptions` | Preflight | Return and inflation assumptions to confirm |
| `cost_basis_equals_value` | Rule | Taxable lots that assume no embedded gains |
| `idle_bank_cash` | Rule | A bank account holding over two years of spending, year after year |
| `unused_contribution_limits` | Rule | Accounts with a contribution limit nothing pays into |
| `unmapped_or_mismatched_assets` | Rule | Holdings whose return model does not fit them |
| `liability_payment_inflation_adjusted` | Rule | A loan payment that grows with inflation |
| `sweep_sells_while_cash` | Rule | A sweep selling investments while the cash is already there |
| `shortfall_account_concentration` | Rule | Failures concentrated in one account, or failing paths that end solvent |
| `success_vs_funding_gap` | Rule | Success rate flattered by illiquid property |

New, from this spec and spec 20:

**`roth_conversion_opportunity`** (Rule detects, Reviewer writes; plan; fix).
- *Fires when*, on the shown path: the plan holds tax-deferred money, RMDs fall
  due inside the plan, there are years before RMDs with ordinary income below
  the top of the 22% bracket (`bracket_room(0.22)` > 0), and either the RMD
  years show a surplus (RMDs beyond spending) or their marginal rate is above
  the pre-RMD years'. No `RothConversion` event exists.
- *The rule* supplies the facts as evidence: the unused bracket room by year,
  the first RMD year and amount, the surplus, the tax on withdrawals.
- *The reviewer* writes the note and its paths with the `RothConversions`
  template: typically "up to 12%" and "up to 22%", tax paid from the largest
  taxable account, until the year before RMDs. For a plan retiring before 59½
  it says the conversions are also a ladder for early access. A plan with no
  Roth account gets a path whose first step adds one.
- *Materiality* is measured on after-tax ending balance and lifetime tax (Part
  3), not success rate: a conversion often leaves success unchanged and still
  saves a great deal. The preview tool reports both.
- *As built*: the rule writes the note itself, facts and the 12% / 22%
  template paths included, so a review without the model still shows it; the
  reviewer reads it as ran. Preview stats carry `after_tax_final` (median,
  nominal) and `lifetime_taxes` (median path, nominal), and the materiality
  floor counts either one moving by the median's floor.

**`cash_accumulates`** (Rule; portfolio; fix). Extends `idle_bank_cash`.
- *Fires when*, on the shown path, cash (bank, or uninvested cash in an
  investment account) grows above two years of spending for three or more
  consecutive year-ends, *starting after the plan begins*: typically from the
  first RMD year, when distributions exceed spending. `idle_bank_cash` keeps the
  case where cash is idle from the start.
- *Offers* the `ReinvestCash` template: a yearly Dec 31 event that moves cash
  above a buffer (two years of that year's spending, inflation-adjusted) from
  the account into the plan's largest taxable account and buys its holdings,
  one `AssetPurchase` per holding at its current weight. For uninvested cash
  in an investment account, the purchase happens in place, with no transfer.
- `ReinvestCash` is a template, not a policy: the plan owns the event, and can
  change the buffer, the account or the timing.

**`rmd_missing`** (Rule; plan; fix). The plan holds tax-deferred money and
reaches RMD age, but no event applies RMDs: the engine only takes them when an
`ApplyRmd` effect runs, so the plan silently skips them and understates tax.
Offers a yearly `ApplyRmd` event from the RMD age into the plan's main bank
account. (Correctness: exempt from the materiality floor.)

**`rmd_into_investment_cash`** (Rule; plan; check). An `ApplyRmd` pays into an
investment account. Until Part 1 ships, that cash is never spent; after it, it
is spent but sits uninvested. Offers retargeting the RMD to the bank, or adding
`ReinvestCash` in place.

**`early_withdrawal_penalties`** (Reviewer; plan; fix). The shown path pays
early-withdrawal penalties above a threshold (for example 1% of lifetime
spending). Paths: Penalty-aware withdrawal order (spec 20 funding policy), or a
conversion ladder started five years before the penalized withdrawals. The
reviewer reads the penalties by year from the run's lifetime tax line (and
`inspect_path`). A change cannot set the funding policy, so the
penalty-aware path changes the sweeps that pay for spending; where the policy
is what sells, the note names it.

### Templates added

- `RothConversions { from, to, ceiling_rate, start, end, pay_tax_from }` (Part 2).
- `ReinvestCash { from, to, buffer_years }`: the yearly event above. Its
  transfer amount is `max(0, cash(source) - inflation(<buffer>))`, the buffer
  fixed in plan-start dollars when the template runs; its purchases split by
  the target's weights at that moment, and say so in the event's description.

Both join `Template`, the drafting agent's tool schema, and the review tools,
so a note's path is an `expand_template` step like the existing ones.

## Build order

1. **Cash first.** `withdraw_cash_into`, the Sweep loop, `CashWithdrawal`,
   Drawdown's attribution. `MODEL_VERSION` bump.
2. **After-tax ending balance.** The setting, the metric, the comparison strip.
   It ships before conversions so the comparison is fair from their first day.
3. **Conversions in the engine and plan.** `bracket_room`, the effect (in-kind
   move, tax, RMD guard, tranches, ledger), the effect kind, the template and
   the Plan tab's "Add Roth conversions".
4. **Conversions in Drawdown.** The toggle, the normalization, the marker, the
   comparison, Apply.
5. **Standard plan checks.** The catalogue, its test, the prompt and Review tab
   changes, then the new checks: `rmd_missing` and `rmd_into_investment_cash`
   first (correctness, no dependencies), `cash_accumulates` with
   `ReinvestCash`, then `roth_conversion_opportunity` and
   `early_withdrawal_penalties`, which need Parts 2 and 3.

## Tests

- Cash first: taxable cash covers a sweep with no sale and no tax; tax-deferred
  cash is taxed as ordinary income and penalized before 59½; a Net sweep from
  tax-deferred cash grosses up; an RMD on an account holding cash counts it;
  a plan with no investment cash is unchanged.
- `bracket_room`: equals the indexed ceiling less year-to-date ordinary income,
  floors at zero, includes the standard deduction band, and follows a parameter.
- Conversion: a Dec 31 `bracket_room(0.22)` conversion lands the year's ordinary
  income exactly at the ceiling; lots arrive with their units; tax paid from
  `pay_tax_from` leaves the conversion whole; withholding before 59½ pays the
  penalty; an RMD year skips until the RMD is taken; a tranche withdrawn inside
  five years before 59½ pays the penalty and one after five years does not.
- After-tax balance: equals pre-tax when there is no tax-deferred money; a
  conversion that fills low brackets ahead of high-bracket RMDs beats no
  conversion on it for a fixture built to show that.
- Drawdown: every year still balances (spec 20's identity) with conversions
  on; the conversion marker and tax line up with the ledger; the overlay is
  added only when the plan has no conversion event.
- Checks: every rule and preflight code is in the catalogue and every Rule
  entry has its rule; each new rule fires on a fixture built for it and stays
  quiet on the default plan where it should; `rmd_missing`'s path makes the run
  record RMDs; `cash_accumulates`'s path stops the build-up on the shown path;
  the prompt lists each Rule entry with its ran/notes state.

## Open questions

1. Should the template's default end be the year before RMDs, or the year
   before 63 (when IRMAA's two-year lookback starts to matter)? Default to RMDs
   and let the user move it until IRMAA is modeled.
2. Model QCDs (qualified charitable distributions, from 70½, up to a yearly
   limit, counting toward the RMD and untaxed)? Only if plans carry charitable
   giving; it would be an `ApplyRmd` option.
3. Stack long-term gains on ordinary income (a 0% / 15% / 20% gains schedule)?
   It changes every taxable sale, so it is its own spec, but conversion advice
   is only as good as this.
4. Should `bracket_room` count the year's income still to come (a salary paid
   monthly through December), so a mid-year conversion is right too? It would
   need the engine to project the rest of the year; year-end scheduling avoids
   it for now.
5. Should the catalogue's Reviewer entries be weighted per plan (skip
   `roth_conversion_opportunity` for a plan with no tax-deferred money), or is
   the reviewer trusted to skip what does not apply? Weighting saves tokens;
   start with the reviewer deciding.
