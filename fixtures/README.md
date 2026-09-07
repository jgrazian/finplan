# Scenario fixtures

`*.db` is gitignored, so the working database is not in the repository. These
files are: a scenario's inputs written out flat, so a test scenario survives a
wiped database, a schema migration, or a fresh checkout.

## The format

One JSON document per scenario, holding only what a person entered — profiles,
tax config, assets, accounts, positions, events, triggers, effects. Nothing
computed: no runs, no ledger, no statistics.

Every cross-reference is by **name**, never by rowid, and structures the schema
stores as parent pointers (distributions, trigger trees, effect trees, transfer
amounts) are inlined as trees. The file therefore says nothing about how SQLite
happens to lay the scenario out, which is what lets it outlive the schema.

Profiles and tax configs are user-scoped rather than scenario-scoped, so they
travel with the scenario — it means nothing without them.

## Writing one

```bash
./scripts/export-scenario.py --db finplan.db -o fixtures/default-scenario.json
```

The database name is optional when it holds exactly one scenario; otherwise pass
`--scenario NAME`.

## Rebuilding from one

The target user must already exist — register through the API first, since the
server hashes passwords.

```bash
./scripts/import-scenario.py fixtures/default-scenario.json \
    --db finplan.db --user you@example.com
```

Add `--name` to import under a different scenario name, or `--replace` to
overwrite one that is already there. Registering seeds a user with most of the
stock profiles; the importer reuses any profile whose name already exists,
syncs its ordering from the file, and warns rather than overwrites if the
existing profile's distribution disagrees.

`import-scenario.py` is the only half that knows the current SQLite schema. When
the schema changes, update the importer — the exported files stay valid.

## Checking a round trip

Import into a scratch database and export again; the two files should be
byte-identical.
