# Guest access: FinPlan without signing in

Status: implemented (2026-10-03), rollout steps 1–2; prod stays off until
step 3. First of three: this spec removes the login
wall with the server as it is today. [18](18_plan_crate.md) extracts a
database-free plan crate, and [19](19_local_first_wasm.md) moves plans and the
engine into the browser. Guest access is the bridge until 19 lands; 19 retires
it.

## Problem

`web/app/App.tsx` renders `LoginForm` whenever `/auth/me` returns 401, so a
first-time visitor sees a sign-in form before anything that explains or shows
the product. Every planning route takes `CurrentUser`, so there is no way in
without an account.

## Goals

- A first visit lands in the workbench, able to build a plan and run it,
  with no form.
- Guests cost little: small runs, no AI, and data that is deleted when it is
  abandoned.
- Signing up keeps what the guest built, with no export or import step.
- No change for signed-in users, self-hosted deployments or the TUI.

## Non-goals

- Keeping data on the device. Guest plans live on the server under a
  retention limit; [19](19_local_first_wasm.md) is the local-data answer. The
  "keep an archive copy in the browser" idea was dropped here because 19
  replaces it.
- Read-only demo plans for signed-out visitors (possible later; the
  onboarding flow already gives a guest a plan in a few clicks).

## Model: a guest is a user

A guest is a `users` row of a new kind with a normal session. Every existing
route keeps working because `CurrentUser` still resolves, and every table
already cascades from `users(id)`, so deleting a guest is one statement (as
`destroy_account` already relies on).

Migration `0019_guest_users.sql`:

```sql
ALTER TABLE users ADD COLUMN kind TEXT NOT NULL DEFAULT 'account'
    CHECK (kind IN ('account', 'guest'));
CREATE INDEX users_guest ON users(kind) WHERE kind = 'guest';
```

`users.email` and `password_hash` are `NOT NULL UNIQUE`. Relaxing them means
rebuilding `users` with foreign keys off, which sqlx's transactional
migrations make awkward and which risks every referencing table. Instead a
guest gets placeholders:

- `email = 'guest-<uuid>@guest.invalid'` (`.invalid` is reserved by RFC 2606,
  so it can never be a real mailbox), and
- `password_hash = '!'`, which `PasswordHash::new` rejects, so
  `verify_password` is always false.

Placeholders must never reach a person:

- `login`, `recovery::issue` and anything that sends mail refuse
  `kind = 'guest'` rows explicitly. Do not rely on the hash failing.
- `UserResponse` gains `guest: bool`, and the web shows "Guest" in place of
  the email and initials.
- `CurrentUser` gains `guest: bool`, read in the same query it already runs.

## Routes

`POST /auth/guest` creates a guest and its session. The web calls it when
`/auth/me` returns 401 and nothing else has happened yet. It:

1. Is refused with 403 when hosted and `FINPLAN_GUEST_ACCESS` is off (default
   on; self-hosted ignores it, see below).
2. Is limited per peer address through `auth::protection` (a new window
   keyed `guest:<ip>`, at most 5 guests per IP per hour) plus a global cap of
   guests created per minute. Hitting a limit returns 429, and the web falls
   back to the sign-in form, so the old behaviour is the floor.
3. Inserts the user, calls `seed::seed_user_library` as `register` does, and
   issues a session with `session::issue`.

`POST /auth/guest/claim` (`{email, password, password_confirmation,
display_name?}`) turns the caller's guest into an account in place: it sets
email, hash, display name and `kind = 'account'`, revokes the guest session
and issues a fresh one, the same as a login. Plans, runs and suggestions stay
because the user id doesn't change. An email that already exists gets "An
account with that email exists. Sign in to bring this plan with you." The
`registration_open` and `account_attempt` checks apply as in `register`.

Signing in to an existing account from a guest session is the other way to
keep a guest plan. Before calling `/auth/login`, the web fetches
`GET /archives` (the guest's own export) and holds it in memory. After the
login succeeds and the guest has active plans, it asks "Bring your guest plan
into this account?", and on yes posts the archive to `/archives/import` with
`name_prefix: "Guest – "`. Import already enforces plan slots and is
idempotent by `request_id`. On login, the server deletes the guest that owned
the cookie the request came with, so the guest is not left behind for the
janitor to collect.

## Entitlements

`billing::entitlements` checks for guests **before** `AccessMode`. In beta
mode, `pro` is true for every user today, and guests must not inherit that.
`Entitlements` gains `guest: bool`.

| Capability | Guest | Free (today) |
|---|---|---|
| `max_iterations` | `FINPLAN_GUEST_MAX_ITERATIONS`, default 100 | 1,000 |
| `saved_plan_limit` | 1 | 1 |
| Goal seeks per month | 0 | 1 |
| AI drafts, plan chat, AI review notes | off (`ai_drafts`, `ai_plan_chat` null) | quota |
| Document upload | off | with drafts |
| Rule-based review notes | on (cheap, no model) | on |
| Archive export | on | on |
| Archive import | on (the import's `saved_plan_limit` check applies) | on |
| Concurrent compute | 1 | 2 |

### One cap for every simulation path

The iteration ceiling is checked in four places today, and they disagree:

- `api/runs.rs` `create_run` uses `entitlements().max_iterations`.
- `api/preview.rs` clamps to `MAX_PREVIEW_ITERATIONS = 5_000` and never
  looks at entitlements, so a Free user's preview can run 5× their run cap
  already.
- `api/what_if.rs` clamps to `MAX_QUICK_ITERATIONS = 500`.
- `api/analysis.rs` `prepare` checks `config.max_iterations` and
  `MAX_ANALYSIS_ITERATIONS`.

Add `billing::iteration_cap(&Entitlements, &ServerConfig) -> usize` and clamp
every path to `min(path constant, iteration_cap)`. This also fixes the Free
preview gap above. The integration test asserts the cap on all four routes for
guest, Free and Pro.

### Compute admission

`admit_compute_inner` allows 2 jobs per user and `COMPUTE_LIMIT = 16` in
total, on top of `FINPLAN_SIM_WORKERS` (default 2). For guests:

- 1 concurrent job per guest, and
- guests together hold at most half of `COMPUTE_LIMIT`, so a burst of guests
  queues behind itself rather than in front of paying users.

The permit needs to know the caller is a guest; pass `CurrentUser.guest`
through `admit_compute_observed`.

### Iteration noise on screen

At 100 iterations a success rate of 80% has a 95% interval of about ±8
points. For guest runs, the Results header shows the interval next to the
rate ("80% ± 8") with "Sign up free for 1,000 iterations". The interval is
`1.96·√(p(1−p)/n)` from `num_iterations`, computed in `web/lib/view`. Guest
runs default to a fixed seed so re-running an unchanged plan doesn't move the
number.

## Retention

The hourly maintenance loop in `observability/runtime.rs` that calls
`purge_sessions` also calls a new `auth::guest::purge`:

- delete guests with no active scenario created more than 24 hours ago
  (drive-bys), and
- delete guests whose newest `sessions.last_seen` is more than
  `FINPLAN_GUEST_RETENTION_DAYS` (default 30, matching `SESSION_TTL_DAYS`) in
  the past, or who have no sessions left.

Log `maintenance.guests_purged` with a count. The web shows retention plainly
in a slim banner on every screen for guests: "Guest plan — deleted after 30
days without a visit. Create a free account to keep it." The banner also
appears on the Export action.

## Web

- `useSession`: on a 401 from `/auth/me`, call `api.auth.guest()` and treat
  the result as the user. A 403 or 429 shows `LoginForm` as today. A 401 later
  in the session still means expiry (`serverMonitor.sessionStarted` is called
  for guests too).
- Header: a "Sign up" primary button and a "Sign in" link instead of the
  account menu. Sign up opens the register form wired to `/auth/guest/claim`.
- Account screen, device list, password and billing: hidden for guests.
- Features that are off (AI, goal seek, analysis): keep the existing locked
  states, with copy that says "Create a free account" rather than "Upgrade".
- The guest banner, as above.

## Self-hosted

When `hosted` is false, guest access is on and uncapped by default
(`max_iterations` follows `config.max_iterations`, as it does for accounts).
A self-hosted single user then never sees a login. Claiming still works for
someone who later wants a password.

## Observability

- `AuthAction::GuestCreated`, `GuestClaimed`, `GuestAdopted` (signed in and
  imported) and `GuestPurged`, emitted through `telemetry.auth`.
- Conversion is measured as claimed plus adopted over created, by week.
  Purged-with-plan versus purged-empty shows whether guests got far enough to
  build something.
- Run metrics already carry the user. Add a `tier` label (guest/free/pro) to
  the run admission counters so guest load is visible on the Grafana board.

## Tests

- Guest creation, rate limit and the 403 when turned off.
- Claim: plans, runs and suggestions survive; session rotated; an existing
  email is refused; a placeholder email can't log in or request recovery.
- Login from a guest cookie deletes that guest, and import from its archive
  respects plan slots.
- `iteration_cap` on runs, preview, what-if and analysis for each tier
  (includes the Free preview fix).
- In beta mode guests are not `pro`.
- Purge: empty guest after 24h, idle guest after the retention window,
  accounts never touched.
- Bindings: `UserResponse.guest` and `Entitlements.guest` regenerated.

## Rollout

1. Server: migration, routes, entitlements, iteration cap, purge. Guest
   access off in prod (`FINPLAN_GUEST_ACCESS=false`).
2. Web: guest session, banner, claim and adopt flows.
3. Turn on in prod. Watch guest run load and the purge counts for a week
   before raising or lowering the cap.

## Implementation notes

Decisions made while building this that the sections above leave open:

- Guest limits apply only when `hosted`. A self-hosted guest is the local
  user: `pro`, uncapped, with AI if the operator configured it. Both kinds
  still get the retention banner, since purge runs everywhere.
- Guest sessions slide: each request moves `expires_at` 30 days out and
  `/auth/me` re-sends the cookie. Without that a guest who visited daily
  would still lose its plan 30 days after its first visit.
- A guest's `default_iterations` starts at the guest cap, so the run effort
  dial never opens above it; claim resets it to the account default.
- Adoption is counted when `POST /archives/import` carries `from_guest: true`
  and is not a replay, not at login.
- Guests are refused the whole Analysis tab (what-if and sweep need `pro`),
  not just goal seek.
- Purge waits an hour before treating a guest with no sessions as idle, since
  `POST /auth/guest` inserts the user just before it issues the session.
- Compute admission takes a `Tier`; `compute_admissions{tier,origin}` and the
  `tier` label on `compute_rejections` feed the dashboard's guest-load panel.

## Open questions

- 100 iterations, or 250 (±5 points instead of ±8)?
- Should guests get one goal seek to show off the feature? It costs a full
  solve.
