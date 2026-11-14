"use strict";
function liveReviewPanel() {
  const r=state.liveReview;
  if(!r)return `<section class="card" id="live-review"><h2>实际留档持续复盘</h2><p role="${state.liveReviewError?"alert":"status"}" class="muted">${escapeHTML(state.liveReviewError||"正在读取持续复盘；原始票组已可查看。")}</p>${state.liveReviewError?'<button class="button" id="retry-live-review">重试复盘</button>':""}</section>`;
  const s=r.summary;
  const statuses={settled:"已结算",prize_pending:"奖金待核实",pending:"待开奖",excluded:"不纳入成绩",missed:"缺档",not_archived:"尚未留档"};
  const comparisons=r.comparisons.filter(c=>c.common_periods);
  return `<section class="card" id="live-review"><div class="card-head"><h2>实际留档持续复盘</h2><button class="button button-small" id="export-live-review">导出复盘证据</button></div><form id="review-window-form" class="actions"><label class="field">最近期数<input id="review-window" type="number" min="1" max="120" required value="${state.reviewWindow}"></label><button class="button">更新窗口</button></form><p class="muted margin-top">请求最近 ${r.requested_limit??"—"} 期，实际列入 ${r.period_count} 期，含缺档和待开奖；期数上限不是已开奖样本数。窗口包含已跟踪的日程与留档后缺期，不受下方归档列表分页或筛选影响。${escapeHTML(r.note)}</p><div class="grid grid-4">${stat("实际整期留档",s.archived_periods,`${s.archived_records}个纳入的原始方案`)}${stat("已开奖 / 缺档",`${s.settled_periods} / ${r.periods.filter(p=>p.status==="missed").length}`,`另有${s.pending_periods}期待开奖`)}${stat("已开奖投入",money(s.settled_cost_fen),`待开奖投入 ${money(s.pending_cost_fen)}`)}${stat("已开奖净收益",money(s.net_fen),`${s.unpriced_periods}个方案奖金待核实`)}</div><div class="table-wrap margin-top"><table><thead><tr><th>实际方案</th><th>已开奖 / 待开奖期</th><th>中奖期 / 见奖率</th><th>前 / 后区平均并集命中</th><th>已开奖投入</th><th>奖金 / 净额</th><th>奖额完整子集</th></tr></thead><tbody>${r.strategies.map(x=>{const v=x.summary;return `<tr><td>${escapeHTML(x.label)}${x.excluded_periods?`<br>排除${x.excluded_periods}期`:""}</td><td>${v.settled_periods} / ${v.pending_periods}</td><td>${v.winning_periods} / ${v.issue_win_rate==null?"—":percent(v.issue_win_rate)}</td><td>${v.mean_front_union_hits?.toFixed(2)??"—"} / ${v.mean_back_union_hits?.toFixed(2)??"—"}</td><td>${money(v.settled_cost_fen)}</td><td>${money(v.total_prize_fen)} / ${money(v.net_fen)}</td><td>${v.priced_periods}个方案<br>成本 ${money(v.priced_cost_fen)} / 奖金 ${money(v.priced_prize_fen)}</td></tr>`;}).join("")}</tbody></table></div>${!r.strategies.length?empty("尚无整期推荐留档","保存实际推荐后，成绩将随开奖缓存更新。"):""}<h3 class="margin-top">共同期同预算对照</h3><p class="muted">只比较双方均有合格已开奖档案，且注数、追加、倍数与金额相同的期号。差值为左减右；结构差异仍可能影响见奖率。</p>${comparisons.length?`<div class="table-wrap"><table><thead><tr><th>左方案 / 右方案</th><th>共同期数</th><th>每期见奖率差</th><th>每期中奖注数差</th><th>净额差</th><th>共同期号</th></tr></thead><tbody>${comparisons.map(c=>`<tr><td>${escapeHTML(c.left)} / ${escapeHTML(c.right)}</td><td>${c.common_periods}</td><td>${percent(c.mean_win_delta)}</td><td>${c.mean_winning_ticket_delta.toFixed(3)}</td><td>${money(c.total_net_delta_fen)}</td><td>${c.pairs.map(p=>escapeHTML(p.issue)).join("、")}</td></tr>`).join("")}</tbody></table></div>`:empty("暂无可比的共同期","待开奖、缺档与预算不同的期号不计零分。没有共同期时不计算差值。")}<details><summary>逐期状态与原始票组摘要（${r.period_count}期）</summary><div class="table-wrap"><table><thead><tr><th>期号</th><th>状态</th><th>实际方案 / 并集命中 / 来源</th></tr></thead><tbody>${r.periods.map(p=>`<tr><td>${escapeHTML(p.issue)}</td><td>${statuses[p.status]}</td><td class="review-provenance">${p.entries.map(e=>`<div>${escapeHTML(e.label)} · ${e.front_union_hits??"待开"}+${e.back_union_hits??"待开"}${e.exclusion_reason?" · "+escapeHTML(e.exclusion_reason):""}<br><span class="hash">原始方案 ${escapeHTML(e.plan_id)}</span></div>`).join("")||"无整期推荐清单"}${p.unbatched_records?`<p>另有${p.unbatched_records}个单独留档，不纳入整期成绩。</p>`:""}</td></tr>`).join("")}</tbody></table></div></details><p class="hash">当前复盘视图 ${escapeHTML(r.content_hash)}</p></section>`;
}
function hitGapText(pattern) {
  const examples=(pattern.examples||[]).map(ticket=>`${fmtNumbers(ticket.front)} + ${fmtNumbers(ticket.back)}`).join("；");
  return `${pattern.front_hits}+${pattern.back_hits} × ${pattern.ticket_count}注${pattern.level?` · ${pattern.level}等奖`:" · 未中奖级"}${examples?` · ${escapeHTML(examples)}`:""}`;
}
function hitGapDrawdown(review) {
  if(review.drawdown_status==="no_settled_periods")return "尚无已开奖";
  if(review.drawdown_status!=="complete")return "待核实";
  return money(review.max_drawdown_fen);
}
function hitGapLoss(review) {
  if(review.loss_run_status!=="complete" && review.loss_run_status!=="interrupted_by_unknown")return "尚无完整金额";
  return `${review.longest_loss_run}期${review.loss_run_status==="interrupted_by_unknown"?"（已知片段）":""}`;
}
function hitGapPanel(report) {
  const rows=(report?.strategies||[]).filter(row=>row.hit_review);
  if(!(report?.strategies||[]).length)return "";
  if(!rows.length)return `<section class="card" id="hit-gap"><h2>单注差额与奖金构成</h2><p class="muted">这份复盘没有单注差额。请重新读取；缺失不会显示成 0。</p></section>`;
  const count=(review,key)=>review.settled_periods?review[key]:"尚无已开奖";
  return `<section class="card" id="hit-gap"><div class="card-head"><h2>单注差额与奖金构成</h2><button id="export-hit-gap" class="button button-small">导出差额证据</button></div><p class="muted">${escapeHTML(rows[0].hit_review.note)}</p><div class="table-wrap"><table><thead><tr><th>方案</th><th>已开奖 / 待开奖</th><th>头奖期</th><th>两区都覆盖但无头奖注</th><th>前区池外 / 未集中期</th><th>后区池外 / 未集中期</th><th>平均最近单注差距</th><th>浮动奖 / 固定奖</th><th>回本分类</th><th>最大回撤 / 连亏</th></tr></thead><tbody>${rows.map(row=>{const review=row.hit_review;return `<tr><td>${escapeHTML(row.label)}</td><td>${review.settled_periods} / ${review.pending_periods}</td><td>${count(review,"jackpot_periods")}</td><td>${count(review,"union_covered_without_jackpot_periods")}</td><td>${count(review,"front_outside_periods")} / ${count(review,"front_scatter_periods")}</td><td>${count(review,"back_outside_periods")} / ${count(review,"back_scatter_periods")}</td><td>${review.mean_joint_gap==null?"尚无已开奖":review.mean_joint_gap.toFixed(2)}</td><td>${review.settled_periods?`${money(review.floating_prize_fen)} / ${money(review.fixed_prize_fen)}`:"尚无已开奖"}</td><td>${review.settled_periods?`已确认浮动奖回本 ${review.floating_recovered_periods} 期<br>固定奖补足 ${review.fixed_assisted_recovered_periods} 期<br>金额未知 ${review.recovery_unknown_periods} 期`:"尚无已开奖"}</td><td>${hitGapDrawdown(review)} / ${hitGapLoss(review)}</td></tr>`;}).join("")}</tbody></table></div><details><summary>逐期最近单注、池外和未集中号码</summary><p class="muted">各区未集中数相对于该区命中最多的注，这两注不一定是同一注。最近单注给出同一注上的前区和后区。池外号码不会因为重排已有票而回到票面。</p>${rows.map(row=>`<h3>${escapeHTML(row.label)}</h3><div class="table-wrap"><table><thead><tr><th>期号</th><th>最近单注</th><th>前区池外 / 未集中</th><th>后区池外 / 未集中</th><th>头奖注</th><th>浮动奖 / 固定奖</th><th>回本</th><th>原票</th></tr></thead><tbody>${row.hit_review.trace.map(item=>{const pending=item.status!=="drawn";const recovery=pending?"待开奖":item.floating_covers_cost==null||item.fixed_assisted_recovery==null?"金额未知":item.floating_covers_cost?"浮动奖够付成本":item.fixed_assisted_recovery?"固定奖补足":"未回到成本";return `<tr><td>${escapeHTML(item.issue)}</td><td>${pending?"待开奖":item.patterns.map(hitGapText).join("<br>")}${item.outside_explains_entire_gap?"<br>差额都在池外":""}</td><td>${pending?"待开奖":`${item.front_outside_pool} / ${item.front_scatter}`}</td><td>${pending?"待开奖":`${item.back_outside_pool} / ${item.back_scatter}`}</td><td>${item.jackpot_tickets??"待开奖"}</td><td>${pending?"待开奖":`${money(item.floating_prize_fen)} / ${money(item.fixed_prize_fen)}`}</td><td>${recovery}</td><td><a href="${base}/reviews?issue=${encodeURIComponent(item.issue)}#record-${encodeURIComponent(item.plan_id)}">查看原票</a></td></tr>`;}).join("")}</tbody></table></div>`).join("")}</details></section>`;
}
function wireLiveReview() {
  if($("retry-live-review"))$("retry-live-review").onclick=()=>loadLiveReview();
  if($("review-window-form"))$("review-window-form").onsubmit=e=>{e.preventDefault();state.reviewWindow=Number($("review-window").value);loadLiveReview();};
  if($("export-live-review"))$("export-live-review").onclick=()=>download("dlt-live-review.json",state.liveReview);
  if($("export-hit-gap"))$("export-hit-gap").onclick=()=>download("dlt-hit-gap.json",{schema_version:1,content_hash:state.liveReview.content_hash,strategies:state.liveReview.strategies.map(row=>({label:row.label,hit_review:row.hit_review})),periods:state.liveReview.periods.map(period=>({issue:period.issue,status:period.status,entries:period.entries.map(entry=>({label:entry.label,plan_id:entry.plan_id,hit_gap:entry.hit_gap}))}))});
  if($("export-budget-increments"))$("export-budget-increments").onclick=()=>download("dlt-budget-increments.json",{content_hash:state.liveReview.content_hash,periods:state.liveReview.periods,budget_increments:state.liveReview.budget_increments});
}
async function loadLiveReview() {
  const revision=state.renderRevision,request=++state.liveReviewRevision;
  state.liveReview=null;state.liveReviewError="";
  if(!$("review-evidence"))return;
  $("review-evidence").innerHTML=liveReviewPanel();
  try {
    const result=await api(`recommendations/live-review?limit=${state.reviewWindow}`);
    if(revision!==state.renderRevision || request!==state.liveReviewRevision || !$("review-evidence"))return;
    state.liveReview=result;$("review-evidence").innerHTML=liveReviewPanel()+hitGapPanel(result)+replacementPanel(result)+sourceOverlapPanel(result)+stabilityPanel(result)+budgetPanel();wireLiveReview();wireReplacementReview();accessibleTables();scrollReviewAnchor();
  } catch(error) {
    if(revision!==state.renderRevision || request!==state.liveReviewRevision || !$("review-evidence"))return;
    state.liveReviewError=`持续复盘读取失败：${error.message}。原始票组仍可查看。`;
    $("review-evidence").innerHTML=liveReviewPanel();wireLiveReview();
  }
}
function budgetPanel() {
  const report=state.liveReview?.budget_increments;if(!report)return "";
  const rows=report.comparisons;
  const number=value=>value==null?"—":value.toFixed(3);
  const detail=zone=>`新增 ${fmtNumbers(zone.added)||"无"}；丢弃 ${fmtNumbers(zone.removed)||"无"}；新增命中 ${zone.added_hits===null?"待开奖":fmtNumbers(zone.added_hits)||"无"}；丢弃命中 ${zone.removed_hits===null?"待开奖":fmtNumbers(zone.removed_hits)||"无"}`;
  return `<section id="budget-increments" class="card"><div class="card-head"><h2>实际预算增量复盘</h2><button id="export-budget-increments" class="button button-small">导出增量证据</button></div><p class="muted">${escapeHTML(report.note)}</p>${rows.length?`<div class="table-wrap"><table><thead><tr><th>原方案 → 新方案</th><th>共同已开奖 / 待开奖</th><th>合计增加投入</th><th>平均前 / 后区覆盖增量</th><th>每新增元 前 / 后区覆盖</th><th>保留两区并集 / 完整票组期数</th><th>完整净额差</th></tr></thead><tbody>${rows.map(row=>`<tr><td>${escapeHTML(row.from_label)} → ${escapeHTML(row.to_label)}</td><td>${row.common_periods} / ${row.pending_periods}</td><td>${money(row.cost_delta_fen)}</td><td>${number(row.mean_front_hit_delta)} / ${number(row.mean_back_hit_delta)}</td><td>${number(row.front_hits_per_extra_yuan)} / ${number(row.back_hits_per_extra_yuan)}</td><td>${row.both_unions_preserved_periods} / ${row.complete_tickets_preserved_periods}</td><td>${money(row.net_delta_fen)}<br>奖额完整子集 ${row.priced_common_periods}期，净额差 ${money(row.priced_net_delta_fen)}</td></tr>`).join("")}</tbody></table></div><details><summary>逐期新增、丢弃号码及原始票组</summary>${rows.map(row=>`<h3>${escapeHTML(row.from_label)} → ${escapeHTML(row.to_label)}</h3>${row.pairs.map(pair=>`<div class="budget-detail"><strong>${escapeHTML(pair.issue)}期 · ${money(pair.from_cost_fen)} → ${money(pair.to_cost_fen)}</strong><p>前区 ${detail(pair.front)}<br>后区 ${detail(pair.back)}</p><p class="muted">移除完整5+2单注 ${pair.removed_complete_tickets}注，新增 ${pair.added_complete_tickets}注。保留号码并集不能替代完整单注检查。</p><div class="actions"><a class="button button-small" href="${base}/reviews?issue=${encodeURIComponent(pair.issue)}#record-${encodeURIComponent(pair.from_plan_id)}">查看原方案</a><a class="button button-small" href="${base}/reviews?issue=${encodeURIComponent(pair.issue)}#record-${encodeURIComponent(pair.to_plan_id)}">查看新方案</a></div></div>`).join("")}`).join("")}</details>`:empty("暂无共同留档方案","需要同一期至少两个合格实际方案；缺档不会补造。")}</section>`;
}

function loadPoolSelection() {
  try {
    const raw=sessionStorage.getItem("dlt-pool-selection");if(!raw)return;
    const data=JSON.parse(raw);if(data.schema_version!==1 || !Number.isInteger(data.threshold) || data.threshold<1 || data.threshold>200)throw new Error("保存格式无效");
    DltPools.analyze(data.pools,data.threshold);
    state.poolSelection=data.pools.map(DltPools.pool);state.poolThreshold=data.threshold;state.poolReferenceId=typeof data.reference_id==="string"?data.reference_id:"";state.poolDeduplicate=data.deduplicate_content===true;state.poolExcludeFlat=data.exclude_flat_scores===true;
  } catch {state.poolStorageWarning="未能恢复当前标签页保存的号码池，请重新选择；分析和导出仍可使用。";}
}
function savePoolSelection() {
  try {sessionStorage.setItem("dlt-pool-selection",JSON.stringify({schema_version:1,pools:state.poolSelection,threshold:state.poolThreshold,reference_id:state.poolReferenceId||"",deduplicate_content:state.poolDeduplicate===true,exclude_flat_scores:state.poolExcludeFlat===true}));state.poolStorageWarning="";}
  catch {state.poolStorageWarning="浏览器未能保存号码池；本次分析和导出仍可使用，刷新后可能丢失选择。";}
}
function poolPanel() {
  if(!state.poolComparisonIssue)state.poolComparisonIssue=state.recommendations.batches[0]?.issue||state.overview.latest_draw?.issue||defaultForm().issue;
  return `<section class="card" id="pool-intersection"><h2>前后区号码池交集与频次</h2><p class="muted">一份原始票组的前区、后区并集分别作为号码池。可跨期、跨来源保留选择；同一来源只计一次，同池号码重复只计一次。选择仅保存在当前标签页。</p><form id="pool-browse-form"><div class="fields">${select("留档来源","pool-browse-source",[["recommendation","实际推荐"],["personal","个人方案"]],state.poolBrowseSource)}${field("按来源期号筛选","pool-browse-filter","text",state.poolBrowseFilter,'pattern="[0-9]{0,5}" placeholder="留空显示全部"')}${field("最低出现池数","pool-threshold","number",state.poolThreshold,'min="1" max="200" required')}</div><div class="actions margin-top"><button class="button">读取来源池</button><button id="pool-select-page" class="button" type="button">选择本页筛选结果</button><button id="pool-clear" class="button" type="button">取消全部勾选</button></div></form><label class="field margin-top">搜索本页名称／前后区号数<input id="pool-source-search" value="${escapeHTML(state.poolSearch||"")}" placeholder="例如：均衡、前区35、后区12"></label><div id="pool-choices" class="margin-top">正在读取原始来源…</div><div id="pool-source-pager" class="actions margin-top"></div><details><summary>手动添加号码池</summary><form id="manual-pool-form"><div class="fields">${field("名称","manual-pool-label","text","手动号码池",'maxlength="80" required')}${field("来源期号（可留空）","manual-pool-issue","text","",'pattern="[0-9]{5}"')}${field("前区 1～35","manual-pool-front","text","",'required placeholder="01 02 03"')}${field("后区 1～12","manual-pool-back","text","",'required placeholder="01 02"')}</div><button class="button margin-top">添加手动池</button></form></details><form id="pool-comparison-form" class="actions margin-top"><label class="field">开奖对照期号<input id="pool-comparison-issue" value="${escapeHTML(state.poolComparisonIssue)}" pattern="[0-9]{5}" required placeholder="五位期号"></label><button class="button">更新开奖对照</button></form><p id="pool-draw-status" class="muted" role="status"></p><p id="pool-storage-warning" class="muted"></p><fieldset class="pool-view-options"><legend>诊断计票方式</legend><label><input type="checkbox" id="pool-deduplicate" ${state.poolDeduplicate?"checked":""}>按本区号码内容去重计频</label><label><input type="checkbox" id="pool-exclude-flat" ${state.poolExcludeFlat?"checked":""}>分别排除确认等分的排名选号来源</label><p class="muted">保留所选原池；开关只改变当前诊断视图，自选候选池、未评分及分值未知来源继续保留。</p></fieldset><div id="selected-pools" class="margin-top"></div><div id="pool-results"></div><div class="actions margin-top"><button id="copy-pool-common" class="button">复制交集号码</button><button id="export-pools-json" class="button">导出全部 JSON</button><button id="export-pools-csv" class="button">导出全部 CSV</button></div></section>`;
}
function poolReport() {return DltPools.analyze(state.poolSelection,state.poolThreshold,{issue:state.poolComparisonIssue,draw:state.poolDraw,reference_pool_id:state.poolReferenceId,lookup_status:state.poolDrawStatus,deduplicate_content:state.poolDeduplicate,exclude_flat_scores:state.poolExcludeFlat});}
function poolBalls(values,back,draw) {
  return values.length?`<div class="balls">${values.map(n=>{const hit=draw?.[back?"back":"front"].includes(n);return `<span class="ball${back?" back":""}${hit?" hit":""}" aria-label="${back?"后":"前"}区${n}${hit?" 已开出":""}">${String(n).padStart(2,"0")}</span>`;}).join("")}</div>`:'<p class="muted">无共同号码</p>';
}
function renderPoolResults() {
  if(!$("pool-results"))return;
  const r=poolReport(),labels=ids=>ids.map(id=>{const p=r.pools.find(p=>p.id===id);return `${escapeHTML(p.label)}（${escapeHTML(p.issue||"手动")}）`;}).join("、")||"无";
  $("pool-storage-warning").textContent=state.poolStorageWarning;
  $("pool-draw-status").textContent=({loading:"正在读取所选期开奖，当前命中保持未知。",failed:"开奖对照读取失败，命中保持未知；点击更新开奖对照可重试。",missing:"暂无该期开奖缓存，命中未知。",drawn:"已读取所选期开奖。"})[state.poolDrawStatus]||"";
  $("selected-pools").innerHTML=`<h3>已选 ${r.pool_count} 个池</h3>${r.pools.map((p,i)=>`<details class="pool-selected"><summary>${escapeHTML(p.label)} · ${escapeHTML(p.issue||"未指定期号")} <button type="button" class="button button-small" data-remove-pool="${i}">移除</button></summary><p class="hash">来源 ${({manual:"手动输入",personal:"个人方案原档",recommendation:"推荐原档"})[p.source]} · ${escapeHTML(p.id)}<br>保存时间 ${escapeHTML(p.created_at||"手动输入，无留档时间证明")}<br>${poolScoreLabel(p)}</p>${poolBalls(p.front,false,null)}<div class="margin-top">${poolBalls(p.back,true,null)}</div></details>`).join("")}`;
  $("pool-results").innerHTML=r.pool_count?`<label class="field">删号对照参考池<select id="pool-reference">${r.pools.map(p=>`<option value="${escapeHTML(p.id)}" ${p.id===r.reference_pool_id?"selected":""}>${escapeHTML(p.label)} · ${escapeHTML(p.issue||"手动")}</option>`).join("")}</select></label><p class="muted">${escapeHTML(r.note)} 原始两区内容相同组 ${r.identical_content_groups.length} 组；${r.options.deduplicate_content?"本区相同内容只计一池":"当前逐来源计频"}。${r.draw_comparison.cross_issue_pool_count}个池来自其他期号。全部命中只对照 ${escapeHTML(r.draw_comparison.issue||"尚未指定")} 期：${r.draw_comparison.status==="drawn"?"已开奖":"待开奖或尚无缓存，命中未知"}。</p><div class="grid grid-2">${["front","back"].map(zone=>{const z=r.zones[zone],back=zone==="back";return `<div class="pool-zone"><h3>${back?"后区":"前区"}严格交集 · ${z.common.numbers.length}号</h3>${poolBalls(z.common.numbers,back,state.poolDraw)}<p class="muted">有效计票 ${z.effective_pool_count}池，活动来源 ${z.active_source_count}个；排除确认等分 ${z.excluded_flat_pool_ids.length}个；无确认等分证据 ${z.unknown_score_pool_ids.length}个。${z.effective_pool_count?"":"本区无有效来源。"}</p><p class="muted">交集命中 ${z.common.hit_count??"未知"}；至少${z.minimum_count}池的高频号 ${z.high_frequency.numbers.length}个，命中 ${z.high_frequency.hit_count??"未知"}；并集 ${z.union.numbers.length}个，命中 ${z.union.hit_count??"未知"}。</p>${poolThresholds(z,back)}<div class="table-wrap"><table><thead><tr><th>号码</th><th>池数 / 频率</th><th>开奖对照</th><th>来源</th></tr></thead><tbody>${z.rows.filter(row=>row.count>=z.minimum_count).map(row=>`<tr><td>${String(row.number).padStart(2,"0")}</td><td>${row.count} / ${percent(row.frequency)}</td><td>${row.draw_hit==null?"待开奖":row.draw_hit?"✓ 已开出":"未开出"}</td><td class="pool-provenance"><details><summary>查看</summary><p>包含：${labels(row.included_pool_ids)}</p><p>未包含：${labels(row.excluded_pool_ids)}</p></details></td></tr>`).join("")}</tbody></table></div></div>`;}).join("")}</div><p class="muted margin-top">表格按有效计票池数降序，同频按号码升序。JSON保留全部原始来源及排除理由，CSV导出当前诊断视图的全部并集行，不受显示门槛影响。</p>`:empty("尚未选择号码池","勾选实际留档，或添加手动前后区号码池。");
  document.querySelectorAll("[data-remove-pool]").forEach(b=>b.onclick=e=>{e.preventDefault();state.poolSelection.splice(Number(b.dataset.removePool),1);savePoolSelection();renderPoolResults();renderPoolChoices();});
  $("export-pools-json").disabled=!r.pool_count;$("export-pools-csv").disabled=!r.zones.front.rows.length&&!r.zones.back.rows.length;
  if($("copy-pool-common"))$("copy-pool-common").disabled=!r.pool_count;
  if($("pool-reference"))$("pool-reference").onchange=()=>{state.poolReferenceId=$("pool-reference").value;savePoolSelection();renderPoolResults();};
  if(typeof accessibleTables==="function")accessibleTables();
}
function addPool(pool) {
  if(state.poolSelection.some(p=>p.id===pool.id))return;
  if(state.poolSelection.length>=200)throw new Error("最多选择200个池，请先移除部分来源。");
  state.poolSelection.push(pool);
}
function poolScoreLabel(p) {
  return ["front","back"].map(zone=>{const e=p.score_evidence,status=e?.[zone+"_score_status"],flat=e?.[zone+"_all_scores_equal"],manual=e?.[zone+"_candidate_basis"]==="manual_pool";return `${zone==="front"?"前区":"后区"}：${status==="not_scored"?"未使用历史评分":flat===true?`确认等分${manual?"（自选池保留）":""}`:flat===false?"分值有差异":"原评分未记录"}`;}).join("；");
}
function renderPoolChoices() {
  if(!$("pool-choices"))return;
  $("pool-choices").innerHTML=state.poolChoices.some(p=>DltPools.matches(p,state.poolSearch))?state.poolChoices.map((p,i)=>DltPools.matches(p,state.poolSearch)?`<label class="pool-choice"><input type="checkbox" data-pool-choice="${i}" ${state.poolSelection.some(v=>v.id===p.id)?"checked":""}><span><strong>${escapeHTML(p.label)} · ${escapeHTML(p.issue)}期</strong><small>前区 ${fmtNumbers(p.front)}<br>后区 ${fmtNumbers(p.back)}</small><small>${poolScoreLabel(p)}</small><small class="hash">${escapeHTML(p.plan_id)}</small></span></label>`:"").join(""):empty("当前筛选没有来源池","调整期号筛选或切换来源；已选池会保留。");
  document.querySelectorAll("[data-pool-choice]").forEach(b=>b.onchange=()=>{try{const pool=state.poolChoices[Number(b.dataset.poolChoice)];if(b.checked)addPool(pool);else state.poolSelection=state.poolSelection.filter(p=>p.id!==pool.id);savePoolSelection();renderPoolResults();}catch(e){b.checked=false;toast(e.message,true);}});
  const limit=state.poolBrowseSource==="recommendation"?5:20,pages=Math.max(1,Math.ceil(state.poolBrowseTotal/limit));
  $("pool-source-pager").innerHTML=`<button type="button" class="button button-small" id="pool-source-prev" ${state.poolBrowsePage<=1?"disabled":""}>上一页来源</button><span class="muted">第${state.poolBrowsePage}/${pages}页 · 共${state.poolBrowseTotal}${state.poolBrowseSource==="recommendation"?"期":"个方案"}</span><button type="button" class="button button-small" id="pool-source-next" ${state.poolBrowsePage>=pages?"disabled":""}>下一页来源</button>`;
  for(const [id,delta] of [["pool-source-prev",-1],["pool-source-next",1]])$(id).onclick=()=>action($(id),async()=>{state.poolBrowsePage+=delta;await loadPoolChoices();});
}
async function loadPoolChoices() {
  const revision=state.renderRevision,request=++state.poolBrowseRevision;
  const path=state.poolBrowseSource==="recommendation"?`recommendations?page=${state.poolBrowsePage}&limit=5`:`records?page=${state.poolBrowsePage}&limit=20&kind=personal`;
  const current=()=>revision===state.renderRevision && request===state.poolBrowseRevision && $("pool-choices");
  state.poolChoices=[];state.poolBrowseTotal=0;
  $("pool-choices").innerHTML='<p role="status" class="muted">正在读取原始来源…</p>';
  $("pool-source-pager").innerHTML="";$("pool-select-page").disabled=true;
  try {
    const r=await api(path+`&issue=${encodeURIComponent(state.poolBrowseFilter)}`);if(!current())return;
    state.poolChoices=r.records.map(record=>DltPools.fromRecord({...record,label:r.batches?.find(b=>b.issue===record.plan.request.issue)?.entries.find(e=>e.plan_id===record.id)?.label||record.label},r.selection_evidence?.[record.id]));
    state.poolSelection=state.poolSelection.map(selected=>{
      const fresh=state.poolChoices.find(p=>p.id===selected.id);
      return fresh&&JSON.stringify([fresh.front,fresh.back])===JSON.stringify([selected.front,selected.back])?{...selected,score_evidence:fresh.score_evidence}:selected;
    });
    savePoolSelection();state.poolBrowseTotal=r.total;$("pool-select-page").disabled=false;renderPoolChoices();renderPoolResults();
  } catch(error) {
    if(!current())return;
    $("pool-choices").innerHTML=`<p role="alert">${escapeHTML(state.poolBrowseFilter||"全部期号")}来源池读取失败：${escapeHTML(error.message)}。请点击读取来源池重试，已有选择保留。</p>`;
  }
}
async function loadPoolDraw() {
  const revision=state.renderRevision,issue=state.poolComparisonIssue,request=++state.poolDrawRevision;
  const current=()=>revision===state.renderRevision && request===state.poolDrawRevision && issue===state.poolComparisonIssue && $("pool-results");
  state.poolDraw=null;state.poolDrawStatus=issue?"loading":"missing";renderPoolResults();if(!issue)return;
  try {
    const r=await api(`draws?page=1&limit=1&issue=${encodeURIComponent(issue)}`);if(!current())return;
    state.poolDraw=r.draws.find(d=>d.issue===issue)||null;state.poolDrawStatus=state.poolDraw?"drawn":"missing";renderPoolResults();
  } catch(error) {if(!current())return;state.poolDraw=null;state.poolDrawStatus="failed";renderPoolResults();}
}
function poolThresholds(zone,back) {
  return `<details><summary>各频次门槛删掉了哪些号</summary><p class="muted">相对所选参考池比较；命中是当前期开奖的事后对照。均匀期望只由号码数决定，不是预测置信度。</p><div class="table-wrap"><table><thead><tr><th>最少池数</th><th>号码数 / 命中</th><th>均匀期望命中</th><th>丢弃参考号 / 已中号</th><th>完整号码</th></tr></thead><tbody>${zone.thresholds.map(t=>`<tr><td>${t.minimum_count}</td><td>${t.numbers.length} / ${t.hit_count??"未知"}</td><td>${t.expected_hits_uniform.toFixed(3)}</td><td>${t.removed_from_reference.length} / ${t.lost_hit_count??"未知"}<br>${t.removed_hits_from_reference===null?"待开奖":fmtNumbers(t.removed_hits_from_reference)||"无"}</td><td>${fmtNumbers(t.numbers)||"空池"}</td></tr>`).join("")}</tbody></table></div><p class="muted">${back?"后区":"前区"}入选号命中比例 ${percent(zone.high_frequency.selected_hit_fraction)}；开奖号覆盖率 ${percent(zone.high_frequency.draw_coverage_fraction)}，两者均不代表下一期中奖概率。</p></details>`;
}

function wirePoolPanel() {
  state.poolBrowseRevision=state.poolBrowseRevision||0;state.poolDrawRevision=state.poolDrawRevision||0;
  $("pool-browse-form").onsubmit=e=>{e.preventDefault();action($("pool-browse-form").querySelector("button"),async()=>{state.poolBrowseSource=$("pool-browse-source").value;state.poolBrowseFilter=$("pool-browse-filter").value.trim();state.poolBrowsePage=1;await loadPoolChoices();});};
  for(const [id,key] of [["pool-deduplicate","poolDeduplicate"],["pool-exclude-flat","poolExcludeFlat"]])$(id).onchange=()=>{state[key]=$(id).checked;savePoolSelection();renderPoolResults();};
  $("pool-threshold").onchange=()=>{const value=Number($("pool-threshold").value);if(!Number.isInteger(value)||value<1||value>200){toast("最少出现池数须为1～200的整数。",true);return;}state.poolThreshold=value;savePoolSelection();renderPoolResults();};
  $("pool-select-page").onclick=()=>action($("pool-select-page"),async()=>{if(new Set([...state.poolSelection,...state.poolChoices.filter(p=>DltPools.matches(p,state.poolSearch))].map(p=>p.id)).size>200)throw new Error("最多选择200个池。");state.poolChoices.filter(p=>DltPools.matches(p,state.poolSearch)).forEach(addPool);savePoolSelection();renderPoolChoices();renderPoolResults();});
  $("pool-source-search").oninput=()=>{state.poolSearch=$("pool-source-search").value;renderPoolChoices();};
  $("pool-clear").onclick=()=>{state.poolSelection=[];savePoolSelection();renderPoolChoices();renderPoolResults();};
  $("manual-pool-form").onsubmit=e=>{e.preventDefault();action($("manual-pool-form").querySelector("button"),async()=>{const front=DltPools.numbers($("manual-pool-front").value,35),back=DltPools.numbers($("manual-pool-back").value,12),issue=$("manual-pool-issue").value.trim();const pool=DltPools.pool({id:`manual:${issue}:${front.join("-")}|${back.join("-")}`,label:$("manual-pool-label").value,issue,source:"manual",front,back});if(state.poolSelection.some(p=>p.id===pool.id))throw new Error("相同期号和内容的手动池已在选择中。");addPool(pool);savePoolSelection();renderPoolResults();toast("手动池已添加；它没有原始推荐留档证明。");});};
  $("pool-comparison-form").onsubmit=e=>{e.preventDefault();action($("pool-comparison-form").querySelector("button"),async()=>{state.poolComparisonIssue=$("pool-comparison-issue").value.trim();await loadPoolDraw();});};
  $("copy-pool-common").onclick=()=>action($("copy-pool-common"),async()=>{const r=poolReport();await copy(`前区：${fmtNumbers(r.zones.front.common.numbers)||"无"}\n后区：${fmtNumbers(r.zones.back.common.numbers)||"无"}`);toast("已复制交集号码；购买票组请以原始逐注记录为准。");});
  $("export-pools-json").onclick=()=>download("dlt-pool-intersection.json",poolReport());
  $("export-pools-csv").onclick=()=>download("dlt-pool-intersection.csv",DltPools.csv(poolReport()),"text/csv;charset=utf-8");
  renderPoolResults();loadPoolChoices().catch(e=>toast(e.message,true));loadPoolDraw().catch(e=>toast(e.message,true));
}
