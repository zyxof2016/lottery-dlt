"use strict";
// Pure set analysis shared by the browser and the development checks.
(function (root) {
  const unique = values => [...new Set(values)].sort((a,b)=>a-b);
  function numbers(input, upper) {
    const tokens = Array.isArray(input) ? input : String(input).trim().split(/[\s,，、;；]+/).filter(Boolean);
    if (!tokens.length || tokens.some(n=>!/^\d+$/.test(String(n)) || !Number.isSafeInteger(Number(n)) || Number(n)<1 || Number(n)>upper)) {
      throw new Error(`号码须为1～${upper}的整数，且每个区至少填写一个号码。`);
    }
    return unique(tokens.map(Number));
  }
  function pool(value) {
    if (!value || typeof value.id!=="string" || !value.id || value.id.length>200) throw new Error("号码池缺少有效来源标识。");
    const issue=String(value.issue||"");
    if (issue && !/^\d{5}$/.test(issue)) throw new Error("来源期号须为五位数字。");
    const source=String(value.source||"manual");
    if (!["manual","personal","recommendation"].includes(source)) throw new Error("未知号码池来源。");
    const input=value.score_evidence;
    const evidence=source!=="manual"&&input?.status==="recorded"&&/^[a-f0-9]{64}$/.test(input.content_hash)?{
      status:"recorded",content_hash:input.content_hash,
      front_score_status:input.front_score_status==="recorded_scores"?"recorded_scores":input.front_score_status==="not_scored"?"not_scored":"unknown",
      back_score_status:input.back_score_status==="recorded_scores"?"recorded_scores":input.back_score_status==="not_scored"?"not_scored":"unknown",
      front_candidate_basis:["rank_prefix","manual_pool","whole_zone"].includes(input.front_candidate_basis)?input.front_candidate_basis:"unknown",
      back_candidate_basis:["rank_prefix","manual_pool","whole_zone"].includes(input.back_candidate_basis)?input.back_candidate_basis:"unknown",
      front_all_scores_equal:input.front_score_status==="recorded_scores"&&typeof input.front_all_scores_equal==="boolean"?input.front_all_scores_equal:null,
      back_all_scores_equal:input.back_score_status==="recorded_scores"&&typeof input.back_all_scores_equal==="boolean"?input.back_all_scores_equal:null
    }:null;
    return {score_evidence:evidence,id:value.id,label:String(value.label||"未命名号码池").slice(0,100),issue,source,
      plan_id:String(value.plan_id||"").slice(0,64),created_at:String(value.created_at||"").slice(0,64),
      front:numbers(value.front,35),back:numbers(value.back,12)};
  }
  function fromRecord(record,evidence=null) {
    return pool({score_evidence:evidence,id:`record:${record.id}`,label:record.label,issue:record.plan.request.issue,
      source:record.kind,plan_id:record.id,created_at:record.created_at,
      front:record.plan.tickets.flatMap(t=>t.front),back:record.plan.tickets.flatMap(t=>t.back)});
  }
  function matches(value,query) {
    const p=pool(value),q=String(query||"").trim().toLowerCase();
    return !q || `${p.label} ${p.issue} ${p.id} 前区${p.front.length} 后区${p.back.length}`.toLowerCase().includes(q);
  }
  function analyze(input, threshold=2, comparison={issue:"",draw:null}) {
    if (!Array.isArray(input) || input.length>200) throw new Error("最多选择200个号码池。");
    const pools=input.map(pool), ids=new Set(pools.map(p=>p.id));
    if (ids.size!==pools.length) throw new Error("同一来源不能重复计数。");
    if (!Number.isInteger(threshold) || threshold<1 || threshold>200) throw new Error("最少出现池数须为1～200的整数。");
    const minimum=Math.min(threshold,pools.length||1), issue=String(comparison.issue||"");
    if (issue && !/^\d{5}$/.test(issue)) throw new Error("开奖对照期号无效。");
    const draw=comparison.draw ? {issue:comparison.draw.issue,date:comparison.draw.date,
      front:numbers(comparison.draw.front,35),back:numbers(comparison.draw.back,12)} : null;
    if (draw && (draw.issue!==issue || draw.front.length!==5 || draw.back.length!==2)) throw new Error("开奖对照数据与当前期号不符。");
    const groups=new Map();
    for (const p of pools) {const key=JSON.stringify([p.front,p.back]);if(!groups.has(key))groups.set(key,[]);groups.get(key).push(p.id);}
    const reference=pools.find(p=>p.id===comparison.reference_pool_id)||pools[0]||null;
    const options={deduplicate_content:comparison.deduplicate_content===true,exclude_flat_scores:comparison.exclude_flat_scores===true};
    const zones={};
    for (const zone of ["front","back"]) {
      const excluded=pools.filter(p=>options.exclude_flat_scores&&p.score_evidence?.[zone+"_all_scores_equal"]===true&&p.score_evidence?.[zone+"_candidate_basis"]==="rank_prefix");
      const active=pools.filter(p=>!excluded.includes(p)),byContent=new Map();
      for(const p of active){const key=p[zone].join(",");if(!byContent.has(key))byContent.set(key,[]);byContent.get(key).push(p);}
      const groups=options.deduplicate_content?[...byContent.values()]:active.map(p=>[p]);
      const effective=groups.length,zoneMinimum=Math.min(threshold,effective||1);
      const rows=unique(active.flatMap(p=>p[zone])).map(number=>{
        const included=active.filter(p=>p[zone].includes(number));
        const count=groups.filter(group=>group[0][zone].includes(number)).length;
        return {number,count,source_count:included.length,frequency:count/effective,
          included_pool_ids:included.map(p=>p.id),excluded_pool_ids:active.filter(p=>!p[zone].includes(number)).map(p=>p.id),
          draw_hit:draw?draw[zone].includes(number):null};
      }).sort((a,b)=>b.count-a.count||a.number-b.number);
      const common=rows.filter(r=>effective>0&&r.count===effective).map(r=>r.number).sort((a,b)=>a-b);
      const high=rows.filter(r=>r.count>=zoneMinimum).map(r=>r.number).sort((a,b)=>a-b);
      const review=values=>{
        const lost=reference?reference[zone].filter(n=>!values.includes(n)):[];
        const hits=draw?values.filter(n=>draw[zone].includes(n)):null;
        const lostHits=draw?lost.filter(n=>draw[zone].includes(n)):null;
        return {numbers:values,hit_numbers:hits,hit_count:hits?.length??null,
          expected_hits_uniform:values.length*(zone==="front"?5/35:2/12),
          selected_hit_fraction:hits&&values.length?hits.length/values.length:null,
          draw_coverage_fraction:hits?hits.length/(zone==="front"?5:2):null,
          removed_from_reference:lost,removed_hits_from_reference:lostHits,lost_hit_count:lostHits?.length??null};
      };
      zones[zone]={rows,minimum_count:zoneMinimum,effective_pool_count:effective,active_source_count:active.length,
        excluded_flat_pool_ids:excluded.map(p=>p.id),unknown_score_pool_ids:active.filter(p=>p.score_evidence?.[zone+"_all_scores_equal"]==null).map(p=>p.id),
        preserved_manual_pool_ids:active.filter(p=>p.score_evidence?.[zone+"_candidate_basis"]==="manual_pool").map(p=>p.id),
        same_content_groups:[...byContent.values()].filter(g=>g.length>1).map(g=>g.map(p=>p.id)),
        voting_groups:groups.map(g=>g.map(p=>p.id)),common:review(common),high_frequency:review(high),union:review(rows.map(r=>r.number).sort((a,b)=>a-b)),
        thresholds:Array.from({length:effective},(_,i)=>({minimum_count:i+1,...review(rows.filter(r=>r.count>=i+1).map(r=>r.number).sort((a,b)=>a-b))}))};
    }
    return {schema_version:3,options,pool_count:pools.length,minimum_count:minimum,pools,zones,reference_pool_id:reference?.id||null,
      identical_content_groups:[...groups.values()].filter(g=>g.length>1),
      draw_comparison:{issue,status:draw?"drawn":"pending",lookup_status:draw?"drawn":comparison.lookup_status||"missing",draw,
        cross_issue_pool_count:pools.filter(p=>p.issue && p.issue!==issue).length},
      note:"号码池共识不是中奖概率。同源或相同内容不代表独立预测证据。等分排除仅依据原档案已记录分值，未知或未评分来源不自动排除。跨期池只对照当前期号；本分析不构成新推荐留档。"};
  }
  function csv(report) {
    const cell=value=>{let text=String(value??"");if(/^[\s]*[=+\-@]/.test(text))text="'"+text;return '"'+text.replace(/"/g,'""')+'"';};
    const header=["区域","号码","出现池数","频率","全部所选池数","开奖对照期号","是否开出","包含来源","未包含来源","本区有效计票池数","原包含来源数","内容去重","排除等分来源"];
    const label=id=>{const p=report.pools.find(p=>p.id===id);return `${p.label} [${p.issue||"手动"} ${p.id}]`;};
    const rows=[header];
    for(const zone of ["front","back"])for(const row of report.zones[zone].rows) {
      rows.push([zone==="front"?"前区":"后区",String(row.number).padStart(2,"0"),row.count,row.frequency,report.pool_count,
        report.draw_comparison.issue,row.draw_hit==null?"待开奖":row.draw_hit?"已开出":"未开出",
        row.included_pool_ids.map(label).join(" | "),row.excluded_pool_ids.map(label).join(" | "),report.zones[zone].effective_pool_count,row.source_count,report.options.deduplicate_content?"是":"否",report.zones[zone].excluded_flat_pool_ids.map(label).join(" | ")]);
    }
    return "\uFEFF"+rows.map(row=>row.map(cell).join(",")).join("\r\n")+"\r\n";
  }
  const api={numbers,pool,fromRecord,analyze,csv,matches};
  if(typeof module!=="undefined" && module.exports)module.exports=api;
  else root.DltPools=api;
})(typeof globalThis!=="undefined"?globalThis:this);
