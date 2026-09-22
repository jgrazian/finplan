#!/usr/bin/env python3
"""Consistent WAL-aware backup and isolated restore. Never overwrites a database.

Usage: sqlite-backup.py backup SOURCE DESTINATION
       sqlite-backup.py restore BACKUP NEW_DATABASE
Keep the destination filesystem private/encrypted. This tool does not encrypt.
"""
import argparse
from contextlib import closing
import hashlib
import json
import os
from pathlib import Path
import sqlite3
import tempfile
from datetime import datetime, timezone


def copy_database(source: Path, destination: Path) -> dict:
    source = source.resolve(strict=True)
    destination = destination.absolute()
    if destination.exists() or any(Path(str(destination) + suffix).exists() for suffix in ('-wal', '-shm')):
        raise ValueError('Destination or its SQLite sidecars already exist; choose a fresh path.')
    fd, temporary = tempfile.mkstemp(prefix='.finplan-backup-', dir=destination.parent)
    os.close(fd)
    try:
        with closing(sqlite3.connect(source.as_uri() + '?mode=ro', uri=True)) as src, closing(sqlite3.connect(temporary)) as dst:
            src.backup(dst)
            dst.execute('PRAGMA journal_mode=DELETE')
            integrity = dst.execute('PRAGMA integrity_check').fetchall()
            if integrity != [('ok',)]:
                raise ValueError(f'Integrity verification failed: {integrity}')
            foreign_keys = dst.execute('PRAGMA foreign_key_check').fetchall()
            if foreign_keys:
                raise ValueError('Foreign-key verification failed; no destination was published.')
            counts = {}
            for table in ('users', 'scenarios', 'runs'):
                if dst.execute('SELECT 1 FROM sqlite_master WHERE type=? AND name=?', ('table', table)).fetchone():
                    counts[table] = dst.execute(f'SELECT count(*) FROM {table}').fetchone()[0]
        # Link publishes atomically and fails if another process claimed the path.
        os.link(temporary, destination)
        digest = hashlib.sha256(destination.read_bytes()).hexdigest()
        return {'created_at': datetime.now(timezone.utc).isoformat(), 'sha256': digest, 'bytes': destination.stat().st_size, 'counts': counts, 'integrity': 'ok'}
    finally:
        os.unlink(temporary)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=('backup', 'restore'))
    parser.add_argument('source', type=Path)
    parser.add_argument('destination', type=Path)
    args = parser.parse_args()
    try:
        result = copy_database(args.source, args.destination)
    except (ValueError, OSError, sqlite3.Error) as error:
        parser.exit(1, f'{error}\n')
    print(json.dumps({'operation': args.operation, **result}, indent=2))


if __name__ == '__main__':
    main()
