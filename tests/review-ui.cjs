const {test}=require("node:test");
const assert=require("node:assert/strict");
const fs=require("node:fs");
const vm=require("node:vm");
const path=require("node:path");
const P=require("../web/pools.js");

// Check rendering and session state without a browser or network. This does not
// assert browser layout, event behavior, or replace the Playwright acceptance.
function context(saved) {
  const nodes=new Map(),storage=new Map(saved===undefined?[]:[["dlt-pool-selection",saved]]);
  const state={poolBrowseRevision:0,poolDrawRevision:0,liveReviewRevision:0,renderRevision:1,poolSelection:[],poolThreshold:2,poolComparisonIssue:"26030",poolDraw:null,poolStorageWarning:""};
  const c={state,DltPools:P,document:{querySelectorAll:()=>[]},
    $:id=>{if(!nodes.has(id))nodes.set(id,{innerHTML:"",textContent:"",disabled:false});return nodes.get(id);},
    sessionStorage:{getItem:key=>storage.get(key)||null,setItem:(key,value)=>storage.set(key,value)},
    escapeHTML:value=>String(value??"").replace(/[&<>"']/g,c=>({"&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;","'":"&#39;"}[c])),
    base:"",
    empty:(heading,caption)=>`<div>${heading} ${caption}</div>`,percent:n=>`${(n*100).toFixed(2)}%`,money:n=>n==null?"待核实":`¥${n/100}`,
    fmtNumbers:values=>values.map(n=>String(n).padStart(2,"0")).join(" "),
    stat:(label,value,hint)=>`${label}: ${value} (${hint})`};
  vm.createContext(c);vm.runInContext(fs.readFileSync(path.join(__dirname,"../web/reviews-ui.js"),"utf8"),c);
  return Object.assign(c,{nodes,storage});
}
test("session restore validates format and keeps a usable default after corruption",()=>{
  const p=P.pool({id:"a",front:[1,2],back:[1],label:"saved"});
  const good=context(JSON.stringify({schema_version:1,threshold:3,pools:[p]}));good.loadPoolSelection();
  assert.equal(good.state.poolThreshold,3);assert.equal(good.state.poolSelection[0].id,"a");
  for(const value of ["{",JSON.stringify({schema_version:1,pools:[p]}),JSON.stringify({schema_version:1,threshold:0,pools:[p]})]) {
    const c=context(value);c.loadPoolSelection();assert.equal(c.state.poolThreshold,2);assert.equal(c.state.poolSelection.length,0);assert.match(c.state.poolStorageWarning,/未能恢复/);
  }
});
test("storage failure remains visible and saving successfully clears the warning",()=>{
  const c=context();c.sessionStorage.setItem=()=>{throw new Error("unavailable");};
  c.savePoolSelection();assert.match(c.state.poolStorageWarning,/未能保存/);
  c.renderPoolResults();assert.match(c.nodes.get("pool-storage-warning").textContent,/未能保存/);
  c.sessionStorage.setItem=(key,value)=>c.storage.set(key,value);c.savePoolSelection();assert.equal(c.state.poolStorageWarning,"");
});
test("diagnostic switches survive session restore and old sessions default to false",()=>{
  const c=context(JSON.stringify({schema_version:1,threshold:2,pools:[],deduplicate_content:true,exclude_flat_scores:true}));c.loadPoolSelection();
  assert.equal(c.state.poolDeduplicate,true);assert.equal(c.state.poolExcludeFlat,true);c.savePoolSelection();
  const saved=JSON.parse(c.storage.get("dlt-pool-selection"));assert.equal(saved.deduplicate_content,true);assert.equal(saved.exclude_flat_scores,true);
  const old=context(JSON.stringify({schema_version:1,threshold:2,pools:[]}));old.loadPoolSelection();assert.equal(old.state.poolDeduplicate,false);assert.equal(old.state.poolExcludeFlat,false);
});
test("pool rendering escapes source text and shows pending hits as unknown",()=>{
  const c=context();c.state.poolSelection=[P.pool({id:"a",label:'<img src=x onerror="alert(1)">',front:[1,2],back:[1],issue:"26029"})];
  c.renderPoolResults();const selected=c.nodes.get("selected-pools").innerHTML,result=c.nodes.get("pool-results").innerHTML;
  assert.ok(selected.includes("&lt;img"));assert.ok(!selected.includes("<img"));assert.ok(result.includes("命中 未知"));assert.ok(result.includes("1个池来自其他期号"));
  assert.equal(c.nodes.get("export-pools-json").disabled,false);
  c.state.poolSelection=[];c.renderPoolResults();assert.equal(c.nodes.get("export-pools-json").disabled,true);
});
test("hit gap keeps pending and unknown money distinct from a confirmed zero",()=>{
  const c=context();
  const pending={issue:"26032",status:"pending",joint_gap:null,jackpot_tickets:null,patterns:null,front_outside_pool:null,front_scatter:null,back_outside_pool:null,back_scatter:null,floating_prize_fen:null,fixed_prize_fen:null,floating_covers_cost:null,fixed_assisted_recovery:null,plan_id:"p",outside_explains_entire_gap:null};
  const review={settled_periods:1,pending_periods:1,jackpot_periods:0,union_covered_without_jackpot_periods:1,front_outside_periods:1,front_scatter_periods:0,back_outside_periods:0,back_scatter_periods:1,mean_joint_gap:1,floating_prize_fen:0,fixed_prize_fen:500,floating_recovered_periods:0,fixed_assisted_recovered_periods:1,recovery_unknown_periods:0,max_drawdown_fen:0,drawdown_status:"complete",longest_loss_run:0,loss_run_status:"complete",note:"只读取原票结算",trace:[
    {issue:"26030",status:"drawn",plan_id:"drawn",patterns:[{front_hits:0,back_hits:2,ticket_count:1,level:7,examples:[{front:[6,7,8,9,10],back:[1,2]}]}],outside_explains_entire_gap:true,front_outside_pool:5,front_scatter:0,back_outside_pool:0,back_scatter:0,jackpot_tickets:0,floating_prize_fen:0,fixed_prize_fen:500,floating_covers_cost:false,fixed_assisted_recovery:true},
    pending]};
  const html=c.hitGapPanel({strategies:[{label:'<b>固定</b>',hit_review:review}]});
  assert.ok(html.includes("&lt;b&gt;固定"));assert.ok(!html.includes("<b>固定"));
  assert.ok(html.includes("固定奖补足 1 期"));assert.ok(html.includes("已确认浮动奖回本 0 期"));
  assert.ok(html.includes("待开奖"));assert.ok(html.includes("差额都在池外"));
  const unknown=c.hitGapPanel({strategies:[{label:"浮动",hit_review:{...review,settled_periods:1,floating_prize_fen:null,fixed_prize_fen:null,recovery_unknown_periods:1,max_drawdown_fen:null,drawdown_status:"missing_awards",longest_loss_run:null,loss_run_status:"no_priced_periods",trace:[pending]}}]});
  assert.ok(unknown.includes("待核实"));assert.ok(!unknown.includes("¥0"));assert.ok(unknown.includes("金额未知 1 期"));
  const absent=c.hitGapPanel({strategies:[{label:"旧报告"}]});
  assert.ok(absent.includes("没有单注差额"));assert.ok(!absent.includes("¥0"));
  assert.equal(c.hitGapPanel({strategies:[]}),"");
});
test("live review rendering supports an empty archive and unknown money",()=>{
  const c=context();c.state.reviewWindow=30;c.state.liveReview={summary:{archived_periods:0,archived_records:0,settled_periods:0,pending_periods:0,settled_cost_fen:0,pending_cost_fen:0,net_fen:null,unpriced_periods:0},periods:[],comparisons:[],strategies:[],period_count:0,content_hash:"abc",note:"local archive"};
  const html=c.liveReviewPanel();assert.ok(html.includes("尚无整期推荐留档"));assert.ok(html.includes("暂无可比的共同期"));assert.ok(html.includes("待核实"));
});
const deferred=()=>{let resolve,reject;const promise=new Promise((a,b)=>{resolve=a;reject=b;});return {promise,resolve,reject};};
test("switching A to B to A ignores the oldest response for the same issue",async()=>{
  const c=context(),requests=[];c.api=()=>{const request=deferred();requests.push(request);return request.promise;};
  const draw={issue:"26030",date:"2026-03-01",front:[1,2,3,4,5],back:[1,2]};
  const first=c.loadPoolDraw();c.state.poolComparisonIssue="26031";const second=c.loadPoolDraw();c.state.poolComparisonIssue="26030";const third=c.loadPoolDraw();
  requests[2].resolve({draws:[draw]});await third;assert.equal(c.state.poolDrawStatus,"drawn");
  requests[0].resolve({draws:[]});requests[1].reject(new Error("old failure"));await Promise.all([first,second]);
  assert.equal(c.state.poolDraw.issue,"26030");assert.equal(c.state.poolDrawStatus,"drawn");
});
test("failed draw reads show failure and never reuse the previous hit marks",async()=>{
  const c=context();c.api=async()=>{throw new Error("unavailable");};
  c.state.poolDraw={issue:"26029",front:[1,2,3,4,5],back:[1,2]};
  await c.loadPoolDraw();assert.equal(c.state.poolDraw,null);assert.equal(c.state.poolDrawStatus,"failed");
  assert.match(c.nodes.get("pool-draw-status").textContent,/读取失败/);
});
test("source failure clears stale choices while retaining current selections",async()=>{
  const c=context();const p=P.pool({id:"a",front:[1],back:[1]});c.state.poolSelection=[p];c.state.poolChoices=[p];c.state.poolBrowseFilter="26031";
  const request=deferred();c.api=()=>request.promise;const pending=c.loadPoolChoices();
  assert.equal(c.state.poolChoices.length,0);assert.equal(c.nodes.get("pool-select-page").disabled,true);
  request.reject(new Error("not reachable"));await pending;
  assert.match(c.nodes.get("pool-choices").innerHTML,/26031.*读取失败/);assert.equal(c.state.poolSelection.length,1);
});
test("auxiliary review failure leaves the original ticket content available",async()=>{
  const c=context();c.nodes.set("content",{innerHTML:"original archived tickets"});c.api=async()=>{throw new Error("report unavailable");};
  await c.loadLiveReview();assert.equal(c.nodes.get("content").innerHTML,"original archived tickets");
  assert.equal(c.state.liveReview,null);assert.match(c.nodes.get("review-evidence").innerHTML,/原始票组仍可查看/);
});
