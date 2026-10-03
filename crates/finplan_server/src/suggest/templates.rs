//! Templates live in [`finplan_plan::templates`]; this module maps their error
//! onto the server's and tests them against a stored plan.

use crate::error::ApiError;
use finplan_plan::templates::TemplateError;

impl From<TemplateError> for ApiError {
    fn from(e: TemplateError) -> Self {
        ApiError::bad_request(e.to_string())
    }
}

#[cfg(test)]
mod tests;
