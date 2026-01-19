use crate::model::{AccountId, AssetCoord};

pub type Result<T> = std::result::Result<T, EngineError>;

#[derive(Debug)]
pub enum EngineError {
    AccountNotFound(AccountId),
    AssetNotFound(AssetCoord),
    AssetPriceNotFound(AssetCoord),
    NotAnInvestmentAccount(AccountId),
}

#[derive(Debug)]
pub enum TransferEvaluationError {
    EngineError(EngineError),
    ExternalBalanceReference,
}

impl From<EngineError> for TransferEvaluationError {
    fn from(err: EngineError) -> Self {
        TransferEvaluationError::EngineError(err)
    }
}
pub enum TriggerEventError {
    EngineError(EngineError),
    TransferEvaluationError(TransferEvaluationError),
    DateError(jiff::Error),
}

impl From<EngineError> for TriggerEventError {
    fn from(err: EngineError) -> Self {
        TriggerEventError::EngineError(err)
    }
}

impl From<TransferEvaluationError> for TriggerEventError {
    fn from(err: TransferEvaluationError) -> Self {
        TriggerEventError::TransferEvaluationError(err)
    }
}

impl From<jiff::Error> for TriggerEventError {
    fn from(err: jiff::Error) -> Self {
        TriggerEventError::DateError(err)
    }
}

#[derive(Debug)]
pub enum StateEventError {
    EngineError(EngineError),
    TransferEvaluationError(TransferEvaluationError),
}

impl From<EngineError> for StateEventError {
    fn from(err: EngineError) -> Self {
        StateEventError::EngineError(err)
    }
}

impl From<TransferEvaluationError> for StateEventError {
    fn from(err: TransferEvaluationError) -> Self {
        StateEventError::TransferEvaluationError(err)
    }
}

#[derive(Debug)]
pub enum ApplyError {
    AccountNotFound(AccountId),
    NotACashAccount(AccountId),
    NotAnInvestmentAccount(AccountId),
    InvalidAccountType(AccountId),
}

impl From<EngineError> for ApplyError {
    fn from(err: EngineError) -> Self {
        match err {
            EngineError::AccountNotFound(id) => ApplyError::AccountNotFound(id),
            EngineError::NotAnInvestmentAccount(id) => ApplyError::NotAnInvestmentAccount(id),
            _ => ApplyError::AccountNotFound(AccountId(0)), // Fallback for other errors
        }
    }
}
