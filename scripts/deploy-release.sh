#!/usr/bin/env bash
# Publish the already built release binary. The production database and token stay in place.
set -Eeuo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
host=${DEPLOY_HOST:-ubuntu@1.14.208.189}
app=/data/apps/lottery-dlt
version=$(awk -F'"' '/^version = /{print $2; exit}' Cargo.toml)
binary=$root/target/release/lottery-dlt
test -x "$binary"
sha=$(sha256sum "$binary" | awk '{print $1}')
name="$(date +%Y%m%d)-${sha:0:12}"
commit=$(git rev-parse HEAD)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
tar -C "$root" -czf "$tmp/source.tar.gz" \
  --exclude target --exclude data --exclude test-results --exclude .git \
  --exclude backups --exclude __pycache__ --exclude '*.sqlite3' .
cp deploy/lottery-dlt.service deploy/lottery-dlt-sync.service deploy/lottery-dlt-sync.timer \
  deploy/lottery-dlt-backup.service deploy/lottery-dlt-backup.timer deploy/backup-daily.sh "$tmp/"
python3 - "$tmp/release.json" "$version" "$sha" "$commit" "$name" <<'PY'
import json,sys
json.dump({"version":sys.argv[2],"binary_sha256":sys.argv[3],"git_commit":sys.argv[4],"release":sys.argv[5]}, open(sys.argv[1],"w"), ensure_ascii=False, indent=2)
open(sys.argv[1],"a").write("\n")
PY
ssh -o BatchMode=yes "$host" "mkdir -p '$app/releases/$name' '$app/staging' && test ! -e '$app/releases/$name/lottery-dlt'"
scp -o BatchMode=yes "$binary" "$tmp/lottery-dlt.service" "$tmp/lottery-dlt-sync.service" "$tmp/lottery-dlt-sync.timer" \
  "$tmp/lottery-dlt-backup.service" "$tmp/lottery-dlt-backup.timer" "$tmp/backup-daily.sh" "$tmp/source.tar.gz" "$tmp/release.json" \
  "$host:$app/releases/$name/"
ssh -o BatchMode=yes "$host" "APP='$app' NAME='$name' VERSION='$version' SHA='$sha' bash -s" <<'REMOTE'
set -Eeuo pipefail
release="$APP/releases/$NAME"
test "$(sha256sum "$release/lottery-dlt" | awk '{print $1}')" = "$SHA"
chmod 755 "$release/lottery-dlt"
if ldd "$release/lottery-dlt" | grep -F 'not found'; then exit 1; fi
test "$("$release/lottery-dlt" --version)" = "lottery-dlt $VERSION"
previous=$(readlink -f "$APP/current")
test "$previous" != "$release"
backup="$APP/backups/pre-$VERSION-$NAME"
test ! -e "$backup"
mkdir -m 700 "$backup"
cp "$APP/shared/service.env" "$backup/service.env"
chmod 600 "$backup/service.env"
sync_was_active=$(systemctl is-active lottery-dlt-sync.timer || true)
backup_was_active=$(systemctl is-active lottery-dlt-backup.timer || true)
rollback() {
    trap - ERR
    set +e
    sudo -n systemctl stop lottery-dlt.service
    ln -sfn "$previous" "$APP/current.rollback"
    mv -Tf "$APP/current.rollback" "$APP/current"
    sudo -n systemctl start lottery-dlt.service
    if [ "$sync_was_active" = active ]; then sudo -n systemctl start lottery-dlt-sync.timer; fi
    if [ "$backup_was_active" = active ]; then sudo -n systemctl start lottery-dlt-backup.timer; fi
    printf 'Deployment failed; previous release restored. Database backup remains at %s\n' "$backup" >&2
    exit 1
}
trap rollback ERR
sudo -n systemctl stop lottery-dlt-sync.timer lottery-dlt-backup.timer
while systemctl is-active --quiet lottery-dlt-sync.service; do sleep 1; done
sudo -n systemctl stop lottery-dlt.service
python3 - "$APP" "$backup/database.sqlite3" <<'PY'
import pathlib,sqlite3,sys
source=sqlite3.connect(f"file:{sys.argv[1]}/shared/data/lottery-dlt.sqlite3?mode=ro", uri=True)
target=sqlite3.connect(sys.argv[2])
source.backup(target)
assert target.execute("PRAGMA integrity_check").fetchone()[0]=="ok"
PY
chmod 600 "$backup/database.sqlite3"
cp "$backup/database.sqlite3" "$backup/verify.sqlite3"
chmod 600 "$backup/verify.sqlite3"
"$release/lottery-dlt" --db "$backup/verify.sqlite3" verify > "$APP/staging/$NAME-copy-verify.json"
python3 - "$APP/shared/data/lottery-dlt.sqlite3" "$backup/before.json" <<'PY'
import hashlib,json,sqlite3,sys
conn=sqlite3.connect(f"file:{sys.argv[1]}?mode=ro", uri=True)
out={}
for table,cols in {
    "draws":"issue,body","plans":"id,issue,kind,body,content_hash","settings":"id,body","audit":"seq,time,action,object_id,previous_hash,hash",
    "recommendation_batches":"issue,body,content_hash","operations":"key,body","schedules":"issue,body,content_hash","automation_events":"issue,body","jobs":"id,state,body,history"
}.items():
    rows=conn.execute(f"SELECT {cols} FROM {table} ORDER BY rowid").fetchall()
    out[table]={"count":len(rows),"sha256":hashlib.sha256(repr(rows).encode()).hexdigest()}
json.dump(out, open(sys.argv[2],"w"), indent=2)
PY
ln -s "$release" "$APP/current.next"
mv -Tf "$APP/current.next" "$APP/current"
sudo -n systemctl start lottery-dlt.service
curl -fsS --retry 8 --retry-delay 1 --retry-connrefused http://127.0.0.1:6792/dlt/healthz > "$APP/staging/$NAME-health.json"
python3 - "$APP/staging/$NAME-health.json" "$VERSION" <<'PY'
import json,sys
value=json.load(open(sys.argv[1]))
assert value["status"]=="ok" and value["version"]==sys.argv[2], value
print(json.dumps(value))
PY
"$release/lottery-dlt" --db "$APP/shared/data/lottery-dlt.sqlite3" verify > "$APP/staging/$NAME-live-verify.json"
python3 - "$APP/shared/data/lottery-dlt.sqlite3" "$backup/before.json" <<'PY'
import hashlib,json,sqlite3,sys
conn=sqlite3.connect(f"file:{sys.argv[1]}?mode=ro", uri=True)
before=json.load(open(sys.argv[2]))
for table,cols in {
    "draws":"issue,body","plans":"id,issue,kind,body,content_hash","settings":"id,body","audit":"seq,time,action,object_id,previous_hash,hash",
    "recommendation_batches":"issue,body,content_hash","operations":"key,body","schedules":"issue,body,content_hash","automation_events":"issue,body","jobs":"id,state,body,history"
}.items():
    rows=conn.execute(f"SELECT {cols} FROM {table} ORDER BY rowid").fetchall()
    digest=hashlib.sha256(repr(rows).encode()).hexdigest()
    assert before[table]["count"]==len(rows) and before[table]["sha256"]==digest, table+" changed"
print("draws, plans, settings, audit and operation records unchanged")
PY
trap - ERR
if [ "$sync_was_active" = active ]; then sudo -n systemctl start lottery-dlt-sync.timer; fi
if [ "$backup_was_active" = active ]; then sudo -n systemctl start lottery-dlt-backup.timer; fi
printf 'lottery-dlt %s is current (%s)\n' "$VERSION" "$NAME"
REMOTE
curl -fsS https://1.14.208.189/dlt/healthz
printf '\n'