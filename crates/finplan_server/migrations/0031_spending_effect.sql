-- Dynamic spending (spec 22): `Spending` joins the effect kinds. The account
-- it pays from is from_account_id; its rule (no ids, so stored whole) is the
-- `SpendingRuleSpec` JSON in spending_rule; pass_through is 0 or 1. A rule
-- that reads named accounts lists them as withdrawal items with the new
-- 'portfolio' role; one without any reads every bank and investment account.
--
-- SQLite cannot change a CHECK in place, so both tables are rebuilt, written
-- like 0025 to be safe inside sqlx's transaction with foreign keys on:
--   * the new effects table's self-reference names the new table, so
--     dropping the old one cannot cascade into it;
--   * dropping the old effects table cascades away the withdrawal sources
--     and items, so they are kept aside and put back once the new tables are
--     in place (cleared first, so this also holds with foreign keys off).
PRAGMA defer_foreign_keys = ON;

CREATE TABLE effects_new (
    id           INTEGER PRIMARY KEY,
    scenario_id  INTEGER NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
    event_id     INTEGER REFERENCES events(id) ON DELETE CASCADE,
    parent_id    INTEGER REFERENCES effects_new(id) ON DELETE CASCADE,
    parent_slot  TEXT    CHECK (parent_slot IS NULL OR parent_slot IN ('on_true','on_false')),
    position     INTEGER NOT NULL DEFAULT 0,
    kind         TEXT    NOT NULL CHECK (kind IN (
                     'Income','Expense','AssetPurchase','AssetSale','Sweep',
                     'AdjustBalance','CashTransfer','TriggerEvent','PauseEvent',
                     'ResumeEvent','TerminateEvent','ApplyRmd','Random','RsuVesting',
                     'DeleteAccount','BuyProperty','SellProperty','MarketShock',
                     'RothConversion','Spending')),

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

    -- BuyProperty financing, and the loan a SellProperty pays off.
    loan_account_id        INTEGER REFERENCES accounts(id) ON DELETE CASCADE,
    down_payment_amount_id INTEGER REFERENCES transfer_amounts(id) ON DELETE CASCADE,
    term_months            INTEGER CHECK (term_months IS NULL OR term_months > 0),
    -- SellProperty terms.
    selling_cost_rate      REAL CHECK (selling_cost_rate IS NULL OR selling_cost_rate BETWEEN 0 AND 1),
    gain_exclusion         REAL CHECK (gain_exclusion IS NULL OR gain_exclusion >= 0),
    -- MarketShock: the fraction of market value lost.
    shock_drop             REAL CHECK (shock_drop IS NULL OR (shock_drop > 0 AND shock_drop < 1)),
    -- RothConversion: the bank or taxable account paying the tax; NULL
    -- withholds it from the conversion.
    pay_tax_from_account_id INTEGER REFERENCES accounts(id) ON DELETE CASCADE,
    -- Spending: the rule as JSON, and whether other flows pass through.
    spending_rule          TEXT CHECK (spending_rule IS NULL OR json_valid(spending_rule)),
    pass_through           INTEGER CHECK (pass_through IS NULL OR pass_through IN (0,1)),

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
    CHECK (kind <> 'DeleteAccount' OR to_account_id IS NOT NULL),
    -- BuyProperty: property in to_account_id, cash payer in from_account_id,
    -- price in amount_id; financing is all three loan columns or none.
    CHECK (kind <> 'BuyProperty'   OR (from_account_id IS NOT NULL AND to_account_id IS NOT NULL
                                       AND amount_id IS NOT NULL
                                       AND (loan_account_id IS NULL) = (down_payment_amount_id IS NULL)
                                       AND (loan_account_id IS NULL) = (term_months IS NULL))),
    -- SellProperty: property in from_account_id, proceeds to to_account_id.
    CHECK (kind <> 'SellProperty'  OR (from_account_id IS NOT NULL AND to_account_id IS NOT NULL
                                       AND selling_cost_rate IS NOT NULL AND gain_exclusion IS NOT NULL)),
    CHECK (kind <> 'MarketShock'   OR shock_drop IS NOT NULL),
    CHECK (kind <> 'RothConversion' OR (from_account_id IS NOT NULL AND to_account_id IS NOT NULL
                                        AND amount_id IS NOT NULL)),
    CHECK (kind = 'RothConversion' OR pay_tax_from_account_id IS NULL),
    -- Spending: an event's own effect (never a Random branch), with a rule.
    CHECK (kind <> 'Spending'      OR (from_account_id IS NOT NULL AND event_id IS NOT NULL
                                       AND spending_rule IS NOT NULL AND pass_through IS NOT NULL)),
    CHECK (kind = 'Spending'       OR (spending_rule IS NULL AND pass_through IS NULL))
);

INSERT INTO effects_new
    (id, scenario_id, event_id, parent_id, parent_slot, position, kind,
     from_account_id, to_account_id, asset_id, amount_id, target_event_id,
     amount_mode, income_type, lot_method, probability, units, sell_to_cover,
     loan_account_id, down_payment_amount_id, term_months, selling_cost_rate,
     gain_exclusion, shock_drop, pay_tax_from_account_id)
SELECT id, scenario_id, event_id, parent_id, parent_slot, position, kind,
       from_account_id, to_account_id, asset_id, amount_id, target_event_id,
       amount_mode, income_type, lot_method, probability, units, sell_to_cover,
       loan_account_id, down_payment_amount_id, term_months, selling_cost_rate,
       gain_exclusion, shock_drop, pay_tax_from_account_id
  FROM effects;

CREATE TEMP TABLE kept_withdrawal_sources AS SELECT * FROM effect_withdrawal_sources;
CREATE TEMP TABLE kept_withdrawal_items AS SELECT * FROM effect_withdrawal_source_items;

DROP TABLE effect_withdrawal_source_items;
DROP TABLE effects;
ALTER TABLE effects_new RENAME TO effects;

CREATE INDEX idx_effects_scenario ON effects(scenario_id);
CREATE INDEX idx_effects_event ON effects(event_id, position);
CREATE INDEX idx_effects_parent ON effects(parent_id);

CREATE TABLE effect_withdrawal_source_items (
    id         INTEGER PRIMARY KEY,
    effect_id  INTEGER NOT NULL REFERENCES effects(id) ON DELETE CASCADE,
    role       TEXT    NOT NULL CHECK (role IN ('exclude','custom','portfolio')),
    position   INTEGER NOT NULL DEFAULT 0,
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    asset_id   INTEGER REFERENCES assets(id) ON DELETE CASCADE,
    -- 'custom' entries name a concrete asset; 'exclude' and 'portfolio'
    -- entries name an account.
    CHECK (role <> 'custom' OR asset_id IS NOT NULL)
);
CREATE INDEX idx_wd_items_effect ON effect_withdrawal_source_items(effect_id, role, position);

DELETE FROM effect_withdrawal_sources;
INSERT INTO effect_withdrawal_sources SELECT * FROM kept_withdrawal_sources;
INSERT INTO effect_withdrawal_source_items SELECT * FROM kept_withdrawal_items;
DROP TABLE kept_withdrawal_sources;
DROP TABLE kept_withdrawal_items;

-- What a run's spending rule spent (results::SpendingResults), as JSON.
-- Summary-level like funding_diagnostics, so it survives the pruning of
-- superseded runs' path details. Null for a plan without a rule.
ALTER TABLE run_stats ADD COLUMN spending TEXT;
