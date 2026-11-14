#!/usr/bin/env python3
"""Check the deployed service without creating recommendations or betting records."""
import argparse
import json
import ssl
import urllib.error
import urllib.request
from pathlib import Path


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--url', default='https://1.14.208.189/dlt/')
    parser.add_argument('--env-file', type=Path, default=Path('/data/apps/lottery-dlt/shared/service.env'))
    parser.add_argument('--output', type=Path)
    args = parser.parse_args()
    config = dict(line.split('=', 1) for line in args.env_file.read_text().splitlines() if '=' in line)
    token = config['LOTTERY_ADMIN_TOKEN']
    root = args.url.rstrip('/') + '/'

    def request(path, body=None, authenticated=False):
        headers = {'Content-Type': 'application/json'}
        if authenticated:
            headers['Authorization'] = 'Bearer ' + token
        req = urllib.request.Request(root + path, headers=headers, data=None if body is None else json.dumps(body).encode())
        with urllib.request.urlopen(req, timeout=120, context=ssl.create_default_context()) as response:
            text = response.read().decode()
            return json.loads(text) if response.headers.get_content_type() == 'application/json' else text

    assert request('healthz')['status'] == 'ok'
    overview = request('api/overview')
    before = request('api/records')['records']
    assert overview['draw_count'] >= 300 and overview['admin_token_required']
    assert not overview['data_quality']['stale']
    html = request('')
    assert '/dlt/app.js' in html and '/dlt/styles.css' in html
    assert 'text/html' not in request('app.js')[:100]
    assert 'PLAN GENERATOR' in request('app.js')
    assert '@media(max-width:600px)' in request('styles.css')
    next_issue = f'{int(overview["latest_draw"]["issue"]) + 1:05d}'
    try:
        request('api/plans/generate', {'issue': next_issue})
        raise AssertionError('Unauthenticated write was accepted')
    except urllib.error.HTTPError as error:
        assert error.code == 401
    preview = request('api/plans/generate', {'issue': next_issue, 'strategy': 'coverage', 'bets': 7}, True)
    assert len(preview['tickets']) == 7 and len(preview['front_pool']) == 35
    assert preview['cost_fen'] == 1400
    simulation = request('api/plans/simulate', {'request': preview['request'], 'preview_id': preview['id'], 'trials': 1000}, True)
    assert simulation['plan_id'] == preview['id']
    report = request('api/backtest', {'warmup': 60, 'limit': 20, 'bets': 3}, True)
    assert report['periods'] == 20
    assert all(row['data_cutoff'] < row['issue'] for row in report['trace'])
    research = request('api/research', {'warmup': 60, 'limit': 10, 'bets': 3}, True)
    assert len(research['reports']) == 5
    assert len({row['cost_fen'] for row in research['reports']}) == 1
    assert request('api/records')['records'] == before
    result = {
        'url': root, 'health': 'ok', 'static_assets': 'ok', 'auth_rejects_missing_token': True,
        'generation_preview': 'ok', 'simulation': 'ok', 'causal_backtest': 'ok', 'equal_budget_research': 'ok',
        'draw_count': overview['draw_count'], 'latest_issue': overview['latest_draw']['issue'],
        'latest_date': overview['latest_draw']['date'], 'unchanged_record_count': len(before),
        'missing_issues': len(overview['data_quality']['missing_issues']),
        'latest_award_levels': len(overview['latest_draw']['awards']),
    }
    if args.output:
        args.output.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result, ensure_ascii=False, indent=2))


if __name__ == '__main__':
    main()
