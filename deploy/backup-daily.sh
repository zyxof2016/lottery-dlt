#!/usr/bin/env bash
set -Eeuo pipefail
backup_dir=${LOTTERY_BACKUP_DIR:-/data/apps/lottery-dlt/backups}
mkdir -p "$backup_dir"
stamp=$(date -u +%Y%m%d-%H%M%S)
/data/apps/lottery-dlt/current/lottery-dlt backup --out "$backup_dir/dlt-daily-$stamp.sqlite3"
python3 - "$backup_dir" <<'PY'
import pathlib,sys,time
root=pathlib.Path(sys.argv[1])
for path in root.glob('dlt-daily-*.sqlite3'):
    if path.is_file() and not path.is_symlink() and path.stat().st_mtime<time.time()-30*86400:
        path.unlink()
PY
