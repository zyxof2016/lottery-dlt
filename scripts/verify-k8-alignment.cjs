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
const output=path.resolve(option("--output",path.join(root,"test-results/k8-alignment")));
async function main() {
  const scratch=await fs.mkdtemp(path.join(os.tmpdir(),"dlt-k8-browser-")),token="isolated-browser-test-token-123456";
  const child=spawn(binary,["--db",path.join(scratch,"test.sqlite3"),"serve","--bind","127.0.0.1:0","--base-path","/dlt"],
    {env:{...process.env,LOTTERY_ADMIN_TOKEN:token,LOTTERY_REQUIRE_SCHEDULE:"0",TOKIO_WORKER_THREADS:"2"},stdio:["ignore","pipe","pipe"]});
  let browser,stderr="";child.stderr.on("data",data=>{stderr+=data.toString();});
  try {
    const url=await new Promise((resolve,reject)=>{let out="";const timer=setTimeout(()=>reject(new Error(`Service start timed out: ${stderr}`)),45000);
      child.on("error",e=>{clearTimeout(timer);reject(e);});child.on("exit",code=>{clearTimeout(timer);reject(new Error(`Service exited ${code}: ${stderr}`));});
      child.stdout.on("data",data=>{out+=data.toString();const m=out.match(/http:\/\/127\.0\.0\.1:\d+\/dlt\//);if(m){clearTimeout(timer);resolve(m[0]);}});
    });
    const api=async(endpoint,body)=>{const response=await fetch(url+"api/"+endpoint,{method:body?"POST":"GET",headers:{"Content-Type":"application/json",Authorization:`Bearer ${token}`},body:body?JSON.stringify(body):undefined});const value=await response.json();assert.ok(response.ok,JSON.stringify(value));return value;};
    const year=new Date().getFullYear()%100,issue=`${year}118`,next=`${year}119`,deadline=new Date(Date.now()+3600000).toISOString();
    const preview=await api("plans/generate",{issue,strategy:"coverage",bets:7});
    await api("records",{request:preview.request,preview_id:preview.id,deadline,label:"隔离浏览器测试个人方案"});
    const archived=await api("recommendations/archive",{issue,deadline});await api("recommendations/archive",{issue:next,deadline});
    const date=new Intl.DateTimeFormat("en-CA",{timeZone:"Asia/Shanghai",year:"numeric",month:"2-digit",day:"2-digit"}).format(new Date(deadline));
    const draw={issue,date,...archived.records[2].plan.tickets[0],pool_before_fen:0,source:"synthetic-isolated-browser-test"};
    await api("draws/import",[draw]);
    assert.equal((await api("recommendations/live-review")).periods[1].status,"prize_pending");
    await api("draws/import",[{...draw,awards:{1:{basic_fen:1000000000},2:{basic_fen:10000000}}}]);
    const before=await api("export");
    browser=await chromium.launch({headless:true,...(option("--browser","")?{executablePath:option("--browser","")}:{}),args:["--no-sandbox","--disable-dev-shm-usage"]});
    const context=await browser.newContext({viewport:{width:1440,height:1000},locale:"zh-CN",timezoneId:"Asia/Shanghai",acceptDownloads:true});
    await context.addInitScript(token=>sessionStorage.setItem("dlt-admin-token",token),token);
    const page=await context.newPage(),errors=[];page.on("pageerror",e=>errors.push(e.message));
    await page.goto(url+"reviews",{waitUntil:"networkidle"});
    await page.locator("#pool-choices [data-pool-choice]").first().waitFor();
    assert.equal(await page.locator("h1").textContent(),"每期推荐与复盘");
    assert.match(await page.locator("#live-review").textContent(),/共同期同预算对照/);
    const selectCount=async n=>{await page.waitForFunction(n=>document.querySelector("#selected-pools h3")?.textContent===`已选 ${n} 个池`,n);};
    await page.locator("#budget-increments").waitFor();
    assert.equal(await page.locator("#budget-increments > .table-wrap tbody tr").count(),10);
    await page.locator("#pool-source-search").fill("均衡");
    assert.equal(await page.locator("[data-pool-choice]").count(),2);
    await page.locator("#pool-select-page").click();await selectCount(2);
    await page.locator("#pool-clear").click();await selectCount(0);
    await page.locator("#pool-source-search").fill("");
    await page.locator("#pool-select-page").click();await selectCount(10);
    await page.locator("[data-pool-choice]").first().uncheck();await selectCount(9);
    await page.locator("#pool-clear").click();await selectCount(0);
    await page.locator("[data-pool-choice]").first().check();await selectCount(1);
    await page.locator("#pool-browse-filter").fill(issue);await page.locator("#pool-browse-form button").first().click();
    await page.waitForFunction(()=>document.querySelectorAll("[data-pool-choice]").length===5);
    await page.locator("[data-pool-choice]").first().check();await selectCount(2);
    await page.locator("#pool-comparison-issue").fill(issue);await page.locator("#pool-comparison-form button").click();
    await page.waitForFunction(()=>document.querySelector("#pool-results")?.textContent.includes("1个池来自其他期号"));
    await page.locator("#pool-browse-source").selectOption("personal");await page.locator("#pool-browse-form button").first().click();
    await page.waitForFunction(()=>document.querySelectorAll("[data-pool-choice]").length===1);
    await page.locator("[data-pool-choice]").first().check();await selectCount(3);
    await page.locator("#manual-pool-form").locator("..").locator("summary").click();
    await page.locator("#manual-pool-label").fill('=SUM(1,2)<script>');
    await page.locator("#manual-pool-front").fill("01、02 03 04 05 05");await page.locator("#manual-pool-back").fill("01，02");
    await page.locator("#manual-pool-form button").click();await selectCount(4);
    await page.locator("#manual-pool-front").fill("36");await page.locator("#manual-pool-form button").click();
    await page.waitForFunction(()=>document.querySelector("#toast")?.textContent.includes("1～35"));await selectCount(4);
    await page.locator("#pool-threshold").fill("3");await page.locator("#pool-threshold").dispatchEvent("change");
    const jsonDownload=page.waitForEvent("download");await page.locator("#export-pools-json").click();
    const jsonFile=path.join(scratch,"pools.json");await(await jsonDownload).saveAs(jsonFile);const exported=JSON.parse(await fs.readFile(jsonFile,"utf8"));
    assert.equal(exported.pool_count,4);assert.equal(exported.minimum_count,3);assert.equal(exported.draw_comparison.issue,issue);assert.equal(exported.draw_comparison.status,"drawn");
    assert.ok(exported.pools.some(p=>p.source==="manual"));assert.ok(exported.zones.front.rows.some(r=>r.count<3));
    assert.equal(exported.schema_version,3);assert.equal(exported.zones.front.thresholds.length,4);
    assert.ok(exported.zones.front.thresholds.every(t=>Array.isArray(t.removed_from_reference)));
    await context.grantPermissions(["clipboard-read","clipboard-write"],{origin:new URL(url).origin});
    await page.locator("#copy-pool-common").click();
    const copied=await page.evaluate(()=>navigator.clipboard.readText());assert.ok(copied.includes("前区：") && copied.includes("后区："));
    await page.evaluate(()=>{navigator.clipboard.writeText=async()=>{throw new Error("isolated clipboard denial");};});
    await page.locator("#copy-pool-common").click();await page.waitForFunction(()=>document.querySelector("#toast")?.textContent.includes("isolated clipboard denial"));
    await page.evaluate(()=>{delete navigator.clipboard.writeText;});
    const csvDownload=page.waitForEvent("download");await page.locator("#export-pools-csv").click();
    const csvFile=path.join(scratch,"pools.csv");await(await csvDownload).saveAs(csvFile);
    const csvText=await fs.readFile(csvFile,"utf8");assert.ok(csvText.includes("=SUM"));
    // A manual label may occur after another source in a cell; only a leading
    // formula operator needs the text prefix. Random overlaps must not make this
    // check depend on which source happens to appear first for a number.
    for(const match of csvText.matchAll(/"((?:[^"]|"")*)"(?:,|\r?\n|$)/g))assert.ok(!/^\s*[=+\-@]/.test(match[1].replace(/""/g,'"')));
    await page.reload({waitUntil:"networkidle"});await selectCount(4);assert.equal(await page.locator("#pool-threshold").inputValue(),"3");
    let failReview=true;
    await page.route(/\/api\/recommendations\/live-review\?/,async route=>{
      if(failReview){failReview=false;await route.fulfill({status:503,contentType:"application/json",body:JSON.stringify({error:"isolated review failure"})});}
      else await route.continue();
    });
    await page.reload({waitUntil:"networkidle"});
    assert.equal(await page.locator("[data-expand]").count(),10);
    assert.match(await page.locator("#live-review").textContent(),/读取失败/);
    await page.locator("#retry-live-review").click();await page.locator("#budget-increments").waitFor();
    let failSource=true;
    await page.route(/\/api\/recommendations\?page=/,async route=>{
      if(failSource){failSource=false;await route.fulfill({status:503,contentType:"application/json",body:JSON.stringify({error:"isolated source failure"})});}
      else await route.continue();
    });
    await page.locator("#pool-browse-form button").first().click();
    await page.waitForFunction(()=>document.querySelector("#pool-choices")?.textContent.includes("读取失败"));
    assert.equal(await page.locator("[data-pool-choice]").count(),0);assert.equal(await page.locator("#pool-select-page").isDisabled(),true);await selectCount(4);
    await page.locator("#pool-browse-form button").first().click();await page.locator("[data-pool-choice]").first().waitFor();
    await fs.mkdir(output,{recursive:true});
    await page.locator("#live-review").screenshot({path:path.join(output,"review-desktop.png")});
    await page.locator("#budget-increments").screenshot({path:path.join(output,"budget-desktop.png")});
    const headings={generator:"为下一期准备一组号码",records:"让每一组号码都有记录",reviews:"每期推荐与复盘",draws:"每一期开奖，有据可查",backtest:"把概率与历史分开看",research:"对照研究，保留证据",settings:"可复现，也可核验"};
    for(const width of [1440,375]) {
      await page.setViewportSize({width,height:900});
      for(const tab of ["generator","records","reviews","draws","backtest","research","settings"]) {
        await page.locator(`[data-tab="${tab}"]`).click();
        await page.waitForFunction(tab=>document.querySelector(`nav [data-tab="${tab}"]`)?.getAttribute("aria-current")==="page",tab);
        await page.waitForFunction(heading=>document.querySelector("h1")?.textContent===heading,headings[tab]);
        if(tab==="reviews")await page.locator("#budget-increments").waitFor();
        await page.waitForLoadState("networkidle");
        const overflow=await page.evaluate(()=>({width:innerWidth,scroll:document.documentElement.scrollWidth,elements:[...document.querySelectorAll("main *")].filter(e=>e.getBoundingClientRect().right>innerWidth+1 && getComputedStyle(e).position!=="absolute").slice(0,12).map(e=>({tag:e.tagName,id:e.id,class:e.className,right:e.getBoundingClientRect().right}))}));
        assert.ok(overflow.scroll<=width,`${tab} overflow at ${width}: ${JSON.stringify(overflow)}`);
      }
    }
    await page.goto(url+"reviews#pool-intersection",{waitUntil:"networkidle"});await selectCount(4);
    await page.locator("#pool-intersection").screenshot({path:path.join(output,"pools-mobile.png")});
    const budgetTable=page.locator("#budget-increments > .table-wrap");
    assert.equal(await budgetTable.getAttribute("tabindex"),"0");
    await budgetTable.focus();await page.keyboard.press("ArrowRight");
    await page.waitForFunction(()=>document.querySelector("#budget-increments > .table-wrap")?.scrollLeft>0);
    await page.evaluate(()=>{const original=Storage.prototype.setItem;Storage.prototype.setItem=function(key,value){if(key==="dlt-pool-selection")throw new Error("storage unavailable");return original.call(this,key,value);};});
    await page.locator("[data-remove-pool]").first().click();await selectCount(3);
    assert.match(await page.locator("#pool-storage-warning").textContent(),/未能保存/);
    const after=await api("export");for(const key of ["draws","records","recommendations","settings","audit_entries_verified","selection_evidence"])assert.deepEqual(after[key],before[key]);
    assert.deepEqual(errors,[]);
    const health=await(await fetch(url+"healthz")).json();
    const result={application_version:health.version,desktop_tabs:7,mobile_tabs:7,page_errors:errors,horizontal_overflow:false,real_archive_pools:10,restored_selection_count:4,
      cross_issue_and_source:true,unknown_prizes_then_enrichment:true,invalid_manual_pool_rejected:true,complete_json_csv_exports:true,storage_failure_visible:true,read_only_data_unchanged:true,
      budget_increment_rows:10,threshold_loss_diagnostics:true,filtered_bulk_selection:true,clipboard_copy:true,clipboard_failure_feedback:true,keyboard_table_scroll:true,independent_review_failure_recovery:true,source_failure_keeps_selection:true};
    await fs.writeFile(path.join(output,"verification.json"),JSON.stringify(result,null,2)+"\n");console.log(JSON.stringify(result,null,2));
  } finally {
    if(browser)await browser.close();
    if(child.exitCode===null && child.signalCode===null){const stopped=once(child,"exit");child.kill("SIGINT");await stopped;}
    await fs.rm(scratch,{recursive:true,force:true});
  }
}
main().catch(error=>{console.error(error);process.exitCode=1;});
