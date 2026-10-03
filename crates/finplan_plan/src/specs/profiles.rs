//! Return and inflation profile specs: the distribution shapes and the
//! create/update bodies.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::compile::HISTORY_PRESETS;
use crate::error::{PlanError, PlanResult};
use crate::graph::DistributionRow;

/// What a second return profile of the same name is refused with.
pub const NAME_TAKEN: &str = "a return profile with that name already exists";

/// What a second inflation profile of the same name is refused with.
pub const INFLATION_NAME_TAKEN: &str = "an inflation profile with that name already exists";

/// What kind of holding a profile describes.
///
/// A profile's *name* is the user's — renamed, translated, duplicated — so it
/// cannot be what a client matches on when it decides which profile a ticker
/// belongs to. This is the stable half: a stored fact about what the assumption
/// is for, which survives everything that can happen to a name.
///
/// Null on a profile is the ordinary state and not a defect. It means nobody
/// has said what the profile is for, so nothing picks it automatically — which
/// is exactly right for a profile someone built by hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum AssetClass {
    UsEquity,
    UsSmallCap,
    GlobalEquity,
    IntlEquity,
    Bonds,
    Reit,
    Cash,
    Commodity,
    Crypto,
    Balanced,
}

impl AssetClass {
    const ALL: [AssetClass; 10] = [
        AssetClass::UsEquity,
        AssetClass::UsSmallCap,
        AssetClass::GlobalEquity,
        AssetClass::IntlEquity,
        AssetClass::Bonds,
        AssetClass::Reit,
        AssetClass::Cash,
        AssetClass::Commodity,
        AssetClass::Crypto,
        AssetClass::Balanced,
    ];

    /// Stored as its own name, so the column reads as itself in a query.
    pub fn as_str(self) -> &'static str {
        match self {
            AssetClass::UsEquity => "UsEquity",
            AssetClass::UsSmallCap => "UsSmallCap",
            AssetClass::GlobalEquity => "GlobalEquity",
            AssetClass::IntlEquity => "IntlEquity",
            AssetClass::Bonds => "Bonds",
            AssetClass::Reit => "Reit",
            AssetClass::Cash => "Cash",
            AssetClass::Commodity => "Commodity",
            AssetClass::Crypto => "Crypto",
            AssetClass::Balanced => "Balanced",
        }
    }

    /// Text that names no class reads as none rather than as an error: a column
    /// written by a newer build should leave an older one working.
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.as_str() == text)
    }
}

/// One node of a [`DistributionSpec`] as the columns of a `distributions` row.
///
/// `regimes` holds the nested bull and bear distributions of a regime-switching
/// node: they are rows of their own, which the row for this node points at, so
/// a sink places them first.
#[derive(Debug, Default)]
pub struct DistributionColumns<'a> {
    pub kind: &'static str,
    pub rate: Option<f64>,
    pub mean: Option<f64>,
    pub std_dev: Option<f64>,
    pub scale: Option<f64>,
    pub df: Option<f64>,
    pub bull_to_bear_prob: Option<f64>,
    pub bear_to_bull_prob: Option<f64>,
    pub history_preset: Option<&'a str>,
    pub block_size: Option<i64>,
    pub regimes: Option<(&'a DistributionSpec, &'a DistributionSpec)>,
}

impl DistributionColumns<'_> {
    /// The row these columns make, given its own id and those of the nested
    /// regimes' rows (placed first, if there are any).
    pub fn into_row(self, id: i64, bull_id: Option<i64>, bear_id: Option<i64>) -> DistributionRow {
        DistributionRow {
            id,
            kind: self.kind.to_string(),
            rate: self.rate,
            mean: self.mean,
            std_dev: self.std_dev,
            scale: self.scale,
            df: self.df,
            bull_id,
            bear_id,
            bull_to_bear_prob: self.bull_to_bear_prob,
            bear_to_bull_prob: self.bear_to_bull_prob,
            history_preset: self.history_preset.map(str::to_string),
            block_size: self.block_size,
        }
    }
}

/// The distribution shapes a profile can take. `RegimeSwitching` nests two more
/// distributions, so this mirrors the recursive Rust enum.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "kind")]
#[ts(export)]
pub enum DistributionSpec {
    None,
    Fixed {
        rate: f64,
    },
    Normal {
        mean: f64,
        std_dev: f64,
    },
    LogNormal {
        mean: f64,
        std_dev: f64,
    },
    StudentT {
        mean: f64,
        scale: f64,
        df: f64,
    },
    RegimeSwitching {
        bull: Box<DistributionSpec>,
        bear: Box<DistributionSpec>,
        bull_to_bear_prob: f64,
        bear_to_bull_prob: f64,
    },
    Bootstrap {
        preset: String,
        #[serde(default)]
        block_size: Option<i64>,
    },
}

impl DistributionSpec {
    /// What the `distributions` table's CHECKs and the preset list would
    /// refuse, as a bad request rather than a database error. `depth` counts
    /// nested regimes.
    pub fn validate(&self, depth: usize) -> PlanResult<()> {
        if depth > 8 {
            return Err(PlanError::invalid(
                "distribution nests too deeply; regime models may not be recursive beyond 8 levels",
            ));
        }
        let finite = |values: &[f64]| {
            if values.iter().all(|v| v.is_finite()) {
                Ok(())
            } else {
                Err(PlanError::invalid("distribution figures must be finite"))
            }
        };
        match self {
            DistributionSpec::None => {}
            DistributionSpec::Fixed { rate } => finite(&[*rate])?,
            DistributionSpec::Normal { mean, std_dev }
            | DistributionSpec::LogNormal { mean, std_dev } => {
                finite(&[*mean, *std_dev])?;
                if *std_dev < 0.0 {
                    return Err(PlanError::invalid("std_dev cannot be negative"));
                }
            }
            DistributionSpec::StudentT { mean, scale, df } => {
                finite(&[*mean, *scale, *df])?;
                if *df <= 0.0 {
                    return Err(PlanError::invalid("df must be positive"));
                }
            }
            DistributionSpec::RegimeSwitching {
                bull,
                bear,
                bull_to_bear_prob,
                bear_to_bull_prob,
            } => {
                for p in [bull_to_bear_prob, bear_to_bull_prob] {
                    if !(0.0..=1.0).contains(p) {
                        return Err(PlanError::invalid(
                            "regime switching probabilities are between 0 and 1",
                        ));
                    }
                }
                bull.validate(depth + 1)?;
                bear.validate(depth + 1)?;
            }
            DistributionSpec::Bootstrap { preset, block_size } => {
                if !HISTORY_PRESETS.contains(&preset.as_str()) {
                    return Err(PlanError::invalid(format!(
                        "unknown history preset '{preset}'; expected one of {}",
                        HISTORY_PRESETS.join(", ")
                    )));
                }
                if block_size.is_some_and(|b| b < 1) {
                    return Err(PlanError::invalid("block_size must be at least 1"));
                }
            }
        }
        Ok(())
    }

    /// The `distributions` row this spec is, with its two nested regimes (if
    /// any) left for the caller to place first: the pure half of writing a
    /// distribution, which the server's SQL insert runs for each node of the
    /// tree. `depth` counts nested regimes.
    pub fn columns(&self, depth: usize) -> PlanResult<DistributionColumns<'_>> {
        if depth > 8 {
            return Err(PlanError::invalid(
                "distribution nests too deeply; regime models may not be recursive beyond 8 levels",
            ));
        }

        let mut columns = DistributionColumns::default();
        match self {
            DistributionSpec::None => columns.kind = "None",
            DistributionSpec::Fixed { rate } => {
                columns.kind = "Fixed";
                columns.rate = Some(*rate);
            }
            DistributionSpec::Normal { mean, std_dev } => {
                columns.kind = "Normal";
                columns.mean = Some(*mean);
                columns.std_dev = Some(*std_dev);
            }
            DistributionSpec::LogNormal { mean, std_dev } => {
                columns.kind = "LogNormal";
                columns.mean = Some(*mean);
                columns.std_dev = Some(*std_dev);
            }
            DistributionSpec::StudentT { mean, scale, df } => {
                columns.kind = "StudentT";
                columns.mean = Some(*mean);
                columns.scale = Some(*scale);
                columns.df = Some(*df);
            }
            DistributionSpec::RegimeSwitching {
                bull,
                bear,
                bull_to_bear_prob,
                bear_to_bull_prob,
            } => {
                columns.kind = "RegimeSwitching";
                columns.bull_to_bear_prob = Some(*bull_to_bear_prob);
                columns.bear_to_bull_prob = Some(*bear_to_bull_prob);
                columns.regimes = Some((bull, bear));
            }
            DistributionSpec::Bootstrap { preset, block_size } => {
                if !HISTORY_PRESETS.contains(&preset.as_str()) {
                    return Err(PlanError::invalid(format!(
                        "unknown history preset '{preset}'; expected one of {}",
                        HISTORY_PRESETS.join(", ")
                    )));
                }
                columns.kind = "Bootstrap";
                columns.history_preset = Some(preset.as_str());
                columns.block_size = *block_size;
            }
        }
        Ok(columns)
    }
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct CreateProfile {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub asset_class: Option<AssetClass>,
    pub distribution: DistributionSpec,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct UpdateProfile {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// Doubly optional: absent leaves the class alone, an explicit null
    /// unclassifies the profile. Every other field here reads absent as
    /// "unchanged", which would otherwise make unclassifying unsayable.
    #[serde(default, deserialize_with = "crate::specs::double_option")]
    #[ts(optional)]
    pub asset_class: Option<Option<AssetClass>>,
    #[serde(default)]
    pub distribution: Option<DistributionSpec>,
}

/// those here rather than at compile time.
pub fn check_inflation_kind(spec: &DistributionSpec) -> PlanResult<()> {
    match spec {
        DistributionSpec::StudentT { .. } | DistributionSpec::RegimeSwitching { .. } => {
            Err(PlanError::invalid(
                "inflation profiles support None, Fixed, Normal, LogNormal or Bootstrap",
            ))
        }
        _ => Ok(()),
    }
}
