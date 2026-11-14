const {test}=require("node:test"),assert=require("node:assert/strict"),fs=require("node:fs"),vm=require("node:vm");
const W=require("../web/workflow.js");
const source=fs.readFileSync(require.resolve("../web/transport.js"),"utf8");
const response=(value,status=200)=>({ok:status>=200&&status<300,status,text:async()=>JSON.stringify(value)});
const deferred=()=>{let resolve,reject;const promise=new Promise((a,b)=>{resolve=a;reject=b;});return {promise,resolve,reject};};
function client(fetch){const c={fetch,AbortController,setTimeout,clearTimeout};vm.createContext(c);vm.runInContext(source,c);return c.DltTransport;}
test("hung reads abort within a deadline without replay and a later retry succeeds",async()=>{
  let count=0,signal;const pending=deferred(),t=client(async(url,options)=>{count++;signal=options.signal;return count===1?pending.promise:response({ok:true});});
  await assert.rejects(t.request("/dlt/api/records",{},10),/读取超时/);assert.equal(signal.aborted,true);assert.equal(count,1);
  pending.reject(new Error("late socket failure"));assert.equal((await t.request("/dlt/api/records",{},100)).ok,true);assert.equal(count,2);
});
test("the read deadline also covers a response whose headers arrived but body stalled",async()=>{
  const body=deferred();let signal;const t=client(async(url,options)=>{signal=options.signal;return {ok:true,status:200,text:()=>body.promise};});
  await assert.rejects(t.request("/dlt/api/recommendations",{},10),/读取超时/);assert.equal(signal.aborted,true);
  body.resolve('{"late":true}');
});
test("successful reads clear their timer and preserve prefix and access headers",async()=>{
  let captured;const t=client(async(url,options)=>{captured={url,options};return response({seed:"18446744073709551615"});});
  const r=await t.request("/dlt/api/config",{headers:{Authorization:"Bearer test-only"}},10);assert.equal(r.seed,"18446744073709551615");
  await new Promise(resolve=>setTimeout(resolve,20));assert.equal(captured.options.signal.aborted,false);assert.equal(captured.url,"/dlt/api/config");assert.equal(captured.options.headers.Authorization,"Bearer test-only");
});
test("JSON errors and proxy HTML fail explicitly without being treated as data",async()=>{
  const t=client(async url=>url==="http"?response({error:"服务忙碌"},503):url==="html-error"?{ok:false,status:502,text:async()=>"<html>proxy failure</html>"}:url==="html-ok"?{ok:true,status:200,text:async()=>"<html>proxy login</html>"}:response(null));
  await assert.rejects(t.request("http"),/服务忙碌/);await assert.rejects(t.request("html-error"),/HTTP 502/);await assert.rejects(t.request("html-ok"),/不是有效的 JSON/);await assert.rejects(t.request("null"),/响应格式无效/);
});
test("writes have no read deadline or automatic replay and preserve an uncertain result",async()=>{
  const pending=deferred();let calls=0,options;const t=client(async(url,init)=>{calls++;options=init;return pending.promise;});
  const write=t.request("/dlt/api/records",{method:"POST",body:'{"original":true}'},5);
  await new Promise(resolve=>setTimeout(resolve,20));assert.equal(calls,1);assert.equal(options.signal,undefined);assert.equal(options.body,'{"original":true}');
  pending.resolve(response({id:"original"}));assert.equal((await write).id,"original");
  const broken=client(async()=>{throw new Error("socket closed");});await assert.rejects(broken.request("/dlt/api/config",{method:"PUT"}),/操作结果尚未确认/);
  const proxy=client(async()=>({ok:false,status:502,text:async()=>"<html>proxy failure</html>"}));await assert.rejects(proxy.request("/dlt/api/records",{method:"POST"}),/HTTP 502.*操作结果尚未确认/);
});
test("required failures surface immediately even while auxiliary requests are pending",async()=>{
  const pending=deferred();const core=W.loadSnapshot(async key=>{if(key==="overview")throw new Error("required unavailable");return key==="config"?{}:pending.promise;});
  await assert.rejects(core,/required unavailable/);pending.resolve({schedules:[]});
});
test("optional read timeouts retain core data and cannot later replace unknown status",async()=>{
  const pending=deferred(),t=client(async url=>["operations","schedules"].includes(url)?pending.promise:response({key:url}));
  const snapshot=await W.loadSnapshot(key=>t.request(key,{},15));
  assert.equal(snapshot.overview.key,"overview");assert.equal(snapshot.config.key,"config");assert.deepEqual(snapshot.schedules,[]);assert.equal(snapshot.operations,null);assert.match(snapshot.warnings.operations,/超时/);assert.match(snapshot.warnings.schedules,/超时/);
  pending.resolve(response({strict_schedule:false,schedules:[{issue:"26120"}]}));await Promise.resolve();assert.equal(snapshot.operations,null);assert.deepEqual(snapshot.schedules,[]);
});
