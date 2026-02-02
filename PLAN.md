# Plan: N-Dimensional Parameter Sweeps with Configurable Result Charts

## Overview

Extend the analysis feature to support N-dimensional parameter sweeps with user-configurable result charts. Currently limited to 2 parameters, but `finplan_core`'s `SweepGrid<T>` already supports arbitrary dimensions with `slice_1d`/`slice_2d` operations.

## Key Changes

### 1. Data Structures

**Add `ChartConfigData`** (`crates/finplan/src/data/analysis_data.rs`):
```rust
pub enum ChartType { Scatter1D, Heatmap2D }

pub struct ChartConfigData {
    pub id: usize,
    pub chart_type: ChartType,
    pub x_param_index: usize,           // X-axis dimension
    pub y_param_index: Option<usize>,   // Y-axis dimension (2D only)
    pub metric: AnalysisMetricData,
    pub fixed_values: HashMap<usize, usize>,  // Non-displayed dims -> step index
}
```

**Update `AnalysisConfigData`** to include `chart_configs: Vec<ChartConfigData>`

**Replace `AnalysisResults`** (`crates/finplan/src/state/screen_state.rs`):
- Remove hardcoded `param1_values`, `param2_values`
- Store `finplan_core::analysis::SweepResults` directly (already N-dimensional)
- Add helper methods: `ndim()`, `param_values(dim)`, `midpoint_index(dim)`

**Update `AnalysisState`** to add:
- `chart_configs: Vec<ChartConfigData>`
- `selected_chart_index: usize` (for h/l navigation between chart slots)

### 2. Remove 2-Parameter Limit

**File**: `crates/finplan/src/actions/analysis.rs:49-52`

Replace the hard limit with a reasonable cap (6 dimensions):
```rust
const MAX_SWEEP_DIMENSIONS: usize = 6;
if state.analysis_state.sweep_parameters.len() >= MAX_SWEEP_DIMENSIONS {
    return ActionResult::error("Maximum of 6 sweep parameters supported.");
}
```

### 3. Chart Configuration UI

**Results Panel Layout**:
- Display 2-4 charts side-by-side (based on available width, like current implementation)
- Each chart slot shows configured chart OR empty `[CONFIGURE]` placeholder
- Use existing MIN_CHART_WIDTH (60) / MAX_CHART_WIDTH (80) constraints

**Navigation Flow**:
1. User navigates INTO Results panel (Tab)
2. Use `h`/`l` (or arrow keys) to move between chart slots
3. Selected chart is highlighted
4. Press `Enter` or `c` to configure the selected chart
5. Configuration modal allows picking:
   - Chart type (1D scatter / 2D heatmap)
   - X parameter (from N available)
   - Y parameter (for 2D, from remaining N-1)
   - Metric to display
   - Fixed values for other dimensions (default: midpoint)

**Keybindings** (`crates/finplan/src/data/keybindings_data.rs`):
- `h`/`l` or Left/Right: Navigate between charts (in Results panel)
- `Enter` or `c`: Configure selected chart
- `+` or `a`: Add new chart (if space available)
- `-` or `d`: Delete selected chart

### 4. Chart Rendering with Slicing

**File**: `crates/finplan/src/screens/analysis.rs`

New rendering flow:
1. Calculate how many charts fit (2-4 based on width)
2. For each chart slot:
   - If slot has config: render chart with sliced data
   - If slot empty: show `[CONFIGURE]` placeholder
   - Highlight selected chart (border color/style) when panel focused
3. For configured charts:
   - Build `fixed` array: `None` for displayed dims, `Some(idx)` for others
   - Call `SweepGrid::slice_1d()` or `slice_2d()` to extract data
   - Render using existing chart widgets

```rust
fn render_1d_chart_from_config(&self, results: &AnalysisResults, config: &ChartConfigData) {
    let fixed: Vec<Option<usize>> = (0..results.ndim())
        .map(|dim| {
            if dim == config.x_param_index { None }
            else { Some(config.fixed_values.get(&dim).copied()
                       .unwrap_or_else(|| results.midpoint_index(dim))) }
        }).collect();

    let slice = results.sweep_results.metrics.slice_1d(config.x_param_index, &fixed);
    // Render scatter plot with slice data
}
```

### 5. Persistence

Update `load_from_config` / `to_config` in `AnalysisState` to include chart configs.

---

## Files to Modify

| File | Changes |
|------|---------|
| `crates/finplan/src/data/analysis_data.rs` | Add `ChartType`, `ChartConfigData`, update `AnalysisConfigData` |
| `crates/finplan/src/state/screen_state.rs` | Replace `AnalysisResults`, update `AnalysisState` |
| `crates/finplan/src/actions/analysis.rs` | Remove 2-param limit, add chart config handlers |
| `crates/finplan/src/screens/analysis.rs` | Add `[CONFIGURE]` prompt, slice-based rendering |
| `crates/finplan/src/data/keybindings_data.rs` | Add chart config keybindings |
| `crates/finplan/src/modals/action.rs` | Add chart configuration actions |

---

## Implementation Order

1. **Phase 1**: Add `ChartType` and `ChartConfigData` to `analysis_data.rs`
2. **Phase 2**: Replace `AnalysisResults` to wrap core's `SweepResults`
3. **Phase 3**: Update `AnalysisState` with chart config fields
4. **Phase 4**: Remove 2-parameter limit in `actions/analysis.rs`
5. **Phase 5**: Add `[CONFIGURE]` prompt rendering in results panel
6. **Phase 6**: Implement chart configuration modal flow
7. **Phase 7**: Implement slice-based chart rendering
8. **Phase 8**: Add keybindings and persistence

---

## Verification

1. Configure 3-4 sweep parameters and run analysis
2. Verify `[CONFIGURE]` prompt appears in results
3. Add a 1D scatter chart, select metric and parameter
4. Add a 2D heatmap chart with different parameters
5. Change fixed values for non-displayed dimensions
6. Save scenario, reload, verify chart configs persist
7. Run `cargo fmt` and `cargo clippy`
