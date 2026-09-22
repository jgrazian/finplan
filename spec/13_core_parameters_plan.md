# Core simulation parameters

## Objective and scope

Introduce named, typed inputs that amount expressions and event schedules can
reference. Numeric inputs can also be optimized explicitly. Keep financial
operations represented by `EventEffect` and the existing ledger pipeline.

Parameters are fixed during each simulation run. This version supports Money,
Rate, Date, and Age in the core API. Mutable variables, database/API authoring,
and UI editors are outside its scope.

## Implementation sequence

1. **Define parameter identity and configuration.** Add a type-safe `ParameterId`
   and a typed parameter-value collection on `SimulationConfig`. Default the collection
   during deserialization so older configurations remain readable. Follow the
   existing metadata and builder conventions for human-readable names and lookup.
   Provide an explicit way to override an existing parameter without changing the
   base configuration.
2. **Reference parameters from amounts and schedules.** Add
   `TransferAmount::Parameter(ParameterId)` and builder support. References may
   appear inside every existing expression wrapper and arithmetic operation.
   Add Date and Age references to triggers, including recurring start/end
   conditions. Preserve existing literals and effects.
3. **Bind values at run initialization.** Validate finite Money/Rate values,
   valid Age months, expression types, and calendar references;
   reject undefined references with useful errors before processing any events.
   Resolve references in the run's event copies, including both branches of
   nested `Random` effects. Preserve the original configuration and expression
   structure outside those resolved references. Keep inflation and balance-based
   expressions evaluated at their existing times. Parameter binding must consume
   no random numbers and introduce no per-event lookup overhead.
4. **Target parameters in optimization.** Add an explicit Money/Rate parameter
   optimization target with bounds and ID-based updates. Date/Age are overridden
   as typed values but are not numeric optimization targets. Preserve legacy targets
   for compatibility; document the explicit target as the preferred path for new
   amount/rate optimization. A parameter update must not rewrite other literals
   or unrelated parameters and must reject missing IDs and non-finite values.
5. **Integrate and document.** Update affected configuration constructors and
   exhaustive matches across the workspace. Add a usable example covering named
   parameters, nested amount expressions, and explicit optimization. Do not add
   server schema or frontend authoring features merely to expose the core API.
6. **Verify and deliver.** Add focused behavioral tests, run `cargo fmt`,
   focused core tests, workspace tests and `cargo clippy`, check generated
   TypeScript bindings for drift, and stage the reviewed changes. Suggest a
   commit message without committing.

## Acceptance criteria

- Older serialized configurations load without parameters, and configurations
  with parameter references round-trip through serialization.
- Shared references resolve consistently across multiple effects and events.
- Inflation, scaling, arithmetic, and nested random branches resolve correctly;
  even a missing reference in an unselected branch is rejected at initialization.
- Missing references and non-finite values fail explicitly rather than silently
  skipping an event or falling back to zero.
- Typed arithmetic rejects category mismatches before a run. Date and Age
  references resolve in nested and repeating triggers on their exact dates.
- Parameterized Age requires `birth_date`; invalid ages and out-of-range
  calendar results fail as configuration errors.
- Overriding or optimizing one parameter preserves other values, literal amounts,
  and the original configuration.
- A bound parameter scenario matches an equivalent literal scenario under the
  same seed, including Monte Carlo behavior and ledger output where applicable.
- Existing callers and workspace tests continue to work.

## Historical delegation and review

The initial numeric-only version was implemented by GPT-6 Luna and reviewed by
the parent agent. This section records that earlier baseline; the current API is
documented below.

## Implemented API

- `ParameterValue::{Money(f64), Rate(f64), Date(Date), Age(CalendarAge)}` is
  the registry value type. `CalendarAge::new(years, months)` represents exact
  calendar age; months must be 0 through 11. `SimulationBuilder::parameter`
  accepts a typed value. A bare `f64` is treated as Money for source compatibility.
- Older serialized configurations without `parameters` remain readable. Older
  bare numeric registry values deserialize as Money. Newly serialized values
  carry their explicit variant. An older numeric value that represents a rate
  must be explicitly changed to `Rate` before use in typed expressions; the
  loader cannot infer its intended unit.
- `TransferAmount::Parameter(id)` can occur inside existing amount arithmetic.
  Validation runs before every simulation and checks all random branches. An
  effect amount must have Money type. `Money + Money`, `Money - Money`, and
  `Money * Rate` are valid. `Rate * Rate` yields Rate, allowing a scaled rate
  within a larger expression. Min/Max require compatible types; inflation
  adjustment requires Money. Existing `Fixed(f64)` literals can serve as Money
  or scalar Rate according to their expression context, preserving historic
  `Mul(Fixed(rate), balance)` usage. A direct Rate parameter cannot serve as an
  effect's Money amount, and typed Money/Rate mismatches fail before execution.
- `EventTrigger::DateParameter(id)` and `AgeParameter(id)` work anywhere in a
  trigger tree, including repeating start and end conditions. Builder helpers
  include `on_date_parameter`, `at_age_parameter`, `starting_on_parameter`,
  `starting_at_age_parameter`, `until_date_parameter`, and
  `until_age_parameter`. Date and Age references bind to concrete dates before
  execution. Parameterized Age requires a configured birth date, and invalid
  calendar results return configuration errors. Nested dates are included in
  checkpoint scheduling, so a recurring condition takes effect on its date.
- `SimulationConfig::with_parameter_value(id, value)` returns an independent
  configuration with the same typed parameter replaced. It returns `None` for
  missing IDs, invalid values, or a type change. Numeric optimization preserves
  Money or Rate variants and rejects Date/Age parameters.
- Run `cargo run -p finplan_core --example named_parameters` for all four types
  in a savings and optimization scenario.

The server's scenario compiler currently constructs an empty registry. Server,
TUI, and web authoring are separate work.

## Historical verification of the initial numeric-only baseline (2026-09-22)

The earlier implementation and parent review completed. Eight baseline tests cover parameter
binding, shared references and seeded equivalence, inflation and current balances,
invalid inputs, optimizer isolation, serialization, and builder metadata.

- `cargo fmt` completed.
- `cargo test --workspace --features finplan_server/ts-format` passed: 480 tests,
  with 21 pre-existing ignored documentation tests. Server listener tests ran with
  local socket access after the sandbox blocked them during the baseline run.
- `cargo clippy --workspace --all-targets -- -D warnings` passed. Existing `ts-rs`
  attribute-parser notices about `deserialize_with` remain; no Clippy lints were
  reported.
- `cargo check --workspace --all-targets` and the named-parameters example passed.
- `git diff --exit-code -- web/lib/api/generated` and `git diff --check` passed.

## Typed follow-up verification (2026-09-22)

GPT-6 Sol implemented the typed values, validation, calendar binding, scheduling
fixes, documentation, and regression tests. Parent review is complete.

- `cargo test --workspace --features finplan_server/ts-format` passed all 486
  tests, with the same 21 ignored documentation tests as the baseline.
- After adding the final regression test for an end date between recurring
  payments, `cargo test -p finplan_core` passed all 183 core tests. This also
  verifies that typed and literal age conditions produce equivalent results.
- `cargo fmt`, `cargo check --workspace --all-targets`, and
  `cargo clippy --workspace --all-targets -- -D warnings` passed. The existing
  `ts-rs` attribute-parser notices remain; there are no Clippy lints.
- The named-parameters example runs with all four types.
- Generated TypeScript bindings have no drift, and `git diff --check` passes.
