# Analysis Screen Implementation Plan

Replace the Optimize screen with a new Analysis screen for parameter sweep sensitivity analysis.

## Summary

- **Goal**: Enable users to sweep parameters (retirement age, savings rate, house price, etc.) across ranges and visualize effects on metrics (success rate, net worth at age X, percentiles, taxes, drawdown)
- **Dimensions**: N-dimensional sweeps supported; UI renders 1D slices (line charts) + 2D slices (heatmaps)
- **Execution**: Two-phase: run simulations up-front, analyze with different metrics afterward
- **Persistence**: Session only

## Screen Layout

### 1D Mode
```
┌─ SWEEP PARAMETERS ──────────┬─ METRICS ─────────────────┐
│ > Retirement Age [60-70]    │ [x] Success Rate          │
│                             │ [x] Net Worth at 75       │
│ [a] add  [d] delete         │ [ ] P5/P50/P95            │
├─ CONFIGURATION ─────────────┼─ PROGRESS ────────────────┤
│ MC Iterations: 500          │ [========    ] 60%        │
│ Steps: 6                    │ 4/6 points complete       │
├─ 1D RESULTS ────────────────┴───────────────────────────┤
│ 100%|              *---*---*                            │
│  90%|        *---*'                                     │
│  80%|  *---*'                                           │
│     +----+----+----+----+----+                          │
│       60   62   64   66   68   70                       │
└─────────────────────────────────────────────────────────┘
```

### 2D Mode
```
┌─ 2D HEATMAP - Success Rate (%) ─────────────────────────┐
│        Withdrawal Amount ($K/yr)                        │
│        30    35    40    45    50    55    60          │
│    60  98    95    89    78    62    45    30          │
│ R  62  99    97    92    84    70    52    35          │
│ e  64  99    98    95    89    78    60    42          │
│ t  66  99    99    97    93    85    70    55          │
│    68 100    99    98    96    90    78    62          │
│    70 100   100    99    98    94    85    72          │
│                                                         │
│  Legend: Red <50  Yellow 50-70  Green 70-85  Bright >85│
└─────────────────────────────────────────────────────────┘
```

## Implementation Phases

### Phase 1: Core Engine (`finplan_core`) ✅ COMPLETE

The `analysis` module supports N-dimensional parameter sweeps with a two-phase approach:

| File | Description |
|------|-------------|
| `src/analysis/mod.rs` | Module exports |
| `src/analysis/config.rs` | `SweepConfig`, `SweepParameter`, `SweepTarget`, `SweepGrid<T>` |
| `src/analysis/metrics.rs` | `AnalysisMetric` enum, `compute_metrics()`, `SweepResults` |
| `src/analysis/evaluator.rs` | `sweep_simulate()`, `sweep_evaluate()`, `SweepSimulationResults` |

**Two-Phase Architecture:**

```rust
// Phase 1: Run simulations up-front (expensive, done once)
let sim_results = sweep_simulate(&config, &sweep_config, Some(&progress))?;

// Phase 2: Compute metrics on-demand (fast, repeatable with different metrics)
let results = sim_results.compute_all_metrics(&[AnalysisMetric::SuccessRate]);

// Can compute different metrics without re-running simulations
let other_results = sim_results.compute_all_metrics(&[
    AnalysisMetric::NetWorthAtAge { age: 75 },
    AnalysisMetric::MaxDrawdown,
]);

// Or use combined mode for simpler cases
let results = sweep_evaluate(&config, &sweep_config, progress)?;
```

**N-Dimensional Grid (`SweepGrid<T>`):**

```rust
/// Generic N-dimensional grid with flat backing array and stride-based indexing
pub struct SweepGrid<T> {
    data: Vec<T>,       // Row-major storage
    shape: Vec<usize>,  // Shape per dimension
    strides: Vec<usize>,// Precomputed strides
}

impl<T> SweepGrid<T> {
    fn new(shape: Vec<usize>, default: T) -> Self;
    fn get(&self, indices: &[usize]) -> Option<&T>;
    fn set(&mut self, indices: &[usize], value: T) -> bool;
    fn slice_1d(&self, dim: usize, fixed: &[Option<usize>]) -> Option<Vec<(f64, &T)>>;
    fn slice_2d(&self, dim1: usize, dim2: usize, fixed: &[Option<usize>]) -> Option<(Vec<&T>, usize, usize)>;
}
```

**Simulation Results Storage:**

```rust
/// Stores raw MonteCarloSummary for each grid point
pub struct SweepSimulationResults {
    pub param_values: Vec<Vec<f64>>,           // Values per dimension
    pub param_labels: Vec<String>,             // Labels per dimension
    pub summaries: SweepGrid<Option<MonteCarloSummary>>,
    pub birth_year: i16,
}

impl SweepSimulationResults {
    fn compute_all_metrics(&self, metrics: &[AnalysisMetric]) -> SweepResults;
    fn compute_metric_grid(&self, metric: &AnalysisMetric) -> SweepGrid<f64>;
}
```

**Key types:**
```rust
pub enum AnalysisMetric {
    SuccessRate,
    NetWorthAtAge { age: u8 },
    Percentile { percentile: u8 },
    LifetimeTaxes,
    MaxDrawdown,
    SafeWithdrawalRate { target_success_rate: f64 },
}

pub enum TriggerParam {
    Date,
    Age,
    RepeatingStart(Box<TriggerParam>),
    RepeatingEnd(Box<TriggerParam>),
}

pub enum EffectParam {
    Value,
    Multiplier,
}

#[derive(Default)]
pub enum EffectTarget {
    #[default]
    FirstEligible,
    Index(usize),
}

pub struct SweepParameter {
    pub event_id: EventId,
    pub target: SweepTarget,
    pub min_value: f64,
    pub max_value: f64,
    pub step_count: usize,
}

pub enum SweepTarget {
    Trigger(TriggerParam),
    Effect { param: EffectParam, target: EffectTarget },
    AssetAllocation { account_id: AccountId },
}
```

### Phase 2: TUI State (`finplan`)

**Modify:**
- `state/screen_state.rs` - Replace `OptimizeState` with `AnalysisState`
- `state/panels.rs` - Replace `OptimizePanel` with `AnalysisPanel`
- `state/app_state.rs` - Replace `optimize_state` field
- `modals/action.rs` - Replace `OptimizeAction` with `AnalysisAction`

**New state:**
```rust
pub struct AnalysisState {
    pub focused_panel: AnalysisPanel,
    pub sweep_parameters: Vec<SweepParameter>,  // max 2
    pub selected_metrics: HashSet<AnalysisMetric>,
    pub mc_iterations: usize,
    pub running: bool,
    pub current_point: usize,
    pub total_points: usize,
    pub results: Option<SweepResults>,
    pub selected_result: (usize, usize),  // cursor for 2D
}
```

### Phase 3: TUI Actions

**Create** `actions/analysis.rs` (replaces `optimize.rs`):
- `handle_add_parameter()` - Enumerate all events with sweepable params, show picker
- `handle_configure_parameter()` - Form for min/max/steps
- `handle_select_metrics()` - Multi-select for metrics
- `handle_run_analysis()` - Send to worker thread

**Key function - enumerate sweepable targets:**
```rust
/// Enumerate all sweepable parameters from an event
fn get_sweepable_targets(event: &EventData, event_id: EventId) -> Vec<SweepableTarget> {
    let mut targets = Vec::new();

    // 1. Scan trigger for sweepable params (Age, Date, Repeating start/end)
    scan_trigger_params(&event.trigger, TriggerPath::Root, &mut |path, current_value| {
        targets.push(SweepableTarget {
            event_id,
            event_name: event.name.0.clone(),
            target: SweepTarget::Trigger(path),
            description: format_trigger_description(&event.name.0, &path),
            current_value,
        });
    });

    // 2. Scan effects for sweepable amounts (Fixed values, Scale multipliers)
    for (idx, effect) in event.effects.iter().enumerate() {
        if let Some((param, current_value)) = get_effect_sweepable_param(effect) {
            targets.push(SweepableTarget {
                event_id,
                event_name: event.name.0.clone(),
                target: SweepTarget::Effect {
                    param,
                    target: EffectTarget::Index(idx)
                },
                description: format_effect_description(&event.name.0, effect, idx),
                current_value,
            });
        }
    }

    targets
}

/// Recursively scan trigger for sweepable parameters
fn scan_trigger_params<F>(trigger: &TriggerData, path: TriggerPath, callback: &mut F)
where F: FnMut(TriggerParam, f64)
{
    match trigger {
        TriggerData::Age { years, .. } => {
            callback(path.to_param(), *years as f64);
        }
        TriggerData::Date { date } => {
            // Extract year as sweepable value
            if let Ok(d) = parse_date(date) {
                callback(path.to_param(), d.year() as f64);
            }
        }
        TriggerData::Repeating { start, end, .. } => {
            if let Some(start_trigger) = start {
                scan_trigger_params(start_trigger, path.into_start(), callback);
            }
            if let Some(end_trigger) = end {
                scan_trigger_params(end_trigger, path.into_end(), callback);
            }
        }
        _ => {} // And, Or, thresholds not sweepable for now
    }
}

/// Extract sweepable parameter from an effect
fn get_effect_sweepable_param(effect: &EffectData) -> Option<(EffectParam, f64)> {
    let amount = match effect {
        EffectData::Income { amount, .. } |
        EffectData::Expense { amount, .. } |
        EffectData::AssetPurchase { amount, .. } |
        EffectData::AssetSale { amount, .. } |
        EffectData::Sweep { amount, .. } |
        EffectData::AdjustBalance { amount, .. } |
        EffectData::CashTransfer { amount, .. } => amount,
        _ => return None,
    };

    // Unwrap InflationAdjusted to find Fixed value
    extract_fixed_value(amount).map(|v| (EffectParam::Value, v))
}
```

### Parameter Selection UX Flow

When user presses 'a' to add a sweep parameter:

1. **Event Picker**: Show list of all events with sweepable parameters
   ```
   ┌─ Select Event ────────────────────────────┐
   │ > Retirement (Age trigger: 65)            │
   │   Monthly Salary (Income: $8,000)         │
   │   Social Security (Income: $2,500)        │
   │   Retirement Withdrawal (Sweep: $50,000)  │
   │   House Purchase (Expense: $600,000)      │
   └───────────────────────────────────────────┘
   ```

2. **Target Picker** (if event has multiple sweepable params): Show which part to sweep
   ```
   ┌─ Select Parameter for "Retirement Withdrawal" ─┐
   │ > Trigger: Start Age (currently 65)            │
   │   Effect #1: Sweep Amount (currently $50,000)  │
   └────────────────────────────────────────────────┘
   ```

3. **Range Form**: Configure min/max/steps
   ```
   ┌─ Configure Sweep Range ────────────────┐
   │ Parameter: Retirement Age              │
   │                                        │
   │ Min Value: [60     ]                   │
   │ Max Value: [70     ]                   │
   │ Steps:     [6      ]                   │
   │                                        │
   │ Preview: 60, 62, 64, 66, 68, 70       │
   └────────────────────────────────────────┘
   ```

### Phase 4: Worker Thread

**Modify** `worker.rs`:
- Add `SimulationRequest::SweepAnalysis { config, sweep_config }`
- Add `SimulationResponse::SweepProgress { current, total }`
- Add `SimulationResponse::SweepComplete { results }`

### Phase 5: Screen Implementation

**Create** `screens/analysis.rs` (replaces `optimize.rs`):
- `render_parameters()` - List of sweep parameters
- `render_metrics()` - Checkbox-style metric selector
- `render_config()` - MC iterations, step count
- `render_progress()` - Progress bar during execution
- `render_results_1d()` - ASCII line chart
- `render_results_2d()` - Color-coded heatmap with cursor navigation

### Phase 6: Cleanup

- Remove `screens/optimize.rs`
- Remove `actions/optimize.rs`
- Update `screens/mod.rs` exports
- Update `actions/mod.rs` exports
- Keep `finplan_core/optimization/` for now (may be useful later)

## Files to Modify

### finplan_core
| File | Action |
|------|--------|
| `src/lib.rs` | Add `pub mod analysis;` |
| `src/analysis/mod.rs` | **NEW** |
| `src/analysis/config.rs` | **NEW** |
| `src/analysis/metrics.rs` | **NEW** |
| `src/analysis/evaluator.rs` | **NEW** |

### finplan (TUI)
| File | Action |
|------|--------|
| `src/state/screen_state.rs` | Replace OptimizeState with AnalysisState |
| `src/state/panels.rs` | Replace OptimizePanel with AnalysisPanel |
| `src/state/app_state.rs` | Replace optimize_state field |
| `src/modals/action.rs` | Replace OptimizeAction with AnalysisAction |
| `src/modals/context.rs` | Replace OptimizeContext with AnalysisContext |
| `src/actions/analysis.rs` | **NEW** (replaces optimize.rs) |
| `src/actions/mod.rs` | Update exports |
| `src/screens/analysis.rs` | **NEW** (replaces optimize.rs) |
| `src/screens/mod.rs` | Update exports |
| `src/worker.rs` | Add sweep request/response types |
| `src/app.rs` | Update screen routing if needed |

## Metrics Implementation Notes

| Metric | Source | Notes |
|--------|--------|-------|
| SuccessRate | `MonteCarloStats.success_rate` | Direct |
| NetWorthAtAge | `SimulationResult.wealth_snapshots` | Find snapshot at target year |
| Percentile | `MonteCarloStats.percentile_values` | Direct lookup |
| LifetimeTaxes | `SimulationResult.yearly_taxes` | Sum all years |
| MaxDrawdown | `wealth_snapshots` | Compute peak-to-trough |
| SafeWithdrawalRate | Iterative | Binary search for rate achieving target success |

## Verification

1. Build: `cargo build`
2. Run TUI: `cargo run --bin finplan`
3. Navigate to Analysis tab
4. Add a sweep parameter (e.g., Retirement Age 60-70)
5. Select metrics (Success Rate, Net Worth at 75)
6. Run analysis - verify progress updates
7. Verify 1D line chart displays correctly
8. Add second parameter - verify 2D heatmap
9. Run `cargo clippy` and `cargo fmt`
