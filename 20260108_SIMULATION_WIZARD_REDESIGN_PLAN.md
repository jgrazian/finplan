# Simulation Wizard Redesign Plan

## Overview

This document outlines a complete redesign of the simulation wizard to provide a conversational, user-friendly experience that guides users through building a comprehensive financial simulation. The new wizard will feel like speaking with a financial advisor rather than filling out technical forms.

---

## Table of Contents

1. [Design Philosophy](#design-philosophy)
2. [Current State Analysis](#current-state-analysis)
3. [New Wizard Flow](#new-wizard-flow)
4. [Detailed Step Specifications](#detailed-step-specifications)
5. [Data Mapping](#data-mapping)
6. [Backend API Changes](#backend-api-changes)
7. [UI/UX Guidelines](#uiux-guidelines)
8. [Implementation Phases](#implementation-phases)
9. [Technical Architecture](#technical-architecture)

---

## Design Philosophy

### Guiding Principles

1. **Conversational Tone**: Questions should feel like a friendly financial advisor asking about your life, not a tax form.
2. **Progressive Disclosure**: Start simple, reveal complexity only when needed.
3. **Smart Defaults**: Pre-fill sensible values based on earlier answers (e.g., if age 30, suggest 65 for retirement).
4. **Contextual Help**: Every question should have an "Why does this matter?" tooltip.
5. **Visual Feedback**: Show a running summary of the financial picture as the user progresses.
6. **Skip & Return**: Allow users to skip sections and return later.
7. **Mobile-First**: Design for touch interactions first.

### Target User Personas

1. **Novice**: "I have a 401k and want to know if I can retire at 60" - Needs hand-holding, simple language
2. **Intermediate**: "I have multiple accounts and want to optimize withdrawals" - Understands concepts, wants details
3. **Advanced**: "I want to model specific scenarios with custom events" - Needs full control

---

## Current State Analysis

### Current Wizard Steps (7 Steps)

| Step | Focus | Problems |
|------|-------|----------|
| Basics | Name, dates, portfolio selection | Too technical, asks for portfolio before context |
| Profiles | Inflation & return assumptions | Advanced concept thrown at users too early |
| Asset Linking | Map assets to profiles | Confusing for non-experts |
| Cash Flows | Income & expenses | Requires manual account/asset selection |
| Events | Life events | Shows "coming soon" - not implemented |
| Spending | Retirement spending targets | Withdrawal strategy is confusing |
| Review | Final summary | Just a data dump, not actionable |

### Key Issues

1. **No personal context**: Jumps straight to portfolios without understanding the person
2. **Technical jargon**: "Tax-deferred", "return profiles", "withdrawal strategy"
3. **Manual ID management**: Users must select account/asset IDs
4. **No guidance**: Doesn't explain what each field means for their future
5. **Events not functional**: The most powerful feature is disabled
6. **Assumes expertise**: Expects users to know inflation rates, return expectations

---

## New Wizard Flow

### Overview: 10 Conversational Sections

```
┌─────────────────────────────────────────────────────────────────┐
│  1. WELCOME              "Let's plan your financial future"     │
├─────────────────────────────────────────────────────────────────┤
│  2. ABOUT YOU            Birth date, filing status, state       │
├─────────────────────────────────────────────────────────────────┤
│  3. CURRENT INCOME       Job, salary, frequency, taxes          │
├─────────────────────────────────────────────────────────────────┤
│  4. CURRENT SAVINGS      Bank accounts, emergency fund          │
├─────────────────────────────────────────────────────────────────┤
│  5. INVESTMENTS          Brokerage, 401k, IRA, Roth             │
├─────────────────────────────────────────────────────────────────┤
│  6. REAL ESTATE          Home, rental properties, mortgages     │
├─────────────────────────────────────────────────────────────────┤
│  7. DEBTS                Student loans, car loans, credit cards │
├─────────────────────────────────────────────────────────────────┤
│  8. RETIREMENT GOALS     When? How much? Social Security?       │
├─────────────────────────────────────────────────────────────────┤
│  9. LIFE EVENTS          Major expenses, windfalls, changes     │
├─────────────────────────────────────────────────────────────────┤
│ 10. REVIEW & REFINE      Summary, assumptions, run simulation   │
└─────────────────────────────────────────────────────────────────┘
```

### Progress Visualization

```
Your Financial Picture
━━━━━━━━━━━━━━━━━━━━━━
Net Worth:        $245,000
Monthly Income:   $8,500
Monthly Expenses: $5,200
Retirement Age:   62
Years to Go:      27
━━━━━━━━━━━━━━━━━━━━━━
[=====>          ] 35% complete
```

---

## Detailed Step Specifications

### Step 1: Welcome

**Purpose**: Set expectations, collect simulation name, explain what we'll build together.

**Questions**:
```
"Welcome! I'm going to help you plan your financial future.

In the next few minutes, I'll ask you about:
• Your current financial situation
• Your income and expenses  
• Your retirement goals

At the end, we'll run thousands of simulations to show you 
different possible futures based on market conditions.

Let's give this plan a name:"

[_My Retirement Plan_______________________________]

"What are you hoping to learn?"
○ Can I retire early?
○ How much do I need to save?
○ Will my money last in retirement?
○ I'm just exploring

[Continue →]
```

**Data Collected**:
- `simulation.name` (required)
- `simulation.description` (from goal selection)

---

### Step 2: About You

**Purpose**: Establish personal context for age-based calculations and tax modeling.

**Questions**:
```
"First, tell me a little about yourself."

When were you born?
[Month ▼] [Day ▼] [Year ▼]
         January    15     1985

"You're currently 41 years old."

What's your tax filing status?
○ Single
○ Married Filing Jointly  
○ Married Filing Separately
○ Head of Household

What state do you live in?
[California ▼]

"Got it! California has a 9.3% top marginal state income tax rate."

[← Back]                              [Continue →]
```

**Data Collected**:
- `parameters.birth_date`
- `tax_config.filing_status` (NEW - needs backend support)
- `tax_config.state_rate` (auto-populated from state selection)

**Smart Behaviors**:
- Auto-calculate age and display it
- Show relevant state tax info
- Adjust federal brackets based on filing status

---

### Step 3: Current Income

**Purpose**: Understand income sources to model pre-retirement cash flow.

**Questions**:
```
"Now let's talk about your income."

Do you currently have earned income?
○ Yes, I'm employed
○ Yes, I'm self-employed  
○ No, I'm retired
○ No, I'm not currently working

[If Yes - Employed]

What's your gross annual salary?
$ [____125,000____]  per year
  "That's about $10,417 per month before taxes."

How often are you paid?
○ Weekly
○ Every two weeks (bi-weekly)
○ Twice a month (semi-monthly)
○ Monthly

Does your employer offer a 401(k)?
○ Yes, with employer match
○ Yes, without employer match
○ No
○ I'm not sure

[If yes with match]
What percentage do they match?
[_3_%] up to [_6_%] of your salary
"Your employer will contribute up to $7,500/year if you contribute $7,500."

Are you currently contributing to it?
○ Yes → How much? [_10_%] of salary ([$12,500]/year)
○ No

Do you have any other income sources?
[+ Add another income source]
  Examples: Side business, rental income, alimony, pension

[← Back]                              [Continue →]
```

**Data Collected**:
- Creates Income `CashFlow` or `Event` with:
  - Amount, frequency, inflation adjustment
  - Target account (auto-created or linked)
- 401k contribution as expense `CashFlow` with limit
- Employer match as income `CashFlow` 
- Sets up auto-created "Checking Account" for income destination

**Smart Behaviors**:
- Calculate and show monthly/annual equivalents
- Warn if 401k contribution exceeds IRS limits ($23,500 in 2026)
- Suggest Roth vs Traditional based on income level
- Auto-create events for 401k match

---

### Step 4: Current Savings

**Purpose**: Capture liquid savings (bank accounts, emergency funds).

**Questions**:
```
"Let's see what you've saved so far."

Do you have a checking account?
○ Yes → How much is in it? $ [____15,000____]
○ No

Do you have a savings account or emergency fund?
○ Yes → How much is in it? $ [____35,000____]
        What interest rate does it earn? [_4.5_%] APY
○ No

Do you have a High-Yield Savings Account (HYSA)?
○ Yes → How much? $ [____25,000____] 
        Interest rate? [_5.0_%] APY
○ No

"Great! You have $75,000 in liquid savings. 
Financial advisors typically recommend 3-6 months of expenses 
as an emergency fund."

How much of this is your emergency fund?
$ [____20,000____]
"This should cover about 4 months of typical expenses."

[← Back]                              [Continue →]
```

**Data Collected**:
- Creates Bank `Account` (AccountFlavor::Bank) with:
  - Cash asset(s) with appropriate return profiles
  - Separate tracking for emergency fund (excluded from retirement withdrawals)

**Smart Behaviors**:
- Show current high-yield savings rates for comparison
- Calculate months of expenses covered (once expenses are known)
- Suggest if emergency fund seems low

---

### Step 5: Investments

**Purpose**: Capture investment accounts (taxable brokerage, retirement accounts).

**Questions**:
```
"Now let's look at your investments."

Do you have any of these accounts?

□ Brokerage Account (taxable investment account)
  └─ Balance: $ [____150,000____]
     
     Would you like to specify the investments?
     ○ Keep it simple - assume a diversified portfolio
     ○ I'll enter my specific holdings
     
     [If specific]
     ┌────────────────────────────────────────┐
     │ US Stocks          $ [__80,000__]  53% │
     │ International      $ [__30,000__]  20% │
     │ Bonds              $ [__30,000__]  20% │
     │ Cash/Money Market  $ [__10,000__]   7% │
     │ [+ Add asset class]                    │
     └────────────────────────────────────────┘

□ 401(k) or 403(b)
  └─ Current balance: $ [____320,000____]
     Is this a Traditional or Roth 401(k)?
     ○ Traditional (pre-tax)
     ○ Roth (after-tax)
     ○ I have both → Traditional: $[___] Roth: $[___]

□ Traditional IRA
  └─ Balance: $ [____45,000____]

□ Roth IRA  
  └─ Balance: $ [____85,000____]
     
     Are you currently contributing?
     ○ Yes → $ [____6,500____] per year (max: $7,000)
     ○ No

□ HSA (Health Savings Account)
  └─ Balance: $ [____12,000____]

□ Other retirement accounts
  └─ Type: [____________]
     Balance: $ [____________]

"Looking good! You have $612,000 in investment accounts."

Investment Breakdown:
┌──────────────────────────────────────────┐
│ Tax-Deferred (401k, Trad IRA)   $365,000 │
│ Tax-Free (Roth IRA, Roth 401k)   $85,000 │
│ Taxable (Brokerage)             $150,000 │
│ HSA                              $12,000 │
└──────────────────────────────────────────┘

[← Back]                              [Continue →]
```

**Data Collected**:
- Creates `Account` entries with appropriate `AccountType`:
  - Brokerage → `Taxable`
  - 401k/Traditional IRA → `TaxDeferred`  
  - Roth IRA/Roth 401k → `TaxFree`
  - HSA → `TaxFree` (with special withdrawal rules)
- Creates `Asset` entries within each account
- Links to appropriate `ReturnProfile` based on asset allocation
- Creates contribution `CashFlow` events

**Smart Behaviors**:
- Pre-populate asset allocation suggestions based on age (more stocks if younger)
- Show IRS contribution limits and warn if exceeded
- Explain tax treatment of each account type
- Calculate total retirement savings

---

### Step 6: Real Estate

**Purpose**: Capture property assets and associated debts.

**Questions**:
```
"Do you own any real estate?"

□ Primary Residence (your home)
  └─ Estimated current value: $ [____650,000____]
     
     Do you have a mortgage?
     ○ Yes
       └─ Remaining balance: $ [____380,000____]
          Monthly payment: $ [____2,800____]
          Interest rate: [_3.25_%]
          Years remaining: [_22_] years
          
          Does payment include property tax & insurance?
          ○ Yes, it's escrowed
          ○ No, I pay those separately
            └─ Annual property tax: $ [____8,500____]
               Annual insurance: $ [____1,800____]
     ○ No, I own it outright

     Do you plan to sell this home?
     ○ No, I plan to stay
     ○ Yes, when I retire
     ○ Yes, at a specific age: [__70__]
     ○ Yes, when it reaches a value: $ [________]

□ Investment/Rental Property
  └─ [Similar questions + rental income]

□ Vacation Home
  └─ [Similar questions]

"Your real estate:"
┌─────────────────────────────────────────────┐
│ Home Value:           $650,000              │
│ Mortgage Balance:    -$380,000              │
│ Home Equity:          $270,000              │
│                                             │
│ Monthly Housing Cost: $2,800 (mortgage)     │
│                     + $858 (tax/insurance)  │
│                     ─────────────────────   │
│                       $3,658/month          │
└─────────────────────────────────────────────┘

[← Back]                              [Continue →]
```

**Data Collected**:
- Creates Property `Account` with RealEstate asset
- Creates Liability `Account` for mortgage with:
  - Negative balance
  - Amortization event for payments
- Creates expense `CashFlow` for mortgage payments
- Creates `Event` for planned home sale (if applicable)

**Smart Behaviors**:
- Calculate home equity
- Show total housing cost (PITI)
- Estimate future home value using housing inflation profile
- Model mortgage payoff date
- Handle rental income vs expense

---

### Step 7: Debts

**Purpose**: Capture non-mortgage debts.

**Questions**:
```
"Let's account for any other debts."

Do you have any of these?

□ Student Loans
  └─ Total balance: $ [____45,000____]
     Monthly payment: $ [____450____]
     Average interest rate: [_5.5_%]
     
     What repayment plan are you on?
     ○ Standard (10-year)
     ○ Extended (25-year)  
     ○ Income-driven (IBR, PAYE, REPAYE)
     ○ PSLF (Public Service Loan Forgiveness)

□ Car Loan
  └─ Balance: $ [____18,000____]
     Monthly payment: $ [____425____]
     Months remaining: [_36_]
     
     What's the car worth?
     $ [____22,000____]

□ Credit Card Debt
  └─ Total balance: $ [____5,000____]
     Are you paying this off?
     ○ Yes, $ [____500____] per month
     ○ Minimum payments only
     
□ Personal Loans
  └─ [Balance, payment, rate]

□ Medical Debt
  └─ [Balance, payment plan]

□ Other Debts
  └─ Description: [____________]

"Your debt summary:"
┌─────────────────────────────────────────────┐
│ Student Loans:    $45,000  @ 5.5%  $450/mo  │
│ Car Loan:         $18,000  @ 4.9%  $425/mo  │
│ Credit Cards:      $5,000  @ 22%   $500/mo  │
│ ────────────────────────────────────────    │
│ Total Debt:       $68,000                   │
│ Monthly Payments: $1,375                    │
│                                             │
│ Est. Debt-Free:   April 2031                │
└─────────────────────────────────────────────┘

[← Back]                              [Continue →]
```

**Data Collected**:
- Creates Liability `Account` for each debt type
- Creates expense `CashFlow` for payments
- Creates `Event` for loan payoff (terminate payment CashFlow)
- Car value creates Depreciating asset

**Smart Behaviors**:
- Calculate total monthly debt payments
- Estimate payoff dates
- Warn about high-interest debt (credit cards)
- Suggest debt payoff strategies (avalanche vs snowball)
- Model student loan forgiveness scenarios

---

### Step 8: Retirement Goals

**Purpose**: Define retirement timeline and income needs.

**Questions**:
```
"Let's dream about retirement! 🏖️"

At what age do you want to retire?
[_____62_____] years old

"That's 21 years from now, in 2047."
"You'll have saved approximately $1.8M by then (estimated)."

How much annual income do you think you need in retirement?
○ Use a rule of thumb
  └─ ○ 70% of current income ($87,500/year)
     ○ 80% of current income ($100,000/year)  
     ○ 85% of current income ($106,250/year)
     
○ I have a specific amount in mind
  └─ $ [____85,000____] per year (in today's dollars)
     
○ Help me figure it out
  └─ [Opens expense estimator mini-wizard]
     Current monthly expenses: ~$5,200
     Remove work expenses: -$400 (commute, clothes)
     Add healthcare: +$800
     Add travel/leisure: +$500
     ──────────────────────
     Estimated need: $6,100/month = $73,200/year

"Based on the 4% rule, you'd need about $1.83M saved to 
generate $73,200/year in retirement income."

Do you expect to receive Social Security?
○ Yes
  └─ Do you know your estimated benefit?
     ○ Yes → $ [____2,400____] per month at full retirement age
     ○ No → [We'll estimate based on your income]
     
     When do you plan to claim?
     ○ Early (62) - reduced benefit (~$1,680/mo)
     ○ Full retirement age (67) - full benefit ($2,400/mo)
     ○ Delayed (70) - increased benefit (~$2,976/mo)

○ No, I don't expect Social Security
○ I'm not sure

Do you expect any pension income?
○ Yes → $ [________] per month starting at age [___]
○ No

"Your retirement income plan:"
┌───────────────────────────────────────────────┐
│ Target Annual Income:        $73,200          │
│                                               │
│ Social Security (at 67):    +$28,800          │
│ Pension:                    +$0               │
│ ─────────────────────────────────────         │
│ Gap to fill from savings:    $44,400/year     │
│                                               │
│ Using 4% rule, you need:     $1,110,000       │
│ You're projected to have:    $1,800,000  ✓    │
└───────────────────────────────────────────────┘

[← Back]                              [Continue →]
```

**Data Collected**:
- `parameters.retirement_age`
- Creates `SpendingTarget` for retirement income:
  - Amount, inflation-adjusted
  - Withdrawal strategy (suggest TaxOptimized)
  - Start trigger: Age-based event
- Creates Social Security income `Event`:
  - Age-based trigger for claiming
  - Inflation-adjusted amount
- Creates Pension income `Event` if applicable

**Smart Behaviors**:
- Calculate years to retirement
- Project portfolio value at retirement
- Show Social Security claiming strategies with breakeven analysis
- Apply 4% rule for context
- Adjust spending for pre/post-Social Security periods

---

### Step 9: Life Events

**Purpose**: Capture major one-time events and life changes.

**Questions**:
```
"Life doesn't always go according to plan. 
Let's account for some major events."

Are you expecting any of these in the future?

□ Career Change
  └─ When? In [_5_] years
     New salary: ○ Higher ($[____]) ○ Lower ($[____]) ○ Similar
     Will there be a gap? ○ No ○ Yes, [_3_] months

□ Major Purchase
  └─ What? [New car_____]
     When? In [_3_] years  
     Cost: $ [____35,000____]
     Will you finance it?
     ○ No, pay cash
     ○ Yes → Down payment: $[____] Term: [__] months

□ Child's Education
  └─ When does college start? In [_8_] years
     Duration: [_4_] years
     Expected annual cost: $ [____40,000____]
     Current 529 balance: $ [____15,000____]
     Annual 529 contribution: $ [____6,000____]

□ Wedding
  └─ When? In [__] years
     Budget: $ [________]

□ Home Renovation  
  └─ When? In [__] years
     Cost: $ [________]

□ Inheritance Expected
  └─ Approximately when? In [__] years
     Estimated amount: $ [________]

□ Downsizing Home
  └─ [Links back to Real Estate section]

□ Starting a Business
  └─ When? In [__] years
     Initial investment: $ [________]
     Expected income after [__] years: $ [________]

□ Healthcare Event (planned surgery, etc.)
  └─ When? In [__] years
     Estimated cost: $ [________]

[+ Add custom event]

"Your planned life events:"
┌────────────────────────────────────────────────────┐
│ 2029: New car purchase                   -$35,000  │
│ 2034: Child starts college               -$40,000  │
│ 2035: College year 2                     -$41,200  │
│ 2036: College year 3                     -$42,440  │
│ 2037: College year 4                     -$43,710  │
│ 2047: Retirement begins                            │
│ 2052: Social Security starts             +$34,560  │
└────────────────────────────────────────────────────┘

[← Back]                              [Continue →]
```

**Data Collected**:
- Creates `Event` entries with appropriate triggers and effects:
  - Career change: Terminate old income, create new income
  - Major purchase: One-time expense event
  - Education: Recurring expense for N years
  - Inheritance: One-time income event
- Links events to appropriate accounts

**Smart Behaviors**:
- Suggest common life events based on age
- Inflate future costs automatically
- Show timeline visualization
- Warn if events create cash flow problems
- Allow linking events (e.g., "after mortgage payoff, increase 401k contribution")

---

### Step 10: Review & Refine

**Purpose**: Show complete picture, allow adjustments, explain assumptions, run simulation.

**Questions**:
```
"Here's your complete financial picture."

┌─────────────────────────────────────────────────────────────────┐
│  YOUR FINANCIAL SNAPSHOT                                        │
├─────────────────────────────────────────────────────────────────┤
│  Net Worth Today:              $757,000                         │
│  ├─ Liquid Savings:             $75,000                         │
│  ├─ Investment Accounts:       $612,000                         │
│  ├─ Real Estate Equity:        $270,000                         │
│  └─ Debts:                     -$200,000                        │
├─────────────────────────────────────────────────────────────────┤
│  Monthly Cash Flow:                                             │
│  ├─ Income (after tax):        +$7,800                          │
│  ├─ Housing:                   -$3,658                          │
│  ├─ Debt Payments:             -$1,375                          │
│  ├─ Savings (401k + Roth):     -$1,550                          │
│  └─ Remaining for expenses:     $1,217                          │
├─────────────────────────────────────────────────────────────────┤
│  Retirement Goal:                                               │
│  ├─ Target Age:                 62                              │
│  ├─ Years to Retirement:        21                              │
│  ├─ Target Income:              $73,200/year                    │
│  └─ Social Security (at 67):    $28,800/year                    │
└─────────────────────────────────────────────────────────────────┘

"We've made some assumptions. You can adjust these:"

[Assumptions] (expandable section)
┌─────────────────────────────────────────────────────────────────┐
│ MARKET ASSUMPTIONS                                              │
│ ─────────────────                                               │
│ Stock Returns:    9.6% ± 16.5%  [Adjust]  ℹ️                    │
│ Bond Returns:     4.5% ± 5.5%   [Adjust]  ℹ️                    │
│ Cash Returns:     3.0% (fixed)  [Adjust]  ℹ️                    │
│ Inflation:        3.5% ± 2.8%   [Adjust]  ℹ️                    │
│ Housing Inflation: 4.0% ± 3.5%  [Adjust]  ℹ️                    │
│                                                                 │
│ TAX ASSUMPTIONS                                                 │
│ ───────────────                                                 │
│ Filing Status:    Married Filing Jointly  [Edit]                │
│ State:            California (9.3%)       [Edit]                │
│ Federal Brackets: 2024 rates              [View/Edit]           │
│ Capital Gains:    15%                     [Adjust]              │
│                                                                 │
│ SIMULATION SETTINGS                                             │
│ ───────────────────                                             │
│ Duration:         50 years (until age 91) [Adjust]              │
│ Iterations:       1,000 Monte Carlo runs  [Adjust]              │
└─────────────────────────────────────────────────────────────────┘

[Advanced Options] (expandable)
┌─────────────────────────────────────────────────────────────────┐
│ □ Include Required Minimum Distributions (RMDs)                 │
│ □ Optimize withdrawal order for tax efficiency                  │
│ □ Model Roth conversion strategies                              │
│ □ Account for sequence of returns risk                          │
└─────────────────────────────────────────────────────────────────┘

[Ready to see your future?]

              ╔══════════════════════════════╗
              ║   🚀 Run Simulation 🚀        ║
              ╚══════════════════════════════╝

         [Save as Draft]    [Export to PDF]

```

**Data Collected**:
- Final review of all `SimulationParameters`
- Adjustment overrides
- Advanced simulation options

**Smart Behaviors**:
- Highlight potential issues (negative cash flow, insufficient retirement savings)
- Explain each assumption in plain language
- Allow quick edits without going back through entire wizard
- Show comparison to "typical" scenarios
- Provide "Quick Fixes" suggestions

---

## Data Mapping

### User Input → SimulationConfig Mapping

| User Input | SimulationConfig Field |
|------------|----------------------|
| Birth date | `birth_date` |
| State | `tax_config.state_rate` |
| Filing status | `tax_config.federal_brackets` (select correct table) |
| Simulation duration | `duration_years` |
| Checking/Savings accounts | `accounts[]` with Bank flavor |
| Investment accounts | `accounts[]` with Investment flavor |
| Real estate | `accounts[]` with Property flavor |
| Debts | `accounts[]` with Liability flavor |
| Salary/Income | `events[]` with Income effect |
| Regular expenses | `events[]` with Expense effect |
| 401k contributions | `events[]` with AssetPurchase effect |
| Retirement spending | `events[]` with Repeating trigger + AssetSale effect |
| Social Security | `events[]` with Age trigger + Income effect |
| Life events | `events[]` with various triggers and effects |
| Market assumptions | `return_profiles`, `inflation_profile` |

### Auto-Generated IDs Strategy

The wizard should internally manage ID generation:
- Accounts: Sequential from 1
- Assets: Sequential within each account
- Events: Sequential from 1
- Use descriptive names stored in frontend-only fields for display

---

## Backend API Changes

### New Endpoints Needed

#### 1. Tax Data Endpoint
```
GET /api/reference/tax-brackets?year=2026&status=married_filing_jointly

Response:
{
  "federal_brackets": [...],
  "standard_deduction": 29200,
  "state_brackets": { "CA": [...] }
}
```

#### 2. Social Security Estimator
```
POST /api/reference/social-security/estimate

Request:
{
  "birth_year": 1985,
  "average_indexed_earnings": 85000,
  "claiming_age": 67
}

Response:
{
  "pia": 2400,
  "early_claiming": { "age": 62, "amount": 1680 },
  "full_retirement": { "age": 67, "amount": 2400 },
  "delayed_claiming": { "age": 70, "amount": 2976 },
  "breakeven_delayed": 82
}
```

#### 3. Wizard Draft Endpoint
```
POST /api/simulations/drafts

Request:
{
  "step": 5,
  "data": { ... partial wizard data ... }
}

Response:
{
  "draft_id": "uuid",
  "expires_at": "2026-01-15T00:00:00Z"
}
```

### Model Changes (Rust)

```rust
// Add to SimulationConfig or create WizardConfig
pub struct WizardMetadata {
    pub filing_status: FilingStatus,
    pub state: UsState,
    pub retirement_age: u8,
    pub social_security_claiming_age: Option<u8>,
    pub social_security_pia: Option<f64>,
}

pub enum FilingStatus {
    Single,
    MarriedFilingJointly,
    MarriedFilingSeparately,
    HeadOfHousehold,
}
```

---

## UI/UX Guidelines

### Design System Components Needed

1. **Money Input** - Already exists, enhance with:
   - Keyboard shortcuts (k for thousands, m for millions)
   - Percentage mode toggle
   - Currency symbol prefix

2. **Age/Date Picker** - Specialized component:
   - Age slider with year display
   - "In X years" quick select
   - Date validation

3. **Progress Sidebar** - Persistent component:
   - Shows current step
   - Displays running totals
   - Clickable to jump to sections

4. **Help Tooltips** - Contextual help:
   - "Why does this matter?" explanations
   - Links to educational content
   - Examples

5. **Summary Cards** - Visual summaries:
   - Net worth breakdown
   - Cash flow overview
   - Timeline visualization

### Animation & Micro-interactions

- Smooth transitions between steps
- Numbers animate when calculated
- Checkmarks when sections complete
- Subtle celebrations at milestones

### Mobile Considerations

- Single-column layout
- Bottom navigation
- Large touch targets
- Swipe between steps
- Voice input for numbers (future)

---

## Implementation Phases

### Phase 1: Foundation (Week 1-2)
**Goal**: New wizard structure with Steps 1-2

- [ ] Create new `SimulationWizardV2` component
- [ ] Implement wizard state management (zustand or context)
- [ ] Build Welcome step
- [ ] Build About You step with date picker
- [ ] Add filing status to backend
- [ ] Create state tax rate lookup
- [ ] Add progress sidebar component

### Phase 2: Income & Savings (Week 3-4)
**Goal**: Steps 3-4 with basic account creation

- [ ] Build Current Income step
- [ ] Build Current Savings step
- [ ] Implement smart account auto-creation
- [ ] Add income frequency calculations
- [ ] Create Money Input v2 component
- [ ] Add 401k limit validation

### Phase 3: Investments & Property (Week 5-6)
**Goal**: Steps 5-6 with complex account types

- [ ] Build Investments step with account type selection
- [ ] Build Real Estate step with mortgage modeling
- [ ] Implement asset allocation selector
- [ ] Add return profile auto-assignment
- [ ] Create debt payoff timeline visualization
- [ ] Build Debts step (Step 7)

### Phase 4: Retirement & Events (Week 7-8)
**Goal**: Steps 8-9 with event system

- [ ] Build Retirement Goals step
- [ ] Implement Social Security estimator (frontend + backend)
- [ ] Build Life Events step
- [ ] Create event builder UI
- [ ] Add timeline visualization
- [ ] Implement spending target creation

### Phase 5: Review & Polish (Week 9-10)
**Goal**: Step 10 and overall polish

- [ ] Build Review & Refine step
- [ ] Implement draft saving
- [ ] Add assumption explanations
- [ ] Create PDF export
- [ ] Add error handling and validation
- [ ] Performance optimization
- [ ] Accessibility audit

### Phase 6: Migration & Cleanup (Week 11)
**Goal**: Transition to new wizard

- [ ] Add feature flag for new wizard
- [ ] Migrate existing simulations
- [ ] Update documentation
- [ ] Remove old wizard code
- [ ] User testing and feedback

---

## Technical Architecture

### State Management

```typescript
// Wizard state using Zustand
interface WizardState {
  // Navigation
  currentStep: number;
  completedSteps: Set<number>;
  
  // User data
  personalInfo: {
    birthDate: Date;
    filingStatus: FilingStatus;
    state: UsState;
  };
  
  income: {
    employed: boolean;
    salary: number;
    payFrequency: PayFrequency;
    employer401k: Employer401kInfo | null;
    otherIncome: IncomeSource[];
  };
  
  savings: {
    checking: number;
    savings: number;
    hysa: number;
    emergencyFund: number;
  };
  
  investments: InvestmentAccount[];
  realEstate: RealEstateProperty[];
  debts: Debt[];
  
  retirement: {
    targetAge: number;
    targetIncome: number;
    socialSecurity: SocialSecurityPlan | null;
    pension: PensionPlan | null;
  };
  
  lifeEvents: LifeEvent[];
  
  // Computed
  calculatedParameters: SimulationParameters;
  
  // Actions
  setPersonalInfo: (info: Partial<PersonalInfo>) => void;
  setIncome: (income: Partial<IncomeInfo>) => void;
  // ... etc
  
  // Derived
  netWorth: number;
  monthlyIncome: number;
  monthlyExpenses: number;
}
```

### File Structure

```
web/components/simulation-wizard-v2/
├── index.tsx                    # Main wizard component
├── WizardContext.tsx            # State provider
├── WizardProgress.tsx           # Progress sidebar
├── WizardNavigation.tsx         # Step navigation
├── hooks/
│   ├── useWizardState.ts        # Zustand store
│   ├── useCalculations.ts       # Derived calculations
│   └── useDraftSave.ts          # Auto-save draft
├── steps/
│   ├── WelcomeStep.tsx
│   ├── AboutYouStep.tsx
│   ├── CurrentIncomeStep.tsx
│   ├── CurrentSavingsStep.tsx
│   ├── InvestmentsStep.tsx
│   ├── RealEstateStep.tsx
│   ├── DebtsStep.tsx
│   ├── RetirementGoalsStep.tsx
│   ├── LifeEventsStep.tsx
│   └── ReviewStep.tsx
├── components/
│   ├── MoneyInputV2.tsx
│   ├── AgePicker.tsx
│   ├── AccountTypeSelector.tsx
│   ├── AssetAllocationPicker.tsx
│   ├── TimelineVisualization.tsx
│   ├── NetWorthCard.tsx
│   └── CashFlowCard.tsx
├── utils/
│   ├── parameterBuilder.ts      # Convert wizard state → SimulationParams
│   ├── taxCalculations.ts
│   ├── socialSecurityCalcs.ts
│   └── validation.ts
└── types.ts
```

### Parameter Builder

```typescript
// Convert friendly wizard state to SimulationParameters
function buildSimulationParameters(state: WizardState): SimulationParameters {
  const accounts: Account[] = [];
  const events: Event[] = [];
  let nextAccountId = 1;
  let nextEventId = 1;
  
  // Create bank accounts from savings
  if (state.savings.checking > 0) {
    accounts.push({
      account_id: nextAccountId++,
      account_type: "Taxable",
      name: "Checking Account",
      assets: [{
        asset_id: 1,
        asset_class: "Cash", // Need to add this
        initial_value: state.savings.checking,
        return_profile_index: 0, // Cash return profile
        name: "Checking"
      }]
    });
  }
  
  // Create income events
  if (state.income.employed && state.income.salary > 0) {
    events.push({
      event_id: nextEventId++,
      trigger: { Repeating: { interval: state.income.payFrequency } },
      effects: [{
        Income: {
          to: 1, // Checking account
          amount: calculatePaycheckAmount(state),
          income_type: "Salary"
        }
      }],
      once: false
    });
  }
  
  // ... continue building
  
  return {
    birth_date: formatDate(state.personalInfo.birthDate),
    duration_years: calculateDuration(state),
    accounts,
    events,
    // ...
  };
}
```

---

## Success Metrics

1. **Completion Rate**: Target 70%+ wizard completion (up from current ~40%)
2. **Time to Complete**: Target <10 minutes for basic simulation
3. **User Satisfaction**: NPS score of 50+
4. **Error Rate**: <5% of submissions fail validation
5. **Return Usage**: 60%+ users run additional simulations

---

## Open Questions

1. **Portfolio Integration**: Should we keep portfolio as separate concept, or integrate account creation into wizard?
2. **Event System**: Wait for event system refactor (20260102_EVENT_SYSTEM_PLAN.md) or build on current system?
3. **Social Security API**: Build estimator in Rust or use external API?
4. **Spouse Support**: How to handle joint planning (separate profile or integrated)?
5. **Import Data**: Should we support importing from financial aggregators (Plaid)?

---

## Appendix: Question Bank

### Income Questions
- Do you have a job? Y/N
- What's your gross annual salary?
- How often are you paid?
- Does your employer offer a 401k?
- Do they match contributions? How much?
- What percentage are you contributing?
- Do you have any side income?
- Do you receive any investment income?

### Savings Questions
- Do you have a checking account?
- Do you have a savings account?
- Do you have a high-yield savings account?
- How much of your savings is your emergency fund?
- What interest rate do your accounts earn?

### Investment Questions
- Do you have a brokerage account?
- Do you have a 401k? Traditional or Roth?
- Do you have a Traditional IRA?
- Do you have a Roth IRA?
- Do you have an HSA?
- Would you like to specify your investment mix?

### Real Estate Questions
- Do you own your home?
- What is it worth?
- Do you have a mortgage?
- What's your monthly payment?
- Do you own any rental properties?
- Do you plan to sell your home?

### Debt Questions
- Do you have student loans?
- Do you have a car loan?
- Do you have credit card debt?
- Do you have any other loans?

### Retirement Questions
- At what age do you want to retire?
- How much income do you think you'll need?
- Do you expect Social Security?
- Do you know your estimated benefit?
- When do you plan to claim?
- Do you have a pension?

### Life Event Questions
- Are you planning any major purchases?
- Do you expect to pay for college?
- Are you planning to have children?
- Do you expect any inheritance?
- Are you planning to start a business?

---

*Document created: January 8, 2026*
*Author: Financial Planning Team*
*Status: Draft for Review*
