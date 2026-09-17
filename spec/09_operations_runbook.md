# Hosted WebUI operational handoff

Repository tooling is available; production recovery objectives, storage policy,
provider secrets and support ownership must be supplied by the operator before a
paid launch. Nothing here records a production restore or payment verification.

## Backup and isolated restore

Run `python3 scripts/sqlite-backup.py backup /path/live.db /private/backup.db`.
The SQLite backup API includes committed WAL content, verifies integrity and
foreign keys, publishes a new file without overwriting, and sets mode 0600.
Save the emitted checksum/count receipt beside the backup in encrypted storage
with restricted access. The script itself does not encrypt or upload data.
Operational backups contain personal data, password hashes and billing records;
apply the same access controls as the live service.

Run `python3 scripts/sqlite-backup.py restore /private/backup.db /isolated/restored.db`.
This refuses an existing destination or SQLite sidecars. Start a separate service
against that restored database with production network egress and billing disabled.
Verify sign-in, scenario counts, assumptions, historical runs/ledgers and interrupted
job handling. Record elapsed restore time, checksum, application version and checks.
Never point the rehearsal service at the production database or payment provider.

Before an upgrade, create and verify a backup and test migrations against its
isolated restore. Rollback uses a compatible application version plus a verified
pre-upgrade restore, not assumptions that SQLite schema changes are reversible.
For a real restore, stop all writers first and select the new verified database
path in the service configuration; keep the failed database for investigation.

Operator release checklist:

- Assign named backup/recovery, incident and customer-support owners.
- Specify retention, backup deletion lag, encryption/key rotation, and monitored
  backup cadence based on the chosen recovery point and recovery time objectives.
- Restore a representative database and record measured results before launch and
  after migration changes; the repository unit test is only a small WAL fixture.
- Provision TLS, exact allowed origins, secure cookies, and required hosted secrets.
- Exercise payment activation, duplicate/out-of-order events, failed payments,
  cancellation, reactivation and recovery email in a provider sandbox.
- Supply truthful privacy/retention, model limitations, refund and support pages
  using the actual hosting/email/payment providers and approved business policies.
- Confirm alerts for failed backups, failed jobs, error spikes and provider delivery.

## Continuous validation

`validate-webui.yml` runs formatting, Clippy, Rust tests, generated-binding drift,
frontend type/lint/tests/build, and the WAL backup/restore fixture on pull requests.
It neither deploys nor applies production migrations. A green run does not replace
provider sandbox tests, access-control review or a timed operational restore drill.
