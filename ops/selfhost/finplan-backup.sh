#!/usr/bin/env bash
# Online SQLite backups. Installed as /usr/local/sbin/finplan-backup and run as
# the finplan user by finplan-backup@{hourly,daily}.service.
#   hourly: overwrite backups/hourly.db (a recent hot copy)
#   daily:  write backups/daily-YYYY-MM-DD.db, keeping the newest 30
set -euo pipefail

DB=/var/lib/finplan/finplan.db
DIR=/var/lib/finplan/backups
TOOL=/opt/finplan/current/bin/sqlite-backup.py
KEEP_DAILY=30

[[ -f $DB ]] || { echo "no database yet"; exit 0; }
mkdir -p -m 700 "$DIR"

case ${1:-} in
hourly)
    # sqlite-backup.py never overwrites, so back up to a fresh name and swap it in.
    tmp=$DIR/.hourly-$$.db
    trap 'rm -f "$tmp"' EXIT
    python3 "$TOOL" backup "$DB" "$tmp"
    mv -f "$tmp" "$DIR/hourly.db"
    ;;
daily)
    dest=$DIR/daily-$(date +%F).db
    [[ -e $dest ]] && { echo "$dest already exists"; exit 0; }
    python3 "$TOOL" backup "$DB" "$dest"
    ls -1t "$DIR"/daily-*.db | tail -n +$((KEEP_DAILY + 1)) | xargs -r rm -f
    ;;
*)
    echo "usage: finplan-backup hourly|daily" >&2
    exit 2
    ;;
esac
