//! Asset Builder DSL
//!
//! Provides a fluent API for defining assets with return profiles.
//!
//! # Examples
//!
//! ```ignore
//! use finplan::config::AssetBuilder;
//! use finplan::model::ReturnProfile;
//!
//! // Define an equity asset
//! let vtsax = AssetBuilder::new("VTSAX")
//!     .price(100.0)
//!     .return_profile(ReturnProfile::Fixed(0.10))
//!     .description("Vanguard Total Stock Market Index")
//!     .build();
//!
//! // Define a bond fund
//! let bnd = AssetBuilder::new("BND")
//!     .price(50.0)
//!     .return_profile(ReturnProfile::Fixed(0.04))
//!     .description("Vanguard Total Bond Market Index")
//!     .build();
//! ```

use crate::model::{AssetId, ReturnProfile, ReturnProfileId};

/// Builder for defining an asset type (ticker/fund)
///
/// Assets are defined separately from account positions. You define the asset
/// (name, price, return profile), then reference it when adding positions to accounts.
#[derive(Debug, Clone)]
pub struct AssetBuilder {
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    pub(crate) initial_price: f64,
    pub(crate) return_profile: Option<ReturnProfile>,
    pub(crate) return_profile_name: Option<String>,
    pub(crate) tracking_error: Option<f64>,
}

/// A fully defined asset ready to be added to the simulation
#[derive(Debug, Clone)]
pub struct AssetDefinition {
    pub name: String,
    pub description: Option<String>,
    pub initial_price: f64,
    pub return_profile: ReturnProfile,
    pub return_profile_name: Option<String>,
    pub tracking_error: Option<f64>,
}

impl AssetBuilder {
    /// Create a new asset builder with the given name/ticker
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
            initial_price: 1.0, // Default $1.00 per unit
            return_profile: None,
            return_profile_name: None,
            tracking_error: None,
        }
    }

    // =========================================================================
    // Common Asset Presets
    // =========================================================================

    /// Create a US total stock market fund (like VTSAX/VTI)
    #[must_use]
    pub fn us_total_market(name: impl Into<String>) -> Self {
        Self::new(name)
            .description("US Total Stock Market Index")
            .return_profile(ReturnProfile::Fixed(0.10)) // ~10% historical average
    }

    /// Create an S&P 500 index fund
    #[must_use]
    pub fn sp500(name: impl Into<String>) -> Self {
        Self::new(name)
            .description("S&P 500 Index")
            .return_profile(ReturnProfile::Fixed(0.10))
    }

    /// Create an international stock fund
    #[must_use]
    pub fn international_stock(name: impl Into<String>) -> Self {
        Self::new(name)
            .description("International Stock Index")
            .return_profile(ReturnProfile::Fixed(0.08))
    }

    /// Create a total bond market fund
    #[must_use]
    pub fn total_bond(name: impl Into<String>) -> Self {
        Self::new(name)
            .description("Total Bond Market Index")
            .return_profile(ReturnProfile::Fixed(0.04))
    }

    /// Create a money market / high-yield savings asset
    #[must_use]
    pub fn money_market(name: impl Into<String>) -> Self {
        Self::new(name)
            .description("Money Market / Cash Equivalent")
            .return_profile(ReturnProfile::Fixed(0.04))
    }

    /// Create real estate investment (like a REIT or property)
    #[must_use]
    pub fn real_estate(name: impl Into<String>) -> Self {
        Self::new(name)
            .description("Real Estate Investment")
            .return_profile(ReturnProfile::Fixed(0.06))
    }

    // =========================================================================
    // Builder Methods
    // =========================================================================

    /// Set a description for this asset
    #[must_use]
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set the initial price per unit
    #[must_use]
    pub fn price(mut self, price: f64) -> Self {
        self.initial_price = price;
        self
    }

    /// Set the return profile for this asset
    #[must_use]
    pub fn return_profile(mut self, profile: ReturnProfile) -> Self {
        self.return_profile = Some(profile);
        self
    }

    /// Set a fixed annual return rate
    #[must_use]
    pub fn fixed_return(mut self, rate: f64) -> Self {
        self.return_profile = Some(ReturnProfile::Fixed(rate));
        self
    }

    /// Set the return profile by referencing a named profile
    #[must_use]
    pub fn return_profile_name(mut self, name: impl Into<String>) -> Self {
        self.return_profile_name = Some(name.into());
        self
    }

    /// Set per-asset tracking error (annualized standard deviation).
    /// Adds N(0, tracking_error) noise to each year's return from the base profile,
    /// modeling idiosyncratic risk for assets that don't perfectly track their benchmark.
    #[must_use]
    pub fn tracking_error(mut self, te: f64) -> Self {
        self.tracking_error = Some(te);
        self
    }

    /// Build the asset definition
    #[must_use]
    pub fn build(self) -> AssetDefinition {
        AssetDefinition {
            name: self.name,
            description: self.description,
            initial_price: self.initial_price,
            return_profile: self.return_profile.unwrap_or(ReturnProfile::Fixed(0.0)),
            return_profile_name: self.return_profile_name,
            tracking_error: self.tracking_error,
        }
    }
}

/// A registered asset in the simulation with assigned IDs
#[derive(Debug, Clone)]
pub struct RegisteredAsset {
    pub asset_id: AssetId,
    pub return_profile_id: ReturnProfileId,
    pub name: String,
    pub initial_price: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_asset_builder_basic() {
        let asset = AssetBuilder::new("VTSAX")
            .price(100.0)
            .fixed_return(0.10)
            .description("Total Stock Market")
            .build();

        assert_eq!(asset.name, "VTSAX");
        assert_eq!(asset.initial_price, 100.0);
        assert!(
            matches!(asset.return_profile, ReturnProfile::Fixed(r) if (r - 0.10).abs() < 0.001)
        );
    }

    #[test]
    fn test_us_total_market_preset() {
        let asset = AssetBuilder::us_total_market("VTI").price(200.0).build();

        assert_eq!(asset.name, "VTI");
        assert_eq!(asset.initial_price, 200.0);
        assert!(asset.description.is_some());
    }

    #[test]
    fn test_total_bond_preset() {
        let asset = AssetBuilder::total_bond("BND").build();

        assert_eq!(asset.name, "BND");
        assert!(
            matches!(asset.return_profile, ReturnProfile::Fixed(r) if (r - 0.04).abs() < 0.001)
        );
    }
}
