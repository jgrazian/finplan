//! Templates now live in [`finplan_plan::templates`]; this module re-exports
//! them and maps their error onto the server's.

use crate::error::ApiError;

pub use finplan_plan::templates::*;

impl From<TemplateError> for ApiError {
    fn from(e: TemplateError) -> Self {
        ApiError::bad_request(e.to_string())
    }
}

#[cfg(test)]
mod tests;
