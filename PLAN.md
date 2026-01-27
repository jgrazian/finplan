Use this file as a store of current and future plans for the repo.
Edit the file as needed to track the implementation state, assumptions and reasoning about feature implementation.
When work is complete make sure to update the state of PLAN.md.

--

# Returns Model Enhancement Plan

## Overview

Enhance the returns modeling in `finplan_core` to provide more realistic and sophisticated simulation of investment returns. The current implementation supports only basic distributions (None, Fixed, Normal, LogNormal). This plan adds:

1. More asset class presets with historical data
2. Fat-tailed distributions (Student's t)
3. Regime-switching models (bull/bear markets)
4. Correlated multi-asset returns
5. Historical bootstrap sampling

---

## Phase 1: Asset Class Presets and Student's t Distribution

### 1.1 Add Historical Constants for Major Asset Classes

**File:** `crates/finplan_core/src/model/market.rs`

Add preset constants to `ReturnProfile` for common asset classes:

```rust
impl ReturnProfile {
    // Existing S&P 500
    pub const SP_500_HISTORICAL_FIXED: ReturnProfile = ReturnProfile::Fixed(0.095668);
    pub const SP_500_HISTORICAL_NORMAL: ReturnProfile = ReturnProfile::Normal {
        mean: 0.095668,
        std_dev: 0.165234,
    };

    // US Total Bond Market (1976-2024, Barclays Aggregate proxy)
    pub const US_BOND_HISTORICAL_FIXED: ReturnProfile = ReturnProfile::Fixed(0.052);
    pub const US_BOND_HISTORICAL_NORMAL: ReturnProfile = ReturnProfile::Normal {
        mean: 0.052,
        std_dev: 0.065,
    };

    // International Developed Stocks (MSCI EAFE, 1970-2024)
    pub const INTL_STOCK_HISTORICAL_FIXED: ReturnProfile = ReturnProfile::Fixed(0.075);
    pub const INTL_STOCK_HISTORICAL_NORMAL: ReturnProfile = ReturnProfile::Normal {
        mean: 0.075,
        std_dev: 0.180,
    };

    // US Small Cap (Russell 2000 proxy, 1979-2024)
    pub const SMALL_CAP_HISTORICAL_FIXED: ReturnProfile = ReturnProfile::Fixed(0.105);
    pub const SMALL_CAP_HISTORICAL_NORMAL: ReturnProfile = ReturnProfile::Normal {
        mean: 0.105,
        std_dev: 0.220,
    };

    // REITs (FTSE NAREIT, 1972-2024)
    pub const REITS_HISTORICAL_FIXED: ReturnProfile = ReturnProfile::Fixed(0.095);
    pub const REITS_HISTORICAL_NORMAL: ReturnProfile = ReturnProfile::Normal {
        mean: 0.095,
        std_dev: 0.200,
    };

    // Treasury Bills / Money Market (1928-2024)
    pub const MONEY_MARKET_HISTORICAL_FIXED: ReturnProfile = ReturnProfile::Fixed(0.033);
    pub const MONEY_MARKET_HISTORICAL_NORMAL: ReturnProfile = ReturnProfile::Normal {
        mean: 0.033,
        std_dev: 0.031,
    };

    // US Treasury Long-Term Bonds (1928-2024)
    pub const TREASURY_LONG_HISTORICAL_FIXED: ReturnProfile = ReturnProfile::Fixed(0.052);
    pub const TREASURY_LONG_HISTORICAL_NORMAL: ReturnProfile = ReturnProfile::Normal {
        mean: 0.052,
        std_dev: 0.097,
    };

    // Corporate Bonds (1928-2024)
    pub const CORPORATE_BOND_HISTORICAL_FIXED: ReturnProfile = ReturnProfile::Fixed(0.058);
    pub const CORPORATE_BOND_HISTORICAL_NORMAL: ReturnProfile = ReturnProfile::Normal {
        mean: 0.058,
        std_dev: 0.072,
    };

    // 60/40 Portfolio (US Stock/Bond blend)
    pub const BALANCED_60_40_FIXED: ReturnProfile = ReturnProfile::Fixed(0.078);
    pub const BALANCED_60_40_NORMAL: ReturnProfile = ReturnProfile::Normal {
        mean: 0.078,
        std_dev: 0.110,
    };
}
```

**Rationale for values:**
- S&P 500: Existing values (1928-2024 geometric mean ~9.5%, std dev ~16.5%)
- Bonds: Aggregate bond index since 1976, lower vol before that
- International: MSCI EAFE benchmark, higher vol than US
- Small Cap: Russell 2000 since 1979, higher return/vol
- REITs: NAREIT index, equity-like returns with higher vol
- Money Market: T-bill returns, very low vol
- 60/40: Classic balanced portfolio blend

### 1.2 Add Student's t Distribution

**File:** `crates/finplan_core/src/model/market.rs`

Add new variant to `ReturnProfile` enum:

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ReturnProfile {
    None,
    Fixed(f64),
    Normal { mean: f64, std_dev: f64 },
    LogNormal { mean: f64, std_dev: f64 },
    // NEW: Student's t for fat tails
    StudentT {
        mean: f64,
        scale: f64,      // Similar to std_dev but scaled
        df: f64,         // Degrees of freedom (lower = fatter tails)
    },
}
```

**Implementation in `sample()`:**

```rust
ReturnProfile::StudentT { mean, scale, df } => {
    rand_distr::StudentT::new(*df)
        .map(|d| mean + scale * d.sample(rng))
        .map_err(|_| MarketError::InvalidDistributionParameters {
            profile_type: "StudentT return",
            mean: *mean,
            std_dev: *scale,  // Use scale in error for consistency
            reason: "degrees of freedom must be positive and finite",
        })
}
```

**Add presets:**

```rust
// Student's t with 5 df matches historical equity fat tails well
pub const SP_500_STUDENT_T: ReturnProfile = ReturnProfile::StudentT {
    mean: 0.095668,
    scale: 0.145,  // Adjusted scale for df=5
    df: 5.0,
};
```

**Add to error.rs if needed:**

Extend `MarketError::InvalidDistributionParameters` or add new variant for df validation.

### 1.3 Add Similar Constants to InflationProfile

**File:** `crates/finplan_core/src/model/market.rs`

```rust
impl InflationProfile {
    // Existing
    pub const US_HISTORICAL_FIXED: InflationProfile = InflationProfile::Fixed(0.035432);

    // Add regional variants
    pub const LOW_INFLATION_FIXED: InflationProfile = InflationProfile::Fixed(0.02);
    pub const TARGET_INFLATION_FIXED: InflationProfile = InflationProfile::Fixed(0.025);
    pub const HIGH_INFLATION_NORMAL: InflationProfile = InflationProfile::Normal {
        mean: 0.05,
        std_dev: 0.03,
    };
}
```

---

## Phase 2: Regime-Switching Model

### 2.1 Add RegimeSwitching Variant

**File:** `crates/finplan_core/src/model/market.rs`

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ReturnProfile {
    // ... existing variants ...

    /// Markov regime-switching model with bull/bear states
    RegimeSwitching {
        /// Bull market parameters (higher returns, lower volatility)
        bull_mean: f64,
        bull_std_dev: f64,
        /// Bear market parameters (lower/negative returns, higher volatility)
        bear_mean: f64,
        bear_std_dev: f64,
        /// Annual probability of transitioning from bull to bear
        bull_to_bear_prob: f64,
        /// Annual probability of transitioning from bear to bull
        bear_to_bull_prob: f64,
    },
}
```

**Challenge:** Regime state must persist across years within a simulation run.

**Solution:** Track regime state in `Market` struct:

```rust
#[derive(Debug, Clone)]
pub struct Market {
    inflation_values: Vec<Rate>,
    returns: FxHashMap<ReturnProfileId, Vec<Rate>>,
    assets: FxHashMap<AssetId, (f64, ReturnProfileId)>,
    // NEW: Track current regime per profile for regime-switching
    regime_states: FxHashMap<ReturnProfileId, RegimeState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegimeState {
    Bull,
    Bear,
}
```

**Sampling implementation:**

```rust
ReturnProfile::RegimeSwitching {
    bull_mean, bull_std_dev,
    bear_mean, bear_std_dev,
    bull_to_bear_prob, bear_to_bull_prob,
} => {
    // This requires access to current regime state and RNG for transition
    // Return error indicating regime switching requires special handling
    Err(MarketError::RegimeSwitchingRequiresState)
}
```

**Alternative:** Pre-generate regime sequence in `Market::from_profiles()`:

```rust
pub fn from_profiles<R: Rng + ?Sized>(
    rng: &mut R,
    num_years: usize,
    inflation_profile: &InflationProfile,
    return_profiles: &HashMap<ReturnProfileId, ReturnProfile>,
    assets: &FxHashMap<AssetId, (f64, ReturnProfileId)>,
) -> Result<Self, MarketError> {
    let mut returns: FxHashMap<ReturnProfileId, Vec<f64>> = FxHashMap::default();

    for (rp_id, rp) in return_profiles.iter() {
        let rp_returns = match rp {
            ReturnProfile::RegimeSwitching {
                bull_mean, bull_std_dev,
                bear_mean, bear_std_dev,
                bull_to_bear_prob, bear_to_bull_prob
            } => {
                generate_regime_switching_returns(
                    rng, num_years,
                    *bull_mean, *bull_std_dev,
                    *bear_mean, *bear_std_dev,
                    *bull_to_bear_prob, *bear_to_bull_prob,
                )?
            }
            _ => {
                let mut vals = Vec::with_capacity(num_years);
                for _ in 0..num_years {
                    vals.push(rp.sample(rng)?);
                }
                vals
            }
        };
        returns.insert(*rp_id, rp_returns);
    }

    Ok(Self::new(inflation_values, returns, assets.clone()))
}

fn generate_regime_switching_returns<R: Rng + ?Sized>(
    rng: &mut R,
    num_years: usize,
    bull_mean: f64, bull_std_dev: f64,
    bear_mean: f64, bear_std_dev: f64,
    bull_to_bear_prob: f64, bear_to_bull_prob: f64,
) -> Result<Vec<f64>, MarketError> {
    let mut returns = Vec::with_capacity(num_years);
    let mut in_bull = true;  // Start in bull market

    let bull_dist = rand_distr::Normal::new(bull_mean, bull_std_dev)
        .map_err(|_| MarketError::InvalidDistributionParameters { ... })?;
    let bear_dist = rand_distr::Normal::new(bear_mean, bear_std_dev)
        .map_err(|_| MarketError::InvalidDistributionParameters { ... })?;

    for _ in 0..num_years {
        // Sample return from current regime
        let ret = if in_bull {
            bull_dist.sample(rng)
        } else {
            bear_dist.sample(rng)
        };
        returns.push(ret);

        // Transition regime for next year
        let transition_prob = if in_bull { bull_to_bear_prob } else { bear_to_bull_prob };
        if rng.gen::<f64>() < transition_prob {
            in_bull = !in_bull;
        }
    }

    Ok(returns)
}
```

### 2.2 Add Regime-Switching Presets

```rust
// Based on historical analysis of S&P 500 bull/bear cycles
pub const SP_500_REGIME_SWITCHING: ReturnProfile = ReturnProfile::RegimeSwitching {
    bull_mean: 0.15,
    bull_std_dev: 0.12,
    bear_mean: -0.08,
    bear_std_dev: 0.25,
    bull_to_bear_prob: 0.12,   // ~8 year bull cycles
    bear_to_bull_prob: 0.50,   // ~2 year bear cycles
};
```

---

## Phase 3: Correlated Returns

### 3.1 Add Correlation Matrix Support

**New file:** `crates/finplan_core/src/model/correlation.rs`

```rust
use rustc_hash::FxHashMap;
use crate::model::ReturnProfileId;

/// Correlation matrix for multi-asset returns
#[derive(Debug, Clone)]
pub struct CorrelationMatrix {
    /// Profile IDs in order (defines matrix indices)
    profiles: Vec<ReturnProfileId>,
    /// Lower triangular correlation coefficients (row-major)
    /// For n profiles: n*(n-1)/2 values
    correlations: Vec<f64>,
}

impl CorrelationMatrix {
    pub fn new(profiles: Vec<ReturnProfileId>, correlations: Vec<f64>) -> Result<Self, MarketError> {
        let n = profiles.len();
        let expected_len = n * (n - 1) / 2;
        if correlations.len() != expected_len {
            return Err(MarketError::InvalidCorrelationMatrix { ... });
        }
        // Validate correlations are in [-1, 1]
        for &c in &correlations {
            if c < -1.0 || c > 1.0 {
                return Err(MarketError::InvalidCorrelationCoefficient(c));
            }
        }
        Ok(Self { profiles, correlations })
    }

    /// Get correlation between two profiles
    pub fn get(&self, a: ReturnProfileId, b: ReturnProfileId) -> Option<f64> {
        if a == b { return Some(1.0); }
        let idx_a = self.profiles.iter().position(|&p| p == a)?;
        let idx_b = self.profiles.iter().position(|&p| p == b)?;
        let (i, j) = if idx_a < idx_b { (idx_a, idx_b) } else { (idx_b, idx_a) };
        // Lower triangular index
        let idx = j * (j - 1) / 2 + i;
        self.correlations.get(idx).copied()
    }

    /// Compute Cholesky decomposition for correlated sampling
    pub fn cholesky(&self) -> Result<CholeskyDecomp, MarketError> {
        // Build full correlation matrix
        let n = self.profiles.len();
        let mut matrix = vec![vec![0.0; n]; n];
        for i in 0..n {
            matrix[i][i] = 1.0;
            for j in 0..i {
                let idx = i * (i - 1) / 2 + j;
                matrix[i][j] = self.correlations[idx];
                matrix[j][i] = self.correlations[idx];
            }
        }

        // Cholesky decomposition (L * L^T = matrix)
        let mut l = vec![vec![0.0; n]; n];
        for i in 0..n {
            for j in 0..=i {
                let mut sum = 0.0;
                if i == j {
                    for k in 0..j {
                        sum += l[j][k] * l[j][k];
                    }
                    let val = matrix[j][j] - sum;
                    if val <= 0.0 {
                        return Err(MarketError::CorrelationMatrixNotPositiveDefinite);
                    }
                    l[j][j] = val.sqrt();
                } else {
                    for k in 0..j {
                        sum += l[i][k] * l[j][k];
                    }
                    l[i][j] = (matrix[i][j] - sum) / l[j][j];
                }
            }
        }

        Ok(CholeskyDecomp { l, profiles: self.profiles.clone() })
    }
}

#[derive(Debug, Clone)]
pub struct CholeskyDecomp {
    l: Vec<Vec<f64>>,
    profiles: Vec<ReturnProfileId>,
}

impl CholeskyDecomp {
    /// Generate correlated samples from independent standard normal samples
    pub fn correlate(&self, independent: &[f64]) -> Vec<f64> {
        let n = self.profiles.len();
        let mut correlated = vec![0.0; n];
        for i in 0..n {
            for j in 0..=i {
                correlated[i] += self.l[i][j] * independent[j];
            }
        }
        correlated
    }
}
```

### 3.2 Update Market::from_profiles for Correlated Sampling

```rust
pub fn from_profiles_correlated<R: Rng + ?Sized>(
    rng: &mut R,
    num_years: usize,
    inflation_profile: &InflationProfile,
    return_profiles: &HashMap<ReturnProfileId, ReturnProfile>,
    correlation: &CorrelationMatrix,
    assets: &FxHashMap<AssetId, (f64, ReturnProfileId)>,
) -> Result<Self, MarketError> {
    let cholesky = correlation.cholesky()?;
    let profile_ids: Vec<_> = correlation.profiles.clone();
    let n = profile_ids.len();

    // Get means and std_devs for each profile
    let params: Vec<(f64, f64)> = profile_ids.iter()
        .map(|id| {
            match return_profiles.get(id) {
                Some(ReturnProfile::Normal { mean, std_dev }) => Ok((*mean, *std_dev)),
                Some(ReturnProfile::Fixed(r)) => Ok((*r, 0.0)),
                _ => Err(MarketError::CorrelatedSamplingRequiresNormal),
            }
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut returns: FxHashMap<ReturnProfileId, Vec<f64>> = FxHashMap::default();
    for id in &profile_ids {
        returns.insert(*id, Vec::with_capacity(num_years));
    }

    let standard_normal = rand_distr::StandardNormal;

    for _ in 0..num_years {
        // Generate independent standard normal samples
        let independent: Vec<f64> = (0..n).map(|_| standard_normal.sample(rng)).collect();

        // Apply Cholesky to get correlated standard normals
        let correlated = cholesky.correlate(&independent);

        // Transform to actual returns using mean and std_dev
        for (i, id) in profile_ids.iter().enumerate() {
            let (mean, std_dev) = params[i];
            let ret = mean + std_dev * correlated[i];
            returns.get_mut(id).unwrap().push(ret);
        }
    }

    // Handle profiles not in correlation matrix (sample independently)
    for (rp_id, rp) in return_profiles.iter() {
        if !profile_ids.contains(rp_id) {
            let mut rp_returns = Vec::with_capacity(num_years);
            for _ in 0..num_years {
                rp_returns.push(rp.sample(rng)?);
            }
            returns.insert(*rp_id, rp_returns);
        }
    }

    // ... rest of function (inflation, etc.)
}
```

### 3.3 Default Correlation Presets

```rust
impl CorrelationMatrix {
    /// Standard US asset class correlations (historical averages)
    pub fn us_standard(
        us_stock: ReturnProfileId,
        intl_stock: ReturnProfileId,
        us_bond: ReturnProfileId,
        reits: ReturnProfileId,
    ) -> Self {
        // Historical correlation estimates:
        // US Stock / Intl Stock: 0.75
        // US Stock / US Bond: 0.05
        // US Stock / REITs: 0.60
        // Intl Stock / US Bond: 0.10
        // Intl Stock / REITs: 0.55
        // US Bond / REITs: 0.15
        Self {
            profiles: vec![us_stock, intl_stock, us_bond, reits],
            correlations: vec![
                0.75,              // [1,0]: US Stock / Intl Stock
                0.05, 0.10,        // [2,0], [2,1]: US Bond / ...
                0.60, 0.55, 0.15,  // [3,0], [3,1], [3,2]: REITs / ...
            ],
        }
    }
}
```

---

## Phase 4: Historical Bootstrap

### 4.1 Add Bootstrap Data Structure

**New file:** `crates/finplan_core/src/model/historical.rs`

```rust
use jiff::civil::Date;
use serde::{Deserialize, Serialize};

/// Historical return series for bootstrap sampling
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoricalReturns {
    /// Asset/index name
    pub name: String,
    /// Start year of data
    pub start_year: i16,
    /// Annual returns (index 0 = start_year)
    pub returns: Vec<f64>,
}

impl HistoricalReturns {
    /// S&P 500 annual returns 1928-2024 (97 years)
    pub fn sp500() -> Self {
        Self {
            name: "S&P 500".to_string(),
            start_year: 1928,
            returns: vec![
                0.4381, -0.0830, -0.2512, -0.4384, -0.0864, 0.5399, -0.0144, 0.4756,
                0.3392, -0.3503, 0.2994, -0.0110, -0.1078, -0.1267, 0.1917, 0.2551,
                0.1936, 0.3600, -0.0807, 0.0548, 0.0565, 0.1830, 0.3081, 0.2368,
                0.1867, -0.0099, 0.5256, 0.3262, 0.0744, -0.1046, 0.4372, 0.1206,
                0.0034, 0.2664, -0.0881, 0.2261, 0.1642, 0.1245, -0.0997, 0.2380,
                0.1081, -0.0824, 0.0400, 0.1431, 0.1898, -0.1469, -0.2647, 0.3723,
                0.2393, -0.0718, 0.0656, 0.1844, 0.3242, -0.0491, 0.2155, 0.2256,
                0.0627, 0.3173, 0.1867, 0.0525, 0.1661, 0.3169, -0.0310, 0.3047,
                0.0762, 0.1008, 0.0132, 0.3758, 0.2296, 0.3336, 0.2858, 0.2104,
                -0.0910, -0.1189, -0.2210, 0.2689, 0.1088, 0.0491, 0.1579, 0.0549,
                -0.3700, 0.2646, 0.1506, 0.0211, 0.1600, 0.3239, 0.1369, 0.0138,
                0.1196, 0.2183, -0.0438, 0.3149, 0.1840, 0.2861, -0.1830, 0.2650,
                // Add 2024 when available
            ],
        }
    }

    /// Sample a random year's return
    pub fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> f64 {
        let idx = rng.gen_range(0..self.returns.len());
        self.returns[idx]
    }

    /// Sample n years with replacement
    pub fn sample_years<R: Rng + ?Sized>(&self, rng: &mut R, n: usize) -> Vec<f64> {
        (0..n).map(|_| self.sample(rng)).collect()
    }

    /// Block bootstrap: sample contiguous blocks to preserve autocorrelation
    pub fn block_bootstrap<R: Rng + ?Sized>(
        &self,
        rng: &mut R,
        n: usize,
        block_size: usize,
    ) -> Vec<f64> {
        let mut result = Vec::with_capacity(n);
        while result.len() < n {
            let start = rng.gen_range(0..self.returns.len());
            for i in 0..block_size {
                if result.len() >= n { break; }
                let idx = (start + i) % self.returns.len();
                result.push(self.returns[idx]);
            }
        }
        result.truncate(n);
        result
    }
}
```

### 4.2 Add Bootstrap Variant to ReturnProfile

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ReturnProfile {
    // ... existing variants ...

    /// Bootstrap from historical data
    Bootstrap {
        /// Historical returns to sample from
        history: HistoricalReturns,
        /// Optional block size for block bootstrap (1 = iid sampling)
        block_size: Option<usize>,
    },
}
```

**Note:** This requires `ReturnProfile` to derive `Clone` instead of `Copy` due to `HistoricalReturns` containing `Vec<f64>`.

### 4.3 Multi-Asset Bootstrap with Preserved Correlations

```rust
/// Bootstrap multiple assets together, preserving cross-asset correlations
pub struct MultiAssetHistory {
    /// Asset names
    pub names: Vec<String>,
    /// Start year
    pub start_year: i16,
    /// Returns matrix: returns[year][asset]
    pub returns: Vec<Vec<f64>>,
}

impl MultiAssetHistory {
    /// Sample the same year across all assets (preserves correlation)
    pub fn sample_year<R: Rng + ?Sized>(&self, rng: &mut R) -> Vec<f64> {
        let idx = rng.gen_range(0..self.returns.len());
        self.returns[idx].clone()
    }

    /// Sample n years together
    pub fn sample_years<R: Rng + ?Sized>(&self, rng: &mut R, n: usize) -> Vec<Vec<f64>> {
        (0..n).map(|_| self.sample_year(rng)).collect()
    }
}
```

---

## Phase 5: TUI Integration

### 5.1 Add New Profile Types to TUI

**File:** `crates/finplan/src/data/profiles_data.rs`

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ReturnProfileData {
    None,
    Fixed { rate: f64 },
    Normal { mean: f64, std_dev: f64 },
    LogNormal { mean: f64, std_dev: f64 },
    // NEW
    StudentT { mean: f64, scale: f64, df: f64 },
    RegimeSwitching {
        bull_mean: f64,
        bull_std_dev: f64,
        bear_mean: f64,
        bear_std_dev: f64,
        bull_to_bear_prob: f64,
        bear_to_bull_prob: f64,
    },
    // Bootstrap requires more complex handling (file reference?)
}
```

### 5.2 Update Profile Editor

**File:** `crates/finplan/src/actions/profile.rs`

Add form fields for new profile types:
- StudentT: mean, scale, degrees of freedom
- RegimeSwitching: bull params, bear params, transition probabilities

### 5.3 Add Preset Profile Selection

Add a "Load Preset" option in profile editor that offers:
- S&P 500 (Normal)
- S&P 500 (Student's t)
- S&P 500 (Regime Switching)
- US Total Bond
- International Stocks
- Small Cap
- REITs
- Money Market
- 60/40 Balanced

---

## Implementation Order

| Priority | Task | Complexity | Value |
|----------|------|------------|-------|
| 1 | 1.1 - Asset class presets | Low | High |
| 2 | 1.2 - Student's t distribution | Low | High |
| 3 | 1.3 - Inflation presets | Low | Medium |
| 4 | 5.1-5.3 - TUI integration (basic) | Medium | High |
| 5 | 2.1-2.2 - Regime switching | Medium | High |
| 6 | 3.1-3.3 - Correlated returns | High | Medium |
| 7 | 4.1-4.3 - Historical bootstrap | High | Medium |

---

## Status

- [x] Phase 1.1 - Asset class preset constants (11 asset classes with Fixed/Normal/LogNormal)
- [x] Phase 1.2 - Student's t distribution (variant added, presets for high-volatility assets)
- [x] Phase 1.3 - Inflation preset constants (US_HISTORICAL_FIXED, US_HISTORICAL_NORMAL, US_HISTORICAL_LOG_NORMAL)
- [x] Phase 2.1 - Regime switching variant (Box<ReturnProfile> for bull/bear, sample_sequence for stateful)
- [x] Phase 2.2 - Regime switching presets (sp500_regime_switching_normal, sp500_regime_switching_student_t)
- [x] Phase 3.1 - Correlation matrix structure (CorrelationMatrix with Cholesky decomposition)
- [x] Phase 3.2 - Correlated sampling in Market (Market::from_profiles_correlated)
- [x] Phase 3.3 - Default correlation presets (us_standard, us_extended, stock_bond, independent, near_perfect)
- [x] Phase 4.1 - Historical returns data structure (HistoricalReturns with sampling methods)
- [x] Phase 4.2 - Bootstrap variant (ReturnProfile::Bootstrap with i.i.d. and block bootstrap)
- [x] Phase 4.3 - Multi-asset bootstrap (MultiAssetHistory for correlated sampling)
- [x] Phase 5.1 - TUI profile data types (StudentT and RegimeSwitching variants)
- [x] Phase 5.2 - TUI profile editor updates (forms, pickers, display)
- [x] Phase 5.3 - TUI distribution visualization (histogram rendering for new types)

---

## Phase 1 Implementation Notes (2026-01-26)

### Phase 1.1 - Asset Class Presets
Added comprehensive return profile constants sourced from:
- Robert Shiller, Yale University (S&P 500 since 1871)
- Kenneth French Data Library, Dartmouth (Fama-French factors since 1926)
- Yahoo Finance (ETF data for recent history)

Asset classes covered:
- S&P 500 (97 years)
- US Small Cap (98 years)
- US T-Bills (92 years)
- US Long-Term Bonds (97 years)
- International Developed (34 years)
- Emerging Markets (33 years)
- REITs (22 years)
- Gold (26 years)
- US Aggregate Bonds (23 years)
- US Corporate Bonds (24 years)
- TIPS (23 years)

Also added `historical_returns` module with annual return arrays for future bootstrap sampling.

### Phase 1.2 - Student's t Distribution
Added `ReturnProfile::StudentT { mean, scale, df }` variant for fat-tailed returns.

**Key implementation details:**
- `mean`: Location parameter (expected return)
- `scale`: Scale parameter, computed as `std_dev * sqrt((df-2)/df)` to match target std_dev
- `df`: Degrees of freedom (lower = fatter tails, typically 4-6 for equities)

**Presets added:**
- SP_500_HISTORICAL_STUDENT_T (df=5)
- US_SMALL_CAP_HISTORICAL_STUDENT_T (df=5)
- EMERGING_MARKETS_HISTORICAL_STUDENT_T (df=5)

The Python data script (`scripts/fetch_historical_returns.py`) was updated to automatically
generate StudentT constants for high-volatility assets (std_dev > 5%).

### Phase 1.3 - Inflation Presets
Inflation constants were already present:
- US_HISTORICAL_FIXED (geometric mean: 3.43%)
- US_HISTORICAL_NORMAL (mean: 3.47%, std_dev: 2.79%)
- US_HISTORICAL_LOG_NORMAL

### Phase 2.1-2.2 - Regime Switching (2026-01-26)

Added `ReturnProfile::RegimeSwitching` variant for Markov regime-switching models.

**Design decisions:**
- Used `Box<ReturnProfile>` for bull/bear states instead of just mean/std_dev
- This allows any distribution type (Normal, StudentT, etc.) for each regime
- More flexible and composable, follows same pattern as nested EventTriggers
- Removed `Copy` derive from `ReturnProfile` (required for `Box`)

**Key implementation:**
```rust
RegimeSwitching {
    bull: Box<ReturnProfile>,      // Return profile during bull markets
    bear: Box<ReturnProfile>,      // Return profile during bear markets
    bull_to_bear_prob: f64,        // Annual transition probability
    bear_to_bull_prob: f64,        // Annual transition probability
}
```

**Two sampling modes:**
1. `sample()` - Stateless sampling using steady-state regime probabilities
   - P(bull) = bear_to_bull / (bull_to_bear + bear_to_bull)
   - Useful for one-off sampling
2. `sample_sequence()` - Stateful sampling that maintains regime across years
   - Starts in bull market, transitions based on probabilities
   - Used by `Market::from_profiles()` for proper regime clustering

**Presets added (as functions, not const):**
- `sp500_regime_switching_normal()` - Bull: 15%/12% | Bear: -8%/25%
- `sp500_regime_switching_student_t()` - Same with df=5 fat tails
- `regime_switching()` - Custom constructor helper

**Test coverage:**
- Stateless sampling produces mix of bull/bear returns
- Sequence sampling shows regime persistence (fewer sign-change "runs")
- Works with StudentT sub-profiles
- Custom profile construction
- Integration with Market::from_profiles

---

## Testing Strategy

1. **Unit tests for new distributions:**
   - Verify Student's t sampling produces expected moments
   - Verify regime switching transition probabilities
   - Verify Cholesky decomposition correctness
   - Verify correlated samples have expected correlation

2. **Integration tests:**
   - End-to-end simulation with each new profile type
   - Monte Carlo convergence tests

3. **Validation:**
   - Compare simulated distributions to historical data
   - Verify correlation preservation in multi-asset scenarios

---

## Notes

- Student's t with df=5 approximates equity return distributions well (kurtosis ~6 vs ~3 for normal)
- Regime switching captures the clustering of good/bad years
- Correlation becomes more important with diversified portfolios
- Historical bootstrap is non-parametric "gold standard" but requires good data
- Consider lazy-loading historical data to avoid bloating binary size

---

# Phase 5: TUI Integration for StudentT and RegimeSwitching

## Design Decisions (2026-01-26)

Based on user input, the following design choices were made:

1. **RegimeSwitching UI**: Presets only
   - No custom configuration of bull/bear profiles in the TUI
   - Offer predefined presets: "S&P 500 Regime Switching (Normal)", "S&P 500 Regime Switching (Student-t)"
   - Users who need custom regime-switching can edit YAML directly

2. **StudentT degrees of freedom**: Descriptive dropdown choices
   - "Moderate tails (df=5)" - default, most common for equities
   - "Fat tails (df=3)" - for higher volatility / extreme events
   - "Very fat tails (df=2)" - maximum fat-tail effect

3. **Distribution visualization**: Histogram for both new types
   - StudentT: Render similar to Normal but with visible fat tails
   - RegimeSwitching: Bimodal overlay showing bull and bear distributions

---

## Implementation Plan

### Phase 5.1: Data Layer (`profiles_data.rs`)

Add new variants to `ReturnProfileData`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ReturnProfileData {
    None,
    Fixed { rate: f64 },
    Normal { mean: f64, std_dev: f64 },
    LogNormal { mean: f64, std_dev: f64 },
    // NEW
    StudentT { mean: f64, scale: f64, df: f64 },
    // RegimeSwitching stored with explicit parameters for presets
    RegimeSwitching {
        bull_mean: f64,
        bull_std_dev: f64,
        bear_mean: f64,
        bear_std_dev: f64,
        bull_to_bear_prob: f64,
        bear_to_bull_prob: f64,
    },
}
```

Update conversion methods:
- `to_return_profile()` - convert to core type
- `From<&ReturnProfile>` - convert from core type

**Note:** For RegimeSwitching, we only support Normal distributions in bull/bear to keep the data layer simple. The TUI will offer presets but store the flattened parameters.

### Phase 5.2: Context Layer (`context.rs`)

Add to `ProfileTypeContext`:

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum ProfileTypeContext {
    None,
    Fixed,
    Normal,
    LogNormal,
    // NEW
    StudentT,
    RegimeSwitchingNormal,    // Preset: Normal distributions
    RegimeSwitchingStudentT,  // Preset: Student-t distributions
}
```

Update `FromStr` and `display_name()`:
- "Student's t" / "StudentT" -> `StudentT`
- "Regime Switching (Normal)" -> `RegimeSwitchingNormal`
- "Regime Switching (Student-t)" -> `RegimeSwitchingStudentT`

### Phase 5.3: Profile Actions (`profile.rs`)

Update `handle_profile_type_pick()` with new forms:

**StudentT form fields:**
- Name (text)
- Description (text)
- Mean (percentage)
- Scale (percentage) - computed from std_dev
- Tail Behavior (picker): "Moderate tails", "Fat tails", "Very fat tails"

**RegimeSwitching preset forms:**
Pre-fill with historical S&P 500 values:
- Name (text)
- Description (text)
- (Read-only display of preset parameters)

Update `handle_create_profile()` and `handle_edit_profile()` to handle new types.

### Phase 5.4: Type Picker (`portfolio_profiles.rs`)

Update type picker (line ~2026):

```rust
let types = vec![
    "None".to_string(),
    "Fixed Rate".to_string(),
    "Normal Distribution".to_string(),
    "Log-Normal Distribution".to_string(),
    "Student's t Distribution".to_string(),
    "Regime Switching (Normal)".to_string(),
    "Regime Switching (Student-t)".to_string(),
];
```

### Phase 5.5: Display Updates (`portfolio_profiles.rs`)

**`format_profile_type()`:**
```rust
ReturnProfileData::StudentT { .. } => "Student's t Distribution".to_string(),
ReturnProfileData::RegimeSwitching { .. } => "Regime Switching".to_string(),
```

**Profile details panel (line ~718):**
- StudentT: Show Mean, Scale, df, Tail Behavior
- RegimeSwitching: Show Bull/Bear params, transition probs

**`create_profile_edit_form()`:**
- StudentT: Mean, Scale, Tail Behavior picker
- RegimeSwitching: Read-only params (presets not customizable in UI)

### Phase 5.6: Distribution Visualization (`portfolio_profiles.rs`)

**`render_distribution_inline()`:**

StudentT visualization:
- Similar to Normal but with extended tails
- Use Student's t PDF formula for histogram
- Show legend indicating df value

RegimeSwitching visualization:
- Bimodal histogram: two overlapping bell curves
- Bull regime in green, Bear regime in red
- Alpha/transparency to show overlap
- Legend showing regime names

### Phase 5.7: Preset Shortcuts

Consider adding new keyboard shortcuts:
- '5' = StudentT preset (S&P 500 historical)
- '6' = RegimeSwitching Normal preset
- '7' = RegimeSwitching StudentT preset

Or consolidate into a preset picker modal for cleaner UX.

---

## Files to Modify

| File | Changes |
|------|---------|
| `crates/finplan/src/data/profiles_data.rs` | Add StudentT, RegimeSwitching variants |
| `crates/finplan/src/state/context.rs` | Add ProfileTypeContext variants |
| `crates/finplan/src/actions/profile.rs` | Form creation and handling |
| `crates/finplan/src/screens/portfolio_profiles.rs` | Display, picker, visualization |

---

## Implementation Order

1. **Phase 5.1** - Data layer (profiles_data.rs)
2. **Phase 5.2** - Context layer (context.rs)
3. **Phase 5.3** - Profile actions (profile.rs)
4. **Phase 5.4** - Type picker update
5. **Phase 5.5** - Display updates (format, details, edit form)
6. **Phase 5.6** - Distribution visualization
7. **Phase 5.7** - Preset shortcuts (optional)

---

## Status

- [x] Phase 5.1 - Data layer (profiles_data.rs - StudentT and RegimeSwitching variants)
- [x] Phase 5.2 - Context layer (context.rs - ProfileTypeContext variants)
- [x] Phase 5.3 - Profile actions (profile.rs - form creation and handling)
- [x] Phase 5.4 - Type picker (portfolio_profiles.rs - updated picker list)
- [x] Phase 5.5 - Display updates (format_profile_type, details panel, edit forms)
- [x] Phase 5.6 - Distribution visualization (StudentT and RegimeSwitching histograms)
- [x] Phase 5.7 - Preset shortcuts ('5' for StudentT, '6' for RegimeSwitching)

### Implementation Notes (2026-01-26)

**Files Modified:**
1. `crates/finplan/src/data/profiles_data.rs` - Added StudentT and RegimeSwitching variants with conversion logic
2. `crates/finplan/src/state/context.rs` - Added ProfileTypeContext variants (StudentT, RegimeSwitchingNormal, RegimeSwitchingStudentT)
3. `crates/finplan/src/actions/profile.rs` - Form creation with select field for tail behavior, profile creation/editing logic
4. `crates/finplan/src/screens/portfolio_profiles.rs` - Type picker, display formatting, edit forms, distribution rendering

**StudentT Implementation:**
- Form fields: Name, Description, Mean, Std Dev, Tail Behavior (dropdown)
- Tail behavior options: "Moderate tails (df=5)", "Fat tails (df=3)", "Very fat tails (df=2)"
- Scale computed from std_dev: `scale = std_dev * sqrt((df-2)/df)` for df > 2
- Visualization: Histogram with magenta coloring, wider range (±4σ) to show fat tails

**RegimeSwitching Implementation:**
- Preset-only (no custom configuration in UI)
- Two presets: "Regime Switching (Normal)" and "Regime Switching (Student-t)"
- Parameters are read-only in the edit form
- Visualization: Bimodal histogram showing bull (green) and bear (red) distributions
- **Conservative parameters (updated 2026-01-26):**
  - Bull: 12% mean, 12% std dev (was 15% mean)
  - Bear: -5% mean, 22% std dev (was -8% mean, 25% std)
  - Bull->Bear: 15% (~7 year cycles, was 12%/~8 years)
  - Bear->Bull: 40% (~2.5 year cycles, was 50%/~2 years)
  - Expected return: ~7.4% (was ~10.5%, now comparable to LogNormal ~7%)

**Keyboard Shortcuts:**
- '5' = Apply StudentT preset (S&P 500 historical with df=5)
- '6' = Apply RegimeSwitching preset (S&P 500 bull/bear model, conservative)

---

## Phase 3 Implementation Notes (2026-01-26)

### Phase 3.1-3.3 - Correlated Returns

**New file:** `crates/finplan_core/src/model/correlation.rs`

Added `CorrelationMatrix` and `CholeskyDecomp` types for multi-asset correlated return simulation.

**Key Components:**

1. **CorrelationMatrix**
   - Stores correlations in compact lower-triangular format
   - Validates coefficients are in [-1, 1]
   - Provides `cholesky()` for decomposition

2. **CholeskyDecomp**
   - Implements L * L^T = correlation_matrix decomposition
   - `correlate()` transforms independent N(0,1) samples to correlated samples

3. **Preset Correlation Matrices:**
   - `us_standard(us_stock, intl_stock, us_bond, reits)` - 4-asset historical correlations
   - `us_extended(...)` - 7-asset matrix including emerging markets, gold, TIPS
   - `stock_bond(stock, bond, correlation)` - Simple 2-asset case
   - `independent(profiles)` - Zero correlation (all assets independent)
   - `near_perfect(profiles)` - 0.9999 correlation (for testing)

4. **Market::from_profiles_correlated()**
   - Generates returns with correlation structure preserved
   - Supports Normal and Fixed profiles in correlation matrix
   - Other profile types (StudentT, RegimeSwitching) sampled independently
   - Profiles not in correlation matrix sampled independently

**Error Handling:**
- `CorrelationError::NotPositiveDefinite` - Invalid correlation structure
- `CorrelationError::InvalidCoefficient` - Out of range [-1, 1]
- `CorrelationError::UnsupportedProfileType` - Non-Normal in correlation matrix

**Historical Correlation Values (us_standard):**
- US Stock / Intl Stock: 0.75
- US Stock / US Bond: 0.05
- US Stock / REITs: 0.60
- Intl Stock / US Bond: 0.10
- Intl Stock / REITs: 0.55
- US Bond / REITs: 0.15

**Usage Example:**
```rust
let us_stock_id = ReturnProfileId(1);
let bond_id = ReturnProfileId(2);

let mut profiles = HashMap::new();
profiles.insert(us_stock_id, ReturnProfile::Normal { mean: 0.10, std_dev: 0.18 });
profiles.insert(bond_id, ReturnProfile::Normal { mean: 0.05, std_dev: 0.06 });

let correlation = CorrelationMatrix::stock_bond(us_stock_id, bond_id, 0.1).unwrap();

let market = Market::from_profiles_correlated(
    &mut rng,
    30,
    &InflationProfile::Fixed(0.02),
    &profiles,
    &correlation,
    &assets,
).unwrap();
```

---

## Phase 4 Implementation Notes (2026-01-26)

### Phase 4.1 - Historical Returns Data Structure

Added `HistoricalReturns` struct for bootstrap sampling:

```rust
pub struct HistoricalReturns {
    pub name: String,
    pub start_year: i16,
    pub returns: Vec<f64>,
}
```

**Key features:**
- `new()` and `from_static()` constructors
- `sample()` - Single random year (i.i.d. with replacement)
- `sample_years()` - Multiple years i.i.d.
- `block_bootstrap()` - Contiguous blocks (circular) for autocorrelation preservation
- `statistics()` - Computes arithmetic/geometric mean, std_dev, min, max

**Preset constructors** (11 asset classes):
- `sp500()`, `us_small_cap()`, `us_tbills()`, `us_long_bonds()`
- `intl_developed()`, `emerging_markets()`, `reits()`, `gold()`
- `us_agg_bonds()`, `us_corporate_bonds()`, `tips()`

### Phase 4.2 - Bootstrap Variant

Added `ReturnProfile::Bootstrap` variant:

```rust
Bootstrap {
    history: HistoricalReturns,
    block_size: Option<usize>,  // None = i.i.d., Some(n) = block bootstrap
}
```

**Sampling behavior:**
- `sample()` - Random year from history (ignores block_size)
- `sample_sequence()` - Uses block bootstrap if block_size > 1, else i.i.d.

**Preset functions** (12 total):
- `sp500_bootstrap()`, `sp500_bootstrap_block5()` (5-year blocks)
- `us_small_cap_bootstrap()`, `us_tbills_bootstrap()`, `us_long_bonds_bootstrap()`
- `intl_developed_bootstrap()`, `emerging_markets_bootstrap()`
- `reits_bootstrap()`, `gold_bootstrap()`
- `us_agg_bonds_bootstrap()`, `us_corporate_bonds_bootstrap()`, `tips_bootstrap()`
- `bootstrap()` - Custom constructor

### Phase 4.3 - Multi-Asset Bootstrap

Added `MultiAssetHistory` for correlated multi-asset bootstrap:

```rust
pub struct MultiAssetHistory {
    pub names: Vec<String>,
    pub start_year: i16,
    pub returns: Vec<Vec<f64>>,  // [year][asset]
}
```

**Key features:**
- `sample_year()` - Same historical year for all assets (preserves cross-asset correlation)
- `sample_years()` - Multiple years preserving within-year correlation
- `block_bootstrap()` - Block bootstrap preserving both auto and cross-correlation

**Note:** `MultiAssetHistory` is implemented but not yet integrated into `Market::from_profiles()`.
It's available for direct use in custom simulations or future TUI integration.

**Usage Example:**
```rust
// Create bootstrap profile from S&P 500 historical data
let profile = ReturnProfile::sp500_bootstrap();

// Or with 5-year blocks to preserve momentum effects
let profile_block = ReturnProfile::sp500_bootstrap_block5();

// Custom bootstrap from your own data
let custom = ReturnProfile::bootstrap(
    HistoricalReturns::new("Custom Index", 2000, vec![0.10, -0.05, 0.15, ...]),
    Some(3),  // 3-year blocks
);

// Use in simulation
let returns = profile.sample_sequence(&mut rng, 30)?;
```

---

## Returns Model Enhancement - Complete

All phases of the Returns Model Enhancement plan are now complete:

| Phase | Description | Status |
|-------|-------------|--------|
| 1.1 | Asset class preset constants | ✓ Complete |
| 1.2 | Student's t distribution | ✓ Complete |
| 1.3 | Inflation preset constants | ✓ Complete |
| 2.1 | Regime-switching variant | ✓ Complete |
| 2.2 | Regime-switching presets | ✓ Complete |
| 3.1 | Correlation matrix structure | ✓ Complete |
| 3.2 | Correlated sampling in Market | ✓ Complete |
| 3.3 | Default correlation presets | ✓ Complete |
| 4.1 | Historical returns data structure | ✓ Complete |
| 4.2 | Bootstrap variant | ✓ Complete |
| 4.3 | Multi-asset bootstrap | ✓ Complete |
| 5.1 | TUI profile data types | ✓ Complete |
| 5.2 | TUI profile editor updates | ✓ Complete |
| 5.3 | TUI distribution visualization | ✓ Complete |

**Future work:**
- `Market::from_profiles_bootstrap()` for multi-asset correlated bootstrap
- Additional historical data sources (international, sector indices)

---

# Phase 6: Historical Bootstrap TUI Integration

## Overview

Add a scenario-level toggle between "Parametric" and "Historical" returns modes:
- **Parametric mode**: Current behavior - user manually creates profiles (Normal, StudentT, etc.)
- **Historical mode**: Pre-populated Bootstrap profiles from historical data, user only controls block size and asset mappings
- **Both modes persist**: Asset mappings for each mode are stored separately, allowing easy switching without losing work
- **Profiles generated at runtime**: Historical profiles are not serialized - generated from HISTORICAL_PRESETS constant

## Files to Modify

| File | Changes |
|------|---------|
| `crates/finplan/src/data/parameters_data.rs` | Add `ReturnsMode` enum with block_size |
| `crates/finplan/src/data/app_data.rs` | Add `historical_asset_mappings` field |
| `crates/finplan/src/data/profiles_data.rs` | Add `Bootstrap` variant to `ReturnProfileData` |
| `crates/finplan/src/data/ticker_profiles.rs` | Add historical preset key mappings |
| `crates/finplan/src/data/convert.rs` | Generate historical profiles at runtime, select mappings based on mode |
| `crates/finplan/src/screens/portfolio_profiles.rs` | Mode toggle ('h' key), mode-aware rendering, block size picker |
| `crates/finplan/src/actions/profile.rs` | Handle mode-aware asset mapping storage |

## Implementation Order

1. `parameters_data.rs` - Add `ReturnsMode` enum and `historical_block_size`
2. `profiles_data.rs` - Add `Bootstrap` variant with conversion
3. `app_data.rs` - Add `historical_asset_mappings` field only
4. `ticker_profiles.rs` - Add `get_historical_preset_key()` and `get_historical_suggestion()`
5. `convert.rs` - Generate historical profiles at runtime, select mappings based on mode
6. `portfolio_profiles.rs` - Mode toggle, mode-aware rendering, block size picker, histogram
7. `profile.rs` - Handle mode-aware asset mapping storage

## Status

- [ ] Phase 6.1 - ReturnsMode enum and historical_block_size in parameters_data.rs
- [ ] Phase 6.2 - Bootstrap variant in profiles_data.rs
- [ ] Phase 6.3 - historical_asset_mappings in app_data.rs
- [ ] Phase 6.4 - Historical preset key mappings in ticker_profiles.rs
- [ ] Phase 6.5 - Runtime profile generation in convert.rs
- [ ] Phase 6.6 - Mode toggle and UI in portfolio_profiles.rs
- [ ] Phase 6.7 - Mode-aware asset mapping storage

---

# Historical Bootstrap Inflation (2026-01-27)

## Overview

Added support for historical inflation bootstrap sampling in Historical returns mode. When Historical mode is selected, inflation is now automatically sampled from real US CPI historical data using the same block size as the returns profiles.

## Changes Made

### finplan_core

**crates/finplan_core/src/model/market.rs:**
- Added `HistoricalInflation` struct (similar to `HistoricalReturns`) with:
  - `sample()` - single year i.i.d. sampling
  - `sample_years()` - multiple years i.i.d. sampling
  - `block_bootstrap()` - contiguous block sampling for autocorrelation preservation
  - `statistics()` - compute mean, std_dev, etc.
  - `us_cpi()` - preset constructor for US CPI data
- Added `Bootstrap` variant to `InflationProfile` enum:
  ```rust
  Bootstrap {
      history: HistoricalInflation,
      block_size: Option<usize>,
  }
  ```
- Added `sample_sequence()` method to `InflationProfile` for block bootstrap support
- Added `us_historical_bootstrap(block_size)` constructor to `InflationProfile`
- Added `historical_inflation` module with US CPI annual rates (1948-2025, 78 years)
- Updated `Market::from_profiles()` to use `sample_sequence` for inflation
- Changed `InflationProfile` from `Copy` to `Clone` (required for `Bootstrap` variant)

**crates/finplan_core/src/error.rs:**
- Added `MarketError::EmptyHistoricalData` variant

**crates/finplan_core/src/model/mod.rs:**
- Exported `HistoricalInflation`

### finplan (TUI)

**crates/finplan/src/data/convert.rs:**
- Updated `convert_parameters()` to automatically use `InflationProfile::us_historical_bootstrap(block_size)` when Historical returns mode is selected

## Data Source

US CPI Inflation data (All Urban Consumers):
- Source: FRED CPIAUCSL
- Period: 1948-2025 (78 years)
- Arithmetic mean: 3.47%
- Geometric mean: 3.43%
- Standard deviation: 2.79%

## Behavior

- **Historical Mode**: Inflation is automatically sampled from historical US CPI data using the same block size as returns (configured via the block size picker)
- **Parametric Mode**: Inflation uses the user-configured profile (Fixed, Normal, LogNormal, or USHistorical parametric)

This ensures that in Historical mode, both returns and inflation are sampled consistently using the same methodology and block size, preserving temporal correlations between market returns and inflation.
