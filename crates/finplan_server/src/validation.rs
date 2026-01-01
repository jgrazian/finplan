use crate::error::{ApiError, ApiResult};
use finplan::models::SimulationParameters;

/// Validate portfolio creation request
pub fn validate_portfolio_name(name: &str) -> ApiResult<()> {
    if name.trim().is_empty() {
        return Err(ApiError::ValidationError {
            field: "name".to_string(),
            message: "Portfolio name cannot be empty".to_string(),
        });
    }

    if name.len() > 200 {
        return Err(ApiError::ValidationError {
            field: "name".to_string(),
            message: "Portfolio name cannot exceed 200 characters".to_string(),
        });
    }

    Ok(())
}

/// Validate that accounts array is not empty
pub fn validate_portfolio_has_accounts(account_count: usize) -> ApiResult<()> {
    if account_count == 0 {
        return Err(ApiError::ValidationError {
            field: "accounts".to_string(),
            message: "Portfolio must have at least one account".to_string(),
        });
    }
    Ok(())
}

/// Validate simulation name
pub fn validate_simulation_name(name: &str) -> ApiResult<()> {
    if name.trim().is_empty() {
        return Err(ApiError::ValidationError {
            field: "name".to_string(),
            message: "Simulation name cannot be empty".to_string(),
        });
    }

    if name.len() > 200 {
        return Err(ApiError::ValidationError {
            field: "name".to_string(),
            message: "Simulation name cannot exceed 200 characters".to_string(),
        });
    }

    Ok(())
}

/// Validate simulation parameters
pub fn validate_simulation_params(params: &SimulationParameters) -> ApiResult<()> {
    // Validate start_date is present
    let start_date = params.start_date.ok_or_else(|| ApiError::ValidationError {
        field: "start_date".to_string(),
        message: "Start date is required".to_string(),
    })?;

    // Validate duration
    if params.duration_years == 0 {
        return Err(ApiError::ValidationError {
            field: "duration_years".to_string(),
            message: "Duration must be at least 1 year".to_string(),
        });
    }

    if params.duration_years > 200 {
        return Err(ApiError::ValidationError {
            field: "duration_years".to_string(),
            message: "Duration cannot exceed 200 years".to_string(),
        });
    }

    // Validate birth date if present
    if let Some(birth_date) = params.birth_date {
        if birth_date >= start_date {
            return Err(ApiError::ValidationError {
                field: "birth_date".to_string(),
                message: "Birth date must be before start date".to_string(),
            });
        }

        // Simple check: birth date should be at least 1 year before start and not more than 150 years
        let years_diff = start_date.year() - birth_date.year();
        if years_diff > 150 {
            return Err(ApiError::ValidationError {
                field: "birth_date".to_string(),
                message: "Birth date results in unrealistic age (> 150 years)".to_string(),
            });
        }

        if years_diff < 0 {
            return Err(ApiError::ValidationError {
                field: "birth_date".to_string(),
                message: "Birth date cannot be after start date".to_string(),
            });
        }
    }

    Ok(())
}

/// Validate iteration count for Monte Carlo simulations
pub fn validate_iterations(iterations: usize) -> ApiResult<()> {
    if iterations == 0 {
        return Err(ApiError::ValidationError {
            field: "iterations".to_string(),
            message: "Iterations must be greater than 0".to_string(),
        });
    }

    if iterations > 10000 {
        return Err(ApiError::ValidationError {
            field: "iterations".to_string(),
            message: "Iterations cannot exceed 10,000".to_string(),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_portfolio_name() {
        assert!(validate_portfolio_name("Valid Name").is_ok());
        assert!(validate_portfolio_name("").is_err());
        assert!(validate_portfolio_name("   ").is_err());
        assert!(validate_portfolio_name(&"a".repeat(201)).is_err());
    }

    #[test]
    fn test_validate_iterations() {
        assert!(validate_iterations(100).is_ok());
        assert!(validate_iterations(1).is_ok());
        assert!(validate_iterations(10000).is_ok());
        assert!(validate_iterations(0).is_err());
        assert!(validate_iterations(10001).is_err());
    }

    #[test]
    fn test_validate_simulation_dates() {
        use jiff::civil::date;

        let mut params = SimulationParameters {
            start_date: Some(date(2025, 1, 1)),
            duration_years: 25,
            birth_date: None,
            accounts: vec![],
            inflation_profile: Default::default(),
            return_profiles: vec![],
            cash_flows: vec![],
            events: vec![],
            spending_targets: vec![],
            tax_config: Default::default(),
        };

        assert!(validate_simulation_params(&params).is_ok());

        // Test with no start date
        params.start_date = None;
        assert!(validate_simulation_params(&params).is_err());

        // Restore start date
        params.start_date = Some(date(2025, 1, 1));

        // Test with zero duration
        params.duration_years = 0;
        assert!(validate_simulation_params(&params).is_err());

        // Restore duration
        params.duration_years = 25;

        // Test with excessive duration
        params.duration_years = 201;
        assert!(validate_simulation_params(&params).is_err());
    }
}
