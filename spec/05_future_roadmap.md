# Future Roadmap

This document outlines planned improvements to the FinPlan simulation engine and application.

## 1. Goal-Seeking Optimization

### Overview

Add algorithms that find optimal values for simulation parameters to maximize specific objectives.

### Objectives

| Objective | Description |
|-----------|-------------|
| Max Wealth at Retirement | Optimize contributions/allocations to maximize net worth at retirement date |
| Max Wealth at Death | Optimize for maximum estate value at end of simulation |
| Max Sustainable Withdrawal | Find highest safe withdrawal rate for a given success probability |
| Min Tax Lifetime | Minimize total lifetime tax burden |

### Proposed API

```rust
pub struct OptimizationConfig {
    pub objective: OptimizationObjective,
    pub parameters: Vec<OptimizableParameter>,
    pub constraints: Vec<OptimizationConstraint>,
    pub monte_carlo_iterations: usize,
    pub success_threshold: f64,  // e.g., 95% success rate
}

pub enum OptimizationObjective {
    MaximizeWealthAt { date: Date },
    MaximizeWealthAtRetirement,
    MaximizeWealthAtDeath,
    MaximizeSustainableWithdrawal { success_rate: f64 },
    MinimizeLifetimeTax,
}

pub enum OptimizableParameter {
    RetirementAge { min: u8, max: u8 },
    ContributionRate { event_id: EventId, min: f64, max: f64 },
    WithdrawalAmount { event_id: EventId, min: f64, max: f64 },
    AssetAllocation { account_id: AccountId },
}

pub struct OptimizationResult {
    pub optimal_parameters: HashMap<String, f64>,
    pub objective_value: f64,
    pub iterations_run: usize,
    pub convergence_history: Vec<f64>,
}
```

### Implementation Approach

1. **Binary Search**: For single-parameter optimization (e.g., max sustainable withdrawal)
2. **Grid Search**: For 2-3 parameter combinations
3. **Gradient-Free Optimization**: For multi-parameter (Nelder-Mead or similar)

### TUI Integration

- New "Optimize" tab or mode
- Parameter selection UI
- Progress indicator during optimization
- Results visualization with comparison to baseline

---

## 2. What-If Analysis / Sensitivity Analysis

### Overview

Enable quick parameter variations and automatic sensitivity analysis to understand how changes affect outcomes.

### Quick Tweaks

Allow users to create scenario variants without full duplication:

```rust
pub struct ScenarioOverride {
    pub base_scenario: String,
    pub overrides: Vec<ParameterOverride>,
}

pub enum ParameterOverride {
    RetirementAge(u8),
    StartDate(Date),
    Duration(u32),
    EventAmount { event_id: EventId, amount: f64 },
    ReturnProfile { profile_id: ReturnProfileId, mean: f64 },
    InflationRate(f64),
}
```

### Sensitivity Analysis

Automatically vary parameters to show impact ranges:

```rust
pub struct SensitivityConfig {
    pub parameter: SensitivityParameter,
    pub range: SensitivityRange,
    pub steps: usize,
}

pub enum SensitivityParameter {
    RetirementAge,
    MarketReturn,
    Inflation,
    WithdrawalRate,
    ContributionRate { event_id: EventId },
}

pub enum SensitivityRange {
    Absolute { min: f64, max: f64 },
    RelativePercent { minus: f64, plus: f64 },  // e.g., -20% to +20%
}

pub struct SensitivityResult {
    pub parameter_values: Vec<f64>,
    pub success_rates: Vec<f64>,
    pub median_outcomes: Vec<f64>,
    pub percentile_bands: Vec<PercentileBand>,
}
```

### TUI Integration

- "What-If" panel on Results screen
- Quick sliders for common parameters
- Tornado diagram for sensitivity ranking
- Side-by-side comparison view

---

## 3. Estate Planning

### Overview

Model wealth transfer, beneficiaries, and estate taxes.

### New Data Types

```rust
pub struct Beneficiary {
    pub beneficiary_id: BeneficiaryId,
    pub name: String,
    pub relationship: Relationship,
    pub birth_date: Option<Date>,
}

pub enum Relationship {
    Spouse,
    Child,
    Grandchild,
    Other(String),
}

pub struct EstatePlan {
    pub beneficiaries: Vec<Beneficiary>,
    pub account_beneficiaries: HashMap<AccountId, BeneficiaryDesignation>,
    pub estate_tax_config: EstateTaxConfig,
}

pub struct BeneficiaryDesignation {
    pub primary: Vec<(BeneficiaryId, f64)>,     // (beneficiary, percentage)
    pub contingent: Vec<(BeneficiaryId, f64)>,
}

pub struct EstateTaxConfig {
    pub federal_exemption: f64,      // e.g., $12.92M (2023)
    pub federal_rate: f64,           // e.g., 40%
    pub state_exemption: Option<f64>,
    pub state_rate: Option<f64>,
}
```

### New Events/Effects

```rust
pub enum EventEffect {
    // ... existing effects ...

    // Estate planning
    Inheritance {
        from_account: AccountId,
        to_beneficiary: BeneficiaryId,
        amount: TransferAmount,
    },

    SpouseRollover {
        from_account: AccountId,
        to_account: AccountId,
    },

    StretchIra {
        inherited_account: AccountId,
        beneficiary: BeneficiaryId,
    },
}

pub enum EventTrigger {
    // ... existing triggers ...

    Death,  // Trigger estate distribution
    SpouseDeath,
}
```

### Inherited IRA Rules

Model post-SECURE Act rules:
- **Spouse**: Can roll over or treat as own
- **Eligible Designated Beneficiary**: Stretch over life expectancy
- **Non-Eligible Beneficiary**: 10-year rule

### TUI Integration

- Beneficiary management screen
- Estate summary panel
- After-death wealth projection

---

## 4. Additional Future Considerations

### Social Security Modeling

```rust
pub struct SocialSecurityConfig {
    pub primary_pia: f64,           // Primary Insurance Amount
    pub spouse_pia: Option<f64>,
    pub claiming_strategy: ClaimingStrategy,
}

pub enum ClaimingStrategy {
    AgeSpecified(u8),
    Optimized,  // Find optimal claiming age
    FileAndSuspend,
    RestrictedApplication,
}
```

### Healthcare Costs

```rust
pub struct HealthcareConfig {
    pub pre_medicare_premium: f64,
    pub medicare_part_b_premium: f64,
    pub medigap_premium: f64,
    pub healthcare_inflation: f64,  // Often higher than general inflation
}
```

### State Taxes

```rust
pub struct StateTaxConfig {
    pub state: String,
    pub income_brackets: Vec<TaxBracket>,
    pub retirement_income_exclusion: Option<f64>,
    pub social_security_taxable: bool,
}
```

---

## Implementation Priority

### Phase 1 (Foundation)
1. What-If quick tweaks (scenario overrides)
2. Sensitivity analysis infrastructure
3. Single-parameter optimization (binary search)

### Phase 2 (Optimization)
1. Multi-parameter optimization
2. Sustainable withdrawal calculator
3. Results comparison view

### Phase 3 (Estate)
1. Beneficiary data model
2. Basic inheritance events
3. Estate tax calculations
4. Inherited IRA rules

### Phase 4 (Advanced)
1. Social Security integration
2. Healthcare cost modeling
3. State tax support
4. Advanced optimization algorithms
