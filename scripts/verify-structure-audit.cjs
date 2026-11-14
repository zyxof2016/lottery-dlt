"use strict";
// Disposable loopback service. Synthetic draws stay in its temporary database.
const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const os = require("node:os");
const path = require("node:path");
const {spawn} = require("node:child_process");
const {once} = require("node:events");
const {chromium} = require(process.env.PLAYWRIGHT_MODULE || "playwright");
const args = process.argv.slice(2);
const option = (name, fallback) => {const i = args.indexOf(name); return i < 0 ? fallback : args[i + 1];};
const root = path.resolve(__dirname, "..");
const binary = path.resolve(option("--binary", path.join(root, "target/release/lottery-dlt")));
const output = path.resolve(option("--output", path.join(root, "test-results/structure-audit-1.9.0")));
const request = (overrides) => ({issue:"26030",mode:"dantuo",strategy:"random",bets:1,front_pool:7,back_pool:3,front_banker_count:2,back_banker_count:1,front_numbers:[],back_numbers:[],front_bankers:[],back_bankers:[],window:60,multiplier:1,additional:false,...overrides});
async function main() {
  const scratch = await fs.mkdtemp(path.join(os.tmpdir(), "dlt-structure-audit-"));
  const token = "isolated-browser-test-token-123456";
  const child = spawn(binary, ["--db", path.join(scratch, "test.sqlite3"), "serve", "--bind", "127.0.0.1:0", "--base-path", "/dlt"],
    {env: {...process.env, LOTTERY_ADMIN_TOKEN: token, LOTTERY_REQUIRE_SCHEDULE: "0", TOKIO_WORKER_THREADS: "2"}, stdio: ["ignore", "pipe", "pipe"]});
  let browser, stderr = "";
  child.stderr.on("data", data => {stderr += data.toString();});
  try {
    const url = await new Promise((resolve, reject) => {
      let out = "";
      const timer = setTimeout(() => reject(new Error(`Service start timed out: ${stderr}`)), 45000);
      child.on("error", error => {clearTimeout(timer); reject(error);});
      child.on("exit", code => {clearTimeout(timer); reject(new Error(`Service exited ${code}: ${stderr}`));});
      child.stdout.on("data", data => {out += data.toString(); const match = out.match(/http:\/\/127\.0\.0\.1:\d+\/dlt\//); if (match) {clearTimeout(timer); resolve(match[0]);}});
    });
    const call = async (endpoint, body, method = body ? "POST" : "GET") => {
      const response = await fetch(url + "api/" + endpoint, {method, headers: {"Content-Type": "application/json", Authorization: `Bearer ${token}`}, body: body ? JSON.stringify(body) : undefined});
      const value = await response.json();
      assert.ok(response.ok, JSON.stringify(value));
      return value;
    };
    const draws = Array.from({length: 8}, (_, index) => ({
      issue: `2602${index + 1}`,
      date: new Date(Date.UTC(2026, 2, index + 1)).toISOString().slice(0, 10),
      front: [31, 32, 33, 34, 35],
      back: [11, 12],
      pool_before_fen: 0,
      source: "synthetic-isolated-browser-test",
    }));
    await call("draws/import", draws);
    const explain = async (overrides) => {
      const plan = await call("plans/generate", request(overrides));
      const evidence = await call("plans/explain", {request: plan.request, preview_id: plan.id});
      return {plan, evidence};
    };
    const random = await explain({});
    assert.equal(random.plan.generator_version, "dlt-plan-v3");
    assert.equal(random.evidence.structure_audit.bankers.front.basis, "shuffle_prefix");
    assert.equal(random.evidence.structure_audit.comparison.status, "analytic_tie");
    assert.equal(random.evidence.structure_audit.comparison.chosen_mass, null);
    assert.equal(random.evidence.structure_audit.comparison.chosen_is_best, true);
    assert.ok(random.evidence.structure_audit.comparison.best_split_count > 1);
    const coverage = await explain({mode:"single", strategy:"coverage", bets:7, front_banker_count:0, back_banker_count:0});
    assert.equal(coverage.evidence.structure_audit.objective, "exposure_balance");
    assert.equal(coverage.evidence.structure_audit.comparison.status, "not_applicable");
    assert.equal(coverage.evidence.structure_audit.comparison.chosen_mass, null);
    const before = await call("export");
    browser = await chromium.launch({headless: true, ...(option("--browser", "") ? {executablePath: option("--browser", "")} : {}), args: ["--no-sandbox", "--disable-dev-shm-usage"]});
    const context = await browser.newContext({viewport: {width: 1440, height: 1000}, locale: "zh-CN", timezoneId: "Asia/Shanghai"});
    await context.addInitScript(token => sessionStorage.setItem("dlt-admin-token", token), token);
    const page = await context.newPage(), errors = [];
    page.on("pageerror", error => errors.push(error.message));
    await page.goto(url, {waitUntil: "networkidle"});
    await page.locator("#issue").fill("26030");
    await page.locator("#mode").selectOption("dantuo");
    await page.locator("#strategy").selectOption("hot");
    await page.locator("#front_pool").fill("7");
    await page.locator("#back_pool").fill("3");
    await page.locator("#front_banker_count").fill("2");
    await page.locator("#back_banker_count").fill("1");
    await page.locator("#custom-pools").evaluate(node => {node.open = true;});
    await page.locator("#front_numbers").fill("1 2 3 4 5 6 35");
    await page.locator("#back_numbers").fill("1 2 12");
    await page.locator("#front_bankers").fill("");
    await page.locator("#back_bankers").fill("");
    assert.match(await page.locator("#custom-pools").textContent(), /自选池取最小号码/);
    const generated = page.waitForResponse(response => response.url().includes("/api/plans/generate") && response.request().method() === "POST");
    await page.locator("#generate").click();
    const plan = await (await generated).json();
    assert.equal(plan.generator_version, "dlt-plan-v3");
    assert.deepEqual(plan.front_bankers, [1, 2]);
    assert.ok(plan.tickets.every(ticket => ticket.front.includes(1) && ticket.front.includes(2)));
    await page.locator("#preview-heading").waitFor();
    const explained = page.waitForResponse(response => response.url().includes("/api/plans/explain"));
    await page.locator("#explain-preview").click();
    const evidence = await (await explained).json();
    const audit = evidence.structure_audit;
    assert.equal(audit.bankers.front.basis, "numeric_prefix");
    assert.equal(audit.comparison.status, "compared");
    assert.equal(audit.comparison.chosen_is_best, false);
    assert.equal(typeof audit.comparison.chosen_mass, "string");
    assert.equal(typeof audit.comparison.best_mass, "string");
    assert.notEqual(audit.comparison.chosen_mass, "0");
    assert.ok(!JSON.stringify(audit).includes("建议号码"));
    await page.locator("#structure-audit").waitFor();
    const panel = await page.locator("#structure-audit").textContent();
    assert.match(panel, /组票目标核对/);
    assert.match(panel, /最小的号码/);
    assert.match(panel, /前区胆：自选池中的最小号码 01 02/);
    assert.match(panel, /低于这个最高/);
    assert.match(panel, new RegExp(`原方案连乘质量 ${audit.comparison.chosen_mass}`));
    assert.doesNotMatch(panel, /¥0\.00|质量 0/);
    const previewExport = await call("export");
    for (const key of ["draws", "records", "recommendations", "settings", "audit_entries_verified", "selection_evidence"]) assert.deepEqual(previewExport[key], before[key]);
    const deadline = new Date(Date.now() + 3600000);
    await page.locator("#save-deadline").fill(new Date(deadline.getTime() - deadline.getTimezoneOffset() * 60000).toISOString().slice(0, 16));
    await page.locator("#plan-label").fill("结构核对");
    await page.locator("#save-plan").click();
    await page.locator("#saved-confirmation").waitFor();
    await page.locator('[data-tab="records"]').click();
    await page.locator("[data-evidence-id]").click();
    await page.locator("#structure-audit").waitFor();
    const archived = await page.locator(".selection-evidence").textContent();
    assert.match(archived, /最小的号码/);
    assert.match(archived, /低于这个最高/);
    assert.match(archived, /原档案的选号依据/);
    await fs.mkdir(output, {recursive: true});
    await page.evaluate(() => {const toast = document.querySelector("#toast"); if (toast) toast.hidden = true;});
    await page.locator("#structure-audit").screenshot({path: path.join(output, "01-audit-desktop.png")});
    const headings = {generator: "为下一期准备一组号码", records: "让每一组号码都有记录", reviews: "每期推荐与复盘", draws: "每一期开奖，有据可查", backtest: "把概率与历史分开看", research: "对照研究，保留证据", settings: "可复现，也可核验"};
    for (const width of [1440, 375]) {
      await page.setViewportSize({width, height: 900});
      for (const tab of ["generator", "records", "reviews", "draws", "backtest", "research", "settings"]) {
        await page.locator(`[data-tab="${tab}"]`).click();
        await page.waitForFunction(tab => document.querySelector(`nav [data-tab="${tab}"]`)?.getAttribute("aria-current") === "page", tab);
        await page.waitForFunction(heading => document.querySelector("h1")?.textContent === heading, headings[tab]);
        await page.waitForLoadState("networkidle");
        const overflow = await page.evaluate(() => ({width: innerWidth, scroll: document.documentElement.scrollWidth, elements: [...document.querySelectorAll("main *")].filter(element => element.getBoundingClientRect().right > innerWidth + 1 && getComputedStyle(element).position !== "absolute").slice(0, 12).map(element => ({tag: element.tagName, id: element.id, class: element.className, right: element.getBoundingClientRect().right}))}));
        assert.ok(overflow.scroll <= width, `${tab} overflow at ${width}: ${JSON.stringify(overflow)}`);
      }
    }
    await page.setViewportSize({width: 375, height: 900});
    await page.locator('[data-tab="records"]').click();
    await page.locator("[data-evidence-id]").click();
    await page.locator("#structure-audit").waitFor();
    await page.evaluate(() => {const toast = document.querySelector("#toast"); if (toast) toast.hidden = true;});
    await page.locator("#structure-audit").screenshot({path: path.join(output, "02-audit-mobile.png")});
    await page.locator('[data-tab="settings"]').click();
    await page.waitForFunction(() => document.querySelector("h1")?.textContent === "可复现，也可核验");
    assert.match(await page.locator("main").textContent(), /lottery-dlt 1\.9\.0/);
    const after = await call("export");
    assert.deepEqual(after.draws, before.draws);
    assert.deepEqual(after.recommendations, before.recommendations);
    assert.deepEqual(after.settings, before.settings);
    assert.equal(after.records.length, before.records.length + 1);
    assert.equal(Object.keys(after.selection_evidence).length, Object.keys(before.selection_evidence).length + 1);
    assert.deepEqual(errors, []);
    const health = await (await fetch(url + "healthz")).json();
    const result = {application_version: health.version, generator_version: plan.generator_version, desktop_tabs: 7, mobile_tabs: 7, page_errors: errors, horizontal_overflow: false, numeric_prefix_below_score_product: true, chosen_mass: audit.comparison.chosen_mass, best_mass: audit.comparison.best_mass, uniform_tie_without_mass: true, coverage_not_scored_as_product: true, preview_did_not_write: true, saved_original_tickets: true, read_only_draws_unchanged: true};
    await fs.writeFile(path.join(output, "verification.json"), JSON.stringify(result, null, 2) + "\n");
    console.log(JSON.stringify(result, null, 2));
  } finally {
    if (browser) await browser.close();
    if (child.exitCode === null && child.signalCode === null) {const stopped = once(child, "exit"); child.kill("SIGINT"); await stopped;}
    await fs.rm(scratch, {recursive: true, force: true});
  }
}
main().catch(error => {console.error(error); process.exitCode = 1;});
