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
const output=path.resolve(option("--output",path.join(root,"test-results/replacement-review")));
async function main() {
  const scratch=await fs.mkdtemp(path.join(os.tmpdir(),"dlt-replacements-browser-")),token="isolated-browser-test-token-123456";
  const child=spawn(binary,["--db",path.join(scratch,"test.sqlite3"),"serve","--bind","127.0.0.1:0","--base-path","/dlt"],
    {env:{...process.env,LOTTERY_ADMIN_TOKEN:token,LOTTERY_REQUIRE_SCHEDULE:"0",TOKIO_WORKER_THREADS:"2"},stdio:["ignore","pipe","pipe"]});
  let browser,stderr="";child.stderr.on("data",data=>{stderr+=data.toString();});
  try {
    const url=await new Promise((resolve,reject)=>{let out="";const timer=setTimeout(()=>reject(new Error(`Service start timed out: ${stderr}`)),45000);
      child.on("error",e=>{clearTimeout(timer);reject(e);});child.on("exit",code=>{clearTimeout(timer);reject(new Error(`Service exited ${code}: ${stderr}`));});
      child.stdout.on("data",data=>{out+=data.toString();const m=out.match(/http:\/\/127\.0\.0\.1:\d+\/dlt\//);if(m){clearTimeout(timer);resolve(m[0]);}});
    });
    const api=async(endpoint,body)=>{const response=await fetch(url+"api/"+endpoint,{method:body?"POST":"GET",headers:{"Content-Type":"application/json",Authorization:`Bearer ${token}`},body:body?JSON.stringify(body):undefined});const value=await response.json();assert.ok(response.ok,JSON.stringify(value));return value;};
    const year=String(new Date().getFullYear()%100).padStart(2,"0"),issues=[`${year}118`,`${year}119`,`${year}120`],deadline=new Date(Date.now()+3600000).toISOString();
    const empty=await api("recommendations/live-review");assert.equal(empty.summary.net_fen,null);assert.equal(empty.summary.settled_periods,0);assert.deepEqual(empty.replacement_review.comparisons,[]);
    const batches=[];for(const issue of issues)batches.push(await api("recommendations/archive",{issue,deadline}));
    const pending=await api("recommendations/live-review");assert.equal(pending.summary.net_fen,null);assert.equal(pending.replacement_review.comparisons[0].common_periods,0);
    const sourcePair=issue=>pending.periods.find(p=>p.issue===issue).source_comparisons.find(p=>p.same_budget);
    const first=sourcePair(issues[0]);assert.ok(first.back.removed.length>=2);const from=batches[0].records.find(r=>r.id===first.from_plan_id);
    const date=new Intl.DateTimeFormat("en-CA",{timeZone:"Asia/Shanghai",year:"numeric",month:"2-digit",day:"2-digit"}).format(new Date(deadline));
    const draws=[{issue:issues[0],date,front:from.plan.tickets[0].front,back:first.back.removed.slice(0,2),pool_before_fen:0,source:"synthetic-replacement-browser-test"}];
    await api("draws/import",draws);const unpriced=await api("recommendations/live-review");const group=unpriced.replacement_review.comparisons[0];
    assert.equal(group.common_periods,1);assert.equal(group.pending_periods,2);assert.equal(group.same_zone_sizes,false);assert.equal(group.net_delta_fen,null);
    const negative=group.pairs.find(p=>p.issue===issues[0]);assert.equal(negative.back.hit_delta,-2);assert.equal(negative.back.removed_hits.length,2);
    for(const zone of ["front","back"]){assert.equal(group[zone].net_hit_delta,group[zone].added_hit_count-group[zone].removed_hit_count);assert.equal(group[zone].better_periods+group[zone].tied_periods+group[zone].worse_periods,1);}
    const original=await api("export");
    browser=await chromium.launch({headless:true,...(option("--browser","")?{executablePath:option("--browser","")}:{}),args:["--no-sandbox","--disable-dev-shm-usage"]});
    const context=await browser.newContext({viewport:{width:1440,height:1000},locale:"zh-CN",timezoneId:"Asia/Shanghai",acceptDownloads:true});await context.addInitScript(token=>sessionStorage.setItem("dlt-admin-token",token),token);
    const page=await context.newPage(),errors=[];page.on("pageerror",e=>errors.push(e.message));await fs.mkdir(output,{recursive:true});
    const capture=async name=>{await page.waitForLoadState("networkidle");await page.screenshot({path:path.join(output,name+".png")});await fs.writeFile(path.join(output,name+".txt"),await page.locator("body").ariaSnapshot());};
    await page.goto(url+"reviews#replacement-review",{waitUntil:"networkidle"});await page.locator("#replacement-review").waitFor();assert.equal(await page.locator("#source-period").inputValue(),issues[0]);
    assert.match(await page.locator("#replacement-review").textContent(),/池大小不同/);assert.match(await page.locator("#replacement-review").textContent(),/待核实/);assert.equal(await page.locator("#source-current tbody tr").count(),10);
    await page.locator("#replacement-review").evaluate(e=>e.scrollIntoView());await capture("01-same-budget-desktop");
    await page.locator("#source-period").selectOption(issues[0]);await page.locator("#replacement-current details summary").first().click();assert.match(await page.locator("#replacement-current").textContent(),/净变化 -2/);
    assert.equal(await page.locator(`#replacement-current a[href$="record-${negative.from_plan_id}"]`).count(),1);
    await page.setViewportSize({width:375,height:812});await page.locator("#replacement-current").evaluate(e=>e.scrollIntoView());await capture("02-negative-swap-mobile");assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
    assert.match(await page.locator("#source-current").textContent(),/不可计算/);assert.match(await page.locator("#source-current").textContent(),/未评分/);
    await page.locator("#source-overlap").evaluate(e=>e.scrollIntoView());await capture("03-overlap-mobile");assert.equal(await page.locator("#source-overlap .table-wrap").getAttribute("tabindex"),"0");assert.ok(await page.locator("#source-overlap .table-wrap").evaluate(e=>e.scrollWidth>e.clientWidth));
    const download=page.waitForEvent("download");await page.locator("#export-replacements").click();const file=path.join(scratch,"replacements.json");await(await download).saveAs(file);const exported=JSON.parse(await fs.readFile(file,"utf8"));assert.equal(exported.content_hash,unpriced.content_hash);assert.deepEqual(exported.replacement_review,unpriced.replacement_review);assert.equal(exported.periods.length,3);
    const afterRead=await api("export");for(const key of ["records","recommendations","settings","draws","selection_evidence","audit_entries_verified"])assert.deepEqual(afterRead[key],original[key]);
    // Only the explicit award enrichment changes the test DB. Pool geometry stays frozen.
    await api("draws/import",draws.map(d=>({...d,awards:{1:{basic_fen:1000000000},2:{basic_fen:10000000}}})));const priced=await api("recommendations/live-review");assert.equal(priced.replacement_review.comparisons[0].priced_common_periods,1);assert.ok(Number.isInteger(priced.replacement_review.comparisons[0].net_delta_fen));
    const geometry=r=>r.periods.map(p=>({issue:p.issue,pairs:p.source_comparisons.map(pair=>[pair.from_plan_id,pair.to_plan_id,...["front","back"].map(zone=>{const z=pair[zone];return {added:z.added,removed:z.removed,common:z.common,union:z.union,jaccard:z.jaccard,correlation:z.selection_indicator_correlation};})])}));assert.deepEqual(geometry(priced),geometry(unpriced));
    const beforeRead=await api("export");for(const record of original.records){const latest=beforeRead.records.find(r=>r.id===record.id);for(const key of ["id","plan","created_at","deadline","schedule","label","kind"])assert.deepEqual(latest[key],record[key]);}
    await page.locator("#refresh-records").click();await page.waitForFunction(hash=>state.liveReview?.content_hash===hash,priced.content_hash);assert.equal(await page.locator("#source-period").inputValue(),issues[0]);
    await page.setViewportSize({width:1440,height:1000});await page.locator("#source-overlap").evaluate(e=>e.scrollIntoView());await capture("04-overlap-desktop");
    await page.locator("#source-period").selectOption(issues[2]);assert.match(await page.locator("#replacement-current").textContent(),/待开奖/);assert.match(await page.locator("#source-current").textContent(),/待开奖，可查看原池重叠/);
    await page.locator("#review-window").fill("1");await page.locator("#review-window-form button").click();await page.waitForFunction(()=>state.liveReview?.period_count===1);assert.equal(await page.evaluate(()=>state.liveReview.summary.net_fen),null);assert.match(await page.locator("#replacement-review").textContent(),/暂无已开奖/);
    await page.locator("#replacement-review").evaluate(e=>e.scrollIntoView());await capture("05-pending-window-desktop");
    const after=await api("export");for(const key of ["records","recommendations","settings","draws","selection_evidence","audit_entries_verified"])assert.deepEqual(after[key],beforeRead[key]);
    assert.deepEqual(errors,[]);const health=await(await fetch(url+"healthz")).json();const result={application_version:health.version,empty_and_pending_profit_unknown:true,common_settled_periods:1,pending_periods:2,actual_source_pairs_per_issue:10,budget_mismatches_excluded:true,pool_size_differences_visible:true,lost_hits_subtracted:true,negative_back_delta:-2,per_zone_gain_identity:true,undefined_full_zone_correlation:true,period_switching:true,full_json_export:true,awards_enrichment_preserves_geometry:true,original_archives_unchanged:true,read_only_diagnostics:true,mobile_scrollable_table:true,horizontal_overflow:false,page_errors:errors};
    await fs.writeFile(path.join(output,"verification.json"),JSON.stringify(result,null,2)+"\n");console.log(JSON.stringify(result,null,2));
  } finally {
    if(browser)await browser.close();
    if(child.exitCode===null && child.signalCode===null){const stopped=once(child,"exit");child.kill("SIGINT");await stopped;}
    await fs.rm(scratch,{recursive:true,force:true});
  }
}
main().catch(error=>{console.error(error);process.exitCode=1;});
