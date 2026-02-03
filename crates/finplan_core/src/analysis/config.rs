//! Configuration types for parameter sweep analysis.

use crate::model::{AccountId, EventId};
use serde::{Deserialize, Serialize};

/// Target for sweeping a trigger parameter
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TriggerParam {
    /// Modify a Date trigger's date (sweep by year offset)
    Date,
    /// Modify an Age trigger's years field
    Age,
    /// Modify the start trigger of a Repeating event
    RepeatingStart(Box<TriggerParam>),
    /// Modify the end trigger of a Repeating event
    RepeatingEnd(Box<TriggerParam>),
}

/// Target for sweeping an effect parameter
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EffectParam {
    /// Modify the Fixed value in an amount (unwraps InflationAdjusted if present)
    Value,
    /// Modify Scale multiplier
    Multiplier,
}

/// Specifies which effect to target within an event
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub enum EffectTarget {
    /// Apply to first eligible effect (Income, Expense, Sweep, AssetPurchase, AssetSale, etc.)
    #[default]
    FirstEligible,
    /// Apply to specific effect by index (0-based)
    Index(usize),
}

/// What part of an event to sweep
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SweepTarget {
    /// Sweep a trigger parameter
    Trigger(TriggerParam),
    /// Sweep an effect parameter
    Effect {
        param: EffectParam,
        target: EffectTarget,
    },
    /// Sweep asset allocation for an account
    AssetAllocation { account_id: AccountId },
}

/// N-dimensional grid storage with flat backing array and stride-based indexing.
///
/// Stores values in row-major order where the last dimension varies fastest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SweepGrid<T> {
    /// The data stored in row-major order
    data: Vec<T>,
    /// Shape of each dimension (e.g., [5, 10, 3] for a 5x10x3 grid)
    shape: Vec<usize>,
    /// Precomputed strides for index calculation
    strides: Vec<usize>,
}

impl<T: Clone> SweepGrid<T> {
    /// Create a new grid with the given shape, filled with the default value.
    pub fn new(shape: Vec<usize>, default: T) -> Self {
        let total_size: usize = shape.iter().product();
        let strides = compute_strides(&shape);
        Self {
            data: vec![default; total_size],
            shape,
            strides,
        }
    }

    /// Create a grid from existing data. Data must be in row-major order.
    pub fn from_data(shape: Vec<usize>, data: Vec<T>) -> Option<Self> {
        let total_size: usize = shape.iter().product();
        if data.len() != total_size {
            return None;
        }
        let strides = compute_strides(&shape);
        Some(Self {
            data,
            shape,
            strides,
        })
    }

    /// Get the shape of the grid
    pub fn shape(&self) -> &[usize] {
        &self.shape
    }

    /// Get the number of dimensions
    pub fn ndim(&self) -> usize {
        self.shape.len()
    }

    /// Get the total number of elements
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Check if the grid is empty
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Convert multi-dimensional indices to flat index
    pub fn flat_index(&self, indices: &[usize]) -> Option<usize> {
        if indices.len() != self.shape.len() {
            return None;
        }
        let mut flat = 0;
        for (i, (&idx, &size)) in indices.iter().zip(&self.shape).enumerate() {
            if idx >= size {
                return None;
            }
            flat += idx * self.strides[i];
        }
        Some(flat)
    }

    /// Convert flat index to multi-dimensional indices
    pub fn multi_index(&self, flat: usize) -> Option<Vec<usize>> {
        if flat >= self.data.len() {
            return None;
        }
        let mut indices = Vec::with_capacity(self.shape.len());
        let mut remaining = flat;
        for &stride in &self.strides {
            indices.push(remaining / stride);
            remaining %= stride;
        }
        Some(indices)
    }

    /// Get a reference to the value at the given indices
    pub fn get(&self, indices: &[usize]) -> Option<&T> {
        self.flat_index(indices).map(|i| &self.data[i])
    }

    /// Get a mutable reference to the value at the given indices
    pub fn get_mut(&mut self, indices: &[usize]) -> Option<&mut T> {
        self.flat_index(indices).map(|i| &mut self.data[i])
    }

    /// Set the value at the given indices
    pub fn set(&mut self, indices: &[usize], value: T) -> bool {
        if let Some(i) = self.flat_index(indices) {
            self.data[i] = value;
            true
        } else {
            false
        }
    }

    /// Get a reference to the underlying data
    pub fn data(&self) -> &[T] {
        &self.data
    }

    /// Get a mutable reference to the underlying data
    pub fn data_mut(&mut self) -> &mut [T] {
        &mut self.data
    }

    /// Iterate over all indices in row-major order
    pub fn indices(&self) -> GridIndices {
        GridIndices {
            shape: self.shape.clone(),
            current: vec![0; self.shape.len()],
            done: self.data.is_empty(),
        }
    }

    /// Iterate over (indices, value) pairs
    pub fn iter(&self) -> impl Iterator<Item = (Vec<usize>, &T)> {
        self.indices().zip(self.data.iter())
    }

    /// Extract a 1D slice along a dimension at fixed indices for other dimensions.
    /// Returns the values and their coordinates along the extracted dimension.
    pub fn slice_1d(&self, dim: usize, fixed: &[Option<usize>]) -> Option<Vec<(f64, &T)>>
    where
        T: Clone,
    {
        if dim >= self.ndim() || fixed.len() != self.ndim() {
            return None;
        }
        // Verify fixed indices are provided for all dimensions except `dim`
        for (i, f) in fixed.iter().enumerate() {
            if i != dim && f.is_none() {
                return None;
            }
        }

        let mut result = Vec::with_capacity(self.shape[dim]);
        for idx in 0..self.shape[dim] {
            let mut indices: Vec<usize> = fixed.iter().map(|f| f.unwrap_or(0)).collect();
            indices[dim] = idx;
            if let Some(val) = self.get(&indices) {
                result.push((idx as f64, val));
            }
        }
        Some(result)
    }

    /// Extract a 2D slice for two dimensions at fixed indices for others.
    /// Returns shape (dim1_size, dim2_size) and flattened data in row-major.
    pub fn slice_2d(
        &self,
        dim1: usize,
        dim2: usize,
        fixed: &[Option<usize>],
    ) -> Option<(Vec<&T>, usize, usize)>
    where
        T: Clone,
    {
        if dim1 >= self.ndim() || dim2 >= self.ndim() || dim1 == dim2 {
            return None;
        }
        if fixed.len() != self.ndim() {
            return None;
        }
        // Verify fixed indices for all dims except dim1 and dim2
        for (i, f) in fixed.iter().enumerate() {
            if i != dim1 && i != dim2 && f.is_none() {
                return None;
            }
        }

        let rows = self.shape[dim1];
        let cols = self.shape[dim2];
        let mut result = Vec::with_capacity(rows * cols);

        for i1 in 0..rows {
            for i2 in 0..cols {
                let mut indices: Vec<usize> = fixed.iter().map(|f| f.unwrap_or(0)).collect();
                indices[dim1] = i1;
                indices[dim2] = i2;
                if let Some(val) = self.get(&indices) {
                    result.push(val);
                }
            }
        }
        Some((result, rows, cols))
    }
}

impl<T: Default + Clone> SweepGrid<T> {
    /// Create a new grid with default values
    pub fn with_default(shape: Vec<usize>) -> Self {
        Self::new(shape, T::default())
    }
}

/// Compute strides for row-major order
fn compute_strides(shape: &[usize]) -> Vec<usize> {
    if shape.is_empty() {
        return Vec::new();
    }
    let mut strides = vec![1; shape.len()];
    for i in (0..shape.len() - 1).rev() {
        strides[i] = strides[i + 1] * shape[i + 1];
    }
    strides
}

/// Iterator over all indices in a grid
pub struct GridIndices {
    shape: Vec<usize>,
    current: Vec<usize>,
    done: bool,
}

impl Iterator for GridIndices {
    type Item = Vec<usize>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }

        let result = self.current.clone();

        // Increment indices (row-major: last dimension varies fastest)
        for i in (0..self.shape.len()).rev() {
            self.current[i] += 1;
            if self.current[i] < self.shape[i] {
                break;
            }
            self.current[i] = 0;
            if i == 0 {
                self.done = true;
            }
        }

        Some(result)
    }
}

/// Complete sweep parameter specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SweepParameter {
    /// The event to modify
    pub event_id: EventId,
    /// What part of the event to sweep
    pub target: SweepTarget,
    /// Minimum value for the sweep
    pub min_value: f64,
    /// Maximum value for the sweep
    pub max_value: f64,
    /// Number of steps (points) in the sweep
    pub step_count: usize,
}

impl SweepParameter {
    /// Create a new sweep parameter for an age trigger
    pub fn age(event_id: EventId, min_age: u8, max_age: u8, steps: usize) -> Self {
        Self {
            event_id,
            target: SweepTarget::Trigger(TriggerParam::Age),
            min_value: min_age as f64,
            max_value: max_age as f64,
            step_count: steps,
        }
    }

    /// Create a new sweep parameter for an effect value
    pub fn effect_value(event_id: EventId, min: f64, max: f64, steps: usize) -> Self {
        Self {
            event_id,
            target: SweepTarget::Effect {
                param: EffectParam::Value,
                target: EffectTarget::FirstEligible,
            },
            min_value: min,
            max_value: max,
            step_count: steps,
        }
    }

    /// Generate the sweep values
    pub fn sweep_values(&self) -> Vec<f64> {
        if self.step_count <= 1 {
            return vec![self.min_value];
        }
        let step_size = (self.max_value - self.min_value) / (self.step_count - 1) as f64;
        (0..self.step_count)
            .map(|i| self.min_value + step_size * i as f64)
            .collect()
    }

    /// Get a descriptive label for display
    pub fn label(&self) -> String {
        match &self.target {
            SweepTarget::Trigger(TriggerParam::Age) => {
                format!("Age (Event {})", self.event_id.0)
            }
            SweepTarget::Trigger(TriggerParam::Date) => {
                format!("Date (Event {})", self.event_id.0)
            }
            SweepTarget::Trigger(TriggerParam::RepeatingStart(_)) => {
                format!("Repeat Start (Event {})", self.event_id.0)
            }
            SweepTarget::Trigger(TriggerParam::RepeatingEnd(_)) => {
                format!("Repeat End (Event {})", self.event_id.0)
            }
            SweepTarget::Effect { .. } => {
                format!("Amount (Event {})", self.event_id.0)
            }
            SweepTarget::AssetAllocation { account_id } => {
                format!("Allocation (Account {})", account_id.0)
            }
        }
    }
}

/// Configuration for a sweep analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SweepConfig {
    /// Parameters to sweep (supports N dimensions)
    pub parameters: Vec<SweepParameter>,
    /// Metrics to compute at each point (used when running combined sweep_evaluate)
    pub metrics: Vec<super::AnalysisMetric>,
    /// Number of Monte Carlo iterations per point
    pub mc_iterations: usize,
    /// Number of parallel batches (defaults to CPU count)
    #[serde(default = "default_parallel_batches")]
    pub parallel_batches: usize,
}

fn default_parallel_batches() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}

impl Default for SweepConfig {
    fn default() -> Self {
        Self {
            parameters: Vec::new(),
            metrics: vec![super::AnalysisMetric::SuccessRate],
            mc_iterations: 500,
            parallel_batches: default_parallel_batches(),
        }
    }
}

impl SweepConfig {
    /// Get the number of dimensions in the sweep
    pub fn ndim(&self) -> usize {
        self.parameters.len()
    }

    /// Check if this is a 1D sweep
    pub fn is_1d(&self) -> bool {
        self.parameters.len() == 1
    }

    /// Check if this is a 2D sweep
    pub fn is_2d(&self) -> bool {
        self.parameters.len() == 2
    }

    /// Get total number of sweep points
    pub fn total_points(&self) -> usize {
        self.parameters.iter().map(|p| p.step_count).product()
    }

    /// Get the shape of the sweep grid (step counts for each parameter)
    pub fn grid_shape(&self) -> Vec<usize> {
        self.parameters.iter().map(|p| p.step_count).collect()
    }

    /// Get sweep values for all parameters
    pub fn all_sweep_values(&self) -> Vec<Vec<f64>> {
        self.parameters.iter().map(|p| p.sweep_values()).collect()
    }

    /// Get labels for all parameters
    pub fn labels(&self) -> Vec<String> {
        self.parameters.iter().map(|p| p.label()).collect()
    }
}
