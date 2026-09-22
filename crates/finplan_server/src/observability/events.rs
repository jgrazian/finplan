use super::*;

/// Allowlisted log metadata. Field names must be application-owned literals;
/// never include request values, names, emails, error text, or financial data.
#[derive(Default)]
pub struct EventFields<'a> {
    pub user_id: Option<&'a str>,
    pub scenario_id: Option<i64>,
    pub resource_id: Option<i64>,
    pub fields: &'a [&'static str],
    pub count: Option<u64>,
    pub replay: bool,
}

impl Telemetry {
    /// Log at the persistence boundary, even if subsequent response work fails.
    pub fn mutation(&self, resource: Resource, operation: Operation, fields: &EventFields<'_>) {
        if !fields.replay && operation != Operation::Exported {
            self.mutation_count(resource, operation);
        }
        let event = format!("{}.{}", resource.as_str(), operation.as_str());
        tracing::info!(event, user_id = fields.user_id, scenario_id = fields.scenario_id,
            resource_id = fields.resource_id, submitted_fields = ?fields.fields,
            affected_count = fields.count, replay = fields.replay);
    }

    pub fn auth(&self, action: AuthAction, outcome: AuthOutcome, fields: &EventFields<'_>) {
        self.auth_count(action, outcome);
        let event = if action == AuthAction::Register && outcome == AuthOutcome::Succeeded {
            self.mutation_count(Resource::User, Operation::Created);
            "user.registered".to_owned()
        } else {
            format!("auth.{}", action.as_str())
        };
        tracing::info!(
            event,
            action = action.as_str(),
            outcome = outcome.as_str(),
            user_id = fields.user_id,
            affected_count = fields.count,
            replay = fields.replay
        );
    }

    pub fn auth_throttled(&self) {
        self.auth_count(AuthAction::Throttled, AuthOutcome::Rejected);
        if let Some(suppressed) = self.permit_log(("auth", "throttled")) {
            tracing::warn!(event = "auth.throttled", outcome = "rejected", suppressed);
        }
    }

    /// Count and log one unexpected failure. Use count_error when a caller owns
    /// a more specific named failure event, to avoid duplicate error logs.
    pub fn error(&self, component: Component, class: ErrorClass) {
        self.count_error(component, class);
        tracing::error!(
            event = "server.error",
            component = component.as_str(),
            class = class.as_str()
        );
    }

    pub fn recoverable_error(&self, component: Component, class: ErrorClass) {
        self.recoverable_error_event(component, class, "maintenance.failed");
    }

    /// Named recoverable boundary, with an application-owned event literal.
    pub fn recoverable_error_event(
        &self,
        component: Component,
        class: ErrorClass,
        event: &'static str,
    ) {
        self.count_error(component, class);
        if let Some(suppressed) = self.permit_log((component.as_str(), class.as_str())) {
            tracing::warn!(
                event,
                component = component.as_str(),
                class = class.as_str(),
                suppressed
            );
        }
    }

    pub fn recovered_error(&self, component: Component, class: ErrorClass) {
        if let Some(suppressed) = self.clear_log_limit((component.as_str(), class.as_str())) {
            tracing::info!(
                event = "maintenance.recovered",
                component = component.as_str(),
                class = class.as_str(),
                suppressed
            );
        }
    }
}
