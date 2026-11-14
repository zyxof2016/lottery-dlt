#!/usr/bin/env bash
# After an optimization leaves a tested change, commit, push, and deploy it.
set -u
cat >/dev/null || true
root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$root"
log=$root/.cursor/hooks/release.log
mkdir -p "$(dirname "$log")"
exec 9>/tmp/lottery-dlt-release.lock
if ! flock -n 9; then
  printf '%s\n' '{}'
  exit 0
fi
if ! git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  printf '%s\n' '{}'
  exit 0
fi
if git diff --quiet && git diff --cached --quiet && [ -z "$(git ls-files --others --exclude-standard)" ]; then
  printf '%s\n' '{}'
  exit 0
fi
exec 3>&1
reply() { printf '%s\n' "$1" >&3; }
{
  set -e
  printf '\n== %s ==\n' "$(date --iso-8601=seconds)"
  if ! cargo test --locked --offline --all-targets; then
    reply '{"user_message":"测试未通过，未提交也未部署。"}'
    exit 0
  fi
  git add -A
  if git diff --cached --name-only | grep -Eq '(^|/)\.env$|service\.env$|\.sqlite3$'; then
    git reset -q
    reply '{"user_message":"暂存内容含环境文件或数据库，已停止自动提交和部署。"}'
    exit 0
  fi
  version=$(awk -F'"' '/^version = /{print $2; exit}' Cargo.toml)
  if ! git diff --cached --quiet; then
    git commit -m "$(cat <<EOF
Update lottery-dlt ${version}.

Automatic release after the working tree changed and tests passed.
EOF
)"
  fi
  git push origin HEAD
  "$root/scripts/deploy-release.sh"
  reply '{}'
} >>"$log" 2>&1 || reply '{"user_message":"自动提交、推送或部署没有完成，详情在 .cursor/hooks/release.log。"}'
exit 0