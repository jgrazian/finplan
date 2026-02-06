use std::fmt;

use crate::model::{AccountId, AssetCoord, ReturnProfileId};

/// Errors related to resource lookups
#[derive(Debug, Clone)]
pub enum LookupError {
    AccountNotFound(AccountId),
    AssetNotFound(AssetCoord),
    AssetPriceNotFound(AssetCoord),
    ReturnProfileNotFound(ReturnProfileId),
}

impl fmt::Display for LookupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LookupError::AccountNotFound(id) => write!(f, "account {id:?} not found"),
            LookupError::AssetNotFound(coord) => write!(f, "asset {coord:?} not found"),
            LookupError::AssetPriceNotFound(coord) => {
                write!(f, "price not available for asset {coord:?}")
            }
            LookupError::ReturnProfileNotFound(id) => {
                write!(f, "return profile {id:?} not found")
            }
        }
    }
}

impl std::error::Error for LookupError {}

/// Errors related to account type mismatches
#[derive(Debug, Clone)]
pub enum AccountTypeError {
    NotACashAccount(AccountId),
    NotAnInvestmentAccount(AccountId),
    InvalidAccountType(AccountId),
}

impl fmt::Display for AccountTypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AccountTypeError::NotACashAccount(id) => {
                write!(f, "account {id:?} is not a cash account")
            }
            AccountTypeError::NotAnInvestmentAccount(id) => {
                write!(f, "account {id:?} is not an investment account")
            }
            AccountTypeError::InvalidAccountType(id) => {
                write!(f, "invalid account type for account {id:?}")
            }
        }
    }
}

impl std::error::Error for AccountTypeError {}

/// Errors related to market/distribution operations
#[derive(Debug, Clone)]
pub enum MarketError {
    InvalidDistributionParameters {
        profile_type: &'static str,
        mean: f64,
        std_dev: f64,
        reason: &'static str,
    },
    Lookup(LookupError),
    /// Monte Carlo simulation was cancelled by user request
    Cancelled,
    /// Historical data is empty and cannot be sampled
    EmptyHistoricalData,
    /// Configuration error
    Config(String),
}

impl fmt::Display for MarketError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MarketError::InvalidDistributionParameters {
                profile_type,
                mean,
                std_dev,
                reason,
            } => {
                write!(
                    f,
                    "invalid {profile_type} parameters (mean={mean}, std_dev={std_dev}): {reason}"
                )
            }
            MarketError::Lookup(e) => write!(f, "{e}"),
            MarketError::Cancelled => write!(f, "simulation cancelled"),
            MarketError::EmptyHistoricalData => write!(f, "historical data is empty"),
            MarketError::Config(msg) => write!(f, "configuration error: {msg}"),
        }
    }
}

impl std::error::Error for MarketError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            MarketError::Lookup(e) => Some(e),
            _ => None,
        }
    }
}

impl From<LookupError> for MarketError {
    fn from(e: LookupError) -> Self {
        MarketError::Lookup(e)
    }
}

// Keep EngineError as an alias for backwards compatibility
pub type EngineError = LookupError;

pub type Result<T> = std::result::Result<T, LookupError>;

#[derive(Debug, Clone)]
pub enum TransferEvaluationError {
    Lookup(LookupError),
    ExternalBalanceReference,
    /// Inflation data not available for the requested date range
    InflationDataUnavailable,
}

impl fmt::Display for TransferEvaluationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransferEvaluationError::Lookup(e) => write!(f, "{e}"),
            TransferEvaluationError::ExternalBalanceReference => {
                write!(f, "cannot reference balance of external endpoint")
            }
            TransferEvaluationError::InflationDataUnavailable => {
                write!(
                    f,
                    "inflation data not available for the requested date range"
                )
            }
        }
    }
}

impl std::error::Error for TransferEvaluationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            TransferEvaluationError::Lookup(e) => Some(e),
            _ => None,
        }
    }
}

impl From<LookupError> for TransferEvaluationError {
    fn from(err: LookupError) -> Self {
        TransferEvaluationError::Lookup(err)
    }
}
// Note: EngineError is now a type alias for LookupError, so From<LookupError> covers it

#[derive(Debug)]
pub enum TriggerEventError {
    Lookup(LookupError),
    TransferEvaluation(TransferEvaluationError),
    DateError(jiff::Error),
}

impl fmt::Display for TriggerEventError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TriggerEventError::Lookup(e) => write!(f, "{e}"),
            TriggerEventError::TransferEvaluation(e) => write!(f, "{e}"),
            TriggerEventError::DateError(e) => write!(f, "date calculation error: {e}"),
        }
    }
}

impl std::error::Error for TriggerEventError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            TriggerEventError::Lookup(e) => Some(e),
            TriggerEventError::TransferEvaluation(e) => Some(e),
            TriggerEventError::DateError(e) => Some(e),
        }
    }
}

impl From<LookupError> for TriggerEventError {
    fn from(err: LookupError) -> Self {
        TriggerEventError::Lookup(err)
    }
}

impl From<TransferEvaluationError> for TriggerEventError {
    fn from(err: TransferEvaluationError) -> Self {
        TriggerEventError::TransferEvaluation(err)
    }
}

impl From<jiff::Error> for TriggerEventError {
    fn from(err: jiff::Error) -> Self {
        TriggerEventError::DateError(err)
    }
}

#[derive(Debug, Clone)]
pub enum StateEventError {
    Lookup(LookupError),
    AccountType(AccountTypeError),
    TransferEvaluation(TransferEvaluationError),
}

impl fmt::Display for StateEventError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StateEventError::Lookup(e) => write!(f, "{e}"),
            StateEventError::AccountType(e) => write!(f, "{e}"),
            StateEventError::TransferEvaluation(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for StateEventError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            StateEventError::Lookup(e) => Some(e),
            StateEventError::AccountType(e) => Some(e),
            StateEventError::TransferEvaluation(e) => Some(e),
        }
    }
}

impl From<LookupError> for StateEventError {
    fn from(err: LookupError) -> Self {
        StateEventError::Lookup(err)
    }
}

impl From<AccountTypeError> for StateEventError {
    fn from(err: AccountTypeError) -> Self {
        StateEventError::AccountType(err)
    }
}

impl From<TransferEvaluationError> for StateEventError {
    fn from(err: TransferEvaluationError) -> Self {
        StateEventError::TransferEvaluation(err)
    }
}

#[derive(Debug, Clone)]
pub enum ApplyError {
    Lookup(LookupError),
    AccountType(AccountTypeError),
}

impl fmt::Display for ApplyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ApplyError::Lookup(e) => write!(f, "{e}"),
            ApplyError::AccountType(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ApplyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ApplyError::Lookup(e) => Some(e),
            ApplyError::AccountType(e) => Some(e),
        }
    }
}

impl From<LookupError> for ApplyError {
    fn from(err: LookupError) -> Self {
        ApplyError::Lookup(err)
    }
}

impl From<AccountTypeError> for ApplyError {
    fn from(err: AccountTypeError) -> Self {
        ApplyError::AccountType(err)
    }
}
