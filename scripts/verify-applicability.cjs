"use strict";
// All writes are synthetic fixtures in a disposable loopback database.
const assert=require("node:assert/strict"),fs=require("node:fs/promises"),path=require("node:path"),os=require("node:os");
const {spawn}=require("node:child_process"),{once}=require("node:events");
const {chromium}=require(process.env.PLAYWRIGHT_MODULE||"playwright");
const args=process.argv.slice(2),option=(name,fallback)=>{const i=args.indexOf(name);return i<0?fallback:args[i+1];};
const root=path.resolve(__dirname,".."),output=path.resolve(option("--output",path.join(root,"test-results/applicability")));
async function main(){
  const scratch=await fs.mkdtemp(path.join(os.tmpdir(),"dlt-applicability-")),token="isolated-usability-test-token-123456";
  const child=spawn(path.resolve(option("--binary",path.join(root,"target/debug/lottery-dlt"))),["--db",path.join(scratch,"test.sqlite3"),"serve","--bind","127.0.0.1:0","--base-path","/dlt"],{env:{...process.env,LOTTERY_ADMIN_TOKEN:token,LOTTERY_REQUIRE_SCHEDULE:"0",TOKIO_WORKER_THREADS:"2"},stdio:["ignore","pipe","pipe"]});
  let browser,stderr="";child.stderr.on("data",data=>stderr+=data);
  try{
    const url=await new Promise((resolve,reject)=>{let text="";const timer=setTimeout(()=>reject(new Error("Startup timed out")),45000);child.on("error",reject);child.on("exit",code=>{clearTimeout(timer);reject(new Error(`Service exited ${code}: ${stderr}`));});child.stdout.on("data",data=>{text+=data;const match=text.match(/http:\/\/127\.0\.0\.1:\d+\/dlt\//);if(match){clearTimeout(timer);resolve(match[0]);}});});
    const api=async(endpoint,body)=>{const r=await fetch(url+"api/"+endpoint,{method:body?"POST":"GET",headers:{"Content-Type":"application/json",Authorization:`Bearer ${token}`},body:body?JSON.stringify(body):undefined});const value=await r.json();assert.ok(r.ok,JSON.stringify(value));return value;};
    const target=String(new Date().getFullYear()%100).padStart(2,"0")+"120",deadline=new Date(Date.now()+3600000).toISOString();
    browser=await chromium.launch({executablePath:option("--browser",undefined),headless:true,args:["--no-sandbox","--disable-dev-shm-usage"]});
    const context=await browser.newContext({viewport:{width:1440,height:1000},locale:"zh-CN",timezoneId:"Asia/Shanghai"}),page=await context.newPage(),errors=[];page.on("pageerror",e=>errors.push(e.message));await fs.mkdir(output,{recursive:true});
    const capture=async name=>{await page.waitForLoadState("networkidle");await page.screenshot({path:path.join(output,name+".png")});await fs.writeFile(path.join(output,name+".txt"),await page.locator("body").ariaSnapshot());};
    const nav=async tab=>{await page.locator(`[data-tab="${tab}"]`).click();await page.waitForLoadState("networkidle");};
    await page.goto(url,{waitUntil:"networkidle"});await capture("01-first-use-desktop");
    const firstUseGuide=await page.locator("#usage-readiness").count()>0;
    if(!args.includes("--capture-only")){assert.equal(await page.locator("#generate").isDisabled(),true);assert.match(await page.locator("#usage-readiness").textContent(),/验证管理员访问/);}
    await nav("settings");await page.locator("#admin-token").fill(token);await page.locator("#token-form button").first().click();await page.waitForFunction(()=>sessionStorage.getItem("dlt-admin-token"));await nav("generator");
    await page.locator("#issue").fill(target);await page.locator("#bets").fill("25");
    const generatedResponse=page.waitForResponse(r=>r.url().endsWith("/api/plans/generate"));await page.locator("#generate").click();const generated=await(await generatedResponse).json();await page.locator("#save-deadline").waitFor();
    const local=await page.evaluate(value=>{const d=new Date(value);return new Date(d.getTime()-d.getTimezoneOffset()*60000).toISOString().slice(0,16);},deadline);
    await page.locator("#save-deadline").fill(local);await page.locator("#plan-label").fill("待确认的本期草稿");if(await page.locator("#dismiss-toast").isVisible())await page.locator("#dismiss-toast").click();
    await page.setViewportSize({width:375,height:812});await page.locator("#plan-preview").evaluate(e=>e.scrollIntoView());await capture("02-filled-draft-mobile");
    await nav("records");await nav("generator");await page.locator("#plan-preview").evaluate(e=>e.scrollIntoView());await capture("03-returned-draft-mobile");
    const draftPreserved=(await page.locator("#plan-label").inputValue())==="待确认的本期草稿"&&(await page.locator("#save-deadline").inputValue())===local;
    await page.route("**/api/operations",r=>r.fulfill({status:503,contentType:"application/json",body:JSON.stringify({error:"synthetic operations unavailable"})}));await page.reload({waitUntil:"networkidle"});await capture("04-optional-failure-mobile");
    const optionalFailureIsolated=await page.locator("#plan-form").count()>0;
    if(args.includes("--capture-only")){const result={first_use_guide:firstUseGuide,draft_preserved_on_navigation:draftPreserved,optional_failure_isolated:optionalFailureIsolated,page_errors:errors};await fs.writeFile(path.join(output,"baseline.json"),JSON.stringify(result,null,2)+"\n");console.log(JSON.stringify(result,null,2));return;}
    assert.equal(firstUseGuide,true);assert.equal(draftPreserved,true);assert.equal(optionalFailureIsolated,true);
    assert.equal(await page.locator("#issue").inputValue(),target);assert.equal(await page.locator("#bets").inputValue(),"25");assert.equal(await page.locator("#save-plan").count(),0,"Reload restores inputs, never an old preview");
    assert.match(await page.locator("#auxiliary-status").textContent(),/重试/);assert.match(await page.locator("#draft-status").textContent(),/重新生成/);
    await page.locator("#generate").click();await page.locator("#save-plan").waitFor();assert.equal(await page.locator("#save-plan").isDisabled(),true);assert.match(await page.locator("#save-blocked").textContent(),/重试/);
    await page.unroute("**/api/operations");await page.locator("#retry-auxiliary").click();await page.waitForFunction(()=>document.querySelector("#auxiliary-status").hidden);
    const regeneratedResponse=page.waitForResponse(r=>r.url().endsWith("/api/plans/generate"));await page.locator("#generate").click();const regenerated=await(await regeneratedResponse).json();assert.equal(regenerated.id,generated.id);
    assert.equal(await page.locator("#plan-label").inputValue(),"待确认的本期草稿");assert.equal(await page.locator("#save-deadline").inputValue(),local);
    const before=await api("export");assert.equal(before.records.length,0,"Draft and retry must never save a plan");
    // Saving succeeds independently of the subsequent summary refresh.
    let failSummary=false;await page.route("**/api/overview",r=>failSummary?r.fulfill({status:503,contentType:"application/json",body:JSON.stringify({error:"synthetic summary unavailable"})}):r.continue());
    failSummary=true;await page.locator("#save-plan").click();await page.locator("#saved-confirmation").waitFor();await page.waitForFunction(()=>document.querySelector("#toast")?.textContent.includes("保存已成功"));
    await capture("05-save-success-refresh-failed-mobile");const saved=await api("records");assert.equal(saved.records.length,1);assert.equal(saved.records[0].id,generated.id);assert.deepEqual(saved.records[0].plan.tickets,generated.tickets);
    assert.equal(await page.locator("#save-plan").count(),0);await page.unroute("**/api/overview");
    await page.locator("#view-saved-plan").click();await page.locator(`#detail-${generated.id}.open`).waitFor();assert.equal(await page.locator(`#detail-${generated.id} .ticket-row`).count(),25);
    await capture("06-original-record-mobile");await nav("generator");
    // A record-page failure provides recovery without losing its filter.
    await nav("records");await page.locator("#record-filter").fill(target);await page.locator("#record-filter-form button[type=submit]").click();await page.waitForLoadState("networkidle");
    await page.route("**/api/records?**",r=>r.fulfill({status:503,contentType:"application/json",body:JSON.stringify({error:"synthetic record read unavailable"})}));await page.locator("#refresh-records").click();await page.locator("#retry-page").waitFor();
    await page.unroute("**/api/records?**");await page.locator("#retry-page").click();await page.locator("#record-filter").waitFor();assert.equal(await page.locator("#record-filter").inputValue(),target);assert.equal(await page.locator("[data-expand]").count(),1);
    await nav("generator");await page.locator("#issue").fill(target);await page.locator("#bets").fill("7");await page.locator("#clear-generator-draft").click();assert.equal(await page.locator("#bets").inputValue(),"5");assert.equal(await page.locator("#save-plan").count(),0);
    await page.reload({waitUntil:"networkidle"});assert.equal(await page.locator("#bets").inputValue(),"5");assert.equal((await api("records")).records.length,1);
    // A late response to an earlier save must not erase a newer draft.
    await page.locator("#issue").fill(target);await page.locator("#bets").fill("25");await page.locator("#generate").click();await page.locator("#save-plan").waitFor();await page.locator("#save-deadline").fill(local);
    let releaseSave,requestStarted;const delayed=new Promise(resolve=>releaseSave=resolve),started=new Promise(resolve=>requestStarted=resolve);
    await page.route("**/api/records",async route=>{if(route.request().method()!=="POST")return route.continue();const response=await route.fetch();requestStarted();await delayed;await route.fulfill({response});});
    await page.locator("#save-plan").click();await started;await page.locator("#bets").fill("7");await page.locator("#generate").click();await page.locator("#plan-label").fill("应保留的新草稿");releaseSave();await page.waitForLoadState("networkidle");
    assert.equal(await page.locator("#plan-label").inputValue(),"应保留的新草稿");assert.equal(await page.locator("#saved-confirmation").count(),0);assert.equal(await page.evaluate(()=>JSON.parse(sessionStorage.getItem(draftStorageKey())).label),"应保留的新草稿");await page.unroute("**/api/records");assert.equal((await api("records")).records.length,1);
    // Strict mode needs a freshly verified schedule before saving.
    const operations=await api("operations");await page.route("**/api/operations",r=>r.fulfill({status:200,contentType:"application/json",body:JSON.stringify({...operations,strict_schedule:true})}));
    await page.reload({waitUntil:"networkidle"});await page.locator("#issue").fill(target);await page.locator("#generate").click();await page.locator("#save-plan").waitFor();assert.equal(await page.locator("#save-plan").isDisabled(),true);assert.match(await page.locator("#save-blocked").textContent(),/官方日程/);
    const schedule={issue:target,deadline,verified_at:new Date().toISOString(),basis:"explicit_official_schedule"};await page.route("**/api/schedules",r=>r.fulfill({status:200,contentType:"application/json",body:JSON.stringify({schedules:[schedule]})}));
    await page.reload({waitUntil:"networkidle"});await page.locator("#use-verified-issue").click();await page.locator("#generate").click();await page.locator("#save-plan").waitFor();assert.equal(await page.locator("#save-plan").isDisabled(),false);assert.equal(await page.locator("#save-deadline").getAttribute("readonly"),"");assert.equal(await page.locator("#save-deadline").inputValue(),local);
    await page.unroute("**/api/operations");await page.unroute("**/api/schedules");await page.reload({waitUntil:"networkidle"});await page.locator("#clear-generator-draft").click();
    // Corrupt or unwritable session storage cannot be mistaken for an archive.
    await page.evaluate(()=>sessionStorage.setItem(draftStorageKey(),"{broken"));await page.reload({waitUntil:"networkidle"});assert.match(await page.locator("#draft-status").textContent(),/未能恢复/);assert.equal(await page.locator("#save-plan").count(),0);
    await page.locator("#clear-generator-draft").click();await page.evaluate(()=>{const original=Storage.prototype.setItem;Storage.prototype.setItem=function(key,value){if(key.startsWith("dlt-generator-draft:"))throw new Error("synthetic storage denied");return original.call(this,key,value);};});await page.locator("#bets").fill("7");assert.match(await page.locator("#draft-status").textContent(),/暂未保存/);await page.reload({waitUntil:"networkidle"});
    assert.equal((await api("records")).records.length,1);
    for(const width of [1440,375]){await page.setViewportSize({width,height:width===375?812:1000});for(const tab of ["generator","records","reviews","draws","backtest","research","settings"]){await nav(tab);assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),`${tab} overflow at ${width}`);}}
    assert.deepEqual(errors,[]);const health=await(await fetch(url+"healthz")).json();
    const result={application_version:health.version,first_use_guide:true,draft_preserved_on_navigation:true,refresh_restores_inputs_without_preview:true,same_request_regenerates_same_id:true,optional_failure_isolated:true,auxiliary_retry:true,successful_save_survives_refresh_failure:true,unknown_status_blocks_save:true,strict_schedule_required_before_save:true,verified_schedule_shortcut:true,corrupt_or_unwritable_draft_feedback:true,late_save_preserves_newer_draft:true,one_original_25_ticket_archive:true,record_page_retry_preserves_filter:true,clear_draft_keeps_archive:true,desktop_tabs:7,mobile_tabs:7,horizontal_overflow:false,page_errors:errors};await fs.writeFile(path.join(output,"verification.json"),JSON.stringify(result,null,2)+"\n");console.log(JSON.stringify(result,null,2));
  }finally{if(browser)await browser.close();if(child.exitCode===null && child.signalCode===null){const stopped=once(child,"exit");child.kill("SIGINT");await stopped;}await fs.rm(scratch,{recursive:true,force:true});}
}
main().catch(error=>{console.error(error);process.exitCode=1;});
