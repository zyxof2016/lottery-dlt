#!/usr/bin/env bash
# Additive schema upgrade; caller must have completed isolated workflow checks.
set -Eeuo pipefail
app_root=/data/apps/lottery-dlt
release_dir="$app_root/releases/20261016-db5622e34132"
db_path="$app_root/shared/data/lottery-dlt.sqlite3"
backup_path="$app_root/backups/pre-functional-fix-20261016.sqlite3"
expected_hash=db5622e34132b9897900e4d7f601612ddbf93a04f87ee40553c582d7002c727f
previous=$(readlink "$app_root/current")
test "$previous" != "$release_dir"
test ! -e "$backup_path"
test "$(sha256sum "$release_dir/lottery-dlt" | cut -d ' ' -f 1)" = "$expected_hash"
test -f "$app_root/staging/functional-audit-20261016/workflow/workflow-verification.json"
rollback() {
    trap - ERR
    set +e
    sudo -n systemctl stop lottery-dlt.service
    ln -s "$previous" "$app_root/current.rollback"
    mv -Tf "$app_root/current.rollback" "$app_root/current"
    sudo -n systemctl start lottery-dlt.service lottery-dlt-sync.timer
    printf 'Upgrade failed; restored previous binary. Additive schema and user data retained.\n' >&2
    exit 1
}
trap rollback ERR
sudo -n systemctl stop lottery-dlt-sync.timer lottery-dlt-sync.service lottery-dlt.service
python3 - "$db_path" "$backup_path" <<'PY'
import os, sqlite3, sys
fd=os.open(sys.argv[2], os.O_CREAT|os.O_EXCL|os.O_WRONLY, 0o600)
os.close(fd)
with sqlite3.connect(sys.argv[1]) as source, sqlite3.connect(sys.argv[2]) as target:
    source.backup(target)
    assert target.execute('PRAGMA integrity_check').fetchone()[0]=='ok'
print('Production DB backed up and verified')
PY
"$release_dir/lottery-dlt" --db "$db_path" verify
python3 - "$db_path" "$backup_path" <<'PY'
import json, sqlite3, sys
with sqlite3.connect(sys.argv[1]) as current, sqlite3.connect(sys.argv[2]) as previous:
    for table in ('draws','plans','settings','audit'):
        assert current.execute('SELECT * FROM '+table+' ORDER BY rowid').fetchall()==previous.execute('SELECT * FROM '+table+' ORDER BY rowid').fetchall(), table+' changed'
    assert current.execute('SELECT COUNT(*) FROM recommendation_batches').fetchone()[0]==0
    assert current.execute('PRAGMA integrity_check').fetchone()[0]=='ok'
    print(json.dumps({'production_data_preserved':True,'draws':current.execute('SELECT COUNT(*) FROM draws').fetchone()[0],'plans':current.execute('SELECT COUNT(*) FROM plans').fetchone()[0]}))
PY
ln -s "$release_dir" "$app_root/current.next"
mv -Tf "$app_root/current.next" "$app_root/current"
sudo -n systemctl start lottery-dlt.service
curl -fsS --retry 6 --retry-delay 1 --retry-connrefused http://127.0.0.1:6792/dlt/healthz > "$app_root/staging/functional-audit-20261016/health.json"
python3 - "$app_root/staging/functional-audit-20261016/health.json" <<'PY'
import json,sys
value=json.load(open(sys.argv[1]))
assert value['status']=='ok' and value['version']=='1.0.2'
print(json.dumps(value))
PY
sudo -n systemctl start lottery-dlt-sync.timer
trap - ERR
printf 'Functional fixes deployed; sync timer restored.\n'
