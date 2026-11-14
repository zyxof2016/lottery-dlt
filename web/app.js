"use strict";
const base = document.querySelector('meta[name="app-base"]').content;
const $ = (id) => document.getElementById(id);
const escapeHTML = (v) => String(v ?? "").replace(/[&<>"']/g, (c) => ({"&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;","'":"&#39;"}[c]));
const money = (fen) => fen == null ? "待核实" : `¥${(fen / 100).toFixed(2)}`;
const percent = DltFormat.percent;
const names = {random:"均匀随机",coverage:"均衡覆盖",structural:"后区组合去重",hot:"热号实验",cold:"冷号实验",omission:"遗漏实验",single:"单式多注",compound:"复式",dantuo:"胆拖"};
const tabs = [["generator","方案生成"],["records","我的方案与对奖"],["reviews","每期推荐与复盘"],["draws","开奖历史"],["backtest","概率与历史回测"],["research","策略对照实验"],["settings","系统设置"]];
const state = {tab:"generator",overview:null,records:[],recommendations:{records:[],batches:[]},draws:[],config:null,plan:null,form:null,backtest:null,research:null,stats:null,simulation:null,recordFilter:"",drawFilter:"",generation:0,renderRevision:0,schedules:[],operations:null,recordPage:1,reviewPage:1,drawPage:1,recordTotal:0,reviewTotal:0,drawTotal:0,activeJobs:{},optimizer:null,suite:null,liveReview:null,reviewWindow:30,poolSelection:[],poolThreshold:2,poolChoices:[],poolBrowseSource:"recommendation",poolBrowseFilter:"",poolBrowsePage:1,poolBrowseTotal:0,poolComparisonIssue:"",poolDraw:null,poolStorageWarning:"",poolSearch:"",poolReferenceId:"",poolDrawStatus:"missing",poolDrawRevision:0,poolBrowseRevision:0,liveReviewRevision:0,liveReviewError:"",reviewFilter:"",recordStatus:"",savedPreview:null,previewExpanded:false,poolDeduplicate:false,poolExcludeFlat:false,replacementIssue:"",snapshotWarnings:{},saveDraft:null,draftStored:false,draftRestored:false,draftWarning:""};
const content = $("content");
loadPoolSelection();
loadGeneratorDraft();
function draftStorageKey(){return `dlt-generator-draft:${base||"/"}`;}
function loadGeneratorDraft(){
  try{const raw=sessionStorage.getItem(draftStorageKey());if(!raw)return;const d=DltWorkflow.parseDraft(raw);state.form=d.request;state.saveDraft={issue:d.request.issue,label:d.label,deadline:d.deadline};state.draftStored=true;state.draftRestored=true;}
  catch{state.draftWarning="本标签页的旧草稿未能恢复，请重新填写；原始留档不受影响。";}
}
function persistGeneratorDraft(){
  try{const d=DltWorkflow.draft({schema_version:1,request:state.form,label:state.saveDraft?.label||"",deadline:state.saveDraft?.deadline||""});sessionStorage.setItem(draftStorageKey(),JSON.stringify(d));state.draftStored=true;state.draftWarning="";}
  catch{state.draftWarning="草稿暂未保存到会话，请保留当前页面并检查输入；刷新可能丢失未保存内容。";}
  renderDraftStatus();
}
function renderDraftStatus(){
  if(!$("draft-status"))return;
  const message=state.draftWarning||(state.draftStored?(state.draftRestored?"已恢复本标签页的选号输入；请重新生成并确认后保存。":"输入已暂存在本标签页，切页可继续；刷新后请重新生成。") :"选号输入可在本标签页暂存；只有点击保存才会创建原始档案。");
  if($("draft-status").textContent!==message)$("draft-status").textContent=message;
  $("clear-generator-draft").disabled=!state.draftStored&&!state.draftWarning;
}
function resetGeneratorDraft(){
  try{sessionStorage.removeItem(draftStorageKey());state.draftWarning="";}catch{state.draftWarning="无法清除会话草稿，请检查浏览器存储设置。";return false;}
  state.draftStored=false;state.draftRestored=false;state.saveDraft=null;return true;
}
function prepareGeneratorDraft(){if(state.saveDraft?.issue!==state.form.issue)state.saveDraft={issue:state.form.issue,label:"",deadline:""};persistGeneratorDraft();}
function renderAuxiliaryStatus(){
  const element=$("auxiliary-status");if(!element)return;
  const warnings=state.snapshotWarnings||{};element.hidden=!Object.keys(warnings).length;
  element.innerHTML=element.hidden?"":`<span>${warnings.schedules?`销售日程：${escapeHTML(warnings.schedules)} `:""}${warnings.operations?`自动归档与运行状态：${escapeHTML(warnings.operations)} `:""}已有档案仍可查看。保存前请重试以确认状态。</span><button class="button button-small" id="retry-auxiliary">重试状态读取</button>`;
  if($("retry-auxiliary"))$("retry-auxiliary").onclick=()=>action($("retry-auxiliary"),async()=>{if(await refresh())await render();});
}
function renderReadFailure(error,initial=false){
  content.innerHTML=`<div class="card" role="alert"><h1>${initial?"暂时无法读取数据":"数据读取未完成"}</h1><p class="muted">${escapeHTML(error.message)}</p><p class="muted">输入和筛选仍保留，可重试当前页面。</p><button class="button button-primary" id="retry-page">重试当前页面</button></div>`;
  $("retry-page").onclick=()=>action($("retry-page"),async()=>{if(await refresh())await render();});
}
function usageReadiness(){
  const o=state.overview,s=DltWorkflow.activeSchedule(state.schedules),missingToken=o.admin_token_required&&!sessionStorage.getItem("dlt-admin-token");
  const open=missingToken||!o.draw_count||Object.keys(state.snapshotWarnings||{}).length||state.operations?.strict_schedule&&!s;
  return `<details class="card usage-readiness" id="usage-readiness" ${open?"open":""}><summary><strong>开始前确认</strong><span class="muted"> · 预览 → 保存 → 查票</span></summary><div class="grid grid-3"><div><h3>1. 访问与权限</h3><p class="muted">${missingToken?"当前可浏览；生成与保存需先验证管理员访问。":o.admin_token_required?"当前标签页已填写令牌；访问失败可重新验证。":"本机模式，可直接生成并保存。"}</p><a class="button button-small" href="#settings">${missingToken?"验证管理员访问":"访问与系统设置"}</a></div><div><h3>2. 开奖数据</h3><p class="muted">${o.draw_count?`已缓存${o.draw_count}期，截至${escapeHTML(o.latest_draw.issue)}期${o.data_quality.stale?"，请更新缓存":""}。` :"暂无开奖缓存。随机与均衡可先体验；历史实验需要先准备数据。"}</p><a class="button button-small" href="#draws">${o.draw_count?"查看／更新开奖":"同步或导入开奖"}</a></div><div><h3>3. 目标与截止</h3><p class="muted">${s?`已核实${escapeHTML(s.issue)}期，截止${escapeHTML(new Date(s.deadline).toLocaleString("zh-CN"))}。`:state.snapshotWarnings?.schedules||state.snapshotWarnings?.operations?"日程或运行状态未读取，保存前请重试。":state.operations?.strict_schedule?"尚无有效日程；可预览，保存前须核实官方日程。":"尚无有效日程；请自行核对目标期号与开奖前截止时间。"}</p>${s?`<button class="button button-small" id="use-verified-issue">使用已核实期号</button>`:`<a class="button button-small" href="#settings">核实销售日程</a>`}</div></div><button id="jump-to-generator" class="button button-small margin-top" type="button">前往选号参数</button></details>`;
}
function saveAvailability(plan){
  if(Object.keys(state.snapshotWarnings||{}).length)return "运行或日程状态尚未读取，保存前请点击上方“重试状态读取”。";
  if(state.operations?.strict_schedule&&DltWorkflow.activeSchedule(state.schedules.filter(s=>s.issue===plan.request.issue))===null)return "本服务要求有效的官方日程；当前目标期尚未核实，请到系统设置核实后再保存。";
  return "";
}


async function api(path, body, method = body === undefined ? "GET" : "POST", tokenOverride) {
  const token = tokenOverride ?? sessionStorage.getItem("dlt-admin-token") ?? "";
  const headers = {"Content-Type":"application/json"};
  if (token) headers.Authorization = `Bearer ${token}`;
  return DltTransport.request(`${base}/api/${path}`, {method,headers,body:body === undefined ? undefined : JSON.stringify(body)},DltTransport.readTimeout(path));
}
function toast(message,error=false,next=null) {
  const element=$("toast");element.className=error?"error":"";element.setAttribute("role",error?"alert":"status");
  element.innerHTML=`<span id="toast-message">${escapeHTML(message)}</span>${next?`<a class="toast-action" href="${escapeHTML(next.href)}">${escapeHTML(next.label)}</a>`:""}<button id="dismiss-toast" type="button" aria-label="关闭提示" title="关闭提示"></button>`;
  element.hidden=false;$("dismiss-toast").onclick=()=>{element.hidden=true;};
}
async function action(button, task) {
  if(button){button.disabled=true;button.setAttribute("aria-busy","true");}
  try {await task();} catch(e) {toast(e.message,true);} finally {if(button?.isConnected){button.removeAttribute("aria-busy");if(button.id==="generate")renderEstimate();else button.disabled=button.id==="save-plan"&&Boolean(state.plan&&saveAvailability(state.plan));}}
}
async function refresh() {
  const revision=state.snapshotRevision=(state.snapshotRevision||0)+1;
  let snapshot;
  try{snapshot=await DltWorkflow.loadSnapshot(api);}
  catch(error){if(revision!==state.snapshotRevision)return false;throw error;}
  if(revision!==state.snapshotRevision)return false;
  state.overview=snapshot.overview;state.config=snapshot.config;state.schedules=snapshot.schedules;state.operations=snapshot.operations;state.snapshotWarnings=snapshot.warnings;
  renderAuth();renderAuxiliaryStatus();
  const q=state.overview.data_quality;
  $("quality").hidden=!q.stale&&!q.missing_issues.length;
  $("quality").textContent=state.overview.draw_count ? `开奖缓存截至 ${state.overview.latest_draw.issue} 期（${state.overview.latest_draw.date}）${q.stale?"，请同步最新数据":""}。缓存区间内缺 ${q.missing_issues.length} 期。` : "尚无开奖数据。可同步体彩官方数据，或在开奖历史中导入已核实的 JSON 数据。CSV 可通过命令行导入。随机选号无需历史。";
  return true;
}
function renderAuth() {
  const required=state.overview?.admin_token_required, token=sessionStorage.getItem("dlt-admin-token");
  $("auth-status").hidden=!required;
  $("auth-status").innerHTML=token?'已填写管理员令牌；可在 <a href="#settings">系统设置</a> 验证或退出。':'当前可浏览数据。生成、模拟、回测、同步和保存需要管理员令牌，请先在 <a href="#settings">系统设置</a> 验证登录。';
}
function title(eyebrow, heading, subtitle, right="") {return `<div class="page-title"><div><div class="eyebrow">${eyebrow}</div><h1>${heading}</h1><div class="subtitle">${subtitle}</div></div>${right}</div>`;}
function balls(front, back, draw=null) {
  const zone=(nums,isBack)=>nums.map(n=>{const hit=draw&&(isBack?draw.back:draw.front).includes(n);return `<span class="ball${isBack?" back":""}${hit?" hit":""}" aria-label="${isBack?"后区":"前区"}${String(n).padStart(2,"0")}${hit?"，命中":""}">${String(n).padStart(2,"0")}${hit?'<small class="ball-check" aria-hidden="true">✓</small>':""}</span>`;}).join("");
  return `<div class="balls">${zone(front,false)}<span class="ball-sep">+</span>${zone(back,true)}</div>`;
}
function empty(heading,caption) {return `<div class="empty"><strong>${heading}</strong>${caption}</div>`;}
function stat(label,value,hint="",unit="") {return `<div class="stat"><div class="label">${label}</div><div class="value">${value}<span class="unit">${unit}</span></div><div class="hint">${hint}</div></div>`;}
function field(label,id,type,value,extra="") {return `<label class="field">${label}<input id="${id}" name="${id}" type="${type}" value="${escapeHTML(value)}" ${extra}></label>`;}
function select(label,id,options,selected) {return `<label class="field">${label}<select id="${id}" name="${id}">${options.map(([v,t])=>`<option value="${v}" ${v===selected?"selected":""}>${t}</option>`).join("")}</select></label>`;}
function defaultForm() {return {issue:DltWorkflow.activeSchedule(state.schedules)?.issue||"",mode:"single",strategy:"random",bets:5,front_pool:7,back_pool:3,front_banker_count:1,back_banker_count:0,window:60,multiplier:1,additional:false,front_numbers:[],back_numbers:[],front_bankers:[],back_bankers:[]};}
function fmtNumbers(values) {return values.map(n=>String(n).padStart(2,"0")).join(" ");}
function readForm() {
  const value={...state.form};
  for(const key of ["bets","front_pool","back_pool","front_banker_count","back_banker_count","window","multiplier"])value[key]=Number($(key).value);
  for(const key of ["issue","mode","strategy"])value[key]=$(key).value.trim();
  value.additional=$("additional").checked;
  for(const key of ["front_numbers","back_numbers","front_bankers","back_bankers"])value[key]=$(key).value.trim().split(/[\s,，]+/).filter(Boolean).map(Number);
  if(value.mode==="single"){value.front_numbers=[];value.back_numbers=[];}
  if(value.mode!=="dantuo"){value.front_bankers=[];value.back_bankers=[];value.front_banker_count=0;value.back_banker_count=0;}
  return value;
}
function updateModeFields() {
  const single=$("mode").value==="single", dantuo=$("mode").value==="dantuo";
  for(const option of $("strategy").querySelectorAll('option[value="coverage"],option[value="structural"]'))option.disabled=!single;
  if(!single&&["coverage","structural"].includes($("strategy").value)){$("strategy").value="random";toast("均衡覆盖仅适用于单式，当前改为随机选池。");}
  const experimental=["hot","cold","omission"].includes($("strategy").value);
  $("bets").disabled=!single;
  for(const key of ["front_pool","back_pool"])$(key).disabled=single&&!experimental;
  $("window").disabled=!experimental;
  for(const key of ["front_banker_count","back_banker_count","front_bankers","back_bankers"])$(key).disabled=!dantuo;
  for(const key of ["front_numbers","back_numbers"])$(key).disabled=single;
  for(const key of ["bets","front_pool","back_pool","front_banker_count","back_banker_count","window","front_bankers","back_bankers"])$(key).closest(".field").hidden=$(key).disabled;
  $("custom-pools").hidden=single;
} 
function renderEstimate() {
  const result=DltUsability.estimate(readForm());
  const gate=DltWorkflow.gate(readForm(),state.overview,sessionStorage.getItem("dlt-admin-token"));
  $("plan-estimate").innerHTML=`<strong>${result.bets==null?"请检查投注设置":`${result.bets.toLocaleString()}注 · ${money(result.cost_fen)}`}</strong><span>${escapeHTML(!result.valid?result.message:gate.message||result.message)}</span>${result.valid&&!gate.ready?(gate.href==="#issue"?`<button class="button button-small" id="fix-generation-input" type="button">${gate.action}</button>`:`<a href="${gate.href}">${gate.action}</a>`):""}`;
  $("plan-estimate").className=result.valid?"plan-estimate":"plan-estimate invalid";
  $("generate").disabled=!result.valid||!gate.ready;
  if($("fix-generation-input"))$("fix-generation-input").onclick=()=>{$("issue").focus();$("issue").scrollIntoView({block:"center"});};
}
function generator() {
  if(!state.form)state.form=defaultForm();
  const f=state.form,latest=state.overview.latest_draw;
  const ps=[["随机单式5注","均匀随机 · 5个不同组合","10元"],["均衡覆盖7注","均衡分配各号码的使用次数","14元"],["前区6后区2复式","前区6码展开6注","12元"],["前区5后区4复式","后区4码展开6注","12元"],["双区胆拖","前2胆后1胆 · 共20注","40元"]];
  content.innerHTML=title("PLAN GENERATOR","为下一期准备一组号码","选择投注形态，生成后查看原始票组、成本与覆盖。")+usageReadiness()+
    (latest?`<div class="card"><div class="latest"><div><h3>最近一期开奖 ${escapeHTML(latest.issue)} 期</h3><div class="muted">${escapeHTML(latest.date)}</div></div>${balls(latest.front,latest.back)}</div></div>`:"")+
    `<div class="card"><h2>快捷方案</h2><div class="presets">${ps.map((p,i)=>`<button class="preset" data-preset="${i}"><strong>${p[0]}</strong><span>${p[1]}</span><em>${p[2]} · 基本投注</em></button>`).join("")}</div></div>`+
    `<div class="grid grid-2"><div class="stack"><form id="plan-form" class="card"><div class="card-head"><h2>选号参数</h2><span class="badge">生成后保存</span></div><div class="fields">`+
    field("目标期号","issue","text",f.issue,'placeholder="核对官方期号，如 26119" pattern="[0-9]{5}" required maxlength="5" inputmode="numeric"')+
    select("投注形态","mode",["single","compound","dantuo"].map(v=>[v,names[v]]),f.mode)+
    select("选号策略","strategy",["random","coverage","structural","hot","cold","omission"].map(v=>[v,names[v]]),f.strategy)+
    field("单式注数","bets","number",f.bets,'min="1" max="1000" required')+
    field("前区池大小（含胆）","front_pool","number",f.front_pool,'min="5" max="35" required')+
    field("后区池大小（含胆）","back_pool","number",f.back_pool,'min="2" max="12" required')+
    field("前区胆码个数","front_banker_count","number",f.front_banker_count,'min="0" max="4" required')+
    field("后区胆码个数","back_banker_count","number",f.back_banker_count,'min="0" max="1" required')+
    field("实验历史窗口","window","number",f.window,'min="1" max="500" required')+
    `</div><div class="checks"><label><input id="additional" type="checkbox" ${f.additional?"checked":""}> 追加投注（每注3元）</label><label>倍数 <input id="multiplier" class="short-input" type="number" min="1" max="99" value="${f.multiplier}" required></label></div><details id="custom-pools"><summary>自选号码池与胆码</summary><div class="fields">`+
    [ ["前区号码池","front_numbers"],["后区号码池","back_numbers"],["前区胆码","front_bankers"],["后区胆码","back_bankers"] ].map(([t,k])=>field(t,k,"text",fmtNumbers(f[k]),'placeholder="空格或逗号分隔；胆码留空见下方说明"')).join("")+
    `</div><p class="muted margin-top">号码池包含胆码；自选数量须匹配池大小和胆码个数。胆码留空时，实验策略取评分较高的前几个，自选池取最小号码，随机策略按种子顺序取。这不提高均匀头奖概率。</p></details><div id="plan-estimate" role="status" aria-live="polite"></div><button id="generate" class="button button-primary" type="submit">生成本期方案 →</button><p class="muted margin-top">预览不会自动保存。更改参数后请重新生成；同一期、种子、参数和历史输入保持相同票组。</p><div class="draft-controls"><p class="muted" id="draft-status" role="status"></p><button class="button button-small" id="clear-generator-draft" type="button">清除本页草稿</button></div></form><div class="notice info">不同单注增加头奖组合覆盖；倍投和追加增加投入及对应奖金，不改变这组号码的头奖命中概率。冷热号和遗漏均属于实验策略。</div></div><div id="plan-preview" class="card"></div></div>`;
  $("plan-form").addEventListener("submit",e=>{e.preventDefault();action($("generate"),async()=>{state.form=readForm();const estimate=DltUsability.estimate(state.form),gate=DltWorkflow.gate(state.form,state.overview,sessionStorage.getItem("dlt-admin-token"));if(!estimate.valid||!gate.ready)throw new Error(estimate.valid?gate.message:estimate.message);prepareGeneratorDraft();const revision=++state.generation, request={...state.form};const plan=await api("plans/generate",request);if(revision!==state.generation)return;state.plan=plan;state.draftRestored=false;state.previewExpanded=false;state.simulation=null;if(state.tab==="generator"){renderPreview();$("plan-preview").scrollIntoView({block:"start"});$("preview-heading").focus({preventScroll:true});}toast("方案已生成，尚未保存。");});});
  $("plan-form").addEventListener("input",event=>{if(event.target.id==="mode"&&$("mode").value==="dantuo"&&Number($("front_banker_count").value)+Number($("back_banker_count").value)===0){$("front_banker_count").value="1";if(Number($("front_pool").value)<6)$("front_pool").value="6";}updateModeFields();state.form=readForm();prepareGeneratorDraft();state.generation++;state.plan=null;state.simulation=null;renderEstimate();renderPreview();});
  document.querySelectorAll("[data-preset]").forEach(button=>button.addEventListener("click",()=>{
    const index=Number(button.dataset.preset),issue=$("issue").value.trim();
    state.form={...defaultForm(),issue};
    if(index===1)Object.assign(state.form,{strategy:"coverage",bets:7});
    if(index===2)Object.assign(state.form,{mode:"compound",front_pool:6,back_pool:2});
    if(index===3)Object.assign(state.form,{mode:"compound",front_pool:5,back_pool:4});
    if(index===4)Object.assign(state.form,{mode:"dantuo",front_pool:7,back_pool:3,front_banker_count:2,back_banker_count:1});
    prepareGeneratorDraft();state.generation++;state.plan=null;state.simulation=null;generator();$("issue").focus();
  }));
  $("jump-to-generator").onclick=()=>{$("plan-form").scrollIntoView({block:"start"});$("issue").focus({preventScroll:true});};
  $("clear-generator-draft").onclick=()=>{if(!resetGeneratorDraft()){renderDraftStatus();return;}state.form=defaultForm();state.plan=null;state.savedPreview=null;state.simulation=null;state.generation++;generator();toast("本页草稿已清除，原始留档保留。");};
  if($("use-verified-issue"))$("use-verified-issue").onclick=()=>{const schedule=DltWorkflow.activeSchedule(state.schedules);if(!schedule)return;$("issue").value=schedule.issue;$("issue").dispatchEvent(new Event("input",{bubbles:true}));$("issue").focus();};
  updateModeFields();renderEstimate();renderDraftStatus();renderPreview();
}
function previewTickets(plan,limit) {
  return plan.tickets.slice(0,limit).map((ticket,i)=>`<div class="ticket-row"><div class="ticket-main"><span class="ticket-number">${String(i+1).padStart(2,"0")}</span>${balls(ticket.front,ticket.back)}</div></div>`).join("");
}
function renderPreview() {
  const p=state.plan;
  if(!p){$("plan-preview").innerHTML=empty("先生成，再确认","输入官方目标期号，选择参数。<br>完整票组会显示在这里。");return;}
  const saved=state.savedPreview?.id===p.id?state.savedPreview:null,saveBlocked=saveAvailability(p);
  $("plan-preview").innerHTML=`<div class="card-head"><h2 id="preview-heading" tabindex="-1">${escapeHTML(p.request.issue)} 期票组</h2><span class="badge red">${names[p.request.mode]} · ${p.tickets.length}注</span></div><div class="muted">${names[p.request.strategy]} · ${p.request.multiplier}倍 · ${p.request.additional?"追加":"基本"} ${p.data_cutoff?"· 数据截至 "+escapeHTML(p.data_cutoff):"· 无历史输入"}</div><div class="plan-total"><div class="muted">${p.tickets.length}注 × ${p.request.additional?3:2}元 × ${p.request.multiplier}倍</div><div class="amount"><small>合计</small>${money(p.cost_fen)}</div></div>`+
    (saved?`<div class="notice info margin-top" id="saved-confirmation" role="status">已保存首次档案，号码保持原样。<a class="button button-small" id="view-saved-plan" href="${escapeHTML(DltUsability.savedHref(base,saved))}">查看这组方案与对奖</a></div>`:`<form id="save-form" class="margin-top"><label class="field">开奖前截止时间<input id="save-deadline" type="datetime-local" required><small>核实日程后自动填写；时间使用当前设备时区。没有有效日程时请先核对官方截止时间。</small></label><label class="field margin-top">方案名称（可选）<input id="plan-label" maxlength="80" placeholder="例如：本期均衡方案"></label>${saveBlocked?`<p class="notice margin-top" id="save-blocked" role="status">${escapeHTML(saveBlocked)} <a href="#settings">检查日程与运行状态</a></p>`:""}<button id="save-plan" class="button button-primary margin-top" type="submit" ${saveBlocked?'disabled aria-describedby="save-blocked"':""}>保存并跟踪这组号码</button></form>`)+
    `<div class="actions margin-top"><button id="copy-plan" class="button">复制完整票组</button><button id="simulate-plan" class="button">模拟见奖率</button><button id="explain-preview" class="button" type="button">查看选号依据</button></div><div id="simulation-result" class="muted margin-top"></div><div id="preview-evidence" hidden></div><div class="summary-strip"><span>前区并集<strong>${p.front_pool.length}/35</strong></span><span>后区并集<strong>${p.back_pool.length}/12</strong></span><span>头奖概率<strong>${DltFormat.probability(p.first_prize_probability)}</strong></span></div>`+
    (p.front_bankers.length||p.back_bankers.length?`<div class="notice info">胆码：前区 ${fmtNumbers(p.front_bankers)||"无"} · 后区 ${fmtNumbers(p.back_bankers)||"无"}</div>`:"")+
    `<p class="muted">${escapeHTML(p.note)}</p><h3>逐注号码 · 共${p.tickets.length}注</h3><div id="preview-ticket-list">${previewTickets(p,state.previewExpanded?p.tickets.length:20)}</div>`+
    (p.tickets.length>20?`<p class="muted" id="preview-limit-note">${state.previewExpanded?"已展示全部":"当前展示前20注"}；保存与复制均包含完整${p.tickets.length}注。</p><button id="expand-preview" class="button" ${state.previewExpanded?"hidden":""}>展开全部${p.tickets.length}注</button>`:"")+
    `<details><summary>号码使用次数与核验摘要</summary><h3>前区曝光</h3><div class="exposure">${p.front_exposure.map((n,i)=>`<div class="exposure-item"><strong>${String(i+1).padStart(2,"0")}</strong><span>${n}次</span></div>`).join("")}</div><h3 class="margin-top">后区曝光</h3><div class="exposure">${p.back_exposure.map((n,i)=>`<div class="exposure-item"><strong>${String(i+1).padStart(2,"0")}</strong><span>${n}次</span></div>`).join("")}</div><p class="hash margin-top">票组摘要 ${p.id}<br>历史摘要 ${p.history_hash}</p></details>`;
  if(!saved){
    const schedule=DltWorkflow.activeSchedule(state.schedules.filter(s=>s.issue===p.request.issue));
    if(state.saveDraft?.issue===p.request.issue){$("plan-label").value=state.saveDraft.label;$("save-deadline").value=state.saveDraft.deadline;}
    if(schedule&&(!$("save-deadline").value||state.operations?.strict_schedule)){const date=new Date(schedule.deadline);$("save-deadline").value=new Date(date.getTime()-date.getTimezoneOffset()*60000).toISOString().slice(0,16);$("save-deadline").readOnly=Boolean(state.operations?.strict_schedule);}
    $("save-form").oninput=()=>{state.saveDraft={issue:p.request.issue,label:$("plan-label").value,deadline:$("save-deadline").value};persistGeneratorDraft();};
    $("save-form").onsubmit=e=>{e.preventDefault();action($("save-plan"),async()=>{
      const blocked=saveAvailability(p);if(blocked)throw new Error(blocked);
      const value=$("save-deadline").value;if(!value)throw new Error("请填写已核实的开奖前截止时间。");
      const generation=state.generation;
      const record=await api("records",{request:p.request,preview_id:p.id,deadline:new Date(value).toISOString(),label:$("plan-label").value.trim()});
      if(state.plan?.id===p.id){state.savedPreview=record;if(generation===state.generation)resetGeneratorDraft();}
      if(state.plan?.id===p.id&&state.tab==="generator"){renderPreview();renderDraftStatus();$("view-saved-plan").focus({preventScroll:true});}
      try{await refresh();toast("原始票组已保存，可直接查看这组方案并跟踪对奖。",false,{href:DltUsability.savedHref(base,record),label:"查看这组方案"});}
      catch{toast("保存已成功，概况暂未刷新；可直接查看这组原始方案。",false,{href:DltUsability.savedHref(base,record),label:"查看这组方案"});}
    });};
  }
  if($("expand-preview"))$("expand-preview").onclick=()=>{state.previewExpanded=true;$("preview-ticket-list").innerHTML=previewTickets(p,p.tickets.length);$("expand-preview").hidden=true;$("preview-limit-note").textContent=`已展示全部${p.tickets.length}注；保存与复制包含完整票组。`;};
  $("explain-preview").onclick=()=>showPreviewEvidence(p,$("explain-preview"));
  $("copy-plan").onclick=()=>action($("copy-plan"),async()=>{await copy(p.print_text);toast("已复制当前生成的完整原始票组。");});
  $("simulate-plan").onclick=()=>action($("simulate-plan"),async()=>{
    const trials=Math.min(20000,Math.floor(5000000/p.tickets.length));const r=await api("plans/simulate",{request:p.request,preview_id:p.id,trials});
    if(state.plan?.id!==p.id||state.tab!=="generator")return;
    state.simulation=r;$("simulation-result").textContent=`${r.trials.toLocaleString()}次均匀开奖模拟：整组至少一注中奖 ${percent(r.any_prize_probability)}，95%模拟误差区间 ${r.confidence_interval_95.map(percent).join("～")}。标准固定奖回本下界：低奖池 ${percent(r.fixed_recovery_probability_low)}，高奖池 ${percent(r.fixed_recovery_probability_high)}；不含浮动奖、派奖与税。`;
  });
}

async function copy(text) {
  if(navigator.clipboard&&window.isSecureContext)return navigator.clipboard.writeText(text);
  const input=document.createElement("textarea");input.value=text;document.body.append(input);input.select();
  const ok=document.execCommand("copy");input.remove();if(!ok)throw new Error("浏览器不允许复制，可导出 JSON 查看原始票组。");
}
function download(filename,data,type="application/json") {
  const blob=new Blob([typeof data==="string"?data:JSON.stringify(data,null,2)],{type});const url=URL.createObjectURL(blob);
  const a=document.createElement("a");a.href=url;a.download=filename;document.body.append(a);a.click();a.remove();setTimeout(()=>URL.revokeObjectURL(url),5000);
}

function recordCard(r) {
  const p=r.plan,s=r.settlement,draw=state.draws.find(d=>d.issue===p.request.issue);
  const status=!s?"待开奖":s.unpriced_tickets?"奖金待核实":s.winning_tickets?"已中奖":"未中奖";
  return `<div class="card" id="record-${r.id}"><div class="record-meta"><div><div class="record-title">${escapeHTML(r.label||names[p.request.strategy])} <span class="badge">${escapeHTML(p.request.issue)}期</span></div><div class="muted">${names[p.request.mode]} · ${p.tickets.length}注 · ${p.request.additional?"追加":"基本"} · ${p.request.multiplier}倍 · ${escapeHTML(new Date(r.created_at).toLocaleString("zh-CN"))}</div></div><span class="badge ${s?.winning_tickets?"green":""}">${status}</span></div>`+
    `<div class="summary-strip"><span>投入<strong>${money(p.cost_fen)}</strong></span><span>奖金<strong>${s?money(s.total_prize_fen):"—"}</strong></span><span>净收益<strong>${s?money(s.net_fen):"—"}</strong></span><span>中奖注数<strong>${s?s.winning_tickets:"—"}</strong></span></div>`+
    (draw?`<div class="muted">当期开奖</div><div class="margin-top">${balls(draw.front,draw.back)}</div>`:"")+
    recordCoverage(p,draw)+`<div class="actions margin-top"><button class="button button-small" data-expand="${r.id}" aria-expanded="false" aria-controls="detail-${r.id}">查看原始票组与逐注对奖</button><button class="button button-small" data-copy="${r.id}">复制票组</button><button class="button button-small" data-export="${r.id}">导出 JSON</button><button class="button button-small" data-evidence-id="${r.id}" aria-expanded="false" aria-controls="selection-${r.id}">原始选号依据</button></div><div id="detail-${r.id}" class="record-detail">`+
    `</div><div id="selection-${r.id}" class="margin-top" hidden></div></div>`;
}
function recordDetails(r) {
  const p=r.plan,s=r.settlement,draw=state.draws.find(d=>d.issue===p.request.issue);
  return p.tickets.map((t,i)=>{const result=s?.tickets[i];return `<div class="ticket-row"><div class="ticket-main"><span class="ticket-number">${i+1}</span>${balls(t.front,t.back,draw)}</div><span class="ticket-note">${result?`${result.front_hits}+${result.back_hits} · ${result.level?result.level+"等奖 · "+money(result.amount_fen):"未中"}`:"待开奖"}</span></div>`;}).join("")+
    `<p class="hash margin-top">内容摘要 ${r.id}<br>数据截止 ${escapeHTML(p.data_cutoff||"无")} · 生成规则 ${p.generator_version}<br>截止时间 ${escapeHTML(r.deadline)} · 摘要校验通过</p>` ;
}
function wireRecords() {
  document.querySelectorAll("[data-evidence-id]").forEach(button=>button.onclick=()=>showArchivedEvidence(button.dataset.evidenceId,button));
  document.querySelectorAll("[data-expand]").forEach(b=>b.onclick=()=>{const detail=$("detail-"+b.dataset.expand);if(!detail.dataset.loaded){const record=(state.tab==="reviews"?state.recommendations.records:state.records).find(r=>r.id===b.dataset.expand);detail.innerHTML=recordDetails(record);detail.dataset.loaded="true";}const open=detail.classList.toggle("open");b.setAttribute("aria-expanded",String(open));b.textContent=open?"收起原始票组与逐注对奖":"查看原始票组与逐注对奖";});
  document.querySelectorAll("[data-copy]").forEach(b=>b.onclick=()=>action(b,async()=>{await copy((state.tab==="reviews"?state.recommendations.records:state.records).find(r=>r.id===b.dataset.copy).plan.print_text);toast("已复制归档原票组。");}));
  document.querySelectorAll("[data-export]").forEach(b=>b.onclick=()=>download(`dlt-${b.dataset.export.slice(0,12)}.json`,(state.tab==="reviews"?state.recommendations.records:state.records).find(r=>r.id===b.dataset.export)));
}
function recordSummary(rows) {
  const settled=rows.filter(r=>r.settlement), cost=settled.reduce((n,r)=>n+r.plan.cost_fen,0);
  const prize=!settled.length||settled.some(r=>r.settlement.total_prize_fen==null)?null:settled.reduce((n,r)=>n+r.settlement.total_prize_fen,0);
  return `<div class="grid grid-4">${stat("实际保存方案",rows.length,"按原始票组统计")}${stat("等待开奖",rows.length-settled.length,"刷新后按缓存开奖核对")}${stat("已开奖投入",money(cost),`${settled.length}个已开奖方案`)}${stat("已开奖净收益",money(prize==null?null:prize-cost),settled.length?"有奖额缺失则显示待核实":"暂无已开奖方案，尚无收益结果")}</div>`;
}
function recommendationComparison(rows) {
  const issues=[...new Set(rows.map(r=>r.plan.request.issue))].sort().reverse();
  return issues.map(issue=>{
    const batch=state.recommendations.batches.find(b=>b.issue===issue), group=rows.filter(r=>r.plan.request.issue===issue);
    return `<details class="card margin-top recommendation-period" data-issue="${escapeHTML(issue)}"><summary>${escapeHTML(issue)}期 · 展开原始推荐对比</summary><p class="muted">${batch?`首次留档 ${escapeHTML(new Date(batch.created_at).toLocaleString("zh-CN"))}；本期推荐已固定，再次归档返回原票组。`:"旧版单独留档；没有整期固定清单。"} 各预设预算不同，结果不作为同预算策略排名。</p><div class="table-wrap"><table><thead><tr><th>方案</th><th>注数</th><th>投入</th><th>中奖注数</th><th>奖金</th><th>净收益</th></tr></thead><tbody>${group.map(r=>`<tr><td>${escapeHTML(batch?.entries.find(e=>e.plan_id===r.id)?.label||r.label||names[r.plan.request.strategy])}</td><td>${r.plan.tickets.length}</td><td>${money(r.plan.cost_fen)}</td><td>${r.settlement?r.settlement.winning_tickets:"待开奖"}</td><td>${r.settlement?money(r.settlement.total_prize_fen):"—"}</td><td>${r.settlement?money(r.settlement.net_fen):"—"}</td></tr>`).join("")}</tbody></table></div></details>`;
  }).join("");
}
function records(reviews=false) {
  const source=reviews?state.recommendations.records:state.records.filter(r=>r.kind==="personal");
  const filter=reviews?state.reviewFilter:state.recordFilter;
  const rows=source.filter(r=>!filter||r.plan.request.issue.includes(filter));
  content.innerHTML=title(reviews?"RECOMMENDATION ARCHIVE":"MY PLANS",reviews?"每期推荐与复盘":"让每一组号码都有记录",reviews?"仅展示实际推荐留档；个人方案单独管理。自动任务在已核实日程的提前窗口内固定方案；可查看漏发和失败记录。":"个人保存的号码保持原样；命中号码会在开奖后标为金色。",'<button id="refresh-records" class="button">刷新对奖</button>')+
    (reviews?`<nav class="review-tasks" aria-label="复盘任务"><a href="#recommendation-records">原始票组</a><a href="#live-review">持续复盘</a><a href="#budget-increments">预算增量</a><a href="#replacement-review">换号复盘</a><a href="#source-overlap">来源重叠</a><a href="#stability-review">稳定性</a><a href="#pool-intersection">号码池交集</a></nav>`:"")+
    (reviews?`<details class="card" id="manual-archive"><summary>手动留档本期5个预设</summary><form id="archive-form"><div class="fields">${field("目标期号","archive-issue","text",state.form?.issue||"",'pattern="[0-9]{5}" required placeholder="核对官方期号"')}${field("已核实的截止时间","archive-deadline","datetime-local","","required")}</div><p class="muted margin-top">随机、均衡、两种复式与双区胆拖各一组。首次留档保存整期清单，之后更改种子也不会更换本期推荐。系统不会补造历史推荐。</p><button class="button button-primary" id="archive-all">生成并固定本期5个预设</button></form></details>`:"")+
    `<div id="${reviews?"recommendation-records":"personal-records"}"><p class="muted margin-top">以下汇总仅统计本页记录。</p>`+recordSummary(rows)+`<div class="card margin-top"><div class="card-head"><h2>${reviews?"实际推荐留档":"个人方案列表"} <span class="badge">${rows.length}</span></h2></div><form id="record-filter-form" class="record-filters"><label class="field">期号（支持前缀）<input id="record-filter" placeholder="全部期号" value="${escapeHTML(filter)}" inputmode="numeric" pattern="[0-9]{0,5}" maxlength="5"></label>${reviews?"":select("开奖状态","record-status",[["","全部方案"],["pending","只看待开奖"],["settled","只看已开奖"]],state.recordStatus)}<button class="button button-primary" type="submit">查询方案</button><button id="clear-record-filter" type="button" class="button">清除筛选</button></form><p class="muted">同一票组只保存首次记录。奖金为标准税前金额，活动派奖须另行核实。</p></div>`+
    pager(reviews?state.reviewTotal:state.recordTotal,reviews?state.reviewPage:state.recordPage,reviews?5:20,reviews?"reviewPage":"recordPage")+
    (rows.length?rows.map(recordCard).join(""):`<div class="card">${empty(filter||(!reviews&&state.recordStatus)?"未找到匹配方案":reviews?"还没有推荐留档":"还没有保存的个人方案",filter||(!reviews&&state.recordStatus)?"调整期号或开奖状态，或点击清除筛选。":reviews?"展开手动留档，填写已核实的未来期号和截止时间。":"<a class=button href=\"#generator\">前往方案生成</a>")}</div>`)+(reviews?recommendationComparison(rows):"")+`</div>`+(reviews?`<div id="review-evidence">${liveReviewPanel()}</div>`+poolPanel():"");
  $("refresh-records").onclick=()=>action($("refresh-records"),async()=>{if(!await refresh()||state.tab!==(reviews?"reviews":"records"))return;if(await render())toast("已按缓存开奖复算原始方案。");});
  $("record-filter-form").onsubmit=e=>{e.preventDefault();state[reviews?"reviewFilter":"recordFilter"]=$("record-filter").value.trim();if(!reviews)state.recordStatus=$("record-status").value;state.recordPage=1;state.reviewPage=1;render();};
  $("clear-record-filter").onclick=()=>{state[reviews?"reviewFilter":"recordFilter"]="";if(!reviews)state.recordStatus="";state.recordPage=1;state.reviewPage=1;render();};
  if(reviews)$("archive-form").onsubmit=e=>{e.preventDefault();action($("archive-all"),async()=>{await api("recommendations/archive",{issue:$("archive-issue").value.trim(),deadline:new Date($("archive-deadline").value).toISOString()});await refresh();if(state.tab==="reviews")await render();toast("本期5个预设已固定；已归档的期号返回首次记录。");});};
  wireRecords();wirePager();
  if(reviews){wireLiveReview();wirePoolPanel();loadLiveReview();}
  accessibleTables();
  if(reviews){const schedule=state.schedules.find(s=>new Date(s.deadline)>new Date());if(schedule){$("archive-issue").value=schedule.issue;const d=new Date(schedule.deadline);$("archive-deadline").value=new Date(d.getTime()-d.getTimezoneOffset()*60000).toISOString().slice(0,16);}}
}

function drawHistory() {
  const rows=state.draws.filter(d=>!state.drawFilter||d.issue.includes(state.drawFilter));
  content.innerHTML=title("DRAW HISTORY","每一期开奖，有据可查","官方同步与手动导入使用同一套校验，号码或日期冲突会拒绝整批写入。",'<button id="sync-history" class="button button-primary">同步最近100期</button>')+
    `<div class="card"><div class="card-head"><h2>历史开奖 <span class="badge">${state.drawTotal}期</span></h2><input id="draw-filter" class="button" placeholder="按期号筛选" aria-label="开奖期号筛选" value="${escapeHTML(state.drawFilter)}"></div>`+
    (rows.length?`<div class="table-wrap"><table><thead><tr><th>期号</th><th>开奖日期</th><th>前区 + 后区</th><th>奖表版本</th><th>奖金资料</th></tr></thead><tbody>${rows.map(d=>`<tr><td><strong>${escapeHTML(d.issue)}</strong></td><td>${escapeHTML(d.date)}</td><td>${balls(d.front,d.back)}</td><td>${d.issue>="26014"?"2026 · 七奖级":d.issue>="19019"?"2019 · 九奖级":"早期规则（仅统计）"}</td><td>${Object.keys(d.awards).length}个奖级</td></tr>`).join("")}</tbody></table></div><p class="muted margin-top">每页50期，完整数据可在系统设置导出。</p>`:empty("暂无开奖数据","点击同步官方开奖，或导入已核实的数据。"))+`</div>`+pager(state.drawTotal,state.drawPage,50,"drawPage")+`<div class="grid grid-2"><div class="card"><h2>导入已核实的开奖 JSON</h2><p class="muted">粘贴 Draw 数组或 {"draws": [...]}。金额单位为分，数据不会覆盖已有冲突记录。也支持粘贴官方历史响应（value.list）；CSV 请使用命令行 import。</p><form id="import-form"><label class="field">开奖数据<textarea id="import-json" placeholder='[{"issue":"26014","date":"2026-02-02","front":[16,18,23,34,35],"back":[1,6],"source":"manual"}]' required></textarea></label><div class="actions margin-top"><button class="button button-primary" id="import-button">校验并导入</button></div></form></div><div class="card"><h2>数据与奖金说明</h2><p class="muted">26014期起采用七奖级；此前从19019期起使用九奖级。早于19019期的数据可用于号码统计，当前不核算奖金。</p><p class="muted">新规则固定奖取决于开奖前奖池是否达到8亿元。仅知道开奖后的滚存奖池不能反推当前奖额，程序优先读取各期公布的奖金。</p><p class="muted">缺少奖金时仍可核对命中和奖级；奖金显示待核实。补充同期开奖的奖金资料后，原方案会重新核算。</p><a class="button" href="https://m.lottery.gov.cn/ksjz/m/yxgz_dlt/" target="_blank" rel="noopener noreferrer">体彩官方规则 ↗</a></div></div>`;
  $("draw-filter").onchange=()=>{state.drawFilter=$("draw-filter").value.trim();state.drawPage=1;render();};
  wirePager();
  $("sync-history").onclick=()=>syncDraws($("sync-history"));
  $("import-form").onsubmit=e=>{e.preventDefault();action($("import-button"),async()=>{const value=JSON.parse($("import-json").value);const result=await api("draws/import",value);await refresh();if(state.tab==="draws")await render();toast(`已校验导入，新增或补充 ${result.changed} 期。`);});};
}
function analysisForm(research=false) {
  const id=research?"research":"backtest";
  return `<form id="${id}-form" class="card"><div class="card-head"><h2>${research?"五组同预算对照":"完整选号配置滚动回测"}</h2><span class="badge">只读历史 · 不保存投注</span></div><div class="fields">`+
    (research?"":select("选号策略","bt-strategy",["random","coverage","structural","hot","cold","omission"].map(v=>[v,names[v]]),"random")+select("投注形态","bt-mode",["single","compound","dantuo"].map(v=>[v,names[v]]),"single"))+
    field("每期单式注数","bt-bets","number",5,`min="1" max="${research?30:100}" required`)+field("预热期数","bt-warmup","number",60,'min="5" max="500" required')+
    field("历史窗口","bt-window","number",60,'min="1" max="500" required')+field("回放期数上限","bt-limit","number",100,`min="1" max="${research?500:1500}" required`)+
    field("固定回放种子","bt-seed","text",20261015,'inputmode="numeric" pattern="[0-9]+" required')+
    (research?"":field("前区池大小","bt-front_pool","number",7,'min="5" max="35"')+field("后区池大小","bt-back_pool","number",3,'min="2" max="12"')+field("前区胆数","bt-front_banker_count","number",1,'min="0" max="4"')+field("后区胆数","bt-back_banker_count","number",0,'min="0" max="1"')+field("倍数","bt-multiplier","number",1,'min="1" max="99"'))+
    `</div>`+(research?"":`<label class="checks"><input id="bt-additional" type="checkbox">追加投注</label><details><summary>自选池和胆码（历史回放，仅作描述）</summary><div class="fields">${["front_numbers","back_numbers","front_bankers","back_bankers"].map(k=>field(k.startsWith("front")?"前区"+(k.endsWith("bankers")?"胆码":"号码池"):"后区"+(k.endsWith("bankers")?"胆码":"号码池"),"bt-"+k,"text","",'placeholder="留空按策略选择"')).join("")}</div></details>`)+
    `<p class="muted margin-top">每期只读取此前开奖。支持号码并集覆盖、见奖和已核实奖金下的回本统计。自选池如果事后选定，回放只能描述该固定池表现，不能当作预测检验。</p><div class="actions"><button id="run-${id}" class="button button-primary">${research?"运行五组对照":"提交后台回测"}</button>${research?"":'<button id="copy-generator-config" class="button" type="button">带入当前选号配置</button>'}</div></form>`;
}
function readAnalysis() {
  const result={bets:Number($("bt-bets").value),warmup:Number($("bt-warmup").value),window:Number($("bt-window").value),limit:Number($("bt-limit").value),seed:$("bt-seed").value.trim(),strategy:$("bt-strategy")?.value||"random"};
  if($("bt-mode")){
    result.mode=$("bt-mode").value;
    for(const k of ["front_pool","back_pool","front_banker_count","back_banker_count","multiplier"])result[k]=Number($("bt-"+k).value);
    for(const k of ["front_numbers","back_numbers","front_bankers","back_bankers"])result[k]=$("bt-"+k).value.split(/[\s,，]+/).filter(Boolean).map(Number);
    if(result.mode==="single"){result.front_numbers=[];result.back_numbers=[];}
    if(result.mode!=="dantuo"){result.front_bankers=[];result.back_bankers=[];}
    result.additional=$("bt-additional").checked;
  }
  return result;
}
function backtestView() {
  const t=state.overview.rules;
  content.innerHTML=title("PROBABILITY & BACKTEST","把概率与历史分开看","组合数学给出理论概率；滚动回放只描述冻结规则的历史表现。")+
    `<div class="grid grid-3">${stat("单注头奖概率","1 / 21,425,712","C(35,5) × C(12,2)")}${stat("单注任意奖概率",percent(t.any_prize_probability),"按现行七奖级条件精确计算")}${stat("标准固定奖期望",`¥${t.fixed_prize_expectation_low_yuan.toFixed(3)}–${t.fixed_prize_expectation_high_yuan.toFixed(3)}`,"每注 · 不含一二等奖、派奖与税")}</div><div class="notice margin-top">整组见奖率受到注间重叠影响，不能用单注概率直接乘以注数。生成方案后可运行保留整套票组结构的蒙特卡洛模拟。</div>`+
    `<div class="notice info">回测复用完整选号配置；结构优化位于下方，按同一预算比较三种结构并独立模拟验证。</div>`+analysisForm()+`<div id="backtest-result"></div><div class="card"><h2>现行规则 · 命中与奖金</h2><div class="table-wrap"><table><thead><tr><th>前区 + 后区命中</th><th>奖级</th><th>概率</th><th>开奖前奖池＜8亿</th><th>开奖前奖池≥8亿</th></tr></thead><tbody>${t.rows.filter(r=>r.level).reverse().map(r=>`<tr><td>${r.front_hits}+${r.back_hits}</td><td>${r.level}等奖</td><td>1 / ${Math.round(1/r.probability).toLocaleString()}</td><td>${r.low_pool_yuan==null?"浮动":money(r.low_pool_yuan*100)}</td><td>${r.high_pool_yuan==null?"浮动":money(r.high_pool_yuan*100)}</td></tr>`).join("")}</tbody></table></div></div>`;
  $("backtest-form").onsubmit=e=>{e.preventDefault();action($("run-backtest"),async()=>{toast("正在逐期回放…");state.backtest=await runTask("backtest",readAnalysis());renderBacktest();toast("回测完成，没有写入真实方案。");});};
  $("copy-generator-config").onclick=()=>{const form=state.plan?.request||state.form||defaultForm();for(const [key,value] of Object.entries(form)){const element=$("bt-"+key);if(!element)continue;if(element.type==="checkbox")element.checked=value;else element.value=Array.isArray(value)?fmtNumbers(value):String(value);}$("bt-seed").value=state.config.seed;update();toast("已带入相同选号配置和主种子。");};
  const update=()=>{const mode=$("bt-mode").value, experimental=["hot","cold","omission"].includes($("bt-strategy").value);if(mode!=="single"&&["coverage","structural"].includes($("bt-strategy").value))$("bt-strategy").value="random";$("bt-bets").disabled=mode!=="single";for(const k of ["front_pool","back_pool"])$("bt-"+k).disabled=mode==="single"&&!experimental;for(const k of ["front_banker_count","back_banker_count","front_bankers","back_bankers"])$("bt-"+k).disabled=mode!=="dantuo";for(const k of ["front_numbers","back_numbers"])$("bt-"+k).disabled=mode==="single";};$("bt-mode").onchange=update;$("bt-strategy").onchange=update;update();
  content.insertAdjacentHTML("beforeend",optimizerForm());wireOptimizer();renderBacktest();renderOptimizer();
}
function renderBacktest() {
  const r=state.backtest;if(!r||!$("backtest-result"))return;
  $("backtest-result").innerHTML=`<div class="grid grid-4">${stat("实际回放",r.periods,`${r.first_issue}～${r.last_issue}`,"期")}${stat("整组见奖率",percent(r.issue_win_rate),`${r.winning_issues}期至少一注中奖`)}${stat("中奖单注",r.winning_tickets,"按原始逐期票组计数","注")}${stat("完整净收益",money(r.net_fen),`${r.unpriced_issues}期有奖金待核实`)}</div><div class="card margin-top"><div class="card-head"><h2>逐期结果</h2><button id="export-backtest" class="button button-small">导出完整报告</button></div><p class="muted">${escapeHTML(r.note)}</p><div class="table-wrap"><table><thead><tr><th>目标期</th><th>数据截止</th><th>中奖注数</th><th>前区并集命中</th><th>后区并集命中</th><th>奖金</th></tr></thead><tbody>${r.trace.slice(-100).reverse().map(t=>`<tr><td>${t.issue}</td><td>${t.data_cutoff}</td><td>${t.winning_tickets}</td><td>${t.front_union_hits}/5</td><td>${t.back_union_hits}/2</td><td>${money(t.prize_fen)}</td></tr>`).join("")}</tbody></table></div><p class="muted margin-top">展示最近100期；导出包含全部轨迹。</p></div>`;
  $("export-backtest").onclick=()=>download(`dlt-backtest-${r.first_issue}-${r.last_issue}.json`,r);
}
function researchView() {
  content.innerHTML=title("RESEARCH LAB","对照研究，保留证据","同一期、同一注数、同一预算比较策略；评分与历史差异不当作中奖概率。")+
    `<div class="notice info">提供多种子、按规则分段和历史留出验证，支持后台进度与取消。专家融合及未来预注册确认不在本次实现范围内。五组预先固定为：均匀随机、均衡覆盖、热号、冷号、遗漏。热号和冷号按窗口频次排序，遗漏按窗口内未出现期数排序，同分采用固定种子。研究输出为探索性回放。</div>`+
    analysisForm(true)+`<div id="research-result"></div><div class="card"><div class="card-head"><h2>最近60期号码统计</h2><button id="load-stats" class="button button-small">读取统计</button></div><p class="muted">频次与遗漏仅描述历史，不预测下一期。窗口中未出现时，遗漏数字为下界。</p><div id="statistics-result"></div></div>`;
  $("research-form").onsubmit=e=>{e.preventDefault();action($("run-research"),async()=>{toast("正在按相同窗口比较五组策略…");state.research=await runTask("research",{config:readAnalysis(),seeds:[20261015],validation_periods:20});state.suite=state.research;state.research={reports:state.suite.reports.filter(r=>r.phase==="validation").map(r=>r.report),note:state.suite.note};renderResearch();renderSuite();toast("历史留出对照完成。");});};
  $("load-stats").onclick=()=>action($("load-stats"),async()=>{state.stats=await api("statistics?window=60");renderStats();});content.insertAdjacentHTML("beforeend",`<form id="suite-form" class="card"><h2>多种子独立留出验证</h2><div class="fields">${field("不同随机种子（逗号分隔）","suite-seeds","text","20261015,20261016,20261017")}${field("留出验证期数","suite-validation","number",100,'min="10" max="300" required')}</div><p class="muted margin-top">沿用上方注数、窗口、预热和训练期数；在训练期选择策略，在后续留出期报告配对区间、Holm校正和规则分段。结果不属于未来确认。</p><button class="button button-primary margin-top" id="run-suite">提交多种子研究</button></form><div id="suite-result"></div>`);
  $("suite-form").onsubmit=e=>{e.preventDefault();action($("run-suite"),async()=>{state.suite=await runTask("research",{config:readAnalysis(),seeds:readSuiteSeeds(),validation_periods:Number($("suite-validation").value)});renderSuite();toast("多种子留出报告已保存。");});};
  renderResearch();renderStats();renderSuite();
}
function renderResearch() {
  const r=state.research;if(!r||!$("research-result"))return;
  $("research-result").innerHTML=`<div class="card"><div class="card-head"><h2>同期同预算结果</h2><button id="export-research" class="button button-small">导出研究轨迹</button></div><p class="muted">${escapeHTML(r.note)}</p><div class="table-wrap"><table><thead><tr><th>策略</th><th>回放期数</th><th>每期注数 / 预算</th><th>整组见奖率</th><th>中奖注数</th><th>相对随机每期中奖注差</th><th>完整净收益</th></tr></thead><tbody>${r.reports.map(x=>`<tr><td><strong>${names[x.config.strategy]}</strong></td><td>${x.periods}</td><td>${x.actual_bets}注 / ${money(x.cost_fen/x.periods)}</td><td>${percent(x.issue_win_rate)}</td><td>${x.winning_tickets}</td><td>${x.paired_mean_winning_ticket_delta.toFixed(4)}</td><td>${money(x.net_fen)}</td></tr>`).join("")}</tbody></table></div></div>`;
  $("export-research").onclick=()=>download("dlt-research.json",r);
}
function renderStats() {
  if(!state.stats||!$("statistics-result"))return;const r=state.stats;
  $("statistics-result").innerHTML=`<p class="muted">有效窗口 ${r.window}期，数据截至 ${escapeHTML(r.data_cutoff||"无")}。</p><div class="grid grid-2">${[["前区",r.front],["后区",r.back]].map(([name,rows])=>`<div class="table-wrap"><h3>${name}</h3><table><thead><tr><th>号码</th><th>出现次数</th><th>均匀理论次数</th><th>当前遗漏</th></tr></thead><tbody>${rows.map(x=>`<tr><td>${String(x.number).padStart(2,"0")}</td><td>${x.count}</td><td>${x.expected_count.toFixed(2)}</td><td>${x.omission_censored?"≥":""}${x.omission}</td></tr>`).join("")}</tbody></table></div>`).join("")}</div>`;
}
function settings() {
  content.innerHTML=title("SYSTEM SETTINGS","可复现，也可核验","管理生成种子、访问令牌与数据导出。已保存方案保留原始票面。")+
    `<div class="grid grid-2"><div class="card"><h2>生成主种子</h2><form id="seed-form"><label class="field">当前种子<input id="seed" type="text" inputmode="numeric" pattern="[0-9]+" value="${escapeHTML(state.config.seed)}" required><small>更改种子只影响此后生成的预览，已归档记录保持原样。</small></label><button id="save-seed" class="button button-primary margin-top">保存主种子</button></form></div><div class="card"><h2>管理员访问</h2><form id="token-form"><label class="field">管理员令牌<input id="admin-token" type="password" autocomplete="off" value="${escapeHTML(sessionStorage.getItem("dlt-admin-token")||"")}" placeholder="LOTTERY_ADMIN_TOKEN"><small>${state.overview.admin_token_required?"当前服务的写入与完整数据导出需要管理员令牌。":"当前为本机模式，无需令牌即可操作。"}令牌只存放在当前标签页的会话存储中。</small></label><div class="actions margin-top"><button class="button">验证并在当前标签页使用</button><button id="clear-token" class="button" type="button">退出管理员</button></div></form></div></div><div class="card"><h2>数据与完整性</h2><p class="muted">数据持久化在 SQLite，保存方案采用内容摘要与审计链校验。导出包含开奖、配置和实际方案，可离线查看；完整数据库备份请使用 SQLite 在线备份命令。</p><div class="actions"><button id="export-data" class="button button-primary">校验并导出全部 JSON</button><button id="sync-settings" class="button">同步最近100期</button></div><p class="muted margin-top">本机哈希用于检查内容一致性；若需要独立的开奖前时间证明，应把摘要留到可信外部时间记录。</p></div><div class="card"><h2>版本与规则</h2><p class="muted">lottery-dlt 1.9.0 · 全 Rust 后端 · 本地静态资源 · 生成规则 dlt-plan-v3</p><p class="muted">26014期起为七奖级规则。新规则按开奖前8亿元奖池门槛分档；一、二等奖为浮动奖。奖金优先使用各期公告，当前不自动核算活动派奖及税。</p></div>`;
  $("seed-form").onsubmit=e=>{e.preventDefault();action($("save-seed"),async()=>{const config=await api("config",{seed:$("seed").value.trim()},"PUT");state.snapshotRevision=(state.snapshotRevision||0)+1;state.config=config;state.plan=null;toast("主种子已更新，请重新生成方案。");});};
  $("token-form").onsubmit=e=>{e.preventDefault();action($("token-form").querySelector("button"),async()=>{const token=$("admin-token").value.trim();await api("auth",undefined,"GET",token);sessionStorage.setItem("dlt-admin-token",token);renderAuth();toast("管理员令牌验证通过。");});};
  $("clear-token").onclick=()=>{sessionStorage.removeItem("dlt-admin-token");$("admin-token").value="";renderAuth();toast("已退出当前标签页的管理员访问。");};
  $("export-data").onclick=()=>action($("export-data"),async()=>{download(`dlt-export-${new Date().toISOString().slice(0,10)}.json`,await api("export"));toast("记录摘要与审计链校验通过，数据已导出。");});
  $("sync-settings").onclick=()=>syncDraws($("sync-settings"));
  content.insertAdjacentHTML("beforeend",operationsPanel());wireOperations();
}
async function syncDraws(button) {await action(button,async()=>{toast("正在连接体彩官方接口…");const r=await api("draws/sync",{pages:1});await refresh();render();toast(`同步 ${r.fetched} 期，新增或补充 ${r.changed} 期。`);});}
async function render() {
  document.body.dataset.view=state.tab;
  const revision=++state.renderRevision;
  $("navigation").innerHTML=tabs.map(([id,label])=>`<button data-tab="${id}" class="${state.tab===id?"active":""}" aria-current="${state.tab===id?"page":"false"}">${label}</button>`).join("");
  document.querySelectorAll("[data-tab]").forEach(b=>b.onclick=()=>{location.hash=b.dataset.tab;});
  try {
    if(state.tab==="draws"){const r=await api(`draws?page=${state.drawPage}&limit=50&issue=${encodeURIComponent(state.drawFilter)}`);if(revision!==state.renderRevision)return false;state.draws=r.draws;state.drawTotal=r.total;}
    if(state.tab==="records"){const r=await api(`records?page=${state.recordPage}&limit=20&kind=personal&issue=${encodeURIComponent(state.recordFilter)}&status=${encodeURIComponent(state.recordStatus)}`);if(revision!==state.renderRevision)return false;state.records=r.records;state.recordTotal=r.total;state.draws=r.draws;}
    if(state.tab==="reviews"){const r=await api(`recommendations?page=${state.reviewPage}&limit=5&issue=${encodeURIComponent(state.reviewFilter)}`);if(revision!==state.renderRevision)return false;state.recommendations=r;state.reviewTotal=r.total;state.draws=r.draws;state.liveReview=null;state.liveReviewError="";}
    if(revision!==state.renderRevision)return false;
    ({generator,records:()=>records(false),reviews:()=>records(true),draws:drawHistory,backtest:backtestView,research:researchView,settings}[state.tab])();
    accessibleTables();scrollReviewAnchor();return true;
  } catch(e){if(revision!==state.renderRevision)return false;renderReadFailure(e);toast(e.message,true);return false;}
}
function readTab() {const id=location.hash.slice(1)|| (location.pathname.endsWith("/reviews")?"reviews":"generator");state.tab=id.startsWith("saved-")?"records":["live-review","budget-increments","replacement-review","source-overlap","stability-review","pool-intersection","recommendation-records"].includes(id)||id.startsWith("record-")?"reviews":tabs.some(t=>t[0]===id)?id:"generator";}
window.addEventListener("hashchange",async()=>{const previous=state.tab;readTab();if(previous==="reviews"&&state.tab==="reviews"){scrollReviewAnchor();return;}const tab=state.tab;await render();if(tab===state.tab&&tabs.some(t=>t[0]===location.hash.slice(1))){const heading=content.querySelector("h1");if(heading){heading.tabIndex=-1;heading.focus({preventScroll:true});}window.scrollTo(0,0);}});
document.addEventListener("keydown",event=>{if(event.key==="Escape")$("toast").hidden=true;});
$("sync-top").onclick=()=>syncDraws($("sync-top"));
readTab();
const initialIssue=new URLSearchParams(location.search).get("issue");if(initialIssue&&/^[0-9]{5}$/.test(initialIssue))state[state.tab==="reviews"?"reviewFilter":"recordFilter"]=initialIssue;
refresh().then(updated=>{if(updated)return render();}).catch(e=>{renderReadFailure(e,true);toast(e.message,true);});

function pager(total,page,limit,key) {
  const pages=Math.max(1,Math.ceil(total/limit));
  return `<div class="actions margin-top"><button class="button" data-page-key="${key}" data-page="${page-1}" ${page<=1?"disabled":""}>上一页</button><span class="muted">第${page}/${pages}页 · 共${total}${key==="reviewPage"?"期":"条"}</span><button class="button" data-page-key="${key}" data-page="${page+1}" ${page>=pages?"disabled":""}>下一页</button></div>`;
}
function wirePager(){document.querySelectorAll('[data-page-key]').forEach(b=>b.onclick=()=>{state[b.dataset.pageKey]=Number(b.dataset.page);render();});}
function renderJobs(){
  const rows=Object.values(state.activeJobs);$("job-monitor").hidden=!rows.length;
  $("job-monitor").innerHTML=`<h2>后台任务</h2>${rows.map(j=>`<div class="actions"><span class="muted">${escapeHTML(j.kind)} · ${escapeHTML(j.state)} · ${Math.round(j.progress*100)}%</span><progress max="1" value="${j.progress}"></progress>${["queued","running"].includes(j.state)?`<button class="button button-small" data-cancel-job="${j.id}">取消任务</button>`:""}</div>`).join("")}`;
  document.querySelectorAll('[data-cancel-job]').forEach(b=>b.onclick=()=>action(b,async()=>{await api(`jobs/${b.dataset.cancelJob}/cancel`,{});toast("已请求取消任务。");}));
}
async function runTask(kind,input){
  const submitted=await api("jobs",{kind,input});state.activeJobs[submitted.id]=submitted;renderJobs();
  for(;;){
    const job=await api(`jobs/${submitted.id}`);state.activeJobs[job.id]=job;renderJobs();
    if(job.state==="completed")return job.result;
    if(["failed","cancelled","interrupted"].includes(job.state))throw new Error(job.error||job.state);
    await new Promise(resolve=>setTimeout(resolve,700));
  }
}
function optimizerForm(){return `<form id="optimizer-form" class="card"><h2>固定预算结构优化</h2><div class="fields">${field("目标期号","opt-issue","text",state.form?.issue||defaultForm().issue,'pattern="[0-9]{5}" required')}${field("单式注数","opt-bets","number",7,'min="1" max="30" required')}${select("优化目标","opt-objective",[["seen","整组见奖率"],["coverage","号码及后区组合覆盖"],["recovery","标准固定奖回本下界"]],"seen")}${field("每阶段模拟次数","opt-trials","number",5000,'min="1000" max="20000" required')}</div><p class="muted margin-top">同预算比较随机、均衡和后区组合去重。用一批均匀模拟选优，再用另一批独立模拟验证。选出的真实票组可以带入生成页保存；不代表已证明预测优势。</p><button class="button button-primary" id="run-optimizer">提交结构优化任务</button></form><div id="optimizer-result"></div>`;}
function wireOptimizer(){ $("optimizer-form").onsubmit=e=>{e.preventDefault();action($("run-optimizer"),async()=>{state.optimizer=await runTask("optimize",{request:{...defaultForm(),issue:$("opt-issue").value,bets:Number($("opt-bets").value)},objective:$("opt-objective").value,trials:Number($("opt-trials").value)});renderOptimizer();toast("结构优化及独立验证完成。");});}; }
function renderOptimizer(){
  const r=state.optimizer;if(!r||!$("optimizer-result"))return;
  $("optimizer-result").innerHTML=`<div class="card"><h2>同预算结构比较 · ${money(r.budget_fen)}</h2><p class="muted">${escapeHTML(r.note)}</p><div class="table-wrap"><table><thead><tr><th>结构</th><th>前区并集</th><th>后区并集</th><th>不同后区组合</th><th>选优样本见奖率</th><th>选优样本固定奖回本下界</th><th>独立验证见奖率</th></tr></thead><tbody>${r.candidates.map((x,i)=>`<tr><td>${names[x.plan.request.strategy]}${i===r.selected_index?"（选中）":""}</td><td>${x.structure.front_union}/35</td><td>${x.structure.back_union}/12</td><td>${x.structure.unique_back_pairs}/66</td><td>${percent(x.selection_simulation.any_prize_probability)}</td><td>${percent(x.selection_simulation.fixed_recovery_probability_low)}</td><td>${percent(x.validation_simulation.any_prize_probability)}</td></tr>`).join("")}</tbody></table></div><p class="muted margin-top">选中方案独立验证：见奖率 ${percent(r.independent_validation.any_prize_probability)}，95%模拟区间 ${r.independent_validation.confidence_interval_95.map(percent).join("～")}；低奖池标准固定奖回本下界 ${percent(r.independent_validation.fixed_recovery_probability_low)}。</p><div class="actions"><button id="use-optimized-plan" class="button button-primary">带入选中票组</button><button id="export-optimizer" class="button">导出完整证据</button></div></div>`;
  $("use-optimized-plan").onclick=()=>{state.form=r.selected_plan.request;state.plan=r.selected_plan;state.generation++;location.hash="generator";};
  $("export-optimizer").onclick=()=>download("dlt-structure-optimization.json",r);
}
function renderSuite(){
  const r=state.suite;if(!r||!$("suite-result"))return;
  $("suite-result").innerHTML=`<div class="card"><h2>历史留出验证</h2><p class="muted">训练截止 ${r.selection_end}，验证 ${r.validation_start}～${r.validation_end}；仅依据训练选中 ${names[r.selected_on_training]}。${escapeHTML(r.note)}</p><div class="table-wrap"><table><thead><tr><th>策略</th><th>相对随机见奖率差</th><th>近似95%区间</th><th>Holm校正近似p</th><th>有效开奖期数</th></tr></thead><tbody>${r.validation_comparisons.map(x=>`<tr><td>${names[x.strategy]}</td><td>${x.paired.mean==null?"不足":percent(x.paired.mean)}</td><td>${x.paired.ci95?x.paired.ci95.map(percent).join("～"):"不足"}</td><td>${x.holm_adjusted_p_approx.toFixed(4)}</td><td>${x.paired.n}</td></tr>`).join("")}</tbody></table></div><h3 class="margin-top">按奖表规则分段</h3><div class="table-wrap"><table><thead><tr><th>策略</th><th>奖表版本</th><th>种子×期次</th><th>见奖率</th></tr></thead><tbody>${r.rule_segments.map(x=>`<tr><td>${names[x.strategy]}</td><td>${x.rule_version}</td><td>${x.seed_period_observations}</td><td>${percent(x.win_rate)}</td></tr>`).join("")}</tbody></table></div><button class="button margin-top" id="export-suite">导出报告与原始票组轨迹</button></div>`;
  $("export-suite").onclick=()=>download("dlt-historical-holdout.json",r);
}
function operationsPanel(){
  if(!state.operations)return `<div class="card"><h2>运行状态暂不可用</h2><p class="muted">自动归档、日程要求和备份状态未读取。请点击上方“重试状态读取”，成功前不显示默认设置。</p></div>`;
  const o=state.operations,s=state.schedules.find(s=>new Date(s.deadline)>new Date());
  return `<div class="card"><h2>销售日程与自动推荐</h2><p class="muted">${s?`${escapeHTML(s.issue)}期，截止 ${escapeHTML(new Date(s.deadline).toLocaleString("zh-CN"))}。来源方式：${s.basis==="explicit_official_schedule"?"官方明确销售日程":"官方销售规则与休市表推导，已逐期核对本年实际开奖；临时调整须以公告为准"}。`:'暂无有效销售日程，归档会要求先同步核实。'}<br>${o?.last_sync?.error?"上次同步失败："+escapeHTML(o.last_sync.error):"最近同步："+escapeHTML(o?.last_sync?.at||"尚无记录")}</p><button id="sync-schedule" class="button">核实官方日程</button><form id="automation-form" class="margin-top"><label class="checks"><input type="checkbox" id="automation-enabled" ${o?.automation?.enabled?"checked":""}>启用自动推荐归档</label>${field("截止前多少分钟进入归档窗口","automation-lead","number",o?.automation?.lead_minutes||120,'min="15" max="10080" required')}<p class="muted margin-top">定时任务在窗口内保存五个参考预设，首次保存后固定；这不是实际购票。已知日程的漏发和失败会保留。</p><button class="button button-primary margin-top">保存自动归档设置</button></form><div class="table-wrap margin-top"><table><thead><tr><th>期号</th><th>状态</th><th>说明</th></tr></thead><tbody>${(o?.events||[]).map(e=>`<tr><td>${e.issue}</td><td>${escapeHTML(e.status)}</td><td>${escapeHTML(e.error||"已固定原始票组")}</td></tr>`).join("")}</tbody></table></div></div><div class="card"><h2>备份与恢复验证</h2><p class="muted">最近备份：${escapeHTML(o?.last_backup?.at||"尚无记录")}。备份使用在线一致性快照；恢复验证写入独立临时库，保留生产数据。</p><button id="backup-now" class="button">立即备份并验证恢复</button><div id="backup-result" class="muted margin-top"></div></div><div class="card"><h2>后台任务历史</h2><button id="load-jobs" class="button">查看最近任务</button><div id="jobs-history" class="margin-top"></div></div>`;
}
function wireOperations(){
  if(!state.operations)return;
  $("sync-schedule").onclick=()=>action($("sync-schedule"),async()=>{await api("schedules/sync",{});await refresh();if(state.tab==="settings")render();toast("销售日程已核实。");});
  $("automation-form").onsubmit=e=>{e.preventDefault();action($("automation-form").querySelector("button"),async()=>{await api("automation",{enabled:$("automation-enabled").checked,lead_minutes:Number($("automation-lead").value)},"PUT");await refresh();toast("自动推荐设置已保存。");});};
  $("backup-now").onclick=()=>action($("backup-now"),async()=>{const r=await api("backups",{});await refresh();if($("backup-result"))$("backup-result").textContent=`备份完成，恢复校验 ${r.restore_verified?"通过":"未完成"}，${r.records_verified}个原始方案已验证。`;});
  $("load-jobs").onclick=()=>action($("load-jobs"),async()=>{const r=await api("jobs");if(!$("jobs-history"))return;$("jobs-history").innerHTML=r.jobs.map(j=>`<p class="muted">${escapeHTML(j.kind)} · ${escapeHTML(j.state)} · ${Math.round(j.progress*100)}% · ${escapeHTML(j.error||j.created_at)} <button class="button button-small" data-download-job="${j.id}">下载任务证据</button></p>`).join("")||"尚无任务。";document.querySelectorAll('[data-download-job]').forEach(b=>b.onclick=()=>action(b,async()=>download(`dlt-job-${b.dataset.downloadJob.slice(0,12)}.json`,await api(`jobs/${b.dataset.downloadJob}`))));});
}

function readSuiteSeeds(){const seeds=$("suite-seeds").value.split(/[\s,，]+/).filter(Boolean).map(Number);if(!seeds.every(n=>Number.isSafeInteger(n)&&n>=0))throw new Error("种子须为非负安全整数；大种子请通过Rust命令行或API提供。");return seeds;}

function accessibleTables() {
  content.querySelectorAll(".table-wrap").forEach(element=>{element.tabIndex=0;element.setAttribute("role","region");element.setAttribute("aria-label","数据表格，可使用方向键横向滚动");});
}
function scrollReviewAnchor() {
  const id=location.hash.slice(1),target=id.startsWith("saved-")?"record-"+id.slice(6):id;
  if(["live-review","budget-increments","replacement-review","source-overlap","stability-review","pool-intersection","recommendation-records"].includes(id)||id.startsWith("record-")||id.startsWith("saved-")){
    const element=$(target);if(!element)return;
    if(id.startsWith("record-")||id.startsWith("saved-")){const button=element.querySelector("[data-expand]");if(button?.getAttribute("aria-expanded")==="false")button.click();}
    element.scrollIntoView({block:"start"});
  }
}

function recordCoverage(plan,draw) {
  const front=[...new Set(plan.tickets.flatMap(t=>t.front))],back=[...new Set(plan.tickets.flatMap(t=>t.back))];
  const missed=draw?`开奖漏号：前区 ${fmtNumbers(draw.front.filter(n=>!front.includes(n)))||"无"}；后区 ${fmtNumbers(draw.back.filter(n=>!back.includes(n)))||"无"}。`:'待开奖，命中与漏号未知。';
  return `<p class="record-coverage">实际并集：前区 ${front.length}/35，后区 ${back.length}/12；重复位置 ${plan.tickets.length*5-front.length} / ${plan.tickets.length*2-back.length}。<br>${missed}<br><span class="muted">当前注数不加池或胆码约束时，并集上界为前区 ${Math.min(35,plan.tickets.length*5)}、后区 ${Math.min(12,plan.tickets.length*2)}。并集覆盖与单注命中、奖金分别查看。</span></p>`;
}
