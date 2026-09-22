# Feedback beta deployment

This is a single-host deployment of the current Next.js UI and Rust API, with
SQLite on persistent storage and Caddy terminating HTTPS. It does not deploy the
old Ratzilla/Trunk application. Docker Engine with Compose v2 and a public host
with ports 80/443 reachable are required. Neither image publishes automatically.

## Before opening enrollment

- Choose the domain/host, point DNS at the host, and provision persistent storage.
- Configure a transactional SMTP provider and verified sender, including its DNS
  authentication records. Send and complete a real recovery and verification email
  before asking testers to enter private data. No payment provider is needed.
- Publish actual privacy/retention and beta/model-scope information. The repository
  cannot supply business policies or claim infrastructure guarantees on your behalf.
  The UI currently requests Google Fonts; account for that or self-host the fonts.
- Schedule encrypted off-host backups and test a restore; assign an operator to
  check failed jobs, delivery failures, free disk space, and Contact submissions.
- Decide enrollment: the example keeps registration closed. Opening it allows
  anyone to register; this switch is not an invitation or email-verification gate.
  Existing users can sign in while enrollment is closed.

## Start on the chosen host

From this directory, copy `.env.example` to `.env` and `server.env.example` to
`server.env`. Set the public hostname and SMTP settings; make the public mail URL
match the HTTPS hostname exactly. Protect both files with `chmod 600`. They are
ignored by Git; credentials are excluded from the Docker build context. Do not
paste the resolved Compose configuration into logs or support messages: it can
contain secrets.

```sh
docker compose build
docker compose up -d
docker compose ps
curl --fail https://YOUR_DOMAIN/api/health
```

Only Caddy exposes host ports. The API, SQLite and metrics remain private. The API
image runs as uid 10001; the named volume initializes with its owned `/data`.
Do not replace it with an unprepared host bind mount. The web image runs as the
Node user. Next embeds the API rewrite destination at build time; if the API's
internal address changes, rebuild with the corresponding `FINPLAN_API_ORIGIN`
build argument. Runtime environment changes alone do not rewrite the destination.

The default configuration keeps `FINPLAN_HOSTED=true` and secure cookies enabled.
`FINPLAN_ACCESS_MODE=beta` enables full planning access without subscriptions or
payments. It grants no payment-provider record and ends only when the operator
changes the policy. Switching to `subscription` applies the existing Free/Pro
rules; announce any such change to testers before doing it.

Beta keeps server capacity protections: 16 admitted jobs process-wide, at most 2
per user, plus bounded analysis sizes. The example caps runs at 10,000 iterations
and sets one worker per pool (runs and analyses have separate pools). Beta does
not impose a saved-plan or monthly goal-seek commercial quota. It is intended for
a small feedback cohort; sustained usage and retained data still need monitoring.
Run exactly one API replica against this database: compute admission is currently
process-local, and worker recovery assumes one service owner.

The server can start without SMTP for tests and other integrations, but this
recipe requires working mail before enrollment. SMTP always uses STARTTLS or
implicit TLS; plaintext transport is unsupported. Recovery messages include a
single-use token and instructions to paste it in the app, not a token-bearing URL.
For port 465 set `FINPLAN_SMTP_TLS=implicit` and `FINPLAN_SMTP_PORT=465` together.

After checks pass, set `FINPLAN_REGISTRATION_OPEN=true` in `server.env` and run
`docker compose up -d server`. To stop accepting new accounts, set it back to
`false` and recreate the server. Restarting alone does not reload changed env.

## Smoke test before inviting testers

1. With registration closed, verify a new signup is rejected and existing sign-in
   works. Then explicitly open enrollment for the test.
2. Register a fictional tester; verify secure session cookies and Feedback beta
   access. Check that there is no payment or upgrade prompt.
3. Complete the starter, simulate, compare/report, export, and restore inputs.
4. Request and complete email verification and password recovery. Confirm the old
   password/session no longer works. Check provider delivery records privately.
5. Submit a Contact message and confirm the operator can find it in the database.
6. Restart the API, sign in again if needed, and verify saved data and results.
7. Complete the backup/restore drill below and verify the restored plan in an
   isolated deployment. Keep new enrollment closed if any required check fails.

## Back up and restore

The image includes the existing WAL-aware backup script. Create a fresh destination
on each invocation; it intentionally refuses overwrites. Example from this directory:

```sh
mkdir -p backups
chmod 700 backups
docker compose exec server python3 /usr/local/bin/sqlite-backup.py backup /data/finplan.db /data/beta-backup.db
docker compose cp server:/data/beta-backup.db backups/beta-backup.db
chmod 600 backups/beta-backup.db
```

Use unique dated names for scheduled backups. The copy above is neither encrypted
nor off-host until the operator places it in configured backup storage. Retain
the checksum/count receipt privately. See [the operations runbook](../../spec/09_operations_runbook.md)
for isolated restores, migration rehearsal and rollback. Never delete the volume
with `docker compose down -v` during normal upgrades.

Before replacing images, back up and test migration against a restored copy.
Record the deployed Git revision and retain the matching images. Rollback requires
a compatible application and database backup, not just an older container tag.

## Feedback and operational checks

Contact submissions are persisted, not emailed automatically. Until a support
console is added, use a restricted SQLite connection or a protected backup to
review `contact_messages` (`id`, `user_id`, `topic`, `message`, `status`, `created_at`)
and update its status to `reviewed` or `closed`. Message bodies may contain private
information: do not copy them into general logs or analytics. The operator must
check this inbox during the beta; otherwise the Contact UI is a dead end.

The server emits JSON operational logs without plan contents. Check container
health and disk space, and configure notifications for service downtime, failures,
and backups. The optional Prometheus listener must remain on a private interface;
do not proxy it through Caddy. The current auth limiter sees the Next proxy as a
shared peer: the 30 auth POSTs/minute limit is effectively shared across testers.
Keep the initial cohort small; introduce explicit trusted-proxy handling before
relying on per-client throttling at higher traffic.

CI builds and smoke-tests both images. Docker was not available in the development
environment used to prepare this recipe; a green image job and the real-host smoke
test are required evidence before calling the deployment verified.
