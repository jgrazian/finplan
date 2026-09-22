import importlib.util
from pathlib import Path
import sqlite3
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('backup', Path(__file__).with_name('sqlite-backup.py'))
backup = importlib.util.module_from_spec(spec)
spec.loader.exec_module(backup)


class BackupTests(unittest.TestCase):
    def test_wal_backup_restore_and_no_overwrite(self):
        with tempfile.TemporaryDirectory() as root:
            root = Path(root)
            source, saved, restored = [root / n for n in ('live.db', 'backup.db', 'restored.db')]
            with sqlite3.connect(source) as live:
                live.execute('PRAGMA journal_mode=WAL')
                live.execute('CREATE TABLE scenarios(id INTEGER PRIMARY KEY, name TEXT)')
                live.execute("INSERT INTO scenarios VALUES(1, 'Committed in WAL')")
                live.commit()
                result = backup.copy_database(source, saved)
                self.assertEqual(result['counts']['scenarios'], 1)
                backup.copy_database(saved, restored)
                with sqlite3.connect(restored) as db:
                    self.assertEqual(db.execute('SELECT name FROM scenarios').fetchone()[0], 'Committed in WAL')
                with self.assertRaises(ValueError):
                    backup.copy_database(source, saved)
                self.assertEqual(saved.stat().st_mode & 0o777, 0o600)


if __name__ == '__main__':
    unittest.main()
