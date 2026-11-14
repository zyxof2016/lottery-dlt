#!/usr/bin/env python3
"""Exercise writes only against a disposable DB and a separate loopback service."""
import argparse
from datetime import datetime, timedelta, timezone
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import tempfile
import time
import urllib.request

from playwright.sync_api import expect, sync_playwright


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--browser', required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--env-file', type=Path, default=Path('/data/apps/lottery-dlt/shared/service.env'))
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    config = dict(line.split('=', 1) for line in args.env_file.read_text().splitlines() if '=' in line)
    token = config['LOTTERY_ADMIN_TOKEN']
    production_db = Path(config['LOTTERY_DB'])
    with tempfile.TemporaryDirectory(prefix='dlt-isolated-workflow-', dir=args.output) as scratch:
        database = Path(scratch) / 'test.sqlite3'
        with sqlite3.connect(production_db) as source, sqlite3.connect(database) as target:
            source.backup(target)
            target.execute('DELETE FROM plans')
            target.execute('DELETE FROM audit')
            target.execute('CREATE TABLE IF NOT EXISTS operations (key TEXT PRIMARY KEY, body TEXT NOT NULL)')
            target.execute("INSERT INTO operations VALUES('strict_schedule','false') ON CONFLICT(key) DO UPDATE SET body='false'")
            if target.execute("SELECT name FROM sqlite_master WHERE name='recommendation_batches'").fetchone():
                target.execute('DELETE FROM recommendation_batches')
        env = dict(os.environ, **config)
        env.update(LOTTERY_DB=str(database), LOTTERY_BIND='127.0.0.1:6793', LOTTERY_BASE_PATH='/dlt', LOTTERY_REQUIRE_SCHEDULE='0', LOTTERY_BACKUP_DIR=str(Path(scratch)/'backups'))
        root = 'http://127.0.0.1:6793/dlt/'
        process = None

        def start():
            proc = subprocess.Popen([str(args.binary), 'serve'], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            for _ in range(100):
                if proc.poll() is not None:
                    raise RuntimeError('Isolated test service did not start')
                try:
                    with urllib.request.urlopen(root + 'healthz', timeout=1):
                        return proc
                except OSError:
                    time.sleep(.1)
            proc.terminate()
            proc.wait(timeout=10)
            raise RuntimeError('Isolated test service startup timed out')

        try:
            process = start()
            with sync_playwright() as p:
                browser = p.chromium.launch(executable_path=args.browser, headless=True, args=['--no-sandbox', '--disable-dev-shm-usage'])
                context = browser.new_context(viewport={'width': 1440, 'height': 1000}, locale='zh-CN', timezone_id='Asia/Shanghai')
                page = context.new_page()
                errors = []
                page.on('pageerror', lambda e: errors.append(str(e).replace(token, '[redacted]')))

                def get(path):
                    response = context.request.get(root + 'api/' + path, headers={'Authorization': 'Bearer ' + token})
                    assert response.ok, (path, response.status)
                    return response.json()

                def nav(tab, heading):
                    page.locator(f'[data-tab="{tab}"]').click()
                    expect(page.locator('h1')).to_have_text(heading)

                overview = get('overview')
                issue = f'{int(overview["latest_draw"]["issue"]) + 1:05d}'
                deadline = (datetime.now(timezone.utc) + timedelta(hours=1)).astimezone(timezone(timedelta(hours=8))).strftime('%Y-%m-%dT%H:%M')
                page.goto(root, wait_until='networkidle')
                expect(page.locator('#auth-status')).to_contain_text('需要管理员令牌')
                nav('settings', '可复现，也可核验')
                page.locator('#admin-token').fill('deliberately-invalid-test-token')
                page.locator('#token-form button').first.click()
                expect(page.locator('#toast')).to_have_class('error')
                assert page.evaluate('() => sessionStorage.getItem("dlt-admin-token")') is None
                page.locator('#admin-token').fill(token)
                page.locator('#token-form button').first.click()
                expect(page.locator('#toast')).to_have_text('管理员令牌验证通过。')

                nav('generator', '为下一期准备一组号码')
                page.locator('#issue').fill(issue)
                expect(page.locator('#front_pool')).to_be_disabled()
                expect(page.locator('#front_banker_count')).to_be_disabled()
                page.locator('[data-preset="1"]').click()
                page.locator('#generate').click()
                expect(page.locator('#plan-preview .ticket-row')).to_have_count(7)
                page.locator('#save-deadline').fill(deadline)
                page.locator('#plan-label').fill('隔离环境功能测试 · 不是真实推荐')
                page.locator('#save-plan').click()
                expect(page.locator('#toast')).to_contain_text('原始票组已保存')
                personal = get('records')['records'][0]
                assert personal['kind'] == 'personal' and len(personal['plan']['tickets']) == 7
                assert not get('recommendations')['records']
                page.reload(wait_until='networkidle')
                nav('records', '让每一组号码都有记录')
                expect(page.locator('[data-expand]')).to_have_count(1)
                page.locator('[data-expand]').click()
                expect(page.locator('.record-detail.open .ticket-row')).to_have_count(7)
                with page.expect_download() as pending:
                    page.locator('[data-export]').click()
                exported = Path(scratch) / 'record.json'
                pending.value.save_as(exported)
                assert json.loads(exported.read_text())['plan'] == personal['plan']

                nav('reviews', '每期推荐与复盘')
                expect(page.locator('[data-expand]')).to_have_count(0)
                if not page.locator('#archive-issue').is_visible():
                    page.locator('#manual-archive > summary').click()
                page.locator('#archive-issue').fill(issue)
                page.locator('#archive-deadline').fill(deadline)
                page.locator('#archive-all').click()
                expect(page.locator('.recommendation-period tbody tr')).to_have_count(5)
                frozen = get('recommendations')
                assert len(frozen['batches']) == 1 and len(frozen['records']) == 5
                nav('settings', '可复现，也可核验')
                page.locator('#seed').fill(str(int(get('config')['seed']) + 1))
                page.locator('#save-seed').click()
                expect(page.locator('#toast')).to_contain_text('主种子已更新')
                nav('reviews', '每期推荐与复盘')
                if not page.locator('#archive-issue').is_visible():
                    page.locator('#manual-archive > summary').click()
                page.locator('#archive-issue').fill(issue)
                page.locator('#archive-deadline').fill(deadline)
                with page.expect_response(lambda response: response.url.endswith('/api/recommendations/archive')) as archived:
                    page.locator('#archive-all').click()
                assert archived.value.ok
                expect(page.locator('#archive-all')).to_be_enabled()
                expect(page.locator('#toast')).to_contain_text('本期5个预设已固定')
                assert get('recommendations') == frozen

                process.send_signal(2)
                process.wait(timeout=10)
                process = start()
                assert get('recommendations') == frozen
                assert get('records')['records'][0]['settlement'] is None

                # Deliberately synthetic draw, confined to the disposable DB.
                ticket = personal['plan']['tickets'][0]
                draw = dict(issue=issue, date=datetime.now(timezone.utc).date().isoformat(), front=ticket['front'], back=ticket['back'], source='isolated_workflow_test_only')
                page.reload(wait_until='networkidle')
                nav('draws', '每一期开奖，有据可查')
                page.locator('#import-json').fill(json.dumps([draw]))
                with page.expect_response(lambda response: response.url.endswith('/api/draws/import') and response.request.method == 'POST') as imported:
                    page.locator('#import-button').click()
                assert imported.value.ok
                expect(page.locator('#import-button')).to_be_enabled()
                expect(page.locator('#toast')).to_contain_text('已校验导入')
                settled = next(r for r in get('records')['records'] if r['id'] == personal['id'])
                assert settled['plan'] == personal['plan']
                assert settled['settlement']['winning_tickets'] >= 1
                assert settled['settlement']['total_prize_fen'] is None
                nav('records', '让每一组号码都有记录')
                expect(page.locator('#content')).to_contain_text('奖金待核实')
                page.locator('[data-expand]').click()
                expect(page.locator('.record-detail.open .ticket-row')).to_have_count(7)
                assert page.locator('.ball.hit').count() >= 7
                draw['awards'] = {str(n): {'basic_fen': 123450000 if n == 1 else 500} for n in range(1, 8)}
                nav('draws', '每一期开奖，有据可查')
                page.locator('#import-json').fill(json.dumps({'draws': [draw]}))
                with page.expect_response(lambda response: response.url.endswith('/api/draws/import') and response.request.method == 'POST') as imported:
                    page.locator('#import-button').click()
                assert imported.value.ok
                expect(page.locator('#import-button')).to_be_enabled()
                expect(page.locator('#toast')).to_contain_text('已校验导入')
                settled = next(r for r in get('records')['records'] if r['id'] == personal['id'])
                assert settled['settlement']['total_prize_fen'] >= 123450000
                assert settled['settlement']['net_fen'] == settled['settlement']['total_prize_fen'] - personal['plan']['cost_fen']
                assert settled['plan'] == personal['plan']
                nav('reviews', '每期推荐与复盘')
                expect(page.locator('.recommendation-period tbody tr')).to_have_count(5)
                page.evaluate('() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)))')
                page.screenshot(path=str(args.output / 'isolated-review-desktop.png'), full_page=True)
                page.set_viewport_size({'width': 375, 'height': 812})
                assert page.evaluate('() => document.documentElement.scrollWidth <= innerWidth')
                page.screenshot(path=str(args.output / 'isolated-review-mobile.png'), full_page=True)
                assert not errors, errors
                with sqlite3.connect(database) as db:
                    assert db.execute('PRAGMA integrity_check').fetchone()[0] == 'ok'
                result = dict(environment='disposable copied database on loopback port 6793', synthetic_draw=True,
                              invalid_token_rejected=True, mode_parameters_disabled=True, save_reload_and_export=True,
                              personal_recommendation_separation=True, frozen_batch_survives_seed_change_and_restart=True,
                              missing_award_stays_unknown=True, award_enrichment_settles_original_tickets=True,
                              winning_numbers_highlighted=True, recommendation_comparison_rows=5, page_errors=errors)
                (args.output / 'workflow-verification.json').write_text(json.dumps(result, ensure_ascii=False, indent=2) + '\n')
                print(json.dumps(result, ensure_ascii=False, indent=2))
                browser.close()
        finally:
            if process is not None and process.poll() is None:
                process.send_signal(2)
                process.wait(timeout=10)


if __name__ == '__main__':
    main()
