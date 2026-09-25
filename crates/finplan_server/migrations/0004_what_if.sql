-- What-if: the MarketShock effect, and a per-scenario override stack.
--
-- `effects` is rebuilt once more to admit the new kind and its column, exactly
-- as 0003 did (see there for why the rebuild is safe with foreign keys on and
-- why withdrawal sources are set aside around the drop).

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
                     'DeleteAccount','BuyProperty','SellProperty','MarketShock')),

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
    CHECK (kind <> 'MarketShock'   OR shock_drop IS NOT NULL)
);

INSERT INTO effects_new
    (id, scenario_id, event_id, parent_id, parent_slot, position, kind,
     from_account_id, to_account_id, asset_id, amount_id, target_event_id,
     amount_mode, income_type, lot_method, probability, units, sell_to_cover,
     loan_account_id, down_payment_amount_id, term_months, selling_cost_rate, gain_exclusion)
SELECT id, scenario_id, event_id, parent_id, parent_slot, position, kind,
       from_account_id, to_account_id, asset_id, amount_id, target_event_id,
       amount_mode, income_type, lot_method, probability, units, sell_to_cover,
       loan_account_id, down_payment_amount_id, term_months, selling_cost_rate, gain_exclusion
  FROM effects;

CREATE TEMP TABLE saved_withdrawal_sources AS SELECT * FROM effect_withdrawal_sources;
CREATE TEMP TABLE saved_withdrawal_items AS SELECT * FROM effect_withdrawal_source_items;

DROP TABLE effects;
ALTER TABLE effects_new RENAME TO effects;

INSERT OR IGNORE INTO effect_withdrawal_sources SELECT * FROM temp.saved_withdrawal_sources;
INSERT OR IGNORE INTO effect_withdrawal_source_items SELECT * FROM temp.saved_withdrawal_items;
DROP TABLE temp.saved_withdrawal_sources;
DROP TABLE temp.saved_withdrawal_items;

CREATE INDEX idx_effects_scenario ON effects(scenario_id);
CREATE INDEX idx_effects_event ON effects(event_id, position);
CREATE INDEX idx_effects_parent ON effects(parent_id);

-- The What-if stack a user is building over a scenario: a JSON document of
-- override layers, written and read whole. Like sweep_layout, it belongs to
-- the scenario and goes with it.
CREATE TABLE what_if_stacks (
    scenario_id INTEGER PRIMARY KEY REFERENCES scenarios(id) ON DELETE CASCADE,
    user_id     TEXT    NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    stack       TEXT    NOT NULL,
    updated_at  TEXT    NOT NULL DEFAULT (datetime('now'))
);
