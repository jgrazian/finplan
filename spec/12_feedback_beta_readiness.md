# Feedback beta launch readiness

This records repository readiness, not a completed production launch.

## Implemented

- Explicit hosted beta entitlements, independent of secure deployment settings.
  Beta grants planning features without payment records; API ownership, iteration
  limits and compute admission still apply. Subscription remains the default.
- Beta UI and account access information without pricing/checkout prompts.
- Encrypted SMTP recovery/verification delivery with bounded timeout, secret-safe
  errors/configuration, and single-use tokens. No email vendor is hard-coded.
- Enrollment kill switch that preserves existing users' login.
- Next standalone output, non-root API/web images, persistent SQLite and HTTPS
  Compose recipe; no public API/database/metrics ports.
- CI aligned to the committed pnpm lockfile, plus image builds/smoke checks.

## Still required from the operator before collecting tester data

| Item | Evidence needed |
|---|---|
| Hosting and DNS | Chosen host/domain, TLS working, persistent data volume |
| Email provider | Verified sender, configured runtime secrets, delivered and redeemed recovery/verification tokens |
| Enrollment | Explicit decision to open signup or provide an invitation mechanism; closed is the deployment example default |
| Data disclosures | Actual privacy/retention and model-scope information matching the deployment and third-party requests |
| Recovery | Scheduled encrypted off-host backups and a measured isolated restore |
| Monitoring/support | Named operator, outage/failure/backup notifications and regular review of stored Contact messages |
| Release verification | Green CI including container checks and the real-host end-to-end smoke test |

OAuth, payment processing, subscription checkout, and a separate cloud binary are
not needed for a free feedback cohort. Password-based login with working recovery
is supported. General UI redesign and expanded financial modeling remain separate.

Follow [the beta deployment recipe](../ops/beta/README.md). Live secrets, DNS,
provider setup, policy decisions, and a production restore cannot be inferred or
certified by local tests. There is no automated production deploy or merge in this work.

## Local validation

- `cargo test --workspace`: 472 passed; 21 documentation examples remain ignored.
  Listener tests ran with local socket access.
- `cargo fmt --check` and workspace/all-target Clippy with warnings denied passed.
  ts-rs retains its existing notices about unsupported serde parser attributes.
- Bindings regenerated through `scripts/gen-bindings.sh`.
- Frozen pnpm install, TypeScript, ESLint, all 59 frontend tests and Next production
  build passed. Standalone output served `/`, `/portfolio`, `/account`, static
  JavaScript and the `/api/health` proxy successfully on a temporary local port.
- Existing WAL backup/restore test passed. Compose and workflow YAML parsed.
- Docker is unavailable locally, so container build/start tests are configured in
  CI but have not been executed here. External SMTP and production checks remain
  operator launch gates, not claims implied by passing unit tests.
