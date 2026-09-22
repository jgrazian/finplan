-- FinPlan v0 server schema.
--
-- The pre-release migration chain was consolidated into this baseline. Once a
-- v0 database has shipped, keep this file immutable and add a new numbered
-- migration for every subsequent schema or data change.
--
-- Design notes:
--   * Every entity carries a stable INTEGER (or TEXT uuid) primary key. The dense
--     u16 ids that `finplan_core` uses (AccountId, AssetId, EventId,
--     ReturnProfileId) are *simulation-local indices*, assigned at compile time by
--     `compile::IdMap`. They are deliberately NOT persisted.
--   * `AccountFlavor` is stored as class-table inheritance: one row in `accounts`
--     plus exactly one row in the matching `account_*` detail table.
--   * `ReturnProfile`, `EventTrigger`, `TransferAmount` and `EventEffect` are
--     recursive Rust enums, so they live in self-referential tables with CHECK
--     constraints asserting the columns each variant requires.

PRAGMA foreign_keys = ON;

-- ===========================================================================
-- Identity
-- ===========================================================================

-- The `default_*` columns are the user's own, not any scenario's: they seed
-- new work, and a scenario that has been created keeps whatever it was given.
CREATE TABLE users (
    id                     TEXT    PRIMARY KEY,
    email                  TEXT    NOT NULL UNIQUE COLLATE NOCASE,
    email_verified_at      TEXT,
    password_hash          TEXT    NOT NULL,
    display_name           TEXT,
    birth_date             TEXT,                       -- seeds a new scenario
    default_iterations     INTEGER NOT NULL DEFAULT 2000 CHECK (default_iterations > 0),
    default_duration_years INTEGER NOT NULL DEFAULT 30
                               CHECK (default_duration_years BETWEEN 1 AND 120),
    auto_run               INTEGER NOT NULL DEFAULT 0 CHECK (auto_run IN (0,1)),
    theme_mode             TEXT    NOT NULL DEFAULT 'system'
                                CHECK (theme_mode IN ('light', 'dark', 'system')),
    accent                 TEXT    NOT NULL DEFAULT 'blue'
                                CHECK (accent IN ('blue', 'green', 'purple')),
    created_at             TEXT    NOT NULL DEFAULT (datetime('now')),
    updated_at             TEXT    NOT NULL DEFAULT (datetime('now'))
);

-- Sessions store a SHA-256 of the opaque bearer token, never the token itself.
--
-- `public_id` is what a client is told a session is called, so listing the
-- devices signed in to an account never hands out anything derived from a
-- credential. The user agent is stored verbatim; naming the device from it is
-- presentation, and belongs in the client.
CREATE TABLE sessions (
    token_hash TEXT    PRIMARY KEY,
    public_id  TEXT    NOT NULL UNIQUE,
    user_id    TEXT    NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    user_agent TEXT,
    created_at TEXT    NOT NULL DEFAULT (datetime('now')),
    expires_at TEXT    NOT NULL,
    last_seen  TEXT    NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX idx_sessions_user ON sessions(user_id);
CREATE INDEX idx_sessions_expiry ON sessions(expires_at);

-- ===========================================================================
-- Contact messages
-- ===========================================================================

-- Private messages submitted from the application footer. Delivery and reply
-- workflows are deliberately separate: this table is the durable inbox.
CREATE TABLE contact_messages (
    id         INTEGER PRIMARY KEY,
    user_id    TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    topic      TEXT NOT NULL CHECK (topic IN ('question', 'feedback', 'bug_report')),
    message    TEXT NOT NULL CHECK (length(message) BETWEEN 1 AND 2000),
    status     TEXT NOT NULL DEFAULT 'new'
                    CHECK (status IN ('new', 'reviewed', 'closed')),
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Supports the per-user submission window without scanning the inbox.
CREATE INDEX idx_contact_messages_user_created
    ON contact_messages(user_id, created_at DESC);

-- Ready for a future administrator queue without adding a public read route.
CREATE INDEX idx_contact_messages_status_created
    ON contact_messages(status, created_at);

-- ===========================================================================
-- Distributions (shared by return profiles and inflation profiles)
-- ===========================================================================

CREATE TABLE distributions (
    id                INTEGER PRIMARY KEY,
    user_id           TEXT    NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind              TEXT    NOT NULL CHECK (kind IN (
                          'None','Fixed','Normal','LogNormal','StudentT',
                          'RegimeSwitching','Bootstrap')),

    rate              REAL,     -- Fixed
    mean              REAL,     -- Normal | LogNormal | StudentT
    std_dev           REAL,     -- Normal | LogNormal
    scale             REAL,     -- StudentT
    df                REAL,     -- StudentT

    bull_id           INTEGER REFERENCES distributions(id) ON DELETE CASCADE,
    bear_id           INTEGER REFERENCES distributions(id) ON DELETE CASCADE,
    bull_to_bear_prob REAL,
    bear_to_bull_prob REAL,

    history_preset    TEXT,     -- Bootstrap: 'sp500', 'us_agg_bonds', 'us_cpi', ...
    block_size        INTEGER,  -- Bootstrap: NULL/1 = i.i.d., >1 = block bootstrap

    created_at        TEXT NOT NULL DEFAULT (datetime('now')),

    CHECK (kind <> 'Fixed'    OR rate IS NOT NULL),
    CHECK (kind NOT IN ('Normal','LogNormal') OR (mean IS NOT NULL AND std_dev IS NOT NULL AND std_dev >= 0)),
    CHECK (kind <> 'StudentT' OR (mean IS NOT NULL AND scale IS NOT NULL AND df IS NOT NULL AND df > 0)),
    CHECK (kind <> 'RegimeSwitching' OR (
              bull_id IS NOT NULL AND bear_id IS NOT NULL
              AND bull_to_bear_prob BETWEEN 0 AND 1
              AND bear_to_bull_prob BETWEEN 0 AND 1)),
    CHECK (kind <> 'Bootstrap' OR (history_preset IS NOT NULL AND (block_size IS NULL OR block_size >= 1)))
);
CREATE INDEX idx_distributions_user ON distributions(user_id);

-- A user-level library of named return profiles, reusable across scenarios.
CREATE TABLE return_profiles (
    id              INTEGER PRIMARY KEY,
    user_id         TEXT    NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name            TEXT    NOT NULL,
    description     TEXT,
    asset_class     TEXT,
    distribution_id INTEGER NOT NULL REFERENCES distributions(id),
    sort_order      INTEGER NOT NULL DEFAULT 0,
    created_at      TEXT    NOT NULL DEFAULT (datetime('now')),
    updated_at      TEXT    NOT NULL DEFAULT (datetime('now')),
    UNIQUE (user_id, name)
);
CREATE INDEX idx_return_profiles_user ON return_profiles(user_id);

-- Inflation profiles accept a strict subset of distribution kinds; the compiler
-- rejects StudentT/RegimeSwitching because `InflationProfile` has no such variant.
CREATE TABLE inflation_profiles (
    id              INTEGER PRIMARY KEY,
    user_id         TEXT    NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name            TEXT    NOT NULL,
    description     TEXT,
    distribution_id INTEGER NOT NULL REFERENCES distributions(id),
    sort_order      INTEGER NOT NULL DEFAULT 0,
    created_at      TEXT    NOT NULL DEFAULT (datetime('now')),
    updated_at      TEXT    NOT NULL DEFAULT (datetime('now')),
    UNIQUE (user_id, name)
);
CREATE INDEX idx_inflation_profiles_user ON inflation_profiles(user_id);

-- ===========================================================================
-- Tax configuration
-- ===========================================================================

CREATE TABLE tax_configs (
    id                           INTEGER PRIMARY KEY,
    user_id                      TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name                         TEXT NOT NULL,
    description                  TEXT,
    state_rate                   REAL NOT NULL DEFAULT 0.0  CHECK (state_rate BETWEEN 0 AND 1),
    capital_gains_rate           REAL NOT NULL DEFAULT 0.15 CHECK (capital_gains_rate BETWEEN 0 AND 1),
    early_withdrawal_penalty_rate REAL NOT NULL DEFAULT 0.10 CHECK (early_withdrawal_penalty_rate BETWEEN 0 AND 1),
    created_at                   TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at                   TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE (user_id, name)
);
CREATE INDEX idx_tax_configs_user ON tax_configs(user_id);

CREATE TABLE tax_brackets (
    id            INTEGER PRIMARY KEY,
    tax_config_id INTEGER NOT NULL REFERENCES tax_configs(id) ON DELETE CASCADE,
    threshold     REAL    NOT NULL CHECK (threshold >= 0),
    rate          REAL    NOT NULL CHECK (rate BETWEEN 0 AND 1),
    UNIQUE (tax_config_id, threshold)
);
CREATE INDEX idx_tax_brackets_config ON tax_brackets(tax_config_id);

-- ===========================================================================
-- Scenarios
-- ===========================================================================

CREATE TABLE scenarios (
    id                   INTEGER PRIMARY KEY,
    user_id              TEXT    NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name                 TEXT    NOT NULL,
    description          TEXT,
    start_date           TEXT    NOT NULL,               -- ISO-8601 civil date
    birth_date           TEXT,                           -- required for Age triggers / RMD
    duration_years       INTEGER NOT NULL DEFAULT 30 CHECK (duration_years BETWEEN 1 AND 120),
    inflation_profile_id INTEGER REFERENCES inflation_profiles(id) ON DELETE SET NULL,
    tax_config_id        INTEGER REFERENCES tax_configs(id) ON DELETE SET NULL,
    collect_ledger       INTEGER NOT NULL DEFAULT 1 CHECK (collect_ledger IN (0,1)),
    created_at           TEXT    NOT NULL DEFAULT (datetime('now')),
    updated_at           TEXT    NOT NULL DEFAULT (datetime('now')),
    UNIQUE (user_id, name)
);
CREATE INDEX idx_scenarios_user ON scenarios(user_id);

-- ===========================================================================
-- Assets (scenario-scoped: prices are a property of the scenario)
-- ===========================================================================

CREATE TABLE assets (
    id                INTEGER PRIMARY KEY,
    scenario_id       INTEGER NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
    name              TEXT    NOT NULL,
    description       TEXT,
    initial_price     REAL    NOT NULL DEFAULT 1.0 CHECK (initial_price > 0),
    -- Null while the asset is unmapped: it has a price, but nothing yet making
    -- it move. A run compiles one at flat zero growth rather than refusing,
    -- which is what lets a ticker be created in passing and mapped later.
    return_profile_id INTEGER REFERENCES return_profiles(id),
    tracking_error    REAL    CHECK (tracking_error IS NULL OR tracking_error >= 0),
    sort_order        INTEGER NOT NULL DEFAULT 0,
    created_at        TEXT    NOT NULL DEFAULT (datetime('now')),
    updated_at        TEXT    NOT NULL DEFAULT (datetime('now')),
    UNIQUE (scenario_id, name)
);
CREATE INDEX idx_assets_scenario ON assets(scenario_id);

-- ===========================================================================
-- Accounts: one row here + exactly one row in the matching detail table
-- ===========================================================================

CREATE TABLE accounts (
    id          INTEGER PRIMARY KEY,
    scenario_id INTEGER NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
    name        TEXT    NOT NULL,
    description TEXT,
    flavor      TEXT    NOT NULL CHECK (flavor IN ('Bank','Investment','Property','Liability')),
    sort_order  INTEGER NOT NULL DEFAULT 0,
    created_at  TEXT    NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT    NOT NULL DEFAULT (datetime('now')),
    UNIQUE (scenario_id, name)
);
CREATE INDEX idx_accounts_scenario ON accounts(scenario_id);

CREATE TABLE account_bank (
    account_id        INTEGER PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
    cash_value        REAL    NOT NULL DEFAULT 0.0,
    return_profile_id INTEGER NOT NULL REFERENCES return_profiles(id)
);

CREATE TABLE account_investment (
    account_id             INTEGER PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
    tax_status             TEXT    NOT NULL CHECK (tax_status IN ('Taxable','TaxDeferred','TaxFree')),
    cash_value             REAL    NOT NULL DEFAULT 0.0,
    cash_return_profile_id INTEGER NOT NULL REFERENCES return_profiles(id),
    contribution_limit     REAL    CHECK (contribution_limit IS NULL OR contribution_limit >= 0),
    contribution_period    TEXT    CHECK (contribution_period IS NULL OR contribution_period IN ('Monthly','Yearly')),
    CHECK ((contribution_limit IS NULL) = (contribution_period IS NULL))
);

CREATE TABLE account_property (
    account_id INTEGER PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
    asset_id   INTEGER NOT NULL REFERENCES assets(id) ON DELETE RESTRICT,
    value      REAL    NOT NULL DEFAULT 0.0
);

CREATE TABLE account_liability (
    account_id    INTEGER PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
    principal     REAL NOT NULL DEFAULT 0.0 CHECK (principal >= 0),
    interest_rate REAL NOT NULL DEFAULT 0.0
);

-- Cost-basis lots inside investment accounts.
CREATE TABLE positions (
    id            INTEGER PRIMARY KEY,
    account_id    INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    asset_id      INTEGER NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    purchase_date TEXT    NOT NULL,
    units         REAL    NOT NULL CHECK (units >= 0),
    cost_basis    REAL    NOT NULL CHECK (cost_basis >= 0),
    sort_order    INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_positions_account ON positions(account_id);
CREATE INDEX idx_positions_asset ON positions(asset_id);

-- ===========================================================================
-- Transfer amounts: recursive expression tree
-- ===========================================================================

CREATE TABLE transfer_amounts (
    id          INTEGER PRIMARY KEY,
    scenario_id INTEGER NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
    kind        TEXT    NOT NULL CHECK (kind IN (
                    'Fixed','InflationAdjusted','SourceBalance','ZeroTargetBalance',
                    'TargetToBalance','AssetBalance','AccountTotalBalance',
                    'AccountCashBalance','Min','Max','Sub','Add','Mul','Scale')),

    value       REAL,     -- Fixed | TargetToBalance | Scale (the multiplier)
    account_id  INTEGER REFERENCES accounts(id) ON DELETE CASCADE,  -- *Balance refs
    asset_id    INTEGER REFERENCES assets(id)   ON DELETE CASCADE,  -- AssetBalance
    left_id     INTEGER REFERENCES transfer_amounts(id) ON DELETE CASCADE,
    right_id    INTEGER REFERENCES transfer_amounts(id) ON DELETE CASCADE,

    CHECK (kind NOT IN ('Fixed','TargetToBalance','Scale') OR value IS NOT NULL),
    CHECK (kind NOT IN ('InflationAdjusted','Scale') OR left_id IS NOT NULL),
    CHECK (kind NOT IN ('Min','Max','Sub','Add','Mul') OR (left_id IS NOT NULL AND right_id IS NOT NULL)),
    CHECK (kind <> 'AssetBalance' OR (account_id IS NOT NULL AND asset_id IS NOT NULL)),
    CHECK (kind NOT IN ('AccountTotalBalance','AccountCashBalance') OR account_id IS NOT NULL)
);
CREATE INDEX idx_transfer_amounts_scenario ON transfer_amounts(scenario_id);

-- ===========================================================================
-- Events, triggers, effects
-- ===========================================================================

CREATE TABLE events (
    id          INTEGER PRIMARY KEY,
    scenario_id INTEGER NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
    name        TEXT    NOT NULL,
    description TEXT,
    fires_once  INTEGER NOT NULL DEFAULT 0 CHECK (fires_once IN (0,1)),
    enabled     INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0,1)),
    sort_order  INTEGER NOT NULL DEFAULT 0,
    created_at  TEXT    NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT    NOT NULL DEFAULT (datetime('now')),
    UNIQUE (scenario_id, name)
);
CREATE INDEX idx_events_scenario ON events(scenario_id);

-- Recursive trigger tree. `event_id` is set on the root trigger of an event and
-- NULL on nested children, which are reached via parent_id / start_/end_ links.
CREATE TABLE triggers (
    id            INTEGER PRIMARY KEY,
    scenario_id   INTEGER NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
    event_id      INTEGER UNIQUE REFERENCES events(id) ON DELETE CASCADE,
    kind          TEXT    NOT NULL CHECK (kind IN (
                      'Date','Age','RelativeToEvent','AccountBalance','AssetBalance',
                      'NetWorth','And','Or','Repeating','Manual')),

    on_date       TEXT,     -- Date
    age_years     INTEGER CHECK (age_years IS NULL OR age_years BETWEEN 0 AND 130),
    age_months    INTEGER CHECK (age_months IS NULL OR age_months BETWEEN 0 AND 11),

    ref_event_id  INTEGER REFERENCES events(id) ON DELETE CASCADE, -- RelativeToEvent
    offset_unit   TEXT    CHECK (offset_unit IS NULL OR offset_unit IN ('Days','Months','Years')),
    offset_value  INTEGER,

    account_id    INTEGER REFERENCES accounts(id) ON DELETE CASCADE,
    asset_id      INTEGER REFERENCES assets(id)   ON DELETE CASCADE,
    comparison    TEXT    CHECK (comparison IS NULL OR comparison IN ('GreaterThanOrEqual','LessThanOrEqual')),
    threshold     REAL,

    interval      TEXT    CHECK (interval IS NULL OR interval IN
                      ('Never','Weekly','BiWeekly','Monthly','Quarterly','Yearly')),
    start_trigger_id INTEGER REFERENCES triggers(id) ON DELETE CASCADE,
    end_trigger_id   INTEGER REFERENCES triggers(id) ON DELETE CASCADE,
    max_occurrences  INTEGER CHECK (max_occurrences IS NULL OR max_occurrences > 0),

    -- Membership in an And/Or parent
    parent_id     INTEGER REFERENCES triggers(id) ON DELETE CASCADE,
    position      INTEGER NOT NULL DEFAULT 0,

    CHECK (kind <> 'Date' OR on_date IS NOT NULL),
    CHECK (kind <> 'Age'  OR age_years IS NOT NULL),
    CHECK (kind <> 'RelativeToEvent' OR (ref_event_id IS NOT NULL AND offset_unit IS NOT NULL AND offset_value IS NOT NULL)),
    CHECK (kind <> 'AccountBalance' OR (account_id IS NOT NULL AND comparison IS NOT NULL AND threshold IS NOT NULL)),
    CHECK (kind <> 'AssetBalance'   OR (account_id IS NOT NULL AND asset_id IS NOT NULL AND comparison IS NOT NULL AND threshold IS NOT NULL)),
    CHECK (kind <> 'NetWorth'       OR (comparison IS NOT NULL AND threshold IS NOT NULL)),
    CHECK (kind <> 'Repeating'      OR interval IS NOT NULL),
    -- A trigger is either an event root or a child of something, never both.
    CHECK (event_id IS NULL OR parent_id IS NULL)
);
CREATE INDEX idx_triggers_scenario ON triggers(scenario_id);
CREATE INDEX idx_triggers_parent ON triggers(parent_id, position);
CREATE INDEX idx_triggers_event ON triggers(event_id);

-- Recursive effect list. Top-level effects hang off `event_id` in `position`
-- order; the branches of a `Random` effect hang off `parent_id` + `parent_slot`.
CREATE TABLE effects (
    id           INTEGER PRIMARY KEY,
    scenario_id  INTEGER NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
    event_id     INTEGER REFERENCES events(id) ON DELETE CASCADE,
    parent_id    INTEGER REFERENCES effects(id) ON DELETE CASCADE,
    parent_slot  TEXT    CHECK (parent_slot IS NULL OR parent_slot IN ('on_true','on_false')),
    position     INTEGER NOT NULL DEFAULT 0,
    kind         TEXT    NOT NULL CHECK (kind IN (
                     'Income','Expense','AssetPurchase','AssetSale','Sweep',
                     'AdjustBalance','CashTransfer','TriggerEvent','PauseEvent',
                     'ResumeEvent','TerminateEvent','ApplyRmd','Random','RsuVesting',
                     'DeleteAccount')),

    from_account_id  INTEGER REFERENCES accounts(id) ON DELETE CASCADE,
    to_account_id    INTEGER REFERENCES accounts(id) ON DELETE CASCADE,
    asset_id         INTEGER REFERENCES assets(id)   ON DELETE CASCADE,
    amount_id        INTEGER REFERENCES transfer_amounts(id) ON DELETE CASCADE,
    target_event_id  INTEGER REFERENCES events(id) ON DELETE CASCADE,

    amount_mode  TEXT CHECK (amount_mode IS NULL OR amount_mode IN ('Gross','Net')),
    income_type  TEXT CHECK (income_type IS NULL OR income_type IN ('Taxable','TaxFree')),
    lot_method   TEXT CHECK (lot_method IS NULL OR lot_method IN
                     ('Fifo','Lifo','HighestCost','LowestCost','AverageCost')),

    probability  REAL CHECK (probability IS NULL OR probability BETWEEN 0 AND 1),
    units        REAL CHECK (units IS NULL OR units >= 0),
    sell_to_cover INTEGER CHECK (sell_to_cover IS NULL OR sell_to_cover IN (0,1)),

    CHECK ((event_id IS NULL) <> (parent_id IS NULL)),
    CHECK ((parent_id IS NULL) = (parent_slot IS NULL)),
    CHECK (kind <> 'Income'        OR (to_account_id IS NOT NULL AND amount_id IS NOT NULL AND income_type IS NOT NULL)),
    CHECK (kind <> 'Expense'       OR (from_account_id IS NOT NULL AND amount_id IS NOT NULL)),
    CHECK (kind <> 'AssetPurchase' OR (from_account_id IS NOT NULL AND to_account_id IS NOT NULL
                                       AND asset_id IS NOT NULL AND amount_id IS NOT NULL)),
    CHECK (kind <> 'AssetSale'     OR (from_account_id IS NOT NULL AND amount_id IS NOT NULL)),
    CHECK (kind <> 'Sweep'         OR (to_account_id IS NOT NULL AND amount_id IS NOT NULL AND income_type IS NOT NULL)),
    CHECK (kind <> 'AdjustBalance' OR (to_account_id IS NOT NULL AND amount_id IS NOT NULL)),
    CHECK (kind <> 'CashTransfer'  OR (from_account_id IS NOT NULL AND to_account_id IS NOT NULL AND amount_id IS NOT NULL)),
    CHECK (kind NOT IN ('TriggerEvent','PauseEvent','ResumeEvent','TerminateEvent') OR target_event_id IS NOT NULL),
    CHECK (kind <> 'ApplyRmd'      OR to_account_id IS NOT NULL),
    CHECK (kind <> 'Random'        OR probability IS NOT NULL),
    CHECK (kind <> 'RsuVesting'    OR (to_account_id IS NOT NULL AND asset_id IS NOT NULL AND units IS NOT NULL)),
    CHECK (kind <> 'DeleteAccount' OR to_account_id IS NOT NULL)
);
CREATE INDEX idx_effects_scenario ON effects(scenario_id);
CREATE INDEX idx_effects_event ON effects(event_id, position);
CREATE INDEX idx_effects_parent ON effects(parent_id);

-- `WithdrawalSources` for Sweep effects: header row + ordered member rows.
CREATE TABLE effect_withdrawal_sources (
    effect_id  INTEGER PRIMARY KEY REFERENCES effects(id) ON DELETE CASCADE,
    mode       TEXT NOT NULL CHECK (mode IN ('SingleAsset','SingleAccount','Strategy','Custom')),
    account_id INTEGER REFERENCES accounts(id) ON DELETE CASCADE,
    asset_id   INTEGER REFERENCES assets(id)   ON DELETE CASCADE,
    strategy   TEXT CHECK (strategy IS NULL OR strategy IN
                   ('TaxEfficientEarly','TaxDeferredFirst','TaxFreeFirst','ProRata','PenaltyAware')),

    CHECK (mode <> 'SingleAsset'   OR (account_id IS NOT NULL AND asset_id IS NOT NULL)),
    CHECK (mode <> 'SingleAccount' OR account_id IS NOT NULL),
    CHECK (mode <> 'Strategy'      OR strategy IS NOT NULL)
);

CREATE TABLE effect_withdrawal_source_items (
    id         INTEGER PRIMARY KEY,
    effect_id  INTEGER NOT NULL REFERENCES effects(id) ON DELETE CASCADE,
    role       TEXT    NOT NULL CHECK (role IN ('exclude','custom')),
    position   INTEGER NOT NULL DEFAULT 0,
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    asset_id   INTEGER REFERENCES assets(id) ON DELETE CASCADE,
    -- 'custom' entries name a concrete asset; 'exclude' entries name an account.
    CHECK (role <> 'custom' OR asset_id IS NOT NULL)
);
CREATE INDEX idx_wd_items_effect ON effect_withdrawal_source_items(effect_id, role, position);

-- ===========================================================================
-- Simulation runs
-- ===========================================================================

CREATE TABLE runs (
    id                    INTEGER PRIMARY KEY,
    scenario_id           INTEGER NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
    user_id               TEXT    NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    status                TEXT    NOT NULL DEFAULT 'queued'
                              CHECK (status IN ('queued','running','succeeded','failed','canceled')),
    iterations            INTEGER NOT NULL CHECK (iterations > 0),
    seed                  INTEGER,
    batch_size            INTEGER NOT NULL DEFAULT 100 CHECK (batch_size > 0),
    parallel_batches      INTEGER NOT NULL DEFAULT 4 CHECK (parallel_batches > 0),
    compute_mean          INTEGER NOT NULL DEFAULT 1 CHECK (compute_mean IN (0,1)),
    converge              INTEGER NOT NULL DEFAULT 0 CHECK (converge IN (0,1)),
    max_iterations        INTEGER CHECK (max_iterations IS NULL OR max_iterations > 0),
    completed_iterations  INTEGER NOT NULL DEFAULT 0,
    error_message         TEXT,
    input_hash            TEXT,
    model_version         TEXT,
    snapshot_json         TEXT,
    created_at            TEXT NOT NULL DEFAULT (datetime('now')),
    started_at            TEXT,
    finished_at           TEXT
);
CREATE INDEX idx_runs_scenario ON runs(scenario_id, id DESC);
CREATE INDEX idx_runs_status ON runs(status);

CREATE TABLE run_percentiles (
    run_id     INTEGER NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    percentile REAL    NOT NULL CHECK (percentile BETWEEN 0 AND 1),
    PRIMARY KEY (run_id, percentile)
);

CREATE TABLE run_stats (
    run_id                  INTEGER PRIMARY KEY REFERENCES runs(id) ON DELETE CASCADE,
    num_iterations          INTEGER NOT NULL,
    success_rate            REAL    NOT NULL,
    mean_final_net_worth    REAL    NOT NULL,
    std_dev_final_net_worth REAL    NOT NULL,
    min_final_net_worth     REAL    NOT NULL,
    max_final_net_worth     REAL    NOT NULL,
    lifetime_taxes          REAL    NOT NULL DEFAULT 0.0,
    converged               INTEGER CHECK (converged IS NULL OR converged IN (0,1)),
    convergence_metric      TEXT,
    convergence_value       REAL,
    funding_success_rate    REAL
        CHECK (funding_success_rate IS NULL OR funding_success_rate BETWEEN 0.0 AND 1.0)
);

CREATE TABLE run_percentile_values (
    run_id          INTEGER NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    percentile      REAL    NOT NULL,
    final_net_worth REAL    NOT NULL,
    PRIMARY KEY (run_id, percentile)
);

-- Net-worth bands: one row per (percentile, snapshot). `percentile` is NULL for
-- the mean path so bands and mean share a single table.
CREATE TABLE run_net_worth_points (
    id          INTEGER PRIMARY KEY,
    run_id      INTEGER NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    percentile  REAL,
    step        INTEGER NOT NULL,
    as_of_date  TEXT    NOT NULL,
    net_worth   REAL    NOT NULL
);
CREATE INDEX idx_nw_points ON run_net_worth_points(run_id, percentile, step);

-- Per-account decomposition of the same snapshots (drives the stacked chart).
CREATE TABLE run_account_points (
    id         INTEGER PRIMARY KEY,
    run_id     INTEGER NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    percentile REAL,
    account_id INTEGER NOT NULL,
    step       INTEGER NOT NULL,
    value      REAL    NOT NULL
);
CREATE INDEX idx_account_points ON run_account_points(run_id, percentile, step);

CREATE TABLE run_cash_flows (
    id            INTEGER PRIMARY KEY,
    run_id        INTEGER NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    percentile    REAL,
    year          INTEGER NOT NULL,
    income        REAL NOT NULL DEFAULT 0.0,
    expenses      REAL NOT NULL DEFAULT 0.0,
    contributions REAL NOT NULL DEFAULT 0.0,
    withdrawals   REAL NOT NULL DEFAULT 0.0,
    appreciation  REAL NOT NULL DEFAULT 0.0,
    net_cash_flow REAL NOT NULL DEFAULT 0.0
);
CREATE INDEX idx_cash_flows ON run_cash_flows(run_id, percentile, year);

CREATE TABLE run_taxes (
    id                        INTEGER PRIMARY KEY,
    run_id                    INTEGER NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    percentile                REAL,
    year                      INTEGER NOT NULL,
    ordinary_income           REAL NOT NULL DEFAULT 0.0,
    capital_gains             REAL NOT NULL DEFAULT 0.0,
    tax_free_withdrawals      REAL NOT NULL DEFAULT 0.0,
    federal_tax               REAL NOT NULL DEFAULT 0.0,
    state_tax                 REAL NOT NULL DEFAULT 0.0,
    total_tax                 REAL NOT NULL DEFAULT 0.0,
    early_withdrawal_penalties REAL NOT NULL DEFAULT 0.0
);
CREATE INDEX idx_run_taxes ON run_taxes(run_id, percentile, year);

CREATE TABLE run_warnings (
    id         INTEGER PRIMARY KEY,
    run_id     INTEGER NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    percentile REAL,
    position   INTEGER NOT NULL DEFAULT 0,
    kind       TEXT    NOT NULL,
    as_of_date TEXT,
    event_id   INTEGER,
    message    TEXT    NOT NULL
);
CREATE INDEX idx_run_warnings ON run_warnings(run_id);

-- Frozen labels keep historical account series readable after live accounts
-- are renamed or deleted.
CREATE TABLE run_account_labels (
    run_id    INTEGER NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    account_id INTEGER NOT NULL,
    name       TEXT    NOT NULL,
    sort_order INTEGER NOT NULL,
    PRIMARY KEY(run_id, account_id)
);

-- Cumulative inflation factor per plan year, per stored representative path.
CREATE TABLE run_inflation (
    id         INTEGER PRIMARY KEY,
    run_id     INTEGER NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    percentile REAL,
    year       INTEGER NOT NULL,
    factor     REAL    NOT NULL DEFAULT 1.0
);
CREATE INDEX idx_run_inflation ON run_inflation(run_id, percentile, year);

-- Itemized effects are retained only for the latest successful run in a
-- scenario. Account and event ids are run-local provenance, not live FKs.
CREATE TABLE run_ledger (
    id          INTEGER PRIMARY KEY,
    run_id      INTEGER NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    percentile  REAL,
    position    INTEGER NOT NULL,
    as_of_date  TEXT    NOT NULL,
    year        INTEGER NOT NULL,
    category    TEXT    NOT NULL,
    kind        TEXT    NOT NULL,
    detail      TEXT    NOT NULL,
    amount      REAL,
    basis       REAL,
    basis_label TEXT,
    account_id  INTEGER,
    event_id    INTEGER
);
CREATE INDEX idx_run_ledger ON run_ledger(run_id, percentile, year, position);

-- Independent all-path real-dollar statistics. Representative nominal paths
-- cannot be used to reconstruct these honestly.
CREATE TABLE run_real_stats (
    run_id        INTEGER PRIMARY KEY REFERENCES runs(id) ON DELETE CASCADE,
    base_date     TEXT    NOT NULL,
    num_iterations INTEGER NOT NULL,
    mean          REAL    NOT NULL,
    std_dev       REAL    NOT NULL,
    min           REAL    NOT NULL,
    max           REAL    NOT NULL
);

CREATE TABLE run_real_quantiles (
    run_id     INTEGER NOT NULL REFERENCES run_real_stats(run_id) ON DELETE CASCADE,
    as_of_date TEXT    NOT NULL,
    p5         REAL    NOT NULL,
    p50        REAL    NOT NULL,
    p95        REAL    NOT NULL,
    PRIMARY KEY (run_id, as_of_date),
    CHECK (p5 <= p50 AND p50 <= p95)
);

-- The newest sweep and its client-owned layout survive page reloads.
CREATE TABLE sweep_cache (
    scenario_id INTEGER PRIMARY KEY REFERENCES scenarios(id) ON DELETE CASCADE,
    user_id     TEXT    NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    results     TEXT    NOT NULL,
    created_at  TEXT    NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE sweep_layout (
    scenario_id INTEGER PRIMARY KEY REFERENCES scenarios(id) ON DELETE CASCADE,
    user_id     TEXT    NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    graphs      TEXT    NOT NULL,
    updated_at  TEXT    NOT NULL DEFAULT (datetime('now'))
);

-- Idempotency receipts for guided setup and archive imports.
CREATE TABLE setup_receipts (
    user_id      TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    request_id   TEXT NOT NULL,
    request_json TEXT NOT NULL,
    scenario_id  INTEGER NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
    PRIMARY KEY(user_id, request_id)
);

CREATE TABLE archive_imports (
    user_id     TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    request_id  TEXT NOT NULL,
    input_hash  TEXT NOT NULL,
    result_json TEXT NOT NULL,
    PRIMARY KEY (user_id, request_id)
);

-- Hosted authentication and billing state.
CREATE TABLE auth_action_tokens (
    token_hash TEXT PRIMARY KEY,
    user_id    TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    purpose    TEXT NOT NULL CHECK(purpose IN ('reset','verify')),
    email      TEXT NOT NULL,
    expires_at INTEGER NOT NULL
);
CREATE INDEX auth_action_user ON auth_action_tokens(user_id, purpose);

CREATE TABLE subscriptions (
    user_id         TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    provider        TEXT NOT NULL,
    subscription_id TEXT NOT NULL,
    state           TEXT NOT NULL CHECK(state IN ('active','canceling','past_due','expired')),
    access_until    INTEGER NOT NULL,
    revision        INTEGER NOT NULL,
    UNIQUE(provider, subscription_id)
);

CREATE TABLE billing_receipts (
    provider    TEXT NOT NULL,
    event_id    TEXT NOT NULL,
    received_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY(provider, event_id)
);

CREATE TABLE monthly_goal_seeks (
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    month   TEXT NOT NULL,
    used    INTEGER NOT NULL,
    PRIMARY KEY(user_id, month)
);

CREATE TABLE editable_plans (
    user_id     TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    scenario_id INTEGER NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE
);
