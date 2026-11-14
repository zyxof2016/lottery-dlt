#!/usr/bin/env python3
"""Verify real production analyses; never save betting/recommendation records."""
import argparse
import json
from pathlib import Path
import ssl
import time
import urllib.request


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--url',default='https://1.14.208.189/dlt/')
    parser.add_argument('--env-file',type=Path,default=Path('/data/apps/lottery-dlt/shared/service.env'))
    parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args();args.output.mkdir(parents=True,exist_ok=True)
    config=dict(line.split('=',1) for line in args.env_file.read_text().splitlines() if '=' in line)
    token=config['LOTTERY_ADMIN_TOKEN']
    def request(path,body=None):
        req=urllib.request.Request(args.url.rstrip('/')+'/'+path,data=None if body is None else json.dumps(body).encode(),headers={'Content-Type':'application/json','Authorization':'Bearer '+token})
        with urllib.request.urlopen(req,context=ssl.create_default_context(),timeout=120) as response:return json.load(response)
    before=request('api/records')['records'];health=request('healthz');assert health['version']=='1.1.0'
    operations=request('api/operations');assert operations['strict_schedule'] is True
    schedules=request('api/schedules')['schedules'];schedule=next(s for s in schedules if s['issue']>request('api/overview')['latest_draw']['issue'])
    base=request('api/plans/generate',{'issue':schedule['issue'],'strategy':'coverage','bets':7})
    for pricing in [{'additional':True},{'multiplier':3}]:
        changed=request('api/plans/generate',{**base['request'],**pricing});assert changed['tickets']==base['tickets'];assert changed['cost_fen']!=base['cost_fen']
    requests=[('compound','backtest',{'mode':'compound','strategy':'random','front_pool':6,'back_pool':2,'limit':10}),
              ('dantuo','backtest',{'mode':'dantuo','strategy':'random','front_pool':7,'back_pool':3,'front_banker_count':2,'back_banker_count':1,'limit':10}),
              ('optimizer','optimize',{'request':{'issue':schedule['issue'],'bets':7},'objective':'coverage','trials':1000}),
              ('holdout','research',{'config':{'warmup':60,'limit':20,'bets':3},'seeds':[20261015,20261016],'validation_periods':20})]
    summaries=[]
    for label,kind,input_value in requests:
        queued=request('api/jobs',{'kind':kind,'input':input_value});end=time.monotonic()+180
        while True:
            job=request('api/jobs/'+queued['id'])
            if job['state']=='completed':break
            assert job['state'] in ['queued','running'],job.get('error')
            assert time.monotonic()<end,'Job timeout'
            time.sleep(.2)
        result=job['result']
        if label=='compound':assert result['actual_bets']==6
        if label=='dantuo':assert result['actual_bets']==20
        if label=='optimizer':
            assert len(result['candidates'])==3
            assert all(r['plan']['cost_fen']==1400 for r in result['candidates'])
            assert all(r['validation_simulation']['simulation_seed']!=r['selection_simulation']['simulation_seed'] for r in result['candidates'])
        if label=='holdout':assert result['selection_end']<result['validation_start'] and all(c['paired']['n']==20 for c in result['validation_comparisons'])
        (args.output/(label+'.json')).write_text(json.dumps(job,ensure_ascii=False,indent=2)+'\n')
        summaries.append({'feature':label,'job_id':job['id'],'state':job['state'],'generator_version':job['generator_version']})
    page=request('api/draws?page=1&limit=50');assert len(page['draws'])==50
    assert request('api/records')['records']==before
    value={'version':health['version'],'stable_numbers_for_additional_and_multiplier':True,'schedule_issue':schedule['issue'],'schedule_deadline':schedule['deadline'],'schedule_basis':schedule['basis'],'strict_schedule':True,'automatic_recommendations':operations['automation'],'last_backup':request('api/operations')['last_backup'],'analysis_jobs':summaries,'draw_page_size':50,'unchanged_saved_record_count':len(before)}
    (args.output/'production-optimization-verification.json').write_text(json.dumps(value,ensure_ascii=False,indent=2)+'\n')
    print(json.dumps(value,ensure_ascii=False,indent=2))


if __name__=='__main__':main()
