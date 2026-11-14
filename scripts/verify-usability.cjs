"use strict";
// All writes are synthetic fixtures in a disposable loopback database.
const assert=require("node:assert/strict"),fs=require("node:fs/promises"),path=require("node:path"),os=require("node:os");
const {spawn}=require("node:child_process"),{once}=require("node:events");
const {chromium}=require(process.env.PLAYWRIGHT_MODULE||"playwright");
const args=process.argv.slice(2),option=(name,fallback)=>{const i=args.indexOf(name);return i<0?fallback:args[i+1];};
const root=path.resolve(__dirname,".."),output=path.resolve(option("--output",path.join(root,"test-results/usability")));
async function main(){
  const scratch=await fs.mkdtemp(path.join(os.tmpdir(),"dlt-usability-")),token="isolated-usability-test-token-123456";
  const child=spawn(path.resolve(option("--binary",path.join(root,"target/debug/lottery-dlt"))),["--db",path.join(scratch,"test.sqlite3"),"serve","--bind","127.0.0.1:0","--base-path","/dlt"],{env:{...process.env,LOTTERY_ADMIN_TOKEN:token,LOTTERY_REQUIRE_SCHEDULE:"0",TOKIO_WORKER_THREADS:"2"},stdio:["ignore","pipe","pipe"]});
  let browser,stderr="";child.stderr.on("data",data=>stderr+=data);
  try{
    const url=await new Promise((resolve,reject)=>{let text="";const timer=setTimeout(()=>reject(new Error("Startup timed out")),45000);child.on("error",reject);child.on("exit",code=>{clearTimeout(timer);reject(new Error(`Service exited ${code}: ${stderr}`));});child.stdout.on("data",data=>{text+=data;const match=text.match(/http:\/\/127\.0\.0\.1:\d+\/dlt\//);if(match){clearTimeout(timer);resolve(match[0]);}});});
    const api=async(endpoint,body)=>{const r=await fetch(url+"api/"+endpoint,{method:body?"POST":"GET",headers:{"Content-Type":"application/json",Authorization:`Bearer ${token}`},body:body?JSON.stringify(body):undefined});const value=await r.json();assert.ok(r.ok,JSON.stringify(value));return value;};
    const year=String(new Date().getFullYear()%100).padStart(2,"0"),settled=`${year}118`,pending=`${year}119`,target=`${year}120`,deadline=new Date(Date.now()+3600000).toISOString();
    const save=async(issue,bets,label)=>{const p=await api("plans/generate",{issue,bets});return api("records",{request:p.request,preview_id:p.id,deadline,label});};
    const personal=await save(settled,3,"已开奖核对方案");await save(pending,2,"等待开奖的方案");
    await api("recommendations/archive",{issue:settled,deadline});await api("recommendations/archive",{issue:pending,deadline});
    const date=new Intl.DateTimeFormat("en-CA",{timeZone:"Asia/Shanghai",year:"numeric",month:"2-digit",day:"2-digit"}).format(new Date(deadline));
    await api("draws/import",[{issue:settled,date,...personal.plan.tickets[0],pool_before_fen:0,awards:{1:{basic_fen:1000000000},2:{basic_fen:10000000}},source:"synthetic-usability-test"}]);
    const original=await api("export");
    browser=await chromium.launch({executablePath:option("--browser",undefined),headless:true,args:["--no-sandbox","--disable-dev-shm-usage"]});
    const context=await browser.newContext({viewport:{width:1440,height:1000},locale:"zh-CN",timezoneId:"Asia/Shanghai"});
    await context.addInitScript(token=>sessionStorage.setItem("dlt-admin-token",token),token);
    const page=await context.newPage(),errors=[];let generationRequests=0;page.on("pageerror",error=>errors.push(error.message));page.on("request",request=>{if(request.url().endsWith("/api/plans/generate"))generationRequests++;});await fs.mkdir(output,{recursive:true});
    const capture=async(name)=>{await page.waitForLoadState("networkidle");await fs.writeFile(path.join(output,name+".txt"),await page.locator("body").ariaSnapshot());await page.screenshot({path:path.join(output,name+".png")});};
    const nav=async(tab,heading)=>{await page.locator(`[data-tab="${tab}"]`).click();await page.waitForFunction(heading=>document.querySelector("h1")?.textContent===heading,heading);await page.waitForLoadState("networkidle");};
    await page.goto(url,{waitUntil:"networkidle"});await capture("01-generator-desktop");
    await page.locator("#issue").fill(target);await page.locator('[data-preset="4"]').click();const firstGeneration=page.waitForResponse(r=>r.url().endsWith("/api/plans/generate"));await page.locator("#generate").click();const generated=await(await firstGeneration).json();await page.locator("#save-plan").waitFor();
    await page.setViewportSize({width:375,height:812});await page.locator("#plan-preview").evaluate(e=>e.scrollIntoView({block:"start"}));await capture("02-preview-mobile");
    const baseline={visible_disabled_fields:await page.locator("#plan-form input:disabled:visible,#plan-form select:disabled:visible").count(),save_button_top:await page.locator("#save-plan").evaluate(e=>e.getBoundingClientRect().top),mobile_viewport_height:812};
    await page.setViewportSize({width:1440,height:1000});await nav("records","让每一组号码都有记录");await capture("03-records-desktop");
    await page.setViewportSize({width:375,height:812});await nav("reviews","每期推荐与复盘");await capture("04-reviews-mobile");
    if(args.includes("--capture-only")){await fs.writeFile(path.join(output,"baseline.json"),JSON.stringify(baseline,null,2)+"\n");console.log(JSON.stringify(baseline));return;}
    assert.ok(baseline.save_button_top>=0&&baseline.save_button_top<760,"Saving must be reachable before the long ticket list");
    assert.equal(baseline.visible_disabled_fields,0);
    assert.equal(await page.locator("#manual-archive").getAttribute("open"),null);
    await nav("generator","为下一期准备一组号码");
    const deadlineLocal=await page.evaluate(value=>{const d=new Date(value);return new Date(d.getTime()-d.getTimezoneOffset()*60000).toISOString().slice(0,16);},deadline);
    await page.locator("#save-deadline").fill(deadlineLocal);await page.locator("#plan-label").fill("手机保存验收");
    await page.locator("#save-plan").click();await page.locator("#saved-confirmation").waitFor();await capture("05-saved-mobile");
    const saved=(await api("records")).records.find(r=>r.label==="手机保存验收");assert.ok(saved);assert.equal(saved.id,generated.id);assert.deepEqual(saved.plan.tickets,generated.tickets);
    await page.locator("#dismiss-toast").click();
    await page.locator("#view-saved-plan").click();await page.locator(`#detail-${saved.id}.open`).waitFor();
    assert.equal(await page.locator(`#detail-${saved.id} .ticket-row`).count(),20);assert.equal(await page.locator(`[data-expand="${saved.id}"]`).getAttribute("aria-expanded"),"true");await capture("06-saved-record-mobile");
    await nav("reviews","每期推荐与复盘");assert.equal(await page.locator("[data-expand]").count(),10);assert.equal(await page.locator("#record-filter").inputValue(),"");
    await nav("records","让每一组号码都有记录");assert.equal(await page.locator("#record-filter").inputValue(),target);
    await page.locator("#clear-record-filter").click();await page.waitForFunction(()=>document.querySelectorAll("[data-expand]").length===3);
    await page.locator("#record-status").selectOption("pending");await page.locator("#record-filter-form button[type=submit]").click();await page.waitForFunction(()=>document.querySelectorAll("[data-expand]").length===2);
    await page.locator("#record-status").selectOption("settled");await page.locator("#record-filter-form button[type=submit]").click();await page.waitForFunction(()=>document.querySelectorAll("[data-expand]").length===1);
    assert.match(await page.locator("#personal-records").textContent(),/已开奖核对方案/);
    await page.locator("#record-filter").fill(target);await page.locator("#record-filter-form button[type=submit]").click();await page.waitForFunction(()=>document.querySelector("#personal-records")?.textContent.includes("未找到匹配方案"));
    assert.equal(await page.locator("[data-expand]").count(),0);
    await page.locator("#clear-record-filter").click();await page.waitForFunction(()=>document.querySelectorAll("[data-expand]").length===3);
    const afterSave=await api("export");for(const record of original.records)assert.deepEqual(afterSave.records.find(r=>r.id===record.id),record);
    await nav("generator","为下一期准备一组号码");assert.equal(await page.locator("#plan-form input:disabled:visible,#plan-form select:disabled:visible").count(),0);
    await page.locator("#mode").selectOption("compound");await page.locator("#front_pool").fill("35");await page.locator("#back_pool").fill("12");
    assert.equal(await page.locator("#generate").isDisabled(),true);assert.match(await page.locator("#plan-estimate").textContent(),/1000注上限/);assert.equal(generationRequests,1);
    await page.locator("#mode").selectOption("single");await page.locator("#bets").fill("25");await page.locator("#issue").fill(target);
    assert.match(await page.locator("#plan-estimate").textContent(),/25注.*50\.00/);
    const secondGeneration=page.waitForResponse(r=>r.url().endsWith("/api/plans/generate"));await page.locator("#generate").click();const full=await(await secondGeneration).json();await page.locator("#expand-preview").waitFor();
    assert.equal(await page.locator("#preview-ticket-list .ticket-row").count(),20);
    await page.locator("#save-deadline").fill(deadlineLocal);await page.locator("#plan-label").fill("展开时保留输入");await page.locator("#expand-preview").click();
    assert.equal(await page.locator("#preview-ticket-list .ticket-row").count(),25);assert.equal(await page.locator("#plan-label").inputValue(),"展开时保留输入");assert.equal(await page.locator("#save-deadline").inputValue(),deadlineLocal);
    await context.grantPermissions(["clipboard-read","clipboard-write"],{origin:new URL(url).origin});await page.locator("#copy-plan").click();assert.equal(await page.evaluate(()=>navigator.clipboard.readText()),full.print_text);
    const toastBox=await page.locator("#toast").boundingBox();assert.ok(toastBox.y>=0&&toastBox.y+toastBox.height<=812);
    await page.locator("#save-deadline").fill("2026-01-01T00:00");await page.locator("#save-plan").click();await page.waitForFunction(()=>document.querySelector("#toast")?.getAttribute("role")==="alert");
    assert.equal(await page.locator("#plan-label").inputValue(),"展开时保留输入");await page.keyboard.press("Escape");assert.equal(await page.locator("#toast").isVisible(),false);
    const headings={generator:"为下一期准备一组号码",records:"让每一组号码都有记录",reviews:"每期推荐与复盘",draws:"每一期开奖，有据可查",backtest:"把概率与历史分开看",research:"对照研究，保留证据",settings:"可复现，也可核验"};
    for(const width of [1440,375]){await page.setViewportSize({width,height:width===375?812:1000});for(const [tab,heading] of Object.entries(headings)){await nav(tab,heading);assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),`${tab} overflow at ${width}`);if(width===375){const rects=await page.locator("#navigation button").evaluateAll(elements=>elements.map(e=>({x:e.getBoundingClientRect().x,right:e.getBoundingClientRect().right,height:e.getBoundingClientRect().height})));assert.ok(rects.every(r=>r.x>=0&&r.right<=375&&r.height>=44));}}}
    const final=await api("export");for(const key of ["records","draws","settings","recommendations","audit_entries_verified","selection_evidence"])assert.deepEqual(final[key],afterSave[key]);
    assert.deepEqual(errors,[]);const health=await(await fetch(url+"healthz")).json();
    const result={application_version:health.version,desktop_tabs:7,mobile_tabs:7,visible_disabled_fields:0,save_button_top:baseline.save_button_top,saved_original_tickets:20,direct_saved_record_link:true,status_filters:true,filtered_empty_state:true,over_limit_blocked_before_request:true,large_preview_expansion_preserves_inputs:true,full_copy_tickets:25,visible_error_toast_and_escape:true,mobile_navigation_all_visible:true,page_errors:errors,existing_archives_unchanged:true};
    await fs.writeFile(path.join(output,"verification.json"),JSON.stringify(result,null,2)+"\n");console.log(JSON.stringify(result,null,2));
  }finally{if(browser)await browser.close();if(child.exitCode===null && child.signalCode===null){const stopped=once(child,"exit");child.kill("SIGINT");await stopped;}await fs.rm(scratch,{recursive:true,force:true});}
}
main().catch(error=>{console.error(error);process.exitCode=1;});
