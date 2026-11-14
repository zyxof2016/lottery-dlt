"use strict";
// Synthetic failures and archives stay in a disposable loopback database.
const assert=require("node:assert/strict"),fs=require("node:fs/promises"),path=require("node:path"),os=require("node:os");
const {spawn}=require("node:child_process"),{once}=require("node:events");
const {chromium}=require(process.env.PLAYWRIGHT_MODULE||"playwright");
const args=process.argv.slice(2),option=(name,fallback)=>{const i=args.indexOf(name);return i<0?fallback:args[i+1];};
const root=path.resolve(__dirname,".."),output=path.resolve(option("--output",path.join(root,"test-results/read-recovery")));
const deferred=()=>{let resolve;const promise=new Promise(r=>resolve=r);return {promise,resolve};};
async function main(){
  const scratch=await fs.mkdtemp(path.join(os.tmpdir(),"dlt-read-recovery-")),token="isolated-read-recovery-token-12345";
  const child=spawn(path.resolve(option("--binary",path.join(root,"target/debug/lottery-dlt"))),["--db",path.join(scratch,"test.sqlite3"),"serve","--bind","127.0.0.1:0","--base-path","/dlt"],{env:{...process.env,LOTTERY_ADMIN_TOKEN:token,LOTTERY_REQUIRE_SCHEDULE:"0",TOKIO_WORKER_THREADS:"2"},stdio:["ignore","pipe","pipe"]});
  let browser,stderr="";child.stderr.on("data",data=>stderr+=data);
  try{
    const url=await new Promise((resolve,reject)=>{let text="";const timer=setTimeout(()=>reject(new Error("Startup timed out")),45000);child.on("error",reject);child.on("exit",code=>{clearTimeout(timer);reject(new Error(`Service exited ${code}: ${stderr}`));});child.stdout.on("data",data=>{text+=data;const match=text.match(/http:\/\/127\.0\.0\.1:\d+\/dlt\//);if(match){clearTimeout(timer);resolve(match[0]);}});});
    const api=async(endpoint,body,method=body===undefined?"GET":"POST")=>{const r=await fetch(url+"api/"+endpoint,{method,headers:{"Content-Type":"application/json",Authorization:`Bearer ${token}`},body:body===undefined?undefined:JSON.stringify(body)});const value=await r.json();assert.ok(r.ok,JSON.stringify(value));return value;};
    const overview=await api("overview"),health=await(await fetch(url+"healthz")).json();
    browser=await chromium.launch({executablePath:option("--browser",undefined),headless:true,args:["--no-sandbox","--disable-dev-shm-usage"]});
    const context=await browser.newContext({viewport:{width:1440,height:1000},locale:"zh-CN",timezoneId:"Asia/Shanghai"});
    await context.addInitScript(value=>sessionStorage.setItem("dlt-admin-token",value),token);
    const page=await context.newPage(),errors=[];page.on("pageerror",e=>errors.push(e.message));await fs.mkdir(output,{recursive:true});
    const capture=async name=>{await page.screenshot({path:path.join(output,name+".png")});await fs.writeFile(path.join(output,name+".txt"),await page.locator("body").ariaSnapshot());};
    const nav=async tab=>{await page.locator(`[data-tab="${tab}"]`).click();await page.waitForFunction(tab=>state.tab===tab&&document.querySelector("#content h1"),tab);};
    const auxiliary=deferred();let heldAuxiliary;
    await page.route("**/api/operations",route=>{heldAuxiliary=route;auxiliary.resolve();});
    await page.goto(url,{waitUntil:"domcontentloaded"});await auxiliary.promise;
    if(args.includes("--capture-only")){
      await new Promise(resolve=>setTimeout(resolve,8500));
      const result={application_version:health.version,hung_auxiliary_blocks_generator:await page.locator("#plan-form").count()===0,empty_overview_net:overview.settled_net_fen};
      await capture("01-stuck-startup");await heldAuxiliary.continue();await page.locator("#plan-form").waitFor();await page.unroute("**/api/operations");
      await nav("records");await page.locator("#personal-records").waitFor();result.empty_record_summary=await page.locator("#personal-records .stat").last().innerText();
      let count=0;const first=deferred(),release=deferred();await page.route("**/api/overview",async route=>{const order=++count;if(order===1){first.resolve();await release.promise;}await route.fulfill({json:{...overview,note:order===1?"older snapshot":"newer snapshot"}});});
      await page.evaluate(()=>{window.oldRefresh=refresh();});await first.promise;await page.evaluate(()=>refresh());release.resolve();await page.evaluate(()=>window.oldRefresh);
      result.older_refresh_overwrites_newer=await page.evaluate(()=>state.overview.note==="older snapshot");await page.unroute("**/api/overview");
      await fs.writeFile(path.join(output,"baseline.json"),JSON.stringify(result,null,2)+"\n");console.log(JSON.stringify(result,null,2));return;
    }
    assert.equal(overview.settled_prize_fen,null);assert.equal(overview.settled_net_fen,null);
    await page.locator("#plan-form").waitFor({timeout:10000});assert.match(await page.locator("#auxiliary-status").textContent(),/读取超时/);
    const target=String(new Date().getFullYear()%100).padStart(2,"0")+"120",deadline=new Date(Date.now()+3600000).toISOString();
    await page.locator("#issue").fill(target);await page.locator("#generate").click();await page.locator("#save-plan").waitFor();assert.equal(await page.locator("#save-plan").isDisabled(),true);
    // A success delivered after the deadline cannot silently clear the warning.
    await heldAuxiliary.fulfill({json:{strict_schedule:false}}).catch(()=>{});
    assert.equal(await page.locator("#save-plan").isDisabled(),true);assert.equal(await page.evaluate(()=>state.operations),null);
    await page.locator("#auxiliary-status").evaluate(e=>e.scrollIntoView({block:"start"}));await capture("01-auxiliary-timeout-desktop");await page.unroute("**/api/operations");await page.locator("#retry-auxiliary").click();await page.waitForFunction(()=>document.querySelector("#auxiliary-status").hidden);assert.equal(await page.locator("#save-plan").isDisabled(),false);
    await nav("records");await page.locator("#personal-records").waitFor();assert.match(await page.locator("#personal-records .stat").last().innerText(),/待核实.*暂无已开奖/s);
    // Out-of-order refresh success and failure both lose to the latest refresh.
    for(const fail of [false,true]){
      let count=0;const first=deferred(),release=deferred();await page.route("**/api/overview",async route=>{const order=++count;if(order===1){first.resolve();await release.promise;}await route.fulfill(order===1&&fail?{status:503,json:{error:"old read failed"}}:{json:{...overview,note:order===1?"older snapshot":"newer snapshot"}});});
      await page.evaluate(()=>{window.oldRefresh=refresh();});await first.promise;assert.equal(await page.evaluate(()=>refresh()),true);release.resolve();assert.equal(await page.evaluate(()=>window.oldRefresh),false);assert.equal(await page.evaluate(()=>state.overview.note),"newer snapshot");await page.unroute("**/api/overview");
    }
    // A successfully saved seed must survive a previously started refresh.
    await nav("settings");await page.locator("#seed").waitFor();const oldConfig=await api("config"),configStarted=deferred(),releaseConfig=deferred();
    await page.route("**/api/config",async route=>{if(route.request().method()!=="GET")return route.continue();configStarted.resolve();await releaseConfig.promise;await route.fulfill({json:oldConfig});});
    await page.evaluate(()=>{window.oldRefresh=refresh();});await configStarted.promise;await page.locator("#seed").fill("987654");await page.locator("#save-seed").click();await page.waitForFunction(()=>state.config.seed==="987654");releaseConfig.resolve();assert.equal(await page.evaluate(()=>window.oldRefresh),false);assert.equal(await page.evaluate(()=>state.config.seed),"987654");await page.unroute("**/api/config");assert.equal((await api("config")).seed,"987654");
    // Prepare original pending records; subsequent browser reads must not write.
    const preview=await api("plans/generate",{issue:target,mode:"compound",front_pool:5,back_pool:2,front_numbers:[1,2,3,4,5],back_numbers:[1,2]});
    const personal=await api("records",{request:preview.request,preview_id:preview.id,deadline,label:"网络恢复原始方案"});await api("recommendations/archive",{issue:target,deadline});
    const before=await api("export");assert.equal((await api("overview")).settled_net_fen,null);
    await nav("records");await page.locator(`#record-${personal.id}`).waitFor();assert.match(await page.locator("#personal-records .stat").last().innerText(),/待核实/);await page.locator("#record-filter").fill(target);await page.locator("#record-filter-form button[type=submit]").click();await page.waitForLoadState("networkidle");
    // Use the browser clock for long read deadlines; production limits are intact.
    await page.clock.install();const recordStarted=deferred();let heldRecord;
    await page.route("**/api/records?**",route=>{heldRecord=route;recordStarted.resolve();});await page.locator("#refresh-records").click();await recordStarted.promise;await page.clock.fastForward(15001);await page.locator("#retry-page").waitFor();assert.match(await page.locator("#content").textContent(),/读取超时/);assert.match(await page.locator("#toast").textContent(),/读取超时/);assert.doesNotMatch(await page.locator("#toast").textContent(),/已按缓存开奖复算/);
    await page.setViewportSize({width:375,height:812});await capture("02-record-timeout-mobile");await heldRecord.abort().catch(()=>{});await page.unroute("**/api/records?**");await page.locator("#retry-page").click();await page.locator(`#record-${personal.id}`).waitFor();assert.equal(await page.locator("#record-filter").inputValue(),target);
    await page.locator(`[data-expand="${personal.id}"]`).click();assert.equal(await page.locator(`#detail-${personal.id} .ticket-row`).count(),1);
    // Proxy HTML must enter the report retry path and leave originals visible.
    await page.route("**/api/recommendations/live-review?**",route=>route.fulfill({status:200,contentType:"text/html",body:"<html>synthetic proxy response</html>"}));
    await nav("reviews");await page.locator("#retry-live-review").waitFor();assert.match(await page.locator("#live-review").textContent(),/不是有效的 JSON/);assert.equal(await page.locator("#recommendation-records [data-expand]").count(),5);await capture("03-invalid-report-mobile");
    await page.unroute("**/api/recommendations/live-review?**");await page.locator("#retry-live-review").click();await page.locator("#export-live-review").waitFor();
    const reportStarted=deferred();let heldReport;await page.route("**/api/recommendations/live-review?**",route=>{heldReport=route;reportStarted.resolve();});
    await page.locator("#review-window").fill("10");await page.locator("#review-window-form button").click();await reportStarted.promise;await page.clock.fastForward(60001);await page.locator("#retry-live-review").waitFor();assert.match(await page.locator("#live-review").textContent(),/读取超时/);assert.equal(await page.locator("#recommendation-records [data-expand]").count(),5);
    await heldReport.abort().catch(()=>{});await page.unroute("**/api/recommendations/live-review?**");await page.locator("#retry-live-review").click();await page.locator("#export-live-review").waitFor();assert.equal(await page.locator("#review-window").inputValue(),"10");
    // A delayed old page error cannot replace a page reached by navigation.
    const abandoned=deferred(),releaseAbandoned=deferred();await page.route("**/api/records?**",async route=>{abandoned.resolve();await releaseAbandoned.promise;await route.fulfill({status:503,json:{error:"old page unavailable"}});});
    await page.locator('[data-tab="records"]').click();await abandoned.promise;await nav("generator");await page.locator("#plan-form").waitFor();releaseAbandoned.resolve();await page.waitForLoadState("networkidle");assert.equal(await page.locator("#plan-form").count(),1);assert.equal(await page.locator("#retry-page").count(),0);await page.unroute("**/api/records?**");
    // Core JSON failure is reported promptly even if an optional read is held.
    const failed=await context.newPage();failed.on("pageerror",e=>errors.push(e.message));let heldStartup;
    await failed.route("**/api/overview",route=>route.fulfill({status:200,contentType:"text/html",body:"<html>synthetic bad overview</html>"}));await failed.route("**/api/operations",route=>{heldStartup=route;});
    await failed.goto(url,{waitUntil:"domcontentloaded"});await failed.locator("#retry-page").waitFor({timeout:3000});assert.match(await failed.locator("#content").textContent(),/不是有效的 JSON/);
    if(heldStartup)await heldStartup.continue();await failed.unroute("**/api/operations");await failed.unroute("**/api/overview");await failed.locator("#retry-page").click();await failed.locator("#plan-form").waitFor();await failed.close();
    for(const width of [1440,375]){await page.setViewportSize({width,height:width===375?812:1000});for(const tab of ["generator","records","reviews","draws","backtest","research","settings"]){await nav(tab);await page.waitForLoadState("networkidle");assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),`${tab} overflow at ${width}`);}}
    await nav("records");await page.locator("#personal-records").waitFor();if(await page.locator("#dismiss-toast").isVisible())await page.locator("#dismiss-toast").click();await page.locator("#personal-records .stat").last().evaluate(e=>e.scrollIntoView({block:"center"}));await capture("04-pending-summary-mobile");
    const after=await api("export");for(const key of ["records","draws","recommendations","settings","schedules","selection_evidence","audit_entries_verified"])assert.deepEqual(after[key],before[key],`read recovery changed ${key}`);assert.deepEqual(errors,[]);
    const result={application_version:health.version,optional_timeout_keeps_core_available:true,late_auxiliary_success_ignored:true,unknown_status_blocks_save:true,auxiliary_retry:true,empty_and_pending_profit_unknown:true,older_refresh_success_and_failure_ignored:true,saved_seed_survives_old_refresh:true,record_timeout_retry_preserves_filter:true,failed_refresh_never_reports_success:true,report_timeout_keeps_originals:true,invalid_json_has_retry:true,report_retry_preserves_window:true,navigation_ignores_old_failure:true,core_failure_does_not_wait_for_auxiliary:true,read_recovery_preserves_database:true,long_deadlines_tested_with_browser_clock:true,desktop_tabs:7,mobile_tabs:7,horizontal_overflow:false,page_errors:errors};
    await fs.writeFile(path.join(output,"verification.json"),JSON.stringify(result,null,2)+"\n");console.log(JSON.stringify(result,null,2));
  }finally{if(browser)await browser.close();if(child.exitCode===null&&child.signalCode===null){const stopped=once(child,"exit");child.kill("SIGINT");await stopped;}await fs.rm(scratch,{recursive:true,force:true});}
}
main().catch(error=>{console.error(error);process.exitCode=1;});
