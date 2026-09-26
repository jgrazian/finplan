# Web parameters and value expressions

The Plan sidebar has a collapsible Parameters section below Events. The drawer
fits its contents up to 230 pixels, then scrolls its list under a fixed header.
Collapsing it returns the space to Events. The sidebar has a bounded height and
no drag-resize control, so both lists remain within the column bounds.
Parameters belong to the selected scenario. Selecting one opens its name,
type, value, and event references in the main editor, while retaining the current
event draft. Money allows signed amounts; Rate displays percentages and stores
fractions; Date uses calendar dates; Age stores years and months. Names are unique
within a scenario. Referenced parameters cannot be deleted or change type.

Effect amounts offer Static and Expression modes. Static amounts persist as a
literal or `inflation(literal)`. Expression mode provides source editing,
reference/function insertion, compiler diagnostics with source spans, and a value
preview at plan start when one is available. Returning to Static is available only
for a literal or inflation-adjusted literal, preventing a formula from being lost.
Use `inflation(...)` explicitly in expressions. A root `gross(...)` or `net(...)`
annotation controls the existing amount-mode field for supported effects.

Money and Rate parameters can participate in calculations such as
`inflation($MonthlySpending)` and `$WithdrawalRate * balance("Brokerage")`.
Date and Age parameters are available as typed schedule choices, including the
start and end of a repeating event. Inside expressions use `days_until($Date)`,
`years_until($Date)`, or `age_years($Age)` to obtain a scalar.

Analysis discovers the shared parameters and varies their values across every
reference. Rate bounds appear as percentages; Date bounds use date inputs; Age
bounds use years and months. Calendar ranges use whole days or months, including
when solving, and repeated calendar candidates are removed. Existing saved plans
can run without parameters; add parameters to expose inputs to Analysis.

## API and persistence

- `GET/POST /api/scenarios/{id}/parameters` lists or creates parameters.
- `PATCH/DELETE /api/scenarios/{id}/parameters/{parameter_id}` edits or removes one.
- `POST /api/scenarios/{id}/expressions/validate` accepts the selected effect and
  returns compiler diagnostics, referenced parameter IDs, and a qualified preview.
- `GET /api/scenarios/{id}/analysis/parameters` discovers analysis inputs.
- `AmountSpec::Expression { source }` is the editable amount representation.
  Existing server amount trees are adapted on load; Expression must be the root
  rather than a child of a legacy amount tree.
- `DateParameter` and `AgeParameter` triggers carry a parameter ID that must belong
  to the same scenario and have the matching type.

Analysis transmits Rate as a fraction, Date as UTC epoch days, and Age as years
plus months divided by 12. These coordinates are converted to the core's typed
parameter sweep values and converted back for result axes and solver probes.

The baseline schema in `0001_init.sql` includes typed parameter storage, expression
source, and trigger references. Cloning and archive import/export preserve values
and remap IDs. Exports use archive version 3 so older servers cannot silently lose
expression amounts; version 2 exports remain importable. Renames update stored source through compiled references in the
same transaction, including names that require quoting. The event editor also
rebases parameter names in a retained expression draft.

Expression previews are estimates at plan start, not guarantees of future event
amounts. Runtime balances, inflation, and calendar functions can change the result.
See `14_expression_dsl.md` for syntax and the source/target context of each effect.
