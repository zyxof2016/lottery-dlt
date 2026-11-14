#!/usr/bin/env bash
# Run on Ubuntu with a hash-verified release directory and passing isolated QA.
set -Eeuo pipefail
app_root=/data/apps/lottery-dlt
release_dir=${1:?release directory required}
case "$release_dir" in "$app_root"/releases/20261016-*) ;; *) exit 2 ;; esac
previous=$(readlink "$app_root/current")
backup_dir="$app_root/backups/pre-1.1.0-20261016"
test "$previous" != "$release_dir"
test ! -e "$backup_dir"
test -f "$app_root/staging/optimization-20261016/final-qa/optimization-verification.json"
mkdir -m 700 "$backup_dir"
cp "$app_root/shared/service.env" "$backup_dir/service.env"
for unit in lottery-dlt.service lottery-dlt-sync.service lottery-dlt-sync.timer lottery-dlt-backup.service lottery-dlt-backup.timer; do
    if [ -f "/etc/systemd/system/$unit" ]; then cp "/etc/systemd/system/$unit" "$backup_dir/$unit"; fi
done
rollback() {
    trap - ERR
    set +e
    sudo -n systemctl stop lottery-dlt.service lottery-dlt-sync.timer lottery-dlt-backup.timer
    cp "$backup_dir/service.env" "$app_root/shared/service.env"
    chmod 600 "$app_root/shared/service.env"
    for unit in lottery-dlt.service lottery-dlt-sync.service lottery-dlt-sync.timer lottery-dlt-backup.service lottery-dlt-backup.timer; do
        if [ -f "$backup_dir/$unit" ]; then sudo -n cp "$backup_dir/$unit" "/etc/systemd/system/$unit"; else sudo -n rm -f "/etc/systemd/system/$unit"; fi
    done
    ln -s "$previous" "$app_root/current.rollback"
    mv -Tf "$app_root/current.rollback" "$app_root/current"
    sudo -n systemctl daemon-reload
    sudo -n systemctl start lottery-dlt.service lottery-dlt-sync.timer
    printf 'Deployment failed; previous service configuration restored. Database retained; inspect release compatibility before further rollback.\n' >&2
    exit 1
}
trap rollback ERR
sudo -n systemctl stop lottery-dlt-sync.timer lottery-dlt-sync.service lottery-dlt.service
python3 - "$app_root" "$backup_dir" <<'PY'
import pathlib,sqlite3,sys
root=pathlib.Path(sys.argv[1]);backup=pathlib.Path(sys.argv[2])/'database.sqlite3'
with sqlite3.connect(root/'shared/data/lottery-dlt.sqlite3') as source,sqlite3.connect(backup) as target:
    source.backup(target)
    assert target.execute('PRAGMA integrity_check').fetchone()[0]=='ok'
backup.chmod(0o600)
env=root/'shared/service.env'
lines=[line for line in env.read_text().splitlines() if not line.startswith(('LOTTERY_REQUIRE_SCHEDULE=','LOTTERY_BACKUP_DIR='))]
env.write_text('\n'.join(lines)+f'\nLOTTERY_REQUIRE_SCHEDULE=1\nLOTTERY_BACKUP_DIR={root}/backups\n')
env.chmod(0o600)
print('Database and private service configuration backed up')
PY
"$release_dir/lottery-dlt" --db "$app_root/shared/data/lottery-dlt.sqlite3" verify
"$release_dir/lottery-dlt" --db "$app_root/shared/data/lottery-dlt.sqlite3" schedule > "$app_root/staging/optimization-20261016/production-schedule.json"
python3 - "$app_root" "$backup_dir" <<'PY'
import pathlib,sqlite3,sys
root=pathlib.Path(sys.argv[1]);backup=pathlib.Path(sys.argv[2])/'database.sqlite3'
with sqlite3.connect(root/'shared/data/lottery-dlt.sqlite3') as current,sqlite3.connect(backup) as previous:
    for table in ('draws','plans','settings','audit'):
        assert current.execute('SELECT * FROM '+table+' ORDER BY rowid').fetchall()==previous.execute('SELECT * FROM '+table+' ORDER BY rowid').fetchall(),table+' changed'
print('Original draw data, saved plans, seed and audit entries preserved')
PY
for unit in lottery-dlt.service lottery-dlt-sync.service lottery-dlt-sync.timer lottery-dlt-backup.service lottery-dlt-backup.timer; do
    sudo -n install -m 644 "$release_dir/$unit" "/etc/systemd/system/$unit"
done
ln -s "$release_dir" "$app_root/current.next"
mv -Tf "$app_root/current.next" "$app_root/current"
sudo -n systemctl daemon-reload
sudo -n systemctl start lottery-dlt.service
curl -fsS --retry 6 --retry-delay 1 --retry-connrefused http://127.0.0.1:6792/dlt/healthz > "$app_root/staging/optimization-20261016/production-health.json"
python3 - "$app_root/staging/optimization-20261016/production-health.json" <<'PY'
import json,sys
v=json.load(open(sys.argv[1]));assert v['status']=='ok' and v['version']=='1.1.0'
print(json.dumps(v))
PY
sudo -n systemctl enable --now lottery-dlt-sync.timer lottery-dlt-backup.timer
trap - ERR
sudo -n systemctl start lottery-dlt-backup.service
printf 'Version1.1.0 started; automation and verified daily backup timers enabled.\n'
