# FinPlan Improvement Plan

A comprehensive roadmap for enhancing the Monte Carlo financial planning simulation, backend server, and frontend experience.

---

## Table of Contents

1. [Executive Summary](#executive-summary)
2. [Backend Improvements](#backend-improvements)
   - [Core Simulation Engine](#core-simulation-engine)
   - [Tax System Enhancements](#tax-system-enhancements)
   - [Server & API](#server--api)
3. [Frontend Improvements](#frontend-improvements)
   - [Architecture & Code Quality](#architecture--code-quality)
   - [User Experience](#user-experience)
   - [Missing Features](#missing-features)
4. [New Feature Roadmap](#new-feature-roadmap)
5. [Technical Debt](#technical-debt)
6. [Implementation Phases](#implementation-phases)

---

## Executive Summary

FinPlan is a well-architected Monte Carlo simulation engine for retirement planning. The Rust backend provides a sophisticated event-driven system with strong type safety, while the Next.js frontend offers a modern UI for portfolio and simulation management. This plan identifies **72 improvement opportunities** across 6 categories, organized into 4 implementation phases.

### Current Strengths
- ✅ Event-driven architecture with 13+ trigger types
- ✅ Comprehensive tax modeling (federal brackets, capital gains)
- ✅ Parallel Monte Carlo execution with rayon
- ✅ Clean API layer with SQLite persistence
- ✅ Modern React 19 / Next.js 16 frontend with shadcn/ui

### Key Gaps
- ❌ No RMD (Required Minimum Distribution) support
- ❌ Events UI not implemented (shows "coming soon")
- ❌ No Social Security modeling
- ❌ Single filing status only (no married/joint)
- ❌ No scenario comparison or "what-if" analysis
- ❌ Limited data export capabilities

---

## Backend Improvements

### Core Simulation Engine

#### B1. Required Minimum Distributions (RMD) 🔴 High Priority
**See:** [RMD_IMPLEMENTATION_PLAN.md](RMD_IMPLEMENTATION_PLAN.md)

**Status:** Detailed plan exists, implementation pending

**Summary:**
- Add IRS Uniform Lifetime Table data structure
- Create `EventEffect::CreateRmdWithdrawal` variant
- Track year-end balances for prior-year calculations
- Handle multiple tax-deferred accounts
- Support birth-year-based starting ages (73 or 75)

**Estimated Effort:** 3-5 days

---

#### B2. Social Security Modeling 🔴 High Priority

**Problem:** Retirement simulations lack Social Security income, a critical component for most retirees.

**Implementation:**

```rust
// New models in models.rs
pub struct SocialSecurityConfig {
    /// Primary Insurance Amount (PIA) at full retirement age
    pub pia_amount: f64,
    /// Full retirement age (FRA) - typically 66-67
    pub full_retirement_age: (u8, u8), // (years, months)
    /// Claiming age
    pub claiming_age: (u8, u8),
    /// Optional spouse PIA for spousal benefits
    pub spouse_pia: Option<f64>,
    /// Cost-of-living adjustment assumption
    pub cola_profile: Option<InflationProfile>,
}

pub enum SocialSecurityStrategy {
    /// Claim at specific age
    ClaimAtAge { years: u8, months: u8 },
    /// Claim when a condition is met (e.g., portfolio depletes below threshold)
    ClaimOnCondition { trigger: Box<EventTrigger> },
    /// Optimize based on breakeven analysis
    Optimized,
}
```

**Key Features:**
- Early claiming reduction (62-FRA): ~6.67% per year reduction
- Delayed claiming credits (FRA-70): 8% per year increase
- Spousal benefits (up to 50% of higher earner's PIA)
- Survivor benefits
- COLA adjustments (default 2.5% annual)
- Taxation of benefits (0%, 50%, or 85% based on combined income)

**Benefit Calculation:**
```rust
pub fn calculate_ss_benefit(config: &SocialSecurityConfig) -> f64 {
    let fra_months = config.full_retirement_age.0 as i32 * 12 + config.full_retirement_age.1 as i32;
    let claim_months = config.claiming_age.0 as i32 * 12 + config.claiming_age.1 as i32;
    let months_diff = claim_months - fra_months;
    
    let adjustment = if months_diff < 0 {
        // Early: reduce by 5/9 of 1% for first 36 months, then 5/12 of 1%
        let early_months = -months_diff;
        let first_36 = early_months.min(36) as f64 * (5.0 / 9.0 / 100.0);
        let beyond_36 = (early_months - 36).max(0) as f64 * (5.0 / 12.0 / 100.0);
        -(first_36 + beyond_36)
    } else {
        // Delayed: 8% per year = 2/3 of 1% per month
        months_diff as f64 * (2.0 / 3.0 / 100.0)
    };
    
    config.pia_amount * (1.0 + adjustment)
}
```

**Estimated Effort:** 5-7 days

---

#### B3. Asset Rebalancing 🟡 Medium Priority

**Problem:** No automatic portfolio rebalancing; users must create manual transfer events.

**Implementation:**

```rust
pub struct RebalanceConfig {
    pub target_allocation: HashMap<AssetId, f64>, // Asset -> target percentage
    pub trigger: RebalanceTrigger,
    pub method: RebalanceMethod,
}

pub enum RebalanceTrigger {
    /// Rebalance on schedule
    Scheduled { interval: RepeatInterval },
    /// Rebalance when any asset drifts beyond threshold
    DriftThreshold { percentage: f64 }, // e.g., 5% drift
    /// Both scheduled and drift-based
    Combined { interval: RepeatInterval, threshold: f64 },
}

pub enum RebalanceMethod {
    /// Sell high, buy low across all assets
    SellAndBuy,
    /// Only use new contributions to rebalance (no sales)
    ContributionsOnly,
    /// Only sell from over-allocated assets
    SellOnly,
}
```

**Tax Considerations:**
- Prioritize rebalancing in tax-advantaged accounts (no capital gains)
- Track tax lots for taxable account sales
- Consider tax-loss harvesting opportunities

**Estimated Effort:** 4-6 days

---

#### B4. Improved Cost Basis Tracking 🟡 Medium Priority

**Problem:** Current implementation uses `taxable_gains_percentage` estimate rather than true lot tracking.

**Implementation:**

```rust
pub struct TaxLot {
    pub lot_id: TaxLotId,
    pub asset_id: AssetId,
    pub purchase_date: jiff::civil::Date,
    pub purchase_price: f64, // Cost basis per unit
    pub quantity: f64,
    pub current_value: f64,
}

pub enum CostBasisMethod {
    /// First In, First Out (default)
    FIFO,
    /// Last In, First Out
    LIFO,
    /// Highest cost basis first (minimize gains)
    HighestCost,
    /// Specific lot identification
    SpecificId { lot_ids: Vec<TaxLotId> },
    /// Average cost (for mutual funds)
    AverageCost,
}
```

**Features:**
- Long-term vs. short-term capital gains distinction (1-year holding period)
- Tax-loss harvesting identification
- Wash sale rule tracking (30-day window)

**Estimated Effort:** 5-8 days

---

#### B5. Healthcare Cost Modeling 🟡 Medium Priority

**Problem:** Healthcare is a major retirement expense not currently modeled.

**Implementation:**

```rust
pub struct HealthcareConfig {
    pub pre_medicare_strategy: PreMedicareStrategy,
    pub medicare_config: MedicareConfig,
    pub ltc_insurance: Option<LongTermCareConfig>,
}

pub enum PreMedicareStrategy {
    /// Employer coverage until Medicare
    EmployerCoverage { monthly_premium: f64 },
    /// ACA marketplace
    AcaMarketplace { 
        base_premium: f64,
        subsidy_eligible: bool,
    },
    /// COBRA (up to 18 months)
    Cobra { monthly_premium: f64 },
    /// No coverage (high risk)
    Uninsured,
}

pub struct MedicareConfig {
    /// Part B premium (income-adjusted: IRMAA)
    pub part_b_irmaa_tier: IrmaaTier,
    /// Part D premium
    pub part_d_premium: f64,
    /// Medigap or Medicare Advantage
    pub supplement: MedicareSupplement,
}
```

**IRMAA (Income-Related Monthly Adjustment Amount):**
- Track MAGI from 2 years prior
- Apply premium surcharges based on income brackets
- Affects both Part B and Part D

**Estimated Effort:** 4-5 days

---

#### B6. Inflation Model Improvements 🟢 Low Priority

**Problem:** Single inflation rate applied globally; real-world inflation varies by category.

**Implementation:**

```rust
pub struct DetailedInflation {
    pub general: InflationProfile,
    pub healthcare: InflationProfile, // Typically 5-7% vs 2-3% general
    pub education: InflationProfile,
    pub housing: InflationProfile,
    pub social_security_cola: InflationProfile,
}
```

**Estimated Effort:** 2-3 days

---

#### B7. Monte Carlo Improvements 🟢 Low Priority

**Current:** Simple parallel iteration with fixed seed increment

**Improvements:**
- **Correlation modeling**: Stocks and bonds aren't independent; add correlation matrix
- **Regime switching**: Bear/bull market modeling with different volatility
- **Sequence-of-returns risk**: Track success rate by starting year
- **Convergence detection**: Stop early if additional iterations don't change percentiles

```rust
pub struct MonteCarloConfig {
    pub iterations: usize,
    pub correlation_matrix: Option<CorrelationMatrix>,
    pub convergence_threshold: Option<f64>, // e.g., 0.001 = 0.1%
    pub regime_model: Option<MarketRegimeModel>,
}

pub struct CorrelationMatrix {
    pub assets: Vec<AssetId>,
    pub correlations: Vec<Vec<f64>>, // Symmetric matrix
}
```

**Estimated Effort:** 5-7 days

---

### Tax System Enhancements

#### B8. Filing Status Support 🔴 High Priority

**Problem:** Only single filer brackets supported.

**Implementation:**

```rust
pub enum FilingStatus {
    Single,
    MarriedFilingJointly,
    MarriedFilingSeparately,
    HeadOfHousehold,
}

pub struct TaxConfig {
    pub filing_status: FilingStatus,
    pub federal_brackets: Vec<TaxBracket>,
    pub standard_deduction: f64,
    pub capital_gains_brackets: Vec<TaxBracket>,
    pub state_rate: Option<f64>,
    pub state_brackets: Option<Vec<TaxBracket>>,
}
```

**Default Brackets (2024 MFJ):**
| Rate | Single | Married Filing Jointly |
|------|--------|----------------------|
| 10% | $0 - $11,600 | $0 - $23,200 |
| 12% | $11,600 - $47,150 | $23,200 - $94,300 |
| 22% | $47,150 - $100,525 | $94,300 - $201,050 |
| 24% | $100,525 - $191,950 | $201,050 - $383,900 |
| 32% | $191,950 - $243,725 | $383,900 - $487,450 |
| 35% | $243,725 - $609,350 | $487,450 - $731,200 |
| 37% | $609,350+ | $731,200+ |

**Estimated Effort:** 2-3 days

---

#### B9. State-Specific Tax Modeling 🟡 Medium Priority

**Problem:** Only flat state rate; no state-specific rules.

**Implementation:**

```rust
pub enum StateTaxConfig {
    None, // States with no income tax (FL, TX, WA, etc.)
    Flat { rate: f64 },
    Progressive { brackets: Vec<TaxBracket> },
    // Special cases
    California { brackets: Vec<TaxBracket>, mental_health_surtax: bool },
}
```

**Priority States:**
1. California (progressive, high rates)
2. New York (progressive + NYC tax)
3. Texas/Florida (no state income tax)
4. Other progressive states

**Estimated Effort:** 3-4 days per state

---

#### B10. Roth Conversion Optimization 🟡 Medium Priority

**Problem:** Users must manually model Roth conversions via events.

**Implementation:**

```rust
pub struct RothConversionStrategy {
    pub method: ConversionMethod,
    pub constraints: ConversionConstraints,
}

pub enum ConversionMethod {
    /// Convert fixed amount annually
    FixedAmount { amount: f64 },
    /// Fill up to top of a tax bracket
    FillBracket { max_bracket: f64 }, // e.g., 24%
    /// Optimize based on projected lifetime taxes
    Optimized,
}

pub struct ConversionConstraints {
    /// Don't convert if it pushes into higher bracket
    pub max_marginal_rate: Option<f64>,
    /// Don't convert if it affects Medicare IRMAA
    pub avoid_irmaa: bool,
    /// Stop conversions after this age
    pub max_age: Option<u8>,
}
```

**Estimated Effort:** 4-5 days

---

#### B11. Net Investment Income Tax (NIIT) 🟢 Low Priority

**Problem:** 3.8% NIIT on investment income above threshold not modeled.

**Thresholds (2024):**
- Single: $200,000 MAGI
- Married Filing Jointly: $250,000 MAGI

**Estimated Effort:** 1-2 days

---

### Server & API

#### B12. API Validation & Error Handling 🔴 High Priority

**Problem:** Heavy use of `.unwrap()` and `.expect()`; bad inputs silently fail.

**Implementation:**

```rust
// Create custom error types
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("Portfolio not found: {0}")]
    PortfolioNotFound(i64),
    #[error("Simulation not found: {0}")]
    SimulationNotFound(i64),
    #[error("Invalid parameter: {field} - {message}")]
    ValidationError { field: String, message: String },
    #[error("Database error: {0}")]
    DatabaseError(#[from] rusqlite::Error),
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, body) = match &self {
            ApiError::PortfolioNotFound(_) | ApiError::SimulationNotFound(_) => 
                (StatusCode::NOT_FOUND, json!({ "error": self.to_string() })),
            ApiError::ValidationError { .. } => 
                (StatusCode::BAD_REQUEST, json!({ "error": self.to_string() })),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, json!({ "error": "Internal error" })),
        };
        (status, Json(body)).into_response()
    }
}
```

**Input Validation:**
```rust
// Add validation layer
pub fn validate_simulation_params(params: &SimulationParameters) -> Result<(), ApiError> {
    if params.start_date >= params.end_date {
        return Err(ApiError::ValidationError {
            field: "end_date".to_string(),
            message: "End date must be after start date".to_string(),
        });
    }
    // More validations...
    Ok(())
}
```

**Estimated Effort:** 2-3 days

---

#### B13. Database Connection Pooling 🟡 Medium Priority

**Problem:** `Mutex<Connection>` creates bottleneck under concurrent requests.

**Implementation:**

```toml
# Cargo.toml
[dependencies]
r2d2 = "0.8"
r2d2_sqlite = "0.24"
```

```rust
use r2d2_sqlite::SqliteConnectionManager;

type DbPool = r2d2::Pool<SqliteConnectionManager>;

async fn create_pool() -> DbPool {
    let manager = SqliteConnectionManager::file("finplan.db");
    r2d2::Pool::builder()
        .max_size(10)
        .build(manager)
        .expect("Failed to create pool")
}

// In routes:
async fn get_portfolio(
    State(pool): State<DbPool>,
    Path(id): Path<i64>,
) -> Result<Json<Portfolio>, ApiError> {
    let conn = pool.get()?;
    // Use conn...
}
```

**Estimated Effort:** 1-2 days

---

#### B14. Streaming Results for Large Monte Carlo Runs 🟡 Medium Priority

**Problem:** Large simulations (1000+ iterations) block until complete.

**Implementation:**

```rust
use axum::response::sse::{Event, Sse};
use futures::stream::Stream;

async fn run_simulation_streaming(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(query): Query<RunQuery>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = async_stream::stream! {
        let total = query.iterations.unwrap_or(100);
        let batch_size = 10;
        
        for batch_start in (0..total).step_by(batch_size) {
            let batch_end = (batch_start + batch_size).min(total);
            // Run batch...
            
            yield Ok(Event::default().json_data(ProgressUpdate {
                completed: batch_end,
                total,
                partial_results: Some(aggregated_so_far),
            }));
        }
        
        yield Ok(Event::default().json_data(FinalResult { ... }));
    };
    
    Sse::new(stream)
}
```

**Estimated Effort:** 2-3 days

---

#### B15. Result Caching 🟢 Low Priority

**Problem:** Repeated runs with same parameters recalculate unnecessarily.

**Implementation:**
- Hash simulation parameters + seed
- Check cache before running
- Store results with TTL
- Invalidate on parameter change

**Estimated Effort:** 2-3 days

---

#### B16. API Documentation (OpenAPI/Swagger) 🟢 Low Priority

**Implementation:**

```toml
# Cargo.toml
[dependencies]
utoipa = { version = "4", features = ["axum_extras"] }
utoipa-swagger-ui = { version = "6", features = ["axum"] }
```

```rust
#[derive(utoipa::ToSchema)]
pub struct Portfolio {
    pub id: Option<i64>,
    pub name: String,
    // ...
}

#[utoipa::path(
    get,
    path = "/api/portfolios/{id}",
    params(("id" = i64, Path, description = "Portfolio ID")),
    responses(
        (status = 200, description = "Portfolio found", body = Portfolio),
        (status = 404, description = "Portfolio not found")
    )
)]
async fn get_portfolio(...) { ... }
```

**Estimated Effort:** 2-3 days

---

#### B17. Modularize Server Code 🟢 Low Priority

**Problem:** `main.rs` is 900+ lines; difficult to navigate.

**Proposed Structure:**
```
finplan_server/src/
├── main.rs           # App setup, server start
├── routes/
│   ├── mod.rs
│   ├── portfolios.rs
│   ├── simulations.rs
│   └── runs.rs
├── handlers/
│   ├── mod.rs
│   ├── portfolio_handlers.rs
│   └── simulation_handlers.rs
├── db/
│   ├── mod.rs
│   ├── portfolio_db.rs
│   └── simulation_db.rs
├── models.rs         # API-specific models
└── error.rs          # Error types
```

**Estimated Effort:** 2-3 days

---

## Frontend Improvements

### Architecture & Code Quality

#### F2. Split Large Components 🔴 High Priority

**Problem:** `simulation-wizard.tsx` is 1942 lines; difficult to maintain.

**Proposed Structure:**
```
components/simulation-wizard/
├── index.tsx                    # Main wizard orchestration
├── SimulationWizardContext.tsx  # Shared state context
├── steps/
│   ├── BasicInfoStep.tsx        # Step 1: Name, dates, birth
│   ├── ProfilesStep.tsx         # Step 2: Return/inflation profiles
│   ├── AssetLinkingStep.tsx     # Step 3: Portfolio linking
│   ├── CashFlowsStep.tsx        # Step 4: Income/expenses
│   ├── EventsStep.tsx           # Step 5: Events configuration
│   ├── SpendingStep.tsx         # Step 6: Spending targets
│   └── ReviewStep.tsx           # Step 7: Final review
└── hooks/
    └── useSimulationForm.ts     # Form state management
```

**Estimated Effort:** 3-4 days

---

#### F3. Create Shared Hooks Library 🟡 Medium Priority

**Problem:** Duplicate code across components; no data fetching hooks.

**Implementation:**

```typescript
// hooks/use-api.ts
export function usePortfolio(id: number) {
  const [portfolio, setPortfolio] = useState<Portfolio | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<Error | null>(null);
  
  useEffect(() => {
    api.getPortfolio(id)
      .then(setPortfolio)
      .catch(setError)
      .finally(() => setLoading(false));
  }, [id]);
  
  const refetch = useCallback(() => { /* ... */ }, [id]);
  const update = useCallback(async (data: Partial<Portfolio>) => { /* ... */ }, [id]);
  
  return { portfolio, loading, error, refetch, update };
}

export function useSimulation(id: number) { /* Similar */ }
export function usePortfolios() { /* List with pagination */ }
export function useSimulations() { /* List with pagination */ }
```

**Additional Hooks:**
```typescript
// hooks/use-debounce.ts
export function useDebounce<T>(value: T, delay: number): T { /* ... */ }

// hooks/use-persistent-state.ts (for wizard drafts)
export function usePersistentState<T>(key: string, initial: T): [T, SetState<T>] {
  const [state, setState] = useState<T>(() => {
    const stored = localStorage.getItem(key);
    return stored ? JSON.parse(stored) : initial;
  });
  
  useEffect(() => {
    localStorage.setItem(key, JSON.stringify(state));
  }, [key, state]);
  
  return [state, setState];
}
```

**Estimated Effort:** 2-3 days

---

#### F4. Add Error Boundaries 🟡 Medium Priority

**Implementation:**

```typescript
// components/error-boundary.tsx
'use client';

import { Component, ErrorInfo, ReactNode } from 'react';

interface Props {
  children: ReactNode;
  fallback?: ReactNode;
}

export class ErrorBoundary extends Component<Props, { hasError: boolean; error?: Error }> {
  state = { hasError: false, error: undefined };

  static getDerivedStateFromError(error: Error) {
    return { hasError: true, error };
  }

  componentDidCatch(error: Error, errorInfo: ErrorInfo) {
    console.error('Error boundary caught:', error, errorInfo);
    // Could send to error tracking service
  }

  render() {
    if (this.state.hasError) {
      return this.props.fallback || (
        <div className="p-8 text-center">
          <h2 className="text-xl font-semibold text-red-600">Something went wrong</h2>
          <p className="text-muted-foreground mt-2">{this.state.error?.message}</p>
          <button 
            onClick={() => this.setState({ hasError: false })}
            className="mt-4 px-4 py-2 bg-primary text-white rounded"
          >
            Try again
          </button>
        </div>
      );
    }
    return this.props.children;
  }
}
```

**Estimated Effort:** 1-2 days

---

#### F5. Add Toast Notifications 🟡 Medium Priority

**Problem:** No user feedback for async operations (save, delete, run).

**Implementation:**

```typescript
// Using shadcn/ui toast (already in UI folder)
import { toast } from "@/components/ui/use-toast";

// In components:
const handleSave = async () => {
  try {
    await api.updateSimulation(id, data);
    toast({ title: "Success", description: "Simulation saved" });
  } catch (error) {
    toast({ 
      title: "Error", 
      description: "Failed to save simulation", 
      variant: "destructive" 
    });
  }
};
```

**Estimated Effort:** 1 day

---

#### F6. Add Loading Skeletons 🟢 Low Priority

**Problem:** No loading states during data fetching; pages jump.

**Implementation:**

```typescript
// components/ui/skeleton-card.tsx
export function PortfolioCardSkeleton() {
  return (
    <Card>
      <CardHeader>
        <Skeleton className="h-6 w-48" />
        <Skeleton className="h-4 w-32" />
      </CardHeader>
      <CardContent>
        <Skeleton className="h-10 w-full" />
        <Skeleton className="h-10 w-full mt-2" />
      </CardContent>
    </Card>
  );
}

// Usage:
{loading ? (
  <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
    {[...Array(6)].map((_, i) => <PortfolioCardSkeleton key={i} />)}
  </div>
) : (
  portfolios.map(p => <PortfolioCard key={p.id} portfolio={p} />)
)}
```

**Estimated Effort:** 1-2 days

---

#### F7. Add Form Validation with Zod 🟡 Medium Priority

**Problem:** No input validation; users can save invalid configurations.

**Implementation:**

```typescript
// lib/schemas.ts
import { z } from "zod";

export const portfolioSchema = z.object({
  name: z.string().min(1, "Name is required").max(100, "Name too long"),
  accounts: z.array(z.object({
    name: z.string().min(1, "Account name required"),
    account_type: z.enum(["Taxable", "TaxDeferred", "TaxFree", "Illiquid"]),
    assets: z.array(z.object({
      name: z.string().min(1),
      balance: z.number().min(0, "Balance cannot be negative"),
      return_profile: returnProfileSchema,
    })).min(1, "At least one asset required"),
  })).min(1, "At least one account required"),
});

export const simulationSchema = z.object({
  name: z.string().min(1, "Name is required"),
  parameters: z.object({
    start_date: z.string().regex(/^\d{4}-\d{2}-\d{2}$/, "Invalid date format"),
    end_date: z.string().regex(/^\d{4}-\d{2}-\d{2}$/, "Invalid date format"),
    birth_date: z.string().optional(),
  }).refine(
    data => new Date(data.end_date) > new Date(data.start_date),
    { message: "End date must be after start date" }
  ),
});
```

**Estimated Effort:** 2-3 days

---

### User Experience

#### F8. Events UI (Full Implementation) 🔴 High Priority

**Problem:** Events step shows "coming soon"; users can't configure triggers/effects.

**Implementation:**

```typescript
// components/event-builder/
├── EventBuilder.tsx        # Main event configuration
├── TriggerBuilder.tsx      # Visual trigger builder
├── EffectBuilder.tsx       # Effect configuration
├── ConditionTree.tsx       # For And/Or compound triggers
└── presets/
    ├── RetirementEvent.tsx # Age-based retirement
    ├── RmdEvent.tsx        # RMD setup helper
    └── DebtPayoffEvent.tsx # Balance-triggered

// TriggerBuilder.tsx example
export function TriggerBuilder({ value, onChange }: TriggerBuilderProps) {
  const [triggerType, setTriggerType] = useState<TriggerType>("Date");
  
  return (
    <div className="space-y-4">
      <Select value={triggerType} onValueChange={setTriggerType}>
        <SelectTrigger>
          <SelectValue placeholder="Select trigger type" />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="Date">On specific date</SelectItem>
          <SelectItem value="Age">At age</SelectItem>
          <SelectItem value="AccountBalance">Account balance reaches</SelectItem>
          <SelectItem value="NetWorth">Net worth reaches</SelectItem>
          <SelectItem value="CashFlowEnded">When cash flow ends</SelectItem>
          <SelectItem value="Repeating">Repeating schedule</SelectItem>
          <SelectItem value="And">All conditions (AND)</SelectItem>
          <SelectItem value="Or">Any condition (OR)</SelectItem>
        </SelectContent>
      </Select>
      
      {triggerType === "Age" && (
        <div className="grid grid-cols-2 gap-4">
          <Input 
            type="number" 
            label="Years" 
            min={0} max={120}
            value={value.Age?.years}
            onChange={(e) => onChange({ Age: { years: parseInt(e.target.value), months: value.Age?.months } })}
          />
          <Input 
            type="number" 
            label="Months" 
            min={0} max={11}
            value={value.Age?.months}
            onChange={(e) => onChange({ Age: { ...value.Age, months: parseInt(e.target.value) } })}
          />
        </div>
      )}
      {/* Other trigger type UIs... */}
    </div>
  );
}
```

**Event Presets (Templates):**
- **Retirement Event**: Stops income cash flows, starts withdrawal spending target
- **RMD Event**: Age-triggered annual RMD withdrawal (when backend supports it)
- **Mortgage Payoff**: Balance reaches $0, triggers expense termination
- **Social Security Start**: Age-triggered income creation

**Estimated Effort:** 5-7 days

---

#### F9. Scenario Comparison View 🔴 High Priority

**Problem:** No way to compare different simulation scenarios side-by-side.

**Implementation:**

```typescript
// app/compare/page.tsx
export default function ComparePage() {
  const [selectedSimulations, setSelectedSimulations] = useState<number[]>([]);
  const [results, setResults] = useState<Map<number, AggregatedResults>>();
  
  return (
    <div className="container">
      <h1>Scenario Comparison</h1>
      
      {/* Simulation Selector */}
      <MultiSelect
        options={simulations.map(s => ({ value: s.id, label: s.name }))}
        selected={selectedSimulations}
        onChange={setSelectedSimulations}
        max={4}
      />
      
      {/* Comparison Chart */}
      <ComparisonChart 
        results={results}
        metric="total_portfolio" // or "success_rate", "ending_balance"
      />
      
      {/* Comparison Table */}
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>Metric</TableHead>
            {selectedSimulations.map(id => (
              <TableHead key={id}>{simulations.find(s => s.id === id)?.name}</TableHead>
            ))}
          </TableRow>
        </TableHeader>
        <TableBody>
          <TableRow>
            <TableCell>P50 Ending Balance</TableCell>
            {selectedSimulations.map(id => (
              <TableCell key={id}>
                {formatCurrency(results.get(id)?.ending_balance_p50)}
              </TableCell>
            ))}
          </TableRow>
          {/* More metrics... */}
        </TableBody>
      </Table>
    </div>
  );
}
```

**Key Metrics to Compare:**
- P10/P50/P90 ending balances
- Success rate (% of iterations above $0)
- Sequence-of-returns risk (worst 10% outcomes)
- Total taxes paid
- Total withdrawals
- Asset allocation drift

**Estimated Effort:** 4-5 days

---

#### F10. Data Export (CSV/PDF) 🟡 Medium Priority

**Problem:** No way to export simulation results for external analysis.

**Implementation:**

```typescript
// lib/export.ts
export function exportToCSV(results: AggregatedResults, filename: string) {
  const headers = ["Date", "P10", "P50", "P90"];
  const rows = results.total_portfolio.map(point => [
    point.date,
    point.p10,
    point.p50,
    point.p90
  ]);
  
  const csv = [headers, ...rows]
    .map(row => row.join(","))
    .join("\n");
  
  const blob = new Blob([csv], { type: "text/csv" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = `${filename}.csv`;
  a.click();
}

// PDF using @react-pdf/renderer
export async function exportToPDF(results: AggregatedResults) {
  const doc = (
    <Document>
      <Page>
        <Text>Simulation Results</Text>
        {/* Charts as images */}
        {/* Tables */}
      </Page>
    </Document>
  );
  const blob = await pdf(doc).toBlob();
  // Save blob...
}
```

**Export Options:**
- CSV: Time series data, transaction logs
- PDF: Executive summary with charts
- JSON: Full results for programmatic use

**Estimated Effort:** 2-3 days

---

#### F11. Simulation Cloning/Templates 🟡 Medium Priority

**Problem:** No way to duplicate a simulation or start from a template.

**Implementation:**

```typescript
// Clone existing simulation
async function cloneSimulation(id: number, newName: string) {
  const original = await api.getSimulation(id);
  const cloned = {
    ...original,
    id: undefined,
    name: newName || `${original.name} (Copy)`,
  };
  return api.createSimulation(cloned);
}

// Templates
const SIMULATION_TEMPLATES = [
  {
    name: "Early Retirement (FIRE)",
    description: "Aggressive savings, early retirement at 45-50",
    parameters: { /* ... */ }
  },
  {
    name: "Traditional Retirement",
    description: "Work until 65, Social Security at 67",
    parameters: { /* ... */ }
  },
  {
    name: "Roth Conversion Ladder",
    description: "5-year Roth conversion for early access",
    parameters: { /* ... */ }
  },
];
```

**Estimated Effort:** 2-3 days

---

#### F12. Withdrawal Strategy Sequential Order UI 🟡 Medium Priority

**Problem:** Sequential withdrawal strategy's `order` array not configurable in UI.

**Implementation:**

```typescript
// Drag-and-drop account ordering
import { DndContext, closestCenter } from "@dnd-kit/core";
import { SortableContext, verticalListSortingStrategy } from "@dnd-kit/sortable";

function WithdrawalOrderConfig({ accounts, order, onChange }) {
  const sensors = useSensors(useSensor(PointerSensor));
  
  return (
    <DndContext sensors={sensors} collisionDetection={closestCenter} onDragEnd={handleDragEnd}>
      <SortableContext items={order} strategy={verticalListSortingStrategy}>
        {order.map((accountId) => (
          <SortableAccountItem 
            key={accountId} 
            account={accounts.find(a => a.account_id === accountId)} 
          />
        ))}
      </SortableContext>
    </DndContext>
  );
}
```

**Estimated Effort:** 1-2 days

---

#### F13. Interactive "What-If" Slider 🟢 Low Priority

**Problem:** Changing parameters requires re-running entire simulation.

**Implementation:**
- Cache multiple simulation runs with varied parameters
- Slider adjusts visible result set instantly
- Parameters: Return rate, inflation rate, withdrawal amount, retirement age

**Estimated Effort:** 3-4 days

---

#### F14. Mobile Responsiveness 🟢 Low Priority

**Problem:** Charts have fixed heights; sidebar may not work well on mobile.

**Implementation:**
- Use `useMediaQuery` hook for breakpoint detection
- Collapse charts to single column on mobile
- Drawer-based navigation for mobile sidebar
- Touch-friendly date pickers and inputs

**Estimated Effort:** 2-3 days

---

### Missing Features

#### F15. Tax Configuration UI 🟡 Medium Priority

**Problem:** No way to customize tax brackets from UI; uses hardcoded defaults.

**Implementation:**

```typescript
// components/tax-config-editor.tsx
function TaxConfigEditor({ config, onChange }) {
  return (
    <div className="space-y-6">
      <Select 
        label="Filing Status"
        value={config.filing_status}
        onChange={(v) => onChange({ ...config, filing_status: v })}
      >
        <SelectItem value="Single">Single</SelectItem>
        <SelectItem value="MarriedFilingJointly">Married Filing Jointly</SelectItem>
        <SelectItem value="MarriedFilingSeparately">Married Filing Separately</SelectItem>
        <SelectItem value="HeadOfHousehold">Head of Household</SelectItem>
      </Select>
      
      <div>
        <Label>Federal Tax Brackets</Label>
        <BracketEditor 
          brackets={config.federal_brackets}
          onChange={(b) => onChange({ ...config, federal_brackets: b })}
        />
        <Button variant="outline" onClick={loadDefaultBrackets}>
          Load {config.filing_status} 2024 Defaults
        </Button>
      </div>
      
      <div>
        <Label>State Tax</Label>
        <RadioGroup value={config.state_type} onChange={handleStateTypeChange}>
          <RadioGroupItem value="none">No state income tax</RadioGroupItem>
          <RadioGroupItem value="flat">Flat rate</RadioGroupItem>
          <RadioGroupItem value="progressive">Progressive brackets</RadioGroupItem>
        </RadioGroup>
      </div>
    </div>
  );
}
```

**Estimated Effort:** 2-3 days

---

#### F16. Cash Flow Limits UI 🟡 Medium Priority

**Problem:** `yearly_limit` and `lifetime_limit` fields exist in types but no UI.

**Implementation:**

```typescript
<div className="grid grid-cols-2 gap-4">
  <div>
    <Label>Yearly Limit (optional)</Label>
    <CurrencyInput
      value={cashFlow.yearly_limit}
      onChange={(v) => updateCashFlow({ yearly_limit: v })}
      placeholder="No limit"
    />
    <p className="text-xs text-muted-foreground">
      Maximum total for this cash flow per year
    </p>
  </div>
  <div>
    <Label>Lifetime Limit (optional)</Label>
    <CurrencyInput
      value={cashFlow.lifetime_limit}
      onChange={(v) => updateCashFlow({ lifetime_limit: v })}
      placeholder="No limit"
    />
    <p className="text-xs text-muted-foreground">
      Maximum total over simulation lifetime
    </p>
  </div>
</div>
```

**Estimated Effort:** 1 day

---

#### F17. LogNormal Profile UI 🟢 Low Priority

**Problem:** Only Fixed and Normal return profiles available in UI; LogNormal defined but not exposed.

**Implementation:**
```typescript
{returnType === "LogNormal" && (
  <div className="grid grid-cols-2 gap-4">
    <div>
      <Label>Mean (μ)</Label>
      <Input type="number" step="0.01" ... />
      <p className="text-xs text-muted-foreground">
        Log of expected return
      </p>
    </div>
    <div>
      <Label>Standard Deviation (σ)</Label>
      <Input type="number" step="0.01" min="0" ... />
    </div>
  </div>
)}
```

**Estimated Effort:** 1 hour

---

## New Feature Roadmap

### Phase 1: Immediate Priorities (1-2 weeks)

| ID | Feature | Type | Effort | Impact |
|----|---------|------|--------|--------|
| F1 | Fix TypeScript build error | Frontend | 30 min | Critical |
| B12 | API validation & error handling | Backend | 2-3 days | High |
| F2 | Split simulation-wizard.tsx | Frontend | 3-4 days | High |
| F5 | Toast notifications | Frontend | 1 day | Medium |

### Phase 2: Core Functionality (2-4 weeks)

| ID | Feature | Type | Effort | Impact |
|----|---------|------|--------|--------|
| B1 | RMD implementation | Backend | 3-5 days | High |
| B2 | Social Security modeling | Backend | 5-7 days | High |
| B8 | Filing status support | Backend | 2-3 days | High |
| F8 | Events UI | Frontend | 5-7 days | High |
| F3 | Shared hooks library | Frontend | 2-3 days | Medium |

### Phase 3: Enhanced Analysis (4-8 weeks)

| ID | Feature | Type | Effort | Impact |
|----|---------|------|--------|--------|
| F9 | Scenario comparison | Frontend | 4-5 days | High |
| B3 | Asset rebalancing | Backend | 4-6 days | Medium |
| B10 | Roth conversion optimization | Backend | 4-5 days | Medium |
| F10 | Data export | Frontend | 2-3 days | Medium |
| F7 | Form validation | Frontend | 2-3 days | Medium |

### Phase 4: Advanced Features (8+ weeks)

| ID | Feature | Type | Effort | Impact |
|----|---------|------|--------|--------|
| B4 | Cost basis tracking | Backend | 5-8 days | Medium |
| B5 | Healthcare cost modeling | Backend | 4-5 days | Medium |
| B7 | Monte Carlo improvements | Backend | 5-7 days | Medium |
| B9 | State-specific taxes | Backend | 3-4 days/state | Low |
| F13 | Interactive what-if slider | Frontend | 3-4 days | Low |

---

## Technical Debt

### High Priority

1. **Server `.unwrap()` audit** - Replace all `.unwrap()` with proper error handling
2. **Test coverage** - Add unit tests for frontend components
3. **Database migrations** - Add schema versioning for production deployments
4. **Environment configuration** - Move hardcoded values to config files

### Medium Priority

5. **Duplicate code removal** - Extract `useCurrencyInput`, `formatCurrency`, etc.
6. **API response types** - Ensure all responses have consistent shape with errors
7. **Logging** - Add structured logging to backend (tracing crate)
8. **CI/CD** - Add GitHub Actions for build, test, lint

### Low Priority

9. **Documentation** - Add JSDoc/rustdoc comments to public APIs
10. **Storybook** - Create component documentation
11. **Performance monitoring** - Add metrics collection
12. **Accessibility audit** - Ensure WCAG 2.1 compliance

---

## Implementation Phases

### Phase 1: Foundation (Weeks 1-2)
**Goal:** Fix critical issues, improve code quality

- [ ] F1: Fix TypeScript build error
- [ ] B12: Add API validation and error types
- [ ] F2: Split simulation-wizard.tsx into step components
- [ ] F5: Add toast notifications for user feedback
- [ ] F4: Add error boundaries

**Deliverables:**
- Clean TypeScript build
- Better error messages in UI and API
- Maintainable component structure

---

### Phase 2: Core Features (Weeks 3-6)
**Goal:** Add essential retirement planning features

- [ ] B1: Implement RMD support (see RMD_IMPLEMENTATION_PLAN.md)
- [ ] B2: Add Social Security modeling
- [ ] B8: Support multiple filing statuses
- [ ] F8: Build Events UI for trigger/effect configuration
- [ ] F3: Create shared hooks library
- [ ] F7: Add form validation with Zod

**Deliverables:**
- RMD calculations in simulations
- Social Security income modeling
- Full event configuration from UI
- Input validation throughout app

---

### Phase 3: Analysis & Comparison (Weeks 7-10)
**Goal:** Enable advanced analysis workflows

- [ ] F9: Build scenario comparison view
- [ ] F10: Add CSV/PDF export
- [ ] F11: Implement simulation cloning/templates
- [ ] B3: Add automatic rebalancing
- [ ] B10: Implement Roth conversion strategies
- [ ] B13: Add database connection pooling

**Deliverables:**
- Side-by-side scenario comparison
- Exportable results
- Simulation templates for common scenarios
- Automatic portfolio rebalancing

---

### Phase 4: Polish & Advanced (Weeks 11+)
**Goal:** Advanced features and production readiness

- [ ] B4: Implement cost basis tracking
- [ ] B5: Add healthcare cost modeling
- [ ] B7: Enhanced Monte Carlo (correlation, regimes)
- [ ] B14: Streaming results for large simulations
- [ ] F13: Interactive what-if sliders
- [ ] F14: Mobile responsive design
- [ ] B16: OpenAPI documentation

**Deliverables:**
- Production-ready application
- Advanced tax optimization
- Healthcare planning
- Mobile support

---

## Success Metrics

| Metric | Current | Phase 1 Target | Phase 4 Target |
|--------|---------|----------------|----------------|
| Build passing | ❌ No | ✅ Yes | ✅ Yes |
| Test coverage (backend) | ~60% | 70% | 85% |
| Test coverage (frontend) | 0% | 30% | 60% |
| API endpoints documented | 0% | 50% | 100% |
| Events configurable from UI | ❌ No | ❌ No | ✅ Yes |
| RMD support | ❌ No | ✅ Yes | ✅ Yes |
| Social Security | ❌ No | ✅ Yes | ✅ Yes |
| Scenario comparison | ❌ No | ❌ No | ✅ Yes |
| Export capabilities | ❌ No | ❌ No | ✅ Yes |

---

## Appendix: Reference Links

- [IRS RMD Life Expectancy Tables](https://www.irs.gov/publications/p590b)
- [Social Security Benefit Calculators](https://www.ssa.gov/benefits/retirement/estimator.html)
- [FIRE Movement Wiki](https://www.reddit.com/r/financialindependence/wiki/faq/)
- [Bogleheads Tax-Efficient Fund Placement](https://www.bogleheads.org/wiki/Tax-efficient_fund_placement)
- [shadcn/ui Components](https://ui.shadcn.com/)
- [Axum Web Framework](https://docs.rs/axum/latest/axum/)
