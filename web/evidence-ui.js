"use strict";
function renderStructureAudit(audit) {
  if(!audit||!audit.comparison)return "";
  const cmp=audit.comparison;
  const basis={explicit:"手工指定",numeric_prefix:"自选池中的最小号码",score_prefix:"评分较高的前几个",shuffle_prefix:"种子顺序的前几个",not_prefix:"与自动规则不一致"};
  const zones=["front","back"].flatMap(zone=>{
    const item=audit.bankers?.[zone];
    if(!item||item.basis==="none")return [];
    return [`${zone==="front"?"前区胆":"后区胆"}：${basis[item.basis]||item.basis} ${fmtNumbers(item.numbers||[])}`];
  }).join("；");
  const text=cmp.status==="compared"&&cmp.chosen_mass!=null
    ?`原方案连乘质量 ${cmp.chosen_mass}，同规则最高 ${cmp.best_mass}，最高并列 ${cmp.best_split_count} 种。${cmp.chosen_is_best?"当前取法达到这个最高。":"当前取法低于这个最高。"}${cmp.tickets_eligible_for_best_set==null?"":`已生成的 ${cmp.generated_ticket_count} 注里，有 ${cmp.tickets_eligible_for_best_set} 注达到最高一档。`}`
    :({analytic_tie:`同规模分法全部并列${cmp.best_split_count==null?"":`（${cmp.best_split_count} 种）`}。当前取法也在其中。`,skipped_pool_too_large:"候选组合过多，未逐注比较，质量保持未知。",unknown_scores:"没有原评分，不比较连乘质量，也不把质量记为 0。",not_applicable:"该策略不按连乘质量组票。",full_pool:"复式展开的是整个号码池，没有另一组子集。",not_compared:"整数比较没有完成，质量保持未知。"})[cmp.status]||"质量保持未知。";
  return `<div class="notice info" id="structure-audit"><strong>组票目标核对</strong><p>${escapeHTML(audit.note||"")}</p>${zones?`<p>${escapeHTML(zones)}</p>`:""}<p>${escapeHTML(text)}</p></div>`;
}
function renderSelectionEvidence(result) {
  const audit=renderStructureAudit(result.structure_audit);
  if(result.status==="not_recorded")return `<div class="notice info">${escapeHTML(result.note||"该档案未记录评分，保留为未知。")}</div>${audit}`;
  const e=result.evidence;
  return `<div class="selection-evidence">${audit}<h3>${result.status==="preview_only"?"当前预览的选号依据":"原档案的选号依据"}</h3><p class="muted">${escapeHTML(e.note)} 数据截止 ${escapeHTML(e.data_cutoff||"无")}，评分窗口 ${e.scored_periods}期。</p><div class="grid grid-2">${["front","back"].map(zone=>{
    const z=e[zone],scored=z.score_status==="recorded_scores",boundary=z.boundary_tie;
    const description=!scored?"本策略没有历史评分；下表为初始化顺序，实际构票还受随机抽样或结构约束影响。":z.all_scores_equal?"本区所有号码同分；原始名次由固定种子破局，靠前不代表分值更高。":"分值越高越靠前；相同分值使用原固定种子破局。";
    const kind=({frequency_desc:"出现次数",negative_frequency_desc:"出现次数的相反数",omission_desc:"当前遗漏，未出现时为窗口内下界",no_historical_score:"未使用历史评分"})[z.score_kind]||"未记录";
    const review=result.review?.[zone],show=n=>n===null||n===undefined?"待开奖":fmtNumbers(n)||"无";
    return `<div class="evidence-zone"><h3>${zone==="front"?"前区":"后区"} · ${scored?(z.all_scores_equal?"全部同分":"已记录分值"):"未使用历史评分"}</h3><p class="muted">${description}<br>依据：${escapeHTML(kind)}；候选池${z.candidate_basis==="manual_pool"?"来自自选号码":z.candidate_basis==="rank_prefix"?"按排名截取":"为全区号码"}。</p>${boundary?`<div class="notice">候选截取第${boundary[0]}名；第${boundary[1]}～${boundary[2]}名同分，本次只从这组取前${boundary[0]-boundary[1]+1}个。</div>`:""}<p class="muted">开奖后分解：已入票 ${show(review?.covered)}；入池未构票 ${show(review?.in_pool_not_ticketed)}；池外 ${show(review?.outside_pool)}。</p><div class="table-wrap"><table><thead><tr><th>${scored?"原名次":"初始顺序"}</th><th>号码</th><th>分值</th><th>同分名次范围 / 人数</th><th>次数 / 遗漏</th><th>候选 / 入票</th></tr></thead><tbody>${z.numbers.map(row=>`<tr><td>${row.rank}</td><td>${String(row.number).padStart(2,"0")}${row.banker?"（胆）":""}</td><td>${row.score??"—"}</td><td>${row.tie_count==null?"—":`${row.tie_first_rank}～${row.tie_last_rank} / ${row.tie_count}`}</td><td>${row.frequency??"—"} / ${row.omission_censored?"≥":""}${row.omission??"—"}</td><td>${row.in_candidate_pool?"入池":"池外"} / ${row.in_tickets?"入票":"未入票"}</td></tr>`).join("")}</tbody></table></div></div>`;
  }).join("")}</div><p class="hash">原始方案 ${escapeHTML(e.plan_id)}<br>依据摘要 ${escapeHTML(result.content_hash)}</p></div>`;
}
async function showArchivedEvidence(id,button) {
  const target=$("selection-"+id);if(!target)return;
  if(button.getAttribute("aria-expanded")==="true"){target.hidden=true;button.setAttribute("aria-expanded","false");return;}
  await action(button,async()=>{
    const result=await api(`records/${encodeURIComponent(id)}/evidence`);
    if($("selection-"+id)!==target)return;
    target.innerHTML=renderSelectionEvidence(result);target.hidden=false;button.setAttribute("aria-expanded","true");accessibleTables();
  });
}
async function showPreviewEvidence(plan,button) {
  const target=$("preview-evidence");if(!target)return;
  await action(button,async()=>{
    let result;
    try {result=await api("plans/explain",{request:plan.request,preview_id:plan.id});}
    catch(error){if(state.plan?.id!==plan.id||state.tab!=="generator")return;throw error;}
    if(state.plan?.id!==plan.id||state.tab!=="generator"||$("preview-evidence")!==target)return;
    target.innerHTML=renderSelectionEvidence(result);target.hidden=false;accessibleTables();
  });
}
function stabilityPanel(report) {
  const rows=report.strategies.filter(row=>row.stability);
  if(!rows.length)return "";
  const range=value=>value?value.join("～"):"—";
  return `<section class="card" id="stability-review"><h2>真实留档稳定性与奖金集中度</h2><p class="muted">仅使用窗口内合格的实际已开奖档案，待开奖不计零分。中奖不等于盈利；这里的回本指净额不小于 0，包含只靠固定奖补足成本的期。浮动奖本身够付成本的期数在“单注差额与奖金构成”中分开列出。覆盖范围和单期奖金占比均为已发生的描述，短样本不用于更换默认策略。</p><div class="table-wrap"><table><thead><tr><th>方案</th><th>已开奖 / 奖额完整期</th><th>前 / 后区覆盖范围</th><th>中奖 / 已知盈利 / 回本期</th><th>同年连续未见奖最长</th><th>最大单期奖金占比</th><th>前 / 后区确认等分期</th></tr></thead><tbody>${rows.map(row=>{const s=row.stability;return `<tr><td>${escapeHTML(row.label)}</td><td>${s.settled_periods} / ${s.priced_periods}</td><td>${range(s.front_union_hit_range)} / ${range(s.back_union_hit_range)}</td><td>${s.winning_periods} / ${s.priced_periods?s.profitable_periods:"未知"} / ${s.priced_periods?s.recovered_periods:"未知"}</td><td>${s.longest_consecutive_nonwinning_run}期</td><td>${s.max_single_prize_share==null?({no_settled_periods:"尚无已开奖",missing_awards:"奖额未齐，未知",no_prize:"总奖金为0，不计算"})[s.concentration_status]||"未知":percent(s.max_single_prize_share)}${s.max_single_prize_issues.length?`<br>${s.max_single_prize_issues.map(escapeHTML).join("、")}期`:""}</td><td>${s.front_flat_score_periods} / ${s.back_flat_score_periods}<br>依据缺档 ${s.missing_evidence_periods}期</td></tr>`;}).join("")}</tbody></table></div><details><summary>逐期原始票组与金额</summary>${rows.map(row=>`<h3>${escapeHTML(row.label)}</h3><div class="table-wrap"><table><thead><tr><th>期号</th><th>并集命中 前 / 后</th><th>中奖注数</th><th>成本 / 奖金 / 净额</th><th>原始方案</th></tr></thead><tbody>${row.stability.trace.map(t=>`<tr><td>${escapeHTML(t.issue)}</td><td>${t.front_union_hits} / ${t.back_union_hits}</td><td>${t.winning_tickets}</td><td>${money(t.cost_fen)} / ${money(t.prize_fen)} / ${money(t.net_fen)}</td><td><a href="${base}/reviews?issue=${encodeURIComponent(t.issue)}#record-${encodeURIComponent(t.plan_id)}">查看原票</a></td></tr>`).join("")}</tbody></table></div>`).join("")}</details><p class="muted">盈利和回本仅统计奖额完整期。连期只计算同年连续期序；缺档或跨年会中断。分值未记录或未使用历史评分的来源不会被算作“确认等分”。</p></section>`;
}
