const {test}=require("node:test"),assert=require("node:assert/strict"),fs=require("node:fs"),vm=require("node:vm");
function context(){
  const nodes=new Map(),c={state:{tab:"generator",plan:{id:"one"}},$:id=>nodes.get(id),action:async(_,work)=>work(),accessibleTables:()=>{},escapeHTML:v=>String(v??"").replaceAll("<","&lt;").replaceAll(">","&gt;"),fmtNumbers:n=>n.join(" ")};
  vm.createContext(c);vm.runInContext(fs.readFileSync(require("node:path").join(__dirname,"../web/evidence-ui.js"),"utf8"),c);return Object.assign(c,{nodes});
}
const deferred=()=>{let resolve,reject;const promise=new Promise((a,b)=>{resolve=a;reject=b;});return {promise,resolve,reject};};
test("legacy missing evidence stays unknown and renders escaped server text",()=>{
  const c=context(),html=c.renderSelectionEvidence({status:"not_recorded",note:"<script>旧档案未记录评分</script>"});
  assert.match(html,/未记录评分/);assert.ok(!html.includes("<script>"));assert.ok(!html.includes("全部同分"));
});
test("late preview evidence or errors cannot overwrite a newer generation",async()=>{
  const c=context(),target={innerHTML:"new preview",hidden:true};c.nodes.set("preview-evidence",target);let request=deferred();c.api=()=>request.promise;
  const old=c.showPreviewEvidence({id:"one",request:{}},{});c.state.plan={id:"two"};request.resolve({status:"not_recorded",note:"old evidence"});await old;
  assert.equal(target.innerHTML,"new preview");assert.equal(target.hidden,true);
  c.state.plan={id:"one"};request=deferred();const failed=c.showPreviewEvidence({id:"one",request:{}},{});c.state.tab="records";request.reject(new Error("stale failure"));await failed;
  assert.equal(target.innerHTML,"new preview");
});
test("structure audit is escaped and omitted when the server did not send it",()=>{
  const c=context();
  const zone={score_status:"recorded_scores",all_scores_equal:false,score_kind:"frequency_desc",candidate_basis:"manual_pool",boundary_tie:null,numbers:[]};
  const html=c.renderSelectionEvidence({status:"preview_only",content_hash:"abc",evidence:{note:"依据",data_cutoff:null,scored_periods:3,plan_id:"p",front:zone,back:zone},structure_audit:{note:"<img src=x>",bankers:{front:{basis:"numeric_prefix",numbers:[1,2]},back:{basis:"shuffle_prefix",numbers:[3]}},comparison:{status:"compared",chosen_mass:"4",best_mass:"9",best_split_count:2,chosen_is_best:false,generated_ticket_count:10,tickets_eligible_for_best_set:null}}});
  assert.ok(!html.includes("<img"));assert.match(html,/最小号码/);assert.match(html,/低于这个最高/);assert.match(html,/原方案连乘质量 4/);
  const absent=c.renderSelectionEvidence({status:"not_recorded",note:"旧档案"});
  assert.ok(!absent.includes("组票目标"));
  const unknown=c.renderSelectionEvidence({status:"not_recorded",note:"旧",structure_audit:{note:"质量未知",bankers:{front:{basis:"score_prefix",numbers:[35]},back:{basis:"none",numbers:[]}},comparison:{status:"unknown_scores",chosen_mass:null,best_mass:null,chosen_is_best:null}}});
  assert.match(unknown,/不把质量记为 0/);assert.ok(!unknown.includes("¥0"));assert.doesNotMatch(unknown,/连乘质量 null|连乘质量 0/);
});
test("archive evidence arriving after navigation leaves the new card untouched",async()=>{
  const c=context(),oldTarget={innerHTML:"old"},newTarget={innerHTML:"current"},request=deferred();c.nodes.set("selection-one",oldTarget);c.api=()=>request.promise;
  const button={getAttribute:()=>"false",setAttribute:()=>{throw new Error("must not toggle stale card");}};
  const work=c.showArchivedEvidence("one",button);c.nodes.set("selection-one",newTarget);request.resolve({status:"not_recorded"});await work;
  assert.equal(newTarget.innerHTML,"current");assert.equal(oldTarget.innerHTML,"old");
});
