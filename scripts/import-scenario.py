#!/usr/bin/env python3
"""Rebuild a scenario in finplan.db from a flat file written by export-scenario.py.

The file names things the way a person does -- accounts, assets, profiles and
events by name -- so this script is the one place that has to know the current
SQLite schema. When the schema changes, update this importer; the exported
files stay valid.

User-scoped rows (return profiles, inflation profiles, tax configs) are reused
when a profile of that name already exists for the target user, and inserted
otherwise. The target user must already exist -- register through the API
first, since passwords are hashed by the server.

Usage:
    ./scripts/import-scenario.py fixtures/default-scenario.json \
        [--db finplan.db] [--user EMAIL] [--name NEW_NAME] [--replace]
"""

import argparse
import importlib.util
import json
import pathlib
import sqlite3
import sys


def _load_exporter():
    """Borrow the exporter's readers so both halves share one idea of the format."""
    path = pathlib.Path(__file__).resolve().with_name("export-scenario.py")
    spec = importlib.util.spec_from_file_location("finplan_export_scenario", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


_export = _load_exporter()

SUPPORTED_FORMAT_VERSIONS = {1}


def insert(cur, table, cols):
    keys = list(cols)
    cur.execute(
        f"INSERT INTO {table} ({', '.join(keys)}) VALUES ({', '.join('?' * len(keys))})",
        [cols[k] for k in keys],
    )
    return cur.lastrowid


def insert_distribution(cur, user_id, dist):
    if dist is None:
        return None
    kind = dist["kind"]
    cols = {"user_id": user_id, "kind": kind}
    if kind == "Fixed":
        cols["rate"] = dist["rate"]
    elif kind in ("Normal", "LogNormal"):
        cols["mean"] = dist["mean"]
        cols["std_dev"] = dist["std_dev"]
    elif kind == "StudentT":
        cols["mean"] = dist["mean"]
        cols["scale"] = dist["scale"]
        cols["df"] = dist["df"]
    elif kind == "RegimeSwitching":
        cols["bull_id"] = insert_distribution(cur, user_id, dist["bull"])
        cols["bear_id"] = insert_distribution(cur, user_id, dist["bear"])
        cols["bull_to_bear_prob"] = dist["bull_to_bear_prob"]
        cols["bear_to_bull_prob"] = dist["bear_to_bull_prob"]
    elif kind == "Bootstrap":
        cols["history_preset"] = dist["history_preset"]
        cols["block_size"] = dist.get("block_size")
    return insert(cur, "distributions", cols)


def insert_amount(cur, scenario_id, amt, ids):
    if amt is None:
        return None
    cols = {"scenario_id": scenario_id, "kind": amt["kind"]}
    if "value" in amt:
        cols["value"] = amt["value"]
    if "account" in amt:
        cols["account_id"] = ids["accounts"][amt["account"]]
    if "asset" in amt:
        cols["asset_id"] = ids["assets"][amt["asset"]]
    if "left" in amt:
        cols["left_id"] = insert_amount(cur, scenario_id, amt["left"], ids)
    if "right" in amt:
        cols["right_id"] = insert_amount(cur, scenario_id, amt["right"], ids)
    return insert(cur, "transfer_amounts", cols)


def insert_trigger(cur, scenario_id, trig, ids, event_id=None, parent_id=None, position=0):
    if trig is None:
        return None
    kind = trig["kind"]
    cols = {
        "scenario_id": scenario_id,
        "kind": kind,
        "event_id": event_id,
        "parent_id": parent_id,
        "position": position,
    }
    if kind == "Date":
        cols["on_date"] = trig["on_date"]
    elif kind == "Age":
        cols["age_years"] = trig["age_years"]
        cols["age_months"] = trig.get("age_months")
    elif kind == "RelativeToEvent":
        cols["ref_event_id"] = ids["events"][trig["ref_event"]]
        cols["offset_unit"] = trig["offset_unit"]
        cols["offset_value"] = trig["offset_value"]
    elif kind in ("AccountBalance", "AssetBalance", "NetWorth"):
        if "account" in trig:
            cols["account_id"] = ids["accounts"][trig["account"]]
        if "asset" in trig:
            cols["asset_id"] = ids["assets"][trig["asset"]]
        cols["comparison"] = trig["comparison"]
        cols["threshold"] = trig["threshold"]
    elif kind == "Repeating":
        cols["interval"] = trig["interval"]
        cols["max_occurrences"] = trig.get("max_occurrences")

    # A Repeating trigger's bounds are separate rows it points at, so they go in
    # before the row that references them.
    if kind == "Repeating":
        cols["start_trigger_id"] = insert_trigger(cur, scenario_id, trig.get("start"), ids)
        cols["end_trigger_id"] = insert_trigger(cur, scenario_id, trig.get("end"), ids)

    tid = insert(cur, "triggers", cols)

    for i, child in enumerate(trig.get("children", [])):
        insert_trigger(cur, scenario_id, child, ids, parent_id=tid, position=i)
    return tid


def insert_effect(cur, scenario_id, eff, ids, event_id=None,
                  parent_id=None, parent_slot=None, position=0):
    cols = {
        "scenario_id": scenario_id,
        "kind": eff["kind"],
        "event_id": event_id,
        "parent_id": parent_id,
        "parent_slot": parent_slot,
        "position": position,
    }
    if "from_account" in eff:
        cols["from_account_id"] = ids["accounts"][eff["from_account"]]
    if "to_account" in eff:
        cols["to_account_id"] = ids["accounts"][eff["to_account"]]
    if "asset" in eff:
        cols["asset_id"] = ids["assets"][eff["asset"]]
    if "target_event" in eff:
        cols["target_event_id"] = ids["events"][eff["target_event"]]
    if "amount" in eff:
        cols["amount_id"] = insert_amount(cur, scenario_id, eff["amount"], ids)
    for key in ("amount_mode", "income_type", "lot_method", "probability", "units"):
        if key in eff:
            cols[key] = eff[key]
    if "sell_to_cover" in eff:
        cols["sell_to_cover"] = int(eff["sell_to_cover"])

    eid = insert(cur, "effects", cols)

    src = eff.get("withdrawal_source")
    if src is not None:
        insert(cur, "effect_withdrawal_sources", {
            "effect_id": eid,
            "mode": src["mode"],
            "account_id": ids["accounts"][src["account"]] if "account" in src else None,
            "asset_id": ids["assets"][src["asset"]] if "asset" in src else None,
            "strategy": src.get("strategy"),
        })
        for role in ("exclude", "custom"):
            for i, item in enumerate(src.get(role, [])):
                insert(cur, "effect_withdrawal_source_items", {
                    "effect_id": eid,
                    "role": role,
                    "position": i,
                    "account_id": ids["accounts"][item["account"]],
                    "asset_id": ids["assets"][item["asset"]] if "asset" in item else None,
                })

    for slot in ("on_true", "on_false"):
        for i, child in enumerate(eff.get(slot, [])):
            insert_effect(cur, scenario_id, child, ids,
                          parent_id=eid, parent_slot=slot, position=i)
    return eid


def resolve_user(cur, email):
    if email:
        cur.execute("SELECT id FROM users WHERE email = ?", (email,))
        row = cur.fetchone()
        if row is None:
            raise SystemExit(f"no user with email {email!r}; register through the API first")
        return row[0]
    cur.execute("SELECT id, email FROM users")
    users = cur.fetchall()
    if len(users) != 1:
        raise SystemExit(f"{len(users)} users in this database; pass --user EMAIL")
    return users[0][0]


def reuse_or_insert_profile(cur, table, user_id, prof, extra_cols, warnings):
    """Keep a user's existing profile of that name; only its name is referenced.

    Registering seeds a user with most of these already, so a rebuild normally
    lands on rows that are there. The file still owns the ordering, which is the
    user's; a profile whose *numbers* disagree is reported rather than
    overwritten, since something else is already using it under that name.
    """
    cur.execute(f"SELECT id, distribution_id FROM {table} WHERE user_id = ? AND name = ?",
                (user_id, prof["name"]))
    row = cur.fetchone()
    if row is not None:
        pid, dist_id = row[0], row[1]
        if _export.distribution(cur, dist_id) != prof["distribution"]:
            warnings.append(
                f"{table[:-1].replace('_', ' ')} {prof['name']!r} already exists with a "
                f"different distribution; kept the existing one")
        cur.execute(f"UPDATE {table} SET sort_order = ? WHERE id = ?",
                    (prof.get("sort_order", 0), pid))
        return pid, True
    cols = {
        "user_id": user_id,
        "name": prof["name"],
        "description": prof.get("description"),
        "distribution_id": insert_distribution(cur, user_id, prof["distribution"]),
        "sort_order": prof.get("sort_order", 0),
    }
    cols.update(extra_cols)
    return insert(cur, table, cols), False


def import_doc(conn, doc, user_email=None, new_name=None, replace=False):
    if doc.get("format") != "finplan.scenario":
        raise SystemExit("not a finplan scenario file")
    version = doc.get("format_version")
    if version not in SUPPORTED_FORMAT_VERSIONS:
        raise SystemExit(f"unsupported format_version {version}")

    conn.row_factory = sqlite3.Row
    cur = conn.cursor()
    cur.execute("PRAGMA foreign_keys = ON")
    user_id = resolve_user(cur, user_email)

    scen = doc["scenario"]
    name = new_name or scen["name"]

    cur.execute("SELECT id FROM scenarios WHERE user_id = ? AND name = ?", (user_id, name))
    existing = cur.fetchone()
    if existing is not None:
        if not replace:
            raise SystemExit(
                f"a scenario named {name!r} already exists; pass --replace or --name NEW_NAME")
        cur.execute("DELETE FROM scenarios WHERE id = ?", (existing[0],))

    ids = {"accounts": {}, "assets": {}, "events": {},
           "return_profiles": {}, "inflation_profiles": {}, "tax_configs": {}}
    reused = []
    warnings = []

    for prof in doc.get("return_profiles", []):
        pid, was_reused = reuse_or_insert_profile(
            cur, "return_profiles", user_id, prof,
            {"asset_class": prof.get("asset_class")}, warnings)
        ids["return_profiles"][prof["name"]] = pid
        if was_reused:
            reused.append(f"return profile {prof['name']!r}")

    for prof in doc.get("inflation_profiles", []):
        pid, was_reused = reuse_or_insert_profile(
            cur, "inflation_profiles", user_id, prof, {}, warnings)
        ids["inflation_profiles"][prof["name"]] = pid
        if was_reused:
            reused.append(f"inflation profile {prof['name']!r}")

    for tax in doc.get("tax_configs", []):
        cur.execute("SELECT id FROM tax_configs WHERE user_id = ? AND name = ?",
                    (user_id, tax["name"]))
        row = cur.fetchone()
        if row is not None:
            ids["tax_configs"][tax["name"]] = row[0]
            reused.append(f"tax config {tax['name']!r}")
            continue
        tid = insert(cur, "tax_configs", {
            "user_id": user_id,
            "name": tax["name"],
            "description": tax.get("description"),
            "state_rate": tax["state_rate"],
            "capital_gains_rate": tax["capital_gains_rate"],
            "early_withdrawal_penalty_rate": tax["early_withdrawal_penalty_rate"],
        })
        ids["tax_configs"][tax["name"]] = tid
        for b in tax.get("brackets", []):
            insert(cur, "tax_brackets",
                   {"tax_config_id": tid, "threshold": b["threshold"], "rate": b["rate"]})

    scenario_id = insert(cur, "scenarios", {
        "user_id": user_id,
        "name": name,
        "description": scen.get("description"),
        "start_date": scen["start_date"],
        "birth_date": scen.get("birth_date"),
        "duration_years": scen["duration_years"],
        "inflation_profile_id": ids["inflation_profiles"].get(scen.get("inflation_profile")),
        "tax_config_id": ids["tax_configs"].get(scen.get("tax_config")),
        "collect_ledger": int(scen.get("collect_ledger", True)),
    })

    for asset in doc.get("assets", []):
        ids["assets"][asset["name"]] = insert(cur, "assets", {
            "scenario_id": scenario_id,
            "name": asset["name"],
            "description": asset.get("description"),
            "initial_price": asset["initial_price"],
            "return_profile_id": ids["return_profiles"].get(asset.get("return_profile")),
            "tracking_error": asset.get("tracking_error"),
            "sort_order": asset.get("sort_order", 0),
        })

    for acct in doc.get("accounts", []):
        aid = insert(cur, "accounts", {
            "scenario_id": scenario_id,
            "name": acct["name"],
            "description": acct.get("description"),
            "flavor": acct["flavor"],
            "sort_order": acct.get("sort_order", 0),
        })
        ids["accounts"][acct["name"]] = aid

        flavor = acct["flavor"]
        if flavor == "Bank":
            insert(cur, "account_bank", {
                "account_id": aid,
                "cash_value": acct["cash_value"],
                "return_profile_id": ids["return_profiles"][acct["return_profile"]],
            })
        elif flavor == "Investment":
            insert(cur, "account_investment", {
                "account_id": aid,
                "tax_status": acct["tax_status"],
                "cash_value": acct["cash_value"],
                "cash_return_profile_id": ids["return_profiles"][acct["cash_return_profile"]],
                "contribution_limit": acct.get("contribution_limit"),
                "contribution_period": acct.get("contribution_period"),
            })
        elif flavor == "Property":
            insert(cur, "account_property", {
                "account_id": aid,
                "asset_id": ids["assets"][acct["asset"]],
                "value": acct["value"],
            })
        elif flavor == "Liability":
            insert(cur, "account_liability", {
                "account_id": aid,
                "principal": acct["principal"],
                "interest_rate": acct["interest_rate"],
            })

        for pos in acct.get("positions", []):
            insert(cur, "positions", {
                "account_id": aid,
                "asset_id": ids["assets"][pos["asset"]],
                "purchase_date": pos["purchase_date"],
                "units": pos["units"],
                "cost_basis": pos["cost_basis"],
                "sort_order": pos.get("sort_order", 0),
            })

    # Events land first with no triggers or effects: both can name any other
    # event, so every id has to exist before either is built.
    for event in doc.get("events", []):
        ids["events"][event["name"]] = insert(cur, "events", {
            "scenario_id": scenario_id,
            "name": event["name"],
            "description": event.get("description"),
            "fires_once": int(event.get("fires_once", False)),
            "enabled": int(event.get("enabled", True)),
            "sort_order": event.get("sort_order", 0),
        })

    for event in doc.get("events", []):
        eid = ids["events"][event["name"]]
        if event.get("trigger"):
            insert_trigger(cur, scenario_id, event["trigger"], ids, event_id=eid)
        for i, eff in enumerate(event.get("effects", [])):
            insert_effect(cur, scenario_id, eff, ids, event_id=eid, position=i)

    return scenario_id, name, reused, warnings


def main():
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("file", help="scenario file from export-scenario.py")
    ap.add_argument("--db", default="finplan.db")
    ap.add_argument("--user", help="target user's email (optional when the db holds exactly one)")
    ap.add_argument("--name", help="import under a different scenario name")
    ap.add_argument("--replace", action="store_true",
                    help="delete an existing scenario of the same name first")
    args = ap.parse_args()

    with open(args.file) as fh:
        doc = json.load(fh)

    conn = sqlite3.connect(args.db)
    try:
        scenario_id, name, reused, warnings = import_doc(
            conn, doc, args.user, args.name, args.replace)
        conn.commit()
    except Exception:
        conn.rollback()
        raise
    finally:
        conn.close()

    for note in reused:
        print(f"reused existing {note}", file=sys.stderr)
    for note in warnings:
        print(f"warning: {note}", file=sys.stderr)
    print(f"imported {name!r} as scenario {scenario_id}", file=sys.stderr)


if __name__ == "__main__":
    main()
