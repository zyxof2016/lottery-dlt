"use strict";
// Starts a disposable loopback service. All synthetic writes stay in its new DB.
const assert=require("node:assert/strict");
const fs=require("node:fs/promises");
const os=require("node:os");
const path=require("node:path");
const {spawn}=require("node:child_process");
const {once}=require("node:events");
const {chromium}=require(process.env.PLAYWRIGHT_MODULE||"playwright");
const args=process.argv.slice(2),option=(name,fallback)=>{const i=args.indexOf(name);return i<0?fallback:args[i+1];};
const root=path.resolve(__dirname,".."),binary=path.resolve(option("--binary",path.join(root,"target/debug/lottery-dlt")));
const output=path.resolve(option("--output",path.join(root,"test-results/score-evidence")));
async function main() {
  const scratch=await fs.mkdtemp(path.join(os.tmpdir(),"dlt-evidence-browser-")),token="isolated-browser-test-token-123456";
  const child=spawn(binary,["--db",path.join(scratch,"test.sqlite3"),"serve","--bind","127.0.0.1:0","--base-path","/dlt"],
    {env:{...process.env,LOTTERY_ADMIN_TOKEN:token,LOTTERY_REQUIRE_SCHEDULE:"0",TOKIO_WORKER_THREADS:"2"},stdio:["ignore","pipe","pipe"]});
  let browser,stderr="";child.stderr.on("data",data=>{stderr+=data.toString();});
  try {
    const url=await new Promise((resolve,reject)=>{let out="";const timer=setTimeout(()=>reject(new Error(`Service start timed out: ${stderr}`)),45000);
      child.on("error",e=>{clearTimeout(timer);reject(e);});child.on("exit",code=>{clearTimeout(timer);reject(new Error(`Service exited ${code}: ${stderr}`));});
      child.stdout.on("data",data=>{out+=data.toString();const m=out.match(/http:\/\/127\.0\.0\.1:\d+\/dlt\//);if(m){clearTimeout(timer);resolve(m[0]);}});
    });
    const api=async(endpoint,body)=>{const response=await fetch(url+"api/"+endpoint,{method:body?"POST":"GET",headers:{"Content-Type":"application/json",Authorization:`Bearer ${token}`},body:body?JSON.stringify(body):undefined});const value=await response.json();assert.ok(response.ok,JSON.stringify(value));return value;};
    const year=String(new Date().getFullYear()%100).padStart(2,"0"),issue=`${year}120`,next=`${year}121`,previewIssue=`${year}122`,deadline=new Date(Date.now()+3600000).toISOString();
    const history=Array.from({length:7},(_,i)=>({issue:`${year}${String(i+1).padStart(3,"0")}`,date:`20${year}-01-${String(i+1).padStart(2,"0")}`,front:Array.from({length:5},(_,j)=>i*5+j+1),back:[1,2],pool_before_fen:0,source:"synthetic-score-evidence-test"}));
    await api("draws/import",history);
    const save=async(request,label)=>{const p=await api("plans/generate",request);return api("records",{request:p.request,preview_id:p.id,deadline,label});};
    const request={issue,strategy:"hot",bets:5,front_pool:6,back_pool:3,window:7};
    const hot=await save(request,"确认等分热号"),cold=await save({...request,strategy:"cold"},"确认等分冷号");
    const manual=await save({...request,mode:"compound",front_numbers:hot.plan.front_pool,back_numbers:hot.plan.back_pool,front_pool:hot.plan.front_pool.length,back_pool:hot.plan.back_pool.length},"自选同内容池保留");
    const random=await save({issue,strategy:"random",bets:5},"未使用评分的随机");
    const archived=await api("recommendations/archive",{issue,deadline});await api("recommendations/archive",{issue:next,deadline});
    const originalEvidence=await api(`records/${hot.id}/evidence`);
    assert.equal(originalEvidence.evidence.front.all_scores_equal,true);assert.equal(originalEvidence.evidence.back.all_scores_equal,false);
    assert.deepEqual(originalEvidence.evidence.front.boundary_tie,[6,1,35]);
    browser=await chromium.launch({headless:true,...(option("--browser","")?{executablePath:option("--browser","")}:{}),args:["--no-sandbox","--disable-dev-shm-usage"]});
    const context=await browser.newContext({viewport:{width:1440,height:1000},locale:"zh-CN",timezoneId:"Asia/Shanghai",acceptDownloads:true});
    await context.addInitScript(token=>sessionStorage.setItem("dlt-admin-token",token),token);
    const page=await context.newPage(),errors=[];page.on("pageerror",e=>errors.push(e.message));await fs.mkdir(output,{recursive:true});
    const capture=async name=>{await page.waitForLoadState("networkidle");await page.screenshot({path:path.join(output,name+".png")});await fs.writeFile(path.join(output,name+".txt"),await page.locator("body").ariaSnapshot());};
    const nav=async tab=>{await page.locator(`[data-tab="${tab}"]`).click();await page.waitForLoadState("networkidle");};
    await page.goto(url,{waitUntil:"networkidle"});
    await page.locator("#issue").fill(previewIssue);await page.locator("#strategy").selectOption("hot");await page.locator("#front_pool").fill("6");await page.locator("#back_pool").fill("3");await page.locator("#window").fill("7");
    await page.locator("#generate").click();await page.locator("#explain-preview").click();await page.locator("#preview-evidence .evidence-zone").first().waitFor();
    assert.equal(await page.locator("#preview-evidence tbody tr").count(),47);assert.match(await page.locator("#preview-evidence").textContent(),/全部同分/);
    assert.match(await page.locator("#preview-evidence").textContent(),/第1～35名同分/);await capture("01-preview-evidence-desktop");
    await nav("records");await page.locator(`[data-evidence-id="${hot.id}"]`).click();await page.locator(`#selection-${hot.id} .evidence-zone`).first().waitFor();
    assert.equal(await page.locator(`#selection-${hot.id} tbody tr`).count(),47);assert.match(await page.locator(`#selection-${hot.id}`).textContent(),new RegExp(originalEvidence.content_hash));
    if(await page.locator("#dismiss-toast").isVisible())await page.locator("#dismiss-toast").click();
    await page.setViewportSize({width:375,height:812});await page.locator(`#selection-${hot.id}`).evaluate(e=>e.scrollIntoView());await capture("02-archived-evidence-mobile");
    assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
    await page.locator(`[data-evidence-id="${random.id}"]`).click();await page.locator(`#selection-${random.id} .evidence-zone`).first().waitFor();
    assert.match(await page.locator(`#selection-${random.id}`).textContent(),/未使用历史评分/);
    const date=new Intl.DateTimeFormat("en-CA",{timeZone:"Asia/Shanghai",year:"numeric",month:"2-digit",day:"2-digit"}).format(new Date(deadline));
    const draw={issue,date,...archived.records[2].plan.tickets[0],pool_before_fen:0,source:"synthetic-score-evidence-test"};await api("draws/import",[draw]);
    const missing=await api("recommendations/live-review");assert.ok(missing.strategies.some(s=>s.stability.concentration_status==="missing_awards"));
    await api("draws/import",[{...draw,awards:{1:{basic_fen:1000000000},2:{basic_fen:10000000}}}]);
    const review=await api("recommendations/live-review");assert.ok(review.strategies.every(s=>s.stability.settled_periods===1&&s.stability.pending_periods===1));
    assert.ok(review.strategies.some(s=>s.stability.max_single_prize_share===1));assert.deepEqual((await api(`records/${hot.id}/evidence`)).evidence,originalEvidence.evidence);
    const before=await api("export");assert.equal(before.schema_version,4);
    await nav("reviews");await page.locator("#stability-review").waitFor();assert.match(await page.locator("#stability-review").textContent(),/100\.00%/);
    assert.ok(await page.locator('a[href="#stability-review"]').count());
    await page.locator("#pool-browse-source").selectOption("personal");await page.locator("#pool-browse-form button").first().click();await page.waitForFunction(()=>document.querySelectorAll("[data-pool-choice]").length===4);
    await page.locator("#pool-select-page").click();await page.waitForFunction(()=>document.querySelector("#selected-pools h3")?.textContent==="已选 4 个池");
    const sourceIds=[hot.id,cold.id,manual.id,random.id].map(id=>"record:"+id).sort();
    const originalPools=await page.evaluate(()=>poolReport().pools);
    await page.locator("#pool-deduplicate").check();let report=await page.evaluate(()=>poolReport());assert.equal(report.zones.front.effective_pool_count,3);assert.ok(report.zones.front.same_content_groups.some(g=>g.includes("record:"+hot.id)&&g.includes("record:"+manual.id)));
    await page.locator("#pool-exclude-flat").check();report=await page.evaluate(()=>poolReport());
    assert.deepEqual([...report.zones.front.excluded_flat_pool_ids].sort(),["record:"+hot.id,"record:"+cold.id].sort());assert.deepEqual(report.zones.back.excluded_flat_pool_ids,[]);
    assert.ok(report.zones.front.preserved_manual_pool_ids.includes("record:"+manual.id));assert.ok(report.zones.front.unknown_score_pool_ids.includes("record:"+random.id));
    assert.deepEqual(report.pools,originalPools);assert.deepEqual(report.pools.map(p=>p.id).sort(),sourceIds);
    assert.match(await page.locator("#selected-pools").textContent(),/自选池保留/);
    await page.locator(".pool-view-options").evaluate(e=>e.scrollIntoView());await capture("03-diagnostic-pools-mobile");assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
    await page.setViewportSize({width:1440,height:1000});await capture("04-diagnostic-pools-desktop");
    const pending=page.waitForEvent("download");await page.locator("#export-pools-json").click();const file=path.join(scratch,"pool.json");await(await pending).saveAs(file);assert.deepEqual(JSON.parse(await fs.readFile(file,"utf8")),report);
    await page.reload({waitUntil:"networkidle"});assert.equal(await page.locator("#pool-deduplicate").isChecked(),true);assert.equal(await page.locator("#pool-exclude-flat").isChecked(),true);assert.equal((await page.evaluate(()=>poolReport())).pool_count,4);
    await page.locator("#stability-review").evaluate(e=>e.scrollIntoView());await capture("05-stability-desktop");
    const after=await api("export");for(const key of ["records","draws","settings","recommendations","audit_entries_verified","selection_evidence"])assert.deepEqual(after[key],before[key]);
    assert.deepEqual(errors,[]);const health=await(await fetch(url+"healthz")).json();
    const result={application_version:health.version,preview_and_archive_47_numbers:true,flat_tie_boundary:[6,1,35],unscored_is_not_flat:true,per_zone_flat_exclusion:true,manual_candidate_pool_retained:true,content_deduplication:true,original_sources_unchanged:true,switches_persist:true,diagnostic_json_schema:report.schema_version,backup_json_schema:before.schema_version,missing_awards_concentration_unknown:true,actual_stability_no_pending_zeros:true,read_only_archives_and_evidence:true,horizontal_overflow:false,page_errors:errors};
    await fs.writeFile(path.join(output,"verification.json"),JSON.stringify(result,null,2)+"\n");console.log(JSON.stringify(result,null,2));
  } finally {
    if(browser)await browser.close();
    if(child.exitCode===null && child.signalCode===null){const stopped=once(child,"exit");child.kill("SIGINT");await stopped;}
    await fs.rm(scratch,{recursive:true,force:true});
  }
}
main().catch(error=>{console.error(error);process.exitCode=1;});
