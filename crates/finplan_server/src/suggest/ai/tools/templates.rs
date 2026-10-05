//! `expand_template`: a plain fact lowered to the changes that write it, so a
//! note's path can be a template's expansion. The draft serves the same call
//! with its own definition (`draft::prompt`); both parse and expand here.

use std::sync::OnceLock;

use finplan_plan::templates::{
    EmployerMatchParams, HomePurchaseParams, JobLossParams, LargeExpenseParams, MarketCrashParams,
    NewRef, RecurringExpenseParams, RetirementParams, RetirementSpending, RothConversionsParams,
    RowRef, SalaryParams, SocialSecurityParams, Template, TemplateKind, TemplateRequest, When,
};
use serde_json::{Value, json};
use ts_rs::{Config, TS};

/// Every template kind, as the call names it.
pub const KINDS: [&str; 10] = [
    "salary",
    "employer_match",
    "recurring_expense",
    "retirement",
    "home_purchase",
    "social_security",
    "market_crash",
    "large_expense",
    "job_loss",
    "roth_conversions",
];

/// The TypeScript declarations of the request and every params type, for a
/// loop whose prompt does not already carry them. Built once.
fn reference() -> &'static str {
    static REFERENCE: OnceLock<String> = OnceLock::new();
    REFERENCE.get_or_init(|| {
        let cfg = Config::new().with_large_int("number");
        [
            Template::decl(&cfg),
            TemplateKind::decl(&cfg),
            SalaryParams::decl(&cfg),
            EmployerMatchParams::decl(&cfg),
            RecurringExpenseParams::decl(&cfg),
            RetirementParams::decl(&cfg),
            RetirementSpending::decl(&cfg),
            HomePurchaseParams::decl(&cfg),
            SocialSecurityParams::decl(&cfg),
            MarketCrashParams::decl(&cfg),
            LargeExpenseParams::decl(&cfg),
            JobLossParams::decl(&cfg),
            RothConversionsParams::decl(&cfg),
            When::decl(&cfg),
            RowRef::decl(&cfg),
            NewRef::decl(&cfg),
        ]
        .join("\n")
    })
}

/// The call's input: `kind`, `params` (the kind's params type) and an
/// optional `key_prefix`. With `with_reference`, `params` carries the type
/// declarations in its description.
pub fn input_schema(with_reference: bool) -> Value {
    let params = if with_reference {
        format!(
            "The template's parameters: the params type for the kind, as declared here.\n{}",
            reference()
        )
    } else {
        "The template's parameters (the type named for the kind in the reference).".to_string()
    };
    json!({
        "type": "object",
        "properties": {
            "kind": {"type": "string", "enum": KINDS},
            "key_prefix": {"type": "string", "description": "Prepended to every key the template creates; use a different one per expansion in a path."},
            "params": {"type": "object", "description": params}
        },
        "required": ["kind", "params"]
    })
}

pub fn schema() -> Value {
    input_schema(true)
}

/// Expand one call: the changes and the keys they create, or why not.
pub fn run(input: &Value) -> Result<Value, String> {
    let Some(kind) = input.get("kind").and_then(Value::as_str) else {
        return Err("kind is required".into());
    };
    let mut request = match input.get("params") {
        Some(Value::Object(map)) => map.clone(),
        Some(Value::Null) | None => serde_json::Map::new(),
        Some(_) => return Err("params is an object".into()),
    };
    request.insert("kind".into(), json!(kind));
    request.insert(
        "key_prefix".into(),
        input.get("key_prefix").cloned().unwrap_or(json!("")),
    );
    let request: TemplateRequest = serde_json::from_value(Value::Object(request))
        .map_err(|e| format!("the {kind} parameters do not read: {e}"))?;
    let expansion = request.expand().map_err(|e| e.to_string())?;
    serde_json::to_value(&expansion).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_roth_conversion_expands_to_a_yearly_dec_30_event() {
        let out = run(&json!({
            "kind": "roth_conversions", "key_prefix": "c_",
            "params": {"from_account_id": 3, "to_account_id": 4, "ceiling_rate": 0.24,
                       "start": {"kind": "Date", "on_date": "2031-03-01"}}
        }))
        .unwrap();
        let body = &out["changes"][0]["value"];
        assert_eq!(out["keys"][0]["key"], "c_roth_conversions");
        assert_eq!(body["trigger"]["start_condition"]["on_date"], "2031-12-30");
        assert_eq!(body["effects"][0]["amount"]["source"], "bracket_room(24%)");
        assert!(run(&json!({"kind": "roth_conversions", "params": {}})).is_err());
        assert!(
            schema()["properties"]["params"]["description"]
                .as_str()
                .unwrap()
                .contains("RothConversionsParams")
        );
    }
}
