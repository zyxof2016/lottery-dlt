const {test}=require("node:test"),assert=require("node:assert/strict"),W=require("../web/workflow.js");
const request={issue:"26120",mode:"single",strategy:"random",bets:5,front_pool:7,back_pool:3,front_banker_count:0,back_banker_count:0,window:60,multiplier:1,additional:false,front_numbers:[],back_numbers:[],front_bankers:[],back_bankers:[]};
test("draft serialisation retains inputs but strips tokens previews and arbitrary keys",()=>{
  const raw={schema_version:1,request:{...request,admin_token:"never-store",tickets:["not-an-archive"]},label:"原输入",deadline:"2026-10-26T18:00",token:"never-store",plan:{id:"unverified"}};
  const d=W.parseDraft(JSON.stringify(raw));assert.deepEqual(d,{schema_version:1,request,label:"原输入",deadline:"2026-10-26T18:00"});assert.ok(!JSON.stringify(d).includes("never-store"));
  d.request.front_numbers.push(1);assert.deepEqual(raw.request.front_numbers,[]);
});
test("corrupt oversized or unsafe drafts cannot reach the form",()=>{
  for(const raw of ["{"," ".repeat(16001),JSON.stringify({schema_version:2,request}),JSON.stringify({schema_version:1,request:{...request,mode:"unknown"}}),JSON.stringify({schema_version:1,request:{...request,bets:"1000"}}),JSON.stringify({schema_version:1,request,deadline:"javascript:bad"}),JSON.stringify({schema_version:1,request,label:"a".repeat(81)})])assert.throws(()=>W.parseDraft(raw));
  assert.equal(W.draft({schema_version:1,request:{...request,issue:"",bets:0}}).request.bets,0,"Partial numeric input stays visible for correction");
});
test("generation readiness distinguishes access history experiments and already drawn targets",()=>{
  const empty={draw_count:0,admin_token_required:true};assert.equal(W.gate(request,empty,"").href,"#settings");assert.equal(W.gate(request,empty,"local-token").ready,true);
  assert.equal(W.gate({...request,strategy:"hot"},empty,"local-token").href,"#draws");assert.equal(W.gate({...request,issue:"2612"},empty,"local-token").ready,false);
  assert.equal(W.gate(request,{...empty,draw_count:1,latest_draw:{issue:"26120"}},"local-token").ready,false);
});
test("schedule shortcut only uses a recent verified future deadline within the save window",()=>{
  const now=Date.parse("2026-10-26T06:00:00Z"),s={issue:"26120",deadline:"2026-10-26T12:00:00Z",verified_at:"2026-10-26T05:59:00Z"};
  for(const changed of [{verified_at:"2026-10-26T03:59:00Z"},{verified_at:"2026-10-26T06:00:01Z"},{deadline:"2026-10-25T12:00:00Z"},{deadline:"2026-12-26T12:00:00Z"},{issue:"bad"}])assert.equal(W.activeSchedule([{...s,...changed}],now),null);
  assert.equal(W.activeSchedule([{...s,issue:"26121",deadline:"2026-10-27T12:00:00Z"},s],now).issue,"26120");
});
test("optional snapshot failures retain the core and never invent schedules or operation defaults",async()=>{
  const calls=[],api=async key=>{calls.push(key);if(["schedules","operations"].includes(key))throw new Error(key+" unavailable");return {value:key};};
  const r=await W.loadSnapshot(api);assert.deepEqual(r.overview,{value:"overview"});assert.deepEqual(r.schedules,[]);assert.equal(r.operations,null);assert.equal(r.warnings.operations,"operations unavailable");assert.equal(calls.length,4);
  await assert.rejects(W.loadSnapshot(async key=>{if(key==="overview")throw new Error("required summary missing");return {}; }),/required summary missing/);
});
