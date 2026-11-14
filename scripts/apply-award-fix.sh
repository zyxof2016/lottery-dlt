#!/usr/bin/env bash
# Run on the Ubuntu host after uploading and reviewing repair-prizes dry-run.
set -Eeuo pipefail
app_root=/data/apps/lottery-dlt
release_dir="$app_root/releases/20261015-59e54690e83f"
db_path="$app_root/shared/data/lottery-dlt.sqlite3"
backup_path="$app_root/backups/pre-award-fix-20261015.sqlite3"
previous=$(readlink "$app_root/current")
test "$previous" != "$release_dir"
test ! -e "$backup_path"
backup_ready=0
rollback() {
    trap - ERR
    set +e
    sudo -n systemctl stop lottery-dlt.service
    if [ "$backup_ready" = 1 ]; then
        python3 - "$backup_path" "$db_path" <<'PY'
import sqlite3,sys
with sqlite3.connect(sys.argv[1]) as source,sqlite3.connect(sys.argv[2]) as target:
    source.backup(target)
PY
    fi
    ln -s "$previous" "$app_root/current.rollback"
    mv -Tf "$app_root/current.rollback" "$app_root/current"
    sudo -n systemctl start lottery-dlt.service lottery-dlt-sync.timer
    printf 'Award fix failed; restored the previous release and paused-state database backup.\n' >&2
    exit 1
}
trap rollback ERR
sudo -n systemctl stop lottery-dlt-sync.timer lottery-dlt-sync.service lottery-dlt.service
python3 - "$db_path" "$backup_path" <<'PY'
import os,sqlite3,sys
fd=os.open(sys.argv[2],os.O_CREAT|os.O_EXCL|os.O_WRONLY,0o600)
os.close(fd)
with sqlite3.connect(sys.argv[1]) as source,sqlite3.connect(sys.argv[2]) as target:
    source.backup(target)
    assert target.execute('PRAGMA integrity_check').fetchone()[0]=='ok'
print('Created and verified SQLite backup while writers were stopped')
PY
backup_ready=1
"$release_dir/lottery-dlt" --db "$db_path" repair-prizes --pages 50 --apply > "$app_root/staging/award-repair-applied.json"
"$release_dir/lottery-dlt" --db "$db_path" verify
python3 - "$db_path" "$backup_path" "$app_root/staging/award-repair-applied.json" <<'PY'
import json,sqlite3,sys
with sqlite3.connect(sys.argv[1]) as current,sqlite3.connect(sys.argv[2]) as previous:
    for table in ('plans','settings'):
        assert current.execute('SELECT * FROM '+table+' ORDER BY rowid').fetchall()==previous.execute('SELECT * FROM '+table+' ORDER BY rowid').fetchall(), table+' changed'
    def numbers(c):
        return [(d['issue'],d['date'],d['front'],d['back'],d['pool_before_fen']) for d in (json.loads(r[0]) for r in c.execute('SELECT body FROM draws ORDER BY issue'))]
    assert numbers(current)==numbers(previous),'Draw numbers changed'
    audit=previous.execute('SELECT * FROM audit ORDER BY seq').fetchall()
    assert current.execute('SELECT * FROM audit ORDER BY seq LIMIT ?', (len(audit),)).fetchall()==audit,'Old audit changed'
    repair=json.load(open(sys.argv[3]))
    assert current.execute('SELECT COUNT(*) FROM audit').fetchone()[0]==len(audit)+repair['changed']
    d=json.loads(current.execute("SELECT body FROM draws WHERE issue='25049'").fetchone()[0])
    assert d['awards']['9']['basic_fen']==500 and d['awards']['1']['basic_fen']==936863900
    print(json.dumps({'corrected_draws':repair['changed'],'draw_numbers_preserved':True,'saved_plans_preserved':True,'audit_chain_extended':True}))
PY
ln -s "$release_dir" "$app_root/current.next"
mv -Tf "$app_root/current.next" "$app_root/current"
sudo -n systemctl start lottery-dlt.service
curl -fsS --retry 6 --retry-delay 1 --retry-connrefused http://127.0.0.1:6792/dlt/healthz > "$app_root/staging/award-fix-health.json"
python3 - "$app_root/staging/award-fix-health.json" <<'PY'
import json,sys
p=json.load(open(sys.argv[1]))
assert p['status']=='ok' and p['version']=='1.0.1'
print(json.dumps(p))
PY
sudo -n systemctl start lottery-dlt-sync.timer
trap - ERR
printf 'Award fix deployed and verified; sync timer restored.\n'
