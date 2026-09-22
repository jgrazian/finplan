#!/usr/bin/env python3
"""Export a scenario's *inputs* from finplan.db into a portable flat file.

The output is a schema-independent JSON document: every cross-reference is by
name (account name, asset name, return-profile name, event name), never by the
integer rowids the SQLite schema hands out. Nested structures (distributions,
trigger trees, effect trees, transfer amounts) are inlined as trees rather than
left as a table of parent pointers. Nothing derived is exported -- no runs, no
ledger, no statistics -- so the file describes only what a user typed in.

Usage:
    ./scripts/export-scenario.py [--db finplan.db] [--scenario NAME] [-o FILE]
"""

import argparse
import json
import sqlite3
import sys

FORMAT_VERSION = 1


def rows(cur, sql, args=()):
    cur.execute(sql, args)
    return [dict(r) for r in cur.fetchall()]


def one(cur, sql, args=()):
    got = rows(cur, sql, args)
    return got[0] if got else None


def prune(d):
    """Drop null/absent keys so the file shows only what is actually set."""
    return {k: v for k, v in d.items() if v is not None}


# --- distributions -----------------------------------------------------------

def distribution(cur, dist_id, seen=()):
    if dist_id is None:
        return None
    if dist_id in seen:
        raise ValueError(f"distribution cycle at id {dist_id}")
    d = one(cur, "SELECT * FROM distributions WHERE id = ?", (dist_id,))
    if d is None:
        raise ValueError(f"missing distribution {dist_id}")
    kind = d["kind"]
    out = {"kind": kind}
    if kind == "Fixed":
        out["rate"] = d["rate"]
    elif kind in ("Normal", "LogNormal"):
        out["mean"] = d["mean"]
        out["std_dev"] = d["std_dev"]
    elif kind == "StudentT":
        out["mean"] = d["mean"]
        out["scale"] = d["scale"]
        out["df"] = d["df"]
    elif kind == "RegimeSwitching":
        nxt = seen + (dist_id,)
        out["bull"] = distribution(cur, d["bull_id"], nxt)
        out["bear"] = distribution(cur, d["bear_id"], nxt)
        out["bull_to_bear_prob"] = d["bull_to_bear_prob"]
        out["bear_to_bull_prob"] = d["bear_to_bull_prob"]
    elif kind == "Bootstrap":
        out["history_preset"] = d["history_preset"]
        out["block_size"] = d["block_size"]
    return prune(out)


# --- transfer amounts --------------------------------------------------------

def amount(cur, amount_id, names, seen=()):
    if amount_id is None:
        return None
    if amount_id in seen:
        raise ValueError(f"transfer_amount cycle at id {amount_id}")
    a = one(cur, "SELECT * FROM transfer_amounts WHERE id = ?", (amount_id,))
    if a is None:
        raise ValueError(f"missing transfer_amount {amount_id}")
    kind = a["kind"]
    out = {"kind": kind}
    nxt = seen + (amount_id,)
    if kind in ("Fixed", "TargetToBalance", "Scale"):
        out["value"] = a["value"]
    if a["account_id"] is not None:
        out["account"] = names["accounts"][a["account_id"]]
    if a["asset_id"] is not None:
        out["asset"] = names["assets"][a["asset_id"]]
    if a["left_id"] is not None:
        out["left"] = amount(cur, a["left_id"], names, nxt)
    if a["right_id"] is not None:
        out["right"] = amount(cur, a["right_id"], names, nxt)
    return out


# --- triggers ----------------------------------------------------------------

def trigger(cur, trigger_id, names, seen=()):
    if trigger_id is None:
        return None
    if trigger_id in seen:
        raise ValueError(f"trigger cycle at id {trigger_id}")
    t = one(cur, "SELECT * FROM triggers WHERE id = ?", (trigger_id,))
    if t is None:
        raise ValueError(f"missing trigger {trigger_id}")
    kind = t["kind"]
    out = {"kind": kind}
    nxt = seen + (trigger_id,)
    if kind == "Date":
        out["on_date"] = t["on_date"]
    elif kind == "Age":
        out["age_years"] = t["age_years"]
        out["age_months"] = t["age_months"]
    elif kind == "RelativeToEvent":
        out["ref_event"] = names["events"][t["ref_event_id"]]
        out["offset_unit"] = t["offset_unit"]
        out["offset_value"] = t["offset_value"]
    elif kind in ("AccountBalance", "AssetBalance", "NetWorth"):
        if t["account_id"] is not None:
            out["account"] = names["accounts"][t["account_id"]]
        if t["asset_id"] is not None:
            out["asset"] = names["assets"][t["asset_id"]]
        out["comparison"] = t["comparison"]
        out["threshold"] = t["threshold"]
    elif kind in ("And", "Or"):
        kids = rows(cur, "SELECT id FROM triggers WHERE parent_id = ? ORDER BY position, id",
                    (trigger_id,))
        out["children"] = [trigger(cur, k["id"], names, nxt) for k in kids]
    elif kind == "Repeating":
        out["interval"] = t["interval"]
        out["start"] = trigger(cur, t["start_trigger_id"], names, nxt)
        out["end"] = trigger(cur, t["end_trigger_id"], names, nxt)
        out["max_occurrences"] = t["max_occurrences"]
    return prune(out)


# --- effects -----------------------------------------------------------------

def withdrawal_source(cur, effect_id, names):
    src = one(cur, "SELECT * FROM effect_withdrawal_sources WHERE effect_id = ?", (effect_id,))
    if src is None:
        return None
    out = {"mode": src["mode"]}
    if src["account_id"] is not None:
        out["account"] = names["accounts"][src["account_id"]]
    if src["asset_id"] is not None:
        out["asset"] = names["assets"][src["asset_id"]]
    if src["strategy"] is not None:
        out["strategy"] = src["strategy"]

    items = rows(cur, "SELECT * FROM effect_withdrawal_source_items "
                      "WHERE effect_id = ? ORDER BY role, position, id", (effect_id,))
    excludes, custom = [], []
    for it in items:
        entry = {"account": names["accounts"][it["account_id"]]}
        if it["asset_id"] is not None:
            entry["asset"] = names["assets"][it["asset_id"]]
        (custom if it["role"] == "custom" else excludes).append(entry)
    if excludes:
        out["exclude"] = excludes
    if custom:
        out["custom"] = custom
    return out


def effect(cur, row, names, seen=()):
    eid = row["id"]
    if eid in seen:
        raise ValueError(f"effect cycle at id {eid}")
    nxt = seen + (eid,)
    out = {"kind": row["kind"]}
    if row["from_account_id"] is not None:
        out["from_account"] = names["accounts"][row["from_account_id"]]
    if row["to_account_id"] is not None:
        out["to_account"] = names["accounts"][row["to_account_id"]]
    if row["asset_id"] is not None:
        out["asset"] = names["assets"][row["asset_id"]]
    if row["target_event_id"] is not None:
        out["target_event"] = names["events"][row["target_event_id"]]
    if row["amount_id"] is not None:
        out["amount"] = amount(cur, row["amount_id"], names)
    for col in ("amount_mode", "income_type", "lot_method", "probability", "units"):
        if row[col] is not None:
            out[col] = row[col]
    if row["sell_to_cover"] is not None:
        out["sell_to_cover"] = bool(row["sell_to_cover"])

    src = withdrawal_source(cur, eid, names)
    if src is not None:
        out["withdrawal_source"] = src

    for slot in ("on_true", "on_false"):
        kids = rows(cur, "SELECT * FROM effects WHERE parent_id = ? AND parent_slot = ? "
                         "ORDER BY position, id", (eid, slot))
        if kids:
            out[slot] = [effect(cur, k, names, nxt) for k in kids]
    return out


# --- accounts ----------------------------------------------------------------

def account(cur, row, names):
    aid, flavor = row["id"], row["flavor"]
    out = prune({
        "name": row["name"],
        "description": row["description"],
        "flavor": flavor,
        "sort_order": row["sort_order"],
    })

    if flavor == "Bank":
        d = one(cur, "SELECT * FROM account_bank WHERE account_id = ?", (aid,))
        out["cash_value"] = d["cash_value"]
        out["return_profile"] = names["return_profiles"][d["return_profile_id"]]
    elif flavor == "Investment":
        d = one(cur, "SELECT * FROM account_investment WHERE account_id = ?", (aid,))
        out["tax_status"] = d["tax_status"]
        out["cash_value"] = d["cash_value"]
        out["cash_return_profile"] = names["return_profiles"][d["cash_return_profile_id"]]
        if d["contribution_limit"] is not None:
            out["contribution_limit"] = d["contribution_limit"]
            out["contribution_period"] = d["contribution_period"]
    elif flavor == "Property":
        d = one(cur, "SELECT * FROM account_property WHERE account_id = ?", (aid,))
        out["asset"] = names["assets"][d["asset_id"]]
        out["value"] = d["value"]
    elif flavor == "Liability":
        d = one(cur, "SELECT * FROM account_liability WHERE account_id = ?", (aid,))
        out["principal"] = d["principal"]
        out["interest_rate"] = d["interest_rate"]

    positions = rows(cur, "SELECT * FROM positions WHERE account_id = ? ORDER BY sort_order, id",
                     (aid,))
    if positions:
        out["positions"] = [{
            "asset": names["assets"][p["asset_id"]],
            "purchase_date": p["purchase_date"],
            "units": p["units"],
            "cost_basis": p["cost_basis"],
            "sort_order": p["sort_order"],
        } for p in positions]
    return out


# --- top level ---------------------------------------------------------------

def export(conn, scenario_name=None):
    conn.row_factory = sqlite3.Row
    cur = conn.cursor()

    if scenario_name:
        scenario = one(cur, "SELECT * FROM scenarios WHERE name = ?", (scenario_name,))
        if scenario is None:
            raise SystemExit(f"no scenario named {scenario_name!r}")
    else:
        all_scenarios = rows(cur, "SELECT * FROM scenarios ORDER BY id")
        if len(all_scenarios) != 1:
            raise SystemExit(
                f"{len(all_scenarios)} scenarios in this database; pass --scenario NAME")
        scenario = all_scenarios[0]

    sid, uid = scenario["id"], scenario["user_id"]

    # id -> name lookups, so nothing downstream ever emits a rowid.
    names = {
        "accounts": {r["id"]: r["name"] for r in
                     rows(cur, "SELECT id, name FROM accounts WHERE scenario_id = ?", (sid,))},
        "assets": {r["id"]: r["name"] for r in
                   rows(cur, "SELECT id, name FROM assets WHERE scenario_id = ?", (sid,))},
        "events": {r["id"]: r["name"] for r in
                   rows(cur, "SELECT id, name FROM events WHERE scenario_id = ?", (sid,))},
        "return_profiles": {r["id"]: r["name"] for r in
                            rows(cur, "SELECT id, name FROM return_profiles WHERE user_id = ?", (uid,))},
        "inflation_profiles": {r["id"]: r["name"] for r in
                               rows(cur, "SELECT id, name FROM inflation_profiles WHERE user_id = ?", (uid,))},
        "tax_configs": {r["id"]: r["name"] for r in
                        rows(cur, "SELECT id, name FROM tax_configs WHERE user_id = ?", (uid,))},
    }

    doc = {
        "format": "finplan.scenario",
        "format_version": FORMAT_VERSION,
        "scenario": prune({
            "name": scenario["name"],
            "description": scenario["description"],
            "start_date": scenario["start_date"],
            "birth_date": scenario["birth_date"],
            "duration_years": scenario["duration_years"],
            "collect_ledger": bool(scenario["collect_ledger"]),
            "inflation_profile": names["inflation_profiles"].get(scenario["inflation_profile_id"]),
            "tax_config": names["tax_configs"].get(scenario["tax_config_id"]),
        }),
    }

    # Profiles and tax configs hang off the user, not the scenario, so they are
    # carried along whole -- the scenario is meaningless without them.
    doc["return_profiles"] = [prune({
        "name": p["name"],
        "description": p["description"],
        "asset_class": p["asset_class"],
        "sort_order": p["sort_order"],
        "distribution": distribution(cur, p["distribution_id"]),
    }) for p in rows(cur, "SELECT * FROM return_profiles WHERE user_id = ? ORDER BY sort_order, id", (uid,))]

    doc["inflation_profiles"] = [prune({
        "name": p["name"],
        "description": p["description"],
        "sort_order": p["sort_order"],
        "distribution": distribution(cur, p["distribution_id"]),
    }) for p in rows(cur, "SELECT * FROM inflation_profiles WHERE user_id = ? ORDER BY sort_order, id", (uid,))]

    doc["tax_configs"] = [prune({
        "name": t["name"],
        "description": t["description"],
        "state_rate": t["state_rate"],
        "capital_gains_rate": t["capital_gains_rate"],
        "early_withdrawal_penalty_rate": t["early_withdrawal_penalty_rate"],
        "brackets": [{"threshold": b["threshold"], "rate": b["rate"]} for b in
                     rows(cur, "SELECT * FROM tax_brackets WHERE tax_config_id = ? ORDER BY threshold",
                          (t["id"],))],
    }) for t in rows(cur, "SELECT * FROM tax_configs WHERE user_id = ? ORDER BY id", (uid,))]

    doc["assets"] = [prune({
        "name": a["name"],
        "description": a["description"],
        "initial_price": a["initial_price"],
        "return_profile": names["return_profiles"].get(a["return_profile_id"]),
        "tracking_error": a["tracking_error"],
        "sort_order": a["sort_order"],
    }) for a in rows(cur, "SELECT * FROM assets WHERE scenario_id = ? ORDER BY sort_order, id", (sid,))]

    doc["accounts"] = [account(cur, a, names) for a in
                       rows(cur, "SELECT * FROM accounts WHERE scenario_id = ? ORDER BY sort_order, id", (sid,))]

    events = []
    for e in rows(cur, "SELECT * FROM events WHERE scenario_id = ? ORDER BY sort_order, id", (sid,)):
        root = one(cur, "SELECT id FROM triggers WHERE event_id = ?", (e["id"],))
        effects = rows(cur, "SELECT * FROM effects WHERE event_id = ? ORDER BY position, id", (e["id"],))
        events.append(prune({
            "name": e["name"],
            "description": e["description"],
            "fires_once": bool(e["fires_once"]),
            "enabled": bool(e["enabled"]),
            "sort_order": e["sort_order"],
            "trigger": trigger(cur, root["id"], names) if root else None,
            "effects": [effect(cur, f, names) for f in effects],
        }))
    doc["events"] = events
    return doc


def connect_readonly(path):
    """Open the database without writing to it.

    A WAL database needs a shared-memory file alongside it, which a strictly
    read-only connection cannot create; when that is the case fall back to a
    normal connection (this script only ever reads).
    """
    try:
        conn = sqlite3.connect(f"file:{path}?mode=ro", uri=True)
        conn.execute("SELECT 1 FROM sqlite_master LIMIT 1")
        return conn
    except sqlite3.OperationalError:
        return sqlite3.connect(path)


def main():
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--db", default="finplan.db")
    ap.add_argument("--scenario", help="scenario name (optional when the db holds exactly one)")
    ap.add_argument("-o", "--out", help="output path (default: stdout)")
    args = ap.parse_args()

    conn = connect_readonly(args.db)
    try:
        doc = export(conn, args.scenario)
    finally:
        conn.close()

    text = json.dumps(doc, indent=2, ensure_ascii=False) + "\n"
    if args.out:
        with open(args.out, "w") as fh:
            fh.write(text)
        print(f"wrote {args.out}", file=sys.stderr)
    else:
        sys.stdout.write(text)


if __name__ == "__main__":
    main()
