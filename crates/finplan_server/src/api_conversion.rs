//! Convert API DTOs to SimulationBuilder calls
//!
//! This module converts the REST API types into finplan's builder DSL,
//! resolving names to IDs and validating the configuration.

use crate::api_types::*;
use crate::error::{ApiError, ApiResult};
use finplan_core::config::{
    AccountBuilder, AssetBuilder, EventBuilder, SimulationBuilder, SimulationConfig,
    SimulationMetadata,
};
use finplan_core::model::{IncomeType, LotMethod, RepeatInterval, TaxConfig, TaxStatus};

/// Convert SimulationConfig and metadata back to SimulationConfig for storage
/// Note: This is a pass-through since SimulationConfig IS what we store
pub fn config_to_parameters(
    config: &SimulationConfig,
    _metadata: &SimulationMetadata,
) -> ApiResult<SimulationConfig> {
    Ok(config.clone())
}

impl SimulationRequest {
    /// Convert this request into a (SimulationConfig, SimulationMetadata) pair
    pub fn build(self) -> ApiResult<(SimulationConfig, SimulationMetadata)> {
        let mut builder = SimulationBuilder::new();

        // Timeline
        if let Some(ref date_str) = self.start_date {
            let date = parse_date(date_str)?;
            builder = builder.start_date(date);
        }
        builder = builder.duration_years(self.duration_years);

        if let Some(ref date_str) = self.birth_date {
            let date = parse_date(date_str)?;
            builder = builder.birth_date_obj(date);
        }

        // World assumptions
        if let Some(inflation) = self.inflation_profile {
            builder = builder.inflation_profile(inflation);
        }

        if let Some(tax_config) = self.tax_config {
            builder = builder.tax_config(tax_config.into_tax_config());
        }

        // Return profiles
        for profile_def in self.return_profiles {
            builder = builder.return_profile(profile_def.name, profile_def.profile);
        }

        // Assets
        for asset_def in self.assets {
            builder = builder.asset(asset_def.into_builder()?);
        }

        // Accounts
        for account_def in self.accounts {
            builder = builder.account(account_def.into_builder()?);
        }

        // Positions
        for pos in self.positions {
            if let Some(ref date_str) = pos.purchase_date {
                let purchase_date = parse_date(date_str)?;
                builder = builder.position_dated(
                    &pos.account,
                    &pos.asset,
                    pos.units,
                    pos.cost_basis,
                    purchase_date,
                );
            } else {
                builder = builder.position(&pos.account, &pos.asset, pos.units, pos.cost_basis);
            }
        }

        // Events
        for event_def in self.events {
            builder = builder.event(event_def.into_builder()?);
        }

        Ok(builder.build())
    }
}

impl TaxConfigDef {
    fn into_tax_config(self) -> TaxConfig {
        let mut config = TaxConfig::default();
        if let Some(rate) = self.capital_gains_rate {
            config.capital_gains_rate = rate;
        }
        config
    }
}

impl AssetDef {
    fn into_builder(self) -> ApiResult<AssetBuilder> {
        let mut builder = AssetBuilder::new(&self.name).price(self.price);

        if let Some(desc) = self.description {
            builder = builder.description(desc);
        }

        match self.return_profile {
            ReturnProfileRef::Named(name) => {
                builder = builder.return_profile_name(name);
            }
            ReturnProfileRef::Inline(profile) => {
                builder = builder.return_profile(profile);
            }
        }

        Ok(builder)
    }
}

impl AccountDef {
    fn into_builder(self) -> ApiResult<AccountBuilder> {
        // Note: contribution_limit is not directly supported by AccountBuilder
        // It's applied via InvestmentContainer at a lower level
        let builder = match self.account_type {
            AccountTypeDef::Bank => AccountBuilder::bank_account(&self.name),
            AccountTypeDef::TaxableBrokerage => AccountBuilder::taxable_brokerage(&self.name),
            AccountTypeDef::Traditional401k { .. } => AccountBuilder::traditional_401k(&self.name),
            AccountTypeDef::Roth401k { .. } => AccountBuilder::roth_401k(&self.name),
            AccountTypeDef::TraditionalIra { .. } => AccountBuilder::traditional_ira(&self.name),
            AccountTypeDef::RothIra { .. } => AccountBuilder::roth_ira(&self.name),
            AccountTypeDef::Hsa { .. } => AccountBuilder::hsa(&self.name),
            AccountTypeDef::Custom { tax_status, .. } => {
                // Use the appropriate builder based on tax status
                match tax_status {
                    TaxStatus::Taxable => AccountBuilder::taxable_brokerage(&self.name),
                    TaxStatus::TaxDeferred => AccountBuilder::traditional_401k(&self.name),
                    TaxStatus::TaxFree => AccountBuilder::roth_ira(&self.name),
                }
            }
        };

        let mut builder = builder.cash(self.cash);

        if let Some(desc) = self.description {
            builder = builder.description(desc);
        }

        Ok(builder)
    }
}

impl EventDef {
    fn into_builder(self) -> ApiResult<EventBuilder> {
        if self.effects.is_empty() {
            return Err(ApiError::ValidationError {
                field: "effects".to_string(),
                message: "Event must have at least one effect".to_string(),
            });
        }

        // Build event based on the first effect type
        let builder = match &self.effects[0] {
            EffectDef::Income {
                to_account,
                amount,
                income_type,
                gross,
                ..
            } => {
                let mut b = EventBuilder::income(&self.name)
                    .to_account(to_account)
                    .amount(*amount);
                if *gross {
                    b = b.gross();
                } else {
                    b = b.net();
                }
                match income_type {
                    IncomeType::Taxable => b = b.taxable(),
                    IncomeType::TaxFree => b = b.tax_free(),
                }
                b
            }
            EffectDef::Expense {
                amount,
                from_account,
                ..
            } => {
                let mut b = EventBuilder::expense(&self.name).amount(*amount);
                if let Some(account) = from_account {
                    b = b.from_account(account);
                }
                b
            }
            EffectDef::AssetPurchase {
                amount,
                account,
                asset,
                ..
            } => EventBuilder::asset_purchase(&self.name)
                .amount(*amount)
                .from_account(account)
                .to_asset(account, asset),
            EffectDef::Withdrawal {
                amount,
                to_account,
                source,
                gross,
                lot_method,
            } => {
                let mut b = EventBuilder::withdrawal(&self.name).to_account(to_account);

                // Apply amount
                b = match amount {
                    AmountDef::Fixed { value } => b.amount(*value),
                    AmountDef::Percent { .. } => b.full_balance(), // Approximate - percent not directly supported
                    AmountDef::All => b.full_balance(),
                };

                // Apply gross/net
                if *gross {
                    b = b.gross();
                } else {
                    b = b.net();
                }

                // Apply lot method
                b = match lot_method {
                    LotMethod::Fifo => b.fifo(),
                    LotMethod::Lifo => b.lifo(),
                    LotMethod::HighestCost => b.highest_cost_first(),
                    LotMethod::LowestCost => b.lowest_cost_first(),
                    LotMethod::AverageCost => b.fifo(), // AverageCost not directly supported, use FIFO
                };

                // Apply withdrawal source
                b = match source {
                    WithdrawalSourceDef::Strategy { order, .. } => b.withdrawal_strategy(*order),
                    WithdrawalSourceDef::AccountOrder { accounts } => {
                        b.from_accounts_in_order(accounts.iter().map(|s| s.as_str()))
                    }
                    WithdrawalSourceDef::Asset { account, .. } => b.from_single_account(account),
                };

                b
            }
        };

        let builder = if let Some(desc) = self.description {
            builder.description(desc)
        } else {
            builder
        };

        let builder = if self.once { builder.once() } else { builder };

        // Apply trigger
        let builder = self.trigger.apply_to_builder(builder)?;

        Ok(builder)
    }
}

impl TriggerDef {
    fn apply_to_builder(self, builder: EventBuilder) -> ApiResult<EventBuilder> {
        Ok(match self {
            TriggerDef::Immediate => builder, // Immediate is default behavior
            TriggerDef::Date { date } => {
                let date = parse_date(&date)?;
                builder.on_date(date)
            }
            TriggerDef::Age { years, months } => {
                if let Some(m) = months {
                    builder.at_age_months(years, m)
                } else {
                    builder.at_age(years)
                }
            }
            TriggerDef::Repeating {
                interval,
                start,
                end,
            } => {
                let mut b = match interval {
                    RepeatInterval::Never => builder.once(),
                    RepeatInterval::Weekly => builder.weekly(),
                    RepeatInterval::BiWeekly => builder.biweekly(),
                    RepeatInterval::Monthly => builder.monthly(),
                    RepeatInterval::Quarterly => builder.quarterly(),
                    RepeatInterval::Yearly => builder.yearly(),
                };

                if let Some(start_spec) = start {
                    b = match *start_spec {
                        TriggerStartDef::Date { date } => {
                            let date = parse_date(&date)?;
                            b.starting_on(date)
                        }
                        TriggerStartDef::Age { years, .. } => b.starting_at_age(years),
                    };
                }

                if let Some(end_spec) = end {
                    b = match *end_spec {
                        TriggerEndDef::Date { date } => {
                            let date = parse_date(&date)?;
                            b.until_date(date)
                        }
                        TriggerEndDef::Age { years, .. } => b.until_age(years),
                        TriggerEndDef::Never => b,
                    };
                }

                b
            }
        })
    }
}

fn parse_date(s: &str) -> ApiResult<jiff::civil::Date> {
    s.parse().map_err(|_| ApiError::ValidationError {
        field: "date".to_string(),
        message: format!("Invalid date format: {}", s),
    })
}
