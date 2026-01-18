use finplan_core::model::ReturnProfile;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ReturnProfileTag(pub String);

/// YAML-friendly representation of a return profile
/// Uses explicit field names to avoid serde_saphyr issues with tagged newtype variants
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ReturnProfileData {
    None,
    Fixed { rate: f64 },
    Normal { mean: f64, std_dev: f64 },
    LogNormal { mean: f64, std_dev: f64 },
}

impl ReturnProfileData {
    pub fn to_return_profile(&self) -> ReturnProfile {
        match self {
            ReturnProfileData::None => ReturnProfile::None,
            ReturnProfileData::Fixed { rate } => ReturnProfile::Fixed(*rate),
            ReturnProfileData::Normal { mean, std_dev } => ReturnProfile::Normal {
                mean: *mean,
                std_dev: *std_dev,
            },
            ReturnProfileData::LogNormal { mean, std_dev } => ReturnProfile::LogNormal {
                mean: *mean,
                std_dev: *std_dev,
            },
        }
    }
}

impl From<&ReturnProfile> for ReturnProfileData {
    fn from(profile: &ReturnProfile) -> Self {
        match profile {
            ReturnProfile::None => ReturnProfileData::None,
            ReturnProfile::Fixed(rate) => ReturnProfileData::Fixed { rate: *rate },
            ReturnProfile::Normal { mean, std_dev } => ReturnProfileData::Normal {
                mean: *mean,
                std_dev: *std_dev,
            },
            ReturnProfile::LogNormal { mean, std_dev } => ReturnProfileData::LogNormal {
                mean: *mean,
                std_dev: *std_dev,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileData {
    pub name: ReturnProfileTag,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(flatten)]
    pub profile: ReturnProfileData,
}
