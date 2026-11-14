#!/usr/bin/env python3
"""Browser smoke check; only previews and analyses, never saves a ticket group."""
import argparse
import json
from pathlib import Path
from playwright.sync_api import expect, sync_playwright


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--browser', required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--env-file', type=Path, default=Path('/data/apps/lottery-dlt/shared/service.env'))
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    config = dict(line.split('=', 1) for line in args.env_file.read_text().splitlines() if '=' in line)
    token = config['LOTTERY_ADMIN_TOKEN']
    with sync_playwright() as p:
        browser = p.chromium.launch(executable_path=args.browser, headless=True, args=['--no-sandbox', '--disable-dev-shm-usage'])
        context = browser.new_context(viewport={'width': 1440, 'height': 1000}, locale='zh-CN', timezone_id='Asia/Shanghai')
        page = context.new_page()
        errors = []
        page.on('pageerror', lambda error: errors.append(str(error).replace(token, '[redacted]')))
        page.goto('https://1.14.208.189/dlt/', wait_until='networkidle')
        page.wait_for_selector('nav button')
        assert page.locator('nav button').count() == 7
        before = context.request.get('https://1.14.208.189/dlt/api/records').json()['records']
        overview = context.request.get('https://1.14.208.189/dlt/api/overview').json()
        issue = f'{int(overview["latest_draw"]["issue"]) + 1:05d}'
        headings = {
            'generator': '为下一期准备一组号码', 'records': '让每一组号码都有记录',
            'reviews': '每期推荐与复盘', 'draws': '每一期开奖，有据可查',
            'backtest': '把概率与历史分开看', 'research': '对照研究，保留证据',
            'settings': '可复现，也可核验',
        }
        for tab, heading in headings.items():
            page.locator(f'[data-tab="{tab}"]').click()
            expect(page.locator('h1')).to_have_text(heading)
        page.locator('#admin-token').fill(token)
        page.locator('#token-form button').first.click()
        expect(page.locator('#toast')).to_have_text('管理员令牌验证通过。')
        page.locator('[data-tab="generator"]').click()
        page.locator('#issue').fill(issue)
        for preset, count in [(1, 7), (2, 6), (4, 20)]:
            page.locator(f'[data-preset="{preset}"]').click()
            page.locator('#generate').click()
            page.wait_for_selector('#plan-preview .ticket-row')
            assert page.locator('#plan-preview .ticket-row').count() == count
        page.locator('#multiplier').fill('2')
        assert page.locator('#plan-preview .ticket-row').count() == 0
        page.locator('[data-preset="1"]').click()
        page.locator('#generate').click()
        page.wait_for_selector('#plan-preview .ticket-row')
        page.screenshot(path=str(args.output / 'desktop.png'), full_page=True)
        page.locator('#simulate-plan').click()
        expect(page.locator('#simulation-result')).to_contain_text('整组至少一注中奖', timeout=120000)
        page.locator('[data-tab="backtest"]').click()
        page.locator('#bt-limit').fill('10')
        page.locator('#run-backtest').click()
        page.wait_for_selector('#backtest-result tbody tr', timeout=120000)
        assert page.locator('#backtest-result tbody tr').count() == 10
        page.locator('[data-tab="research"]').click()
        page.locator('#bt-limit').fill('10')
        page.locator('#run-research').click()
        page.wait_for_selector('#research-result tbody tr', timeout=120000)
        assert page.locator('#research-result tbody tr').count() == 5
        page.set_viewport_size({'width': 375, 'height': 812})
        for tab, heading in headings.items():
            page.locator(f'[data-tab="{tab}"]').click()
            expect(page.locator('h1')).to_have_text(heading)
            assert page.evaluate('() => document.documentElement.scrollWidth <= innerWidth'), f'Horizontal overflow in {tab}'
        page.locator('[data-tab="generator"]').click()
        expect(page.locator('h1')).to_have_text(headings['generator'])
        page.evaluate('() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)))')
        page.screenshot(path=str(args.output / 'mobile.png'), full_page=True)
        assert context.request.get('https://1.14.208.189/dlt/api/records').json()['records'] == before
        assert not errors, errors
        result = {'desktop_tabs': 7, 'mobile_tabs': 7, 'preview_ticket_counts': [7, 6, 20], 'parameter_change_invalidates_preview': True,
                  'simulation': 'ok', 'backtest': 'ok', 'research': 'ok', 'horizontal_overflow': False, 'page_errors': errors, 'unchanged_record_count': len(before)}
        (args.output / 'browser-verification.json').write_text(json.dumps(result, indent=2) + '\n')
        print(json.dumps(result, indent=2))
        browser.close()


if __name__ == '__main__':
    # Never put a credential into an exception report.
    import sys
    import traceback
    try:
        main()
    except Exception as error:
        env = Path('/data/apps/lottery-dlt/shared/service.env')
        config = dict(line.split('=', 1) for line in env.read_text().splitlines() if '=' in line) if env.exists() else {}
        print(traceback.format_exc().replace(config.get('LOTTERY_ADMIN_TOKEN', 'no-token'), '[redacted]'), file=sys.stderr)
        sys.exit(1)
