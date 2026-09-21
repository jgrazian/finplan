# WebUI release implementation record

The initial hosted split approved September 13, 2026 is Free: full modeling, one editable saved plan, 1,000 Monte Carlo iterations, one accepted goal seek per UTC calendar month. Pro: unlimited saved plans, comparisons, reports, retained history and up to 50,000 iterations subject to compute admission limits. Initial price metadata is $80/year or $10/month. AI review and share links are deferred. Local operation does not require a subscription. Mobile remains deferred.

## Implemented in this change

- Immutable queued run inputs, effective seed, sampling settings, model identifier and dependency hash. Workers and restart recovery use the stored snapshot. Historical account labels/results survive live account deletion. Old runs remain readable with unavailable provenance explicitly identified.
- Guided, resumable single-person starter; atomic retry-safe account/holding/event creation; explicit funding choice and tax/inflation/return assumptions; preflight before UI run entry points; existing-plan funding and assumption editing.
- Saved-run selection, cross-plan comparisons and printable reports based on stored results and inputs. Reports use the browser's Print / Save PDF flow.
- Versioned JSON input archives, complete referenced assumptions, owner-scoped export, preview and independent transactional restore with retry receipts. Historical input archives can restore a past run as a new plan. Archives exclude results and account/security/billing records; they are not CLI YAML files.
- Hosted origin/cookie protections, bounded authentication work, expiring single-use recovery tokens, session revocation, and explicit local mail sink. Provider-neutral subscription reconciliation, idempotent events and server entitlement enforcement.
- SQLite WAL-aware backup and isolated restore verification, plus server/frontend CI validation.

## Boundaries before accepting payments

Live billing and production transactional email are unconfigured: select providers, connect authenticated provider adapters, and pass their sandbox delivery/webhook tests. The trusted subscription reconciliation interface is tested locally; no browser route grants Pro and no live payment is charged. Run the documented deployment/restore drill on the actual host and set backup retention, delivery monitoring and operational ownership. This implementation does not constitute a completed production deployment.

The guided starter supports one person, checking, a tax-deferred 401(k), and one other investment account; additional household members, benefits, liabilities, accounts and contributions use the advanced editors. Household/estate model extensions, AI review, sharing and mobile layout are later work. Past input restoration creates an independent plan rather than overwriting a live plan. Deleting a whole plan intentionally deletes its run history.

See `09_operations_runbook.md` for operations; `08_hosted_readiness_matrix.md` is the original inventory and should be read with this implementation record.

## Validation evidence

- Full `cargo test --workspace` passed after canonicalizing hashes independently of serde feature unification. Targeted hosted checks also cover direct compute-limit bypass attempts.
- `cargo fmt --check` and workspace/all-target Clippy with warnings denied passed; ts-rs emits its existing procedural-macro notices about `double_option` attributes.
- Frontend typecheck, ESLint, 53 tests and production build passed. API bindings regenerated with the repository script.
- Browser smoke test on localhost: guided five-step fictional setup, preflight acknowledgment, successful new immutable run, retained legacy history and current-run provenance.
- WAL-aware local backup and isolated restore both passed integrity and foreign-key checks, retaining 2 users, 4 scenarios and 2 pre-migration runs. The live application database was backed up before migration. This is a local drill, not evidence of a production restore or external backup policy.
