"use strict";
// Disposable loopback service. Synthetic tickets and draws stay in its temporary database.
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
const output = path.resolve(option("--output", path.join(root, "test-results/hit-gap-1.8.0")));
async function main() {
  const scratch = await fs.mkdtemp(path.join(os.tmpdir(), "dlt-hit-gap-"));
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
    const api = async endpoint => {
      const response = await fetch(url + "api/" + endpoint, {headers: {Authorization: `Bearer ${token}`}});
      const value = await response.json();
      assert.ok(response.ok, JSON.stringify(value));
      return value;
    };
    const post = async (endpoint, body) => {
      const response = await fetch(url + "api/" + endpoint, {method: "POST", headers: {"Content-Type": "application/json", Authorization: `Bearer ${token}`}, body: JSON.stringify(body)});
      const value = await response.json();
      assert.ok(response.ok, JSON.stringify(value));
      return value;
    };
    const year = new Date().getFullYear() % 100;
    const issue = `${year}118`, next = `${year}119`;
    const deadline = new Date(Date.now() + 3600000).toISOString();
    const archived = await post("recommendations/archive", {issue, deadline});
    await post("recommendations/archive", {issue: next, deadline});
    const date = new Intl.DateTimeFormat("en-CA", {timeZone: "Asia/Shanghai", year: "numeric", month: "2-digit", day: "2-digit"}).format(new Date(deadline));
    const source = archived.records[2];
    await post("draws/import", [{issue, date, ...source.plan.tickets[0], pool_before_fen: 0, source: "synthetic-isolated-browser-test"}]);
    const review = await api("recommendations/live-review?limit=30");
    const entries = review.periods.flatMap(period => period.entries);
    for (const entry of entries) {
      const gap = entry.hit_gap;
      if (gap.status === "pending") {assert.equal(gap.joint, null); assert.equal(gap.jackpot_tickets, null); continue;}
      for (const zone of ["front", "back"]) assert.equal(gap[zone].outside_pool + gap[zone].in_pool_not_on_best_ticket, gap[zone].gap);
    }
    const jackpot = entries.find(entry => entry.hit_gap.jackpot_tickets > 0);
    assert.ok(jackpot, "expected one archived ticket to match the synthetic draw");
    assert.equal(jackpot.hit_gap.prize_split.floating_prize_fen, null);
    assert.equal(jackpot.hit_gap.floating_covers_cost, null);
    const strategy = review.strategies.find(row => row.label === jackpot.label);
    assert.equal(strategy.hit_review.drawdown_status, "missing_awards");
    assert.equal(strategy.hit_review.max_drawdown_fen, null);
    assert.ok(strategy.hit_review.jackpot_periods >= 1);
    assert.ok(strategy.hit_review.pending_periods >= 1);
    assert.equal(review.periods.find(period => period.issue === next).entries[0].hit_gap.status, "pending");
    assert.equal(review.requested_limit, 30);
    assert.equal(review.period_count, 2);
    const before = await api("export");
    browser = await chromium.launch({headless: true, ...(option("--browser", "") ? {executablePath: option("--browser", "")} : {}), args: ["--no-sandbox", "--disable-dev-shm-usage"]});
    const context = await browser.newContext({viewport: {width: 1440, height: 1000}, locale: "zh-CN", timezoneId: "Asia/Shanghai", acceptDownloads: true});
    await context.addInitScript(token => sessionStorage.setItem("dlt-admin-token", token), token);
    const page = await context.newPage(), errors = [];
    page.on("pageerror", error => errors.push(error.message));
    await page.goto(url + "reviews", {waitUntil: "networkidle"});
    await page.locator("#hit-gap").waitFor();
    assert.equal(await page.locator("h1").textContent(), "每期推荐与复盘");
    await page.locator("#hit-gap summary").click();
    const panel = await page.locator("#hit-gap").textContent();
    assert.match(panel, /单注差额与奖金构成/);
    assert.match(await page.locator("#live-review").textContent(), /请求最近 30 期，实际列入 2 期/);
    assert.match(panel, /待开奖/);
    assert.match(panel, /池外/);
    const row = page.locator("#hit-gap tbody tr", {hasText: jackpot.label}).first();
    const rowText = await row.textContent();
    assert.match(rowText, /待核实/);
    assert.doesNotMatch(rowText, /¥0\.00 \/ 待核实|待核实 \/ ¥0\.00/);
    assert.match(rowText, /金额未知 1 期/);
    let fixedAssistedRows = 0;
    for (const strategy of review.strategies) {
      const hit = strategy.hit_review;
      const text = await page.locator("#hit-gap tbody tr", {hasText: strategy.label}).first().textContent();
      if (!hit.settled_periods) {assert.match(text, /尚无已开奖/); continue;}
      assert.match(text, new RegExp(`已确认浮动奖回本 ${hit.floating_recovered_periods} 期`));
      assert.match(text, new RegExp(`固定奖补足 ${hit.fixed_assisted_recovered_periods} 期`));
      assert.match(text, new RegExp(`金额未知 ${hit.recovery_unknown_periods} 期`));
      if (hit.fixed_assisted_recovered_periods > 0) {
        fixedAssistedRows += 1;
        assert.equal(hit.floating_recovered_periods, 0);
      }
    }
    const download = page.waitForEvent("download");
    await page.locator("#export-hit-gap").click();
    const file = path.join(scratch, "hit-gap.json");
    await (await download).saveAs(file);
    const exported = JSON.parse(await fs.readFile(file, "utf8"));
    assert.equal(exported.schema_version, 1);
    assert.equal(exported.content_hash, review.content_hash);
    assert.equal(exported.strategies.find(row => row.label === jackpot.label).hit_review.drawdown_status, "missing_awards");
    assert.equal(exported.periods.find(period => period.issue === next).entries[0].hit_gap.status, "pending");
    const table = page.locator("#hit-gap > .table-wrap");
    assert.equal(await table.getAttribute("tabindex"), "0");
    await table.focus();
    await page.keyboard.press("ArrowRight");
    await page.waitForFunction(() => document.querySelector("#hit-gap > .table-wrap")?.scrollLeft > 0);
    await fs.mkdir(output, {recursive: true});
    await page.locator("#hit-gap").screenshot({path: path.join(output, "01-gap-desktop.png")});
    const headings = {generator: "为下一期准备一组号码", records: "让每一组号码都有记录", reviews: "每期推荐与复盘", draws: "每一期开奖，有据可查", backtest: "把概率与历史分开看", research: "对照研究，保留证据", settings: "可复现，也可核验"};
    for (const width of [1440, 375]) {
      await page.setViewportSize({width, height: 900});
      for (const tab of ["generator", "records", "reviews", "draws", "backtest", "research", "settings"]) {
        await page.locator(`[data-tab="${tab}"]`).click();
        await page.waitForFunction(tab => document.querySelector(`nav [data-tab="${tab}"]`)?.getAttribute("aria-current") === "page", tab);
        await page.waitForFunction(heading => document.querySelector("h1")?.textContent === heading, headings[tab]);
        if (tab === "reviews") await page.locator("#hit-gap").waitFor();
        await page.waitForLoadState("networkidle");
        const overflow = await page.evaluate(() => ({width: innerWidth, scroll: document.documentElement.scrollWidth, elements: [...document.querySelectorAll("main *")].filter(element => element.getBoundingClientRect().right > innerWidth + 1 && getComputedStyle(element).position !== "absolute").slice(0, 12).map(element => ({tag: element.tagName, id: element.id, class: element.className, right: element.getBoundingClientRect().right}))}));
        assert.ok(overflow.scroll <= width, `${tab} overflow at ${width}: ${JSON.stringify(overflow)}`);
      }
    }
    await page.setViewportSize({width: 375, height: 900});
    await page.locator('[data-tab="reviews"]').click();
    await page.locator("#hit-gap").waitFor();
    if (await page.locator("#hit-gap summary").getAttribute("aria-expanded") !== "true") await page.locator("#hit-gap details").first().evaluate(node => {node.open = true;});
    await page.locator("#hit-gap").screenshot({path: path.join(output, "02-gap-mobile.png")});
    const after = await api("export");
    for (const key of ["draws", "records", "recommendations", "settings", "audit_entries_verified", "selection_evidence"]) assert.deepEqual(after[key], before[key]);
    assert.deepEqual(errors, []);
    const health = await (await fetch(url + "healthz")).json();
    const result = {application_version: health.version, desktop_tabs: 7, mobile_tabs: 7, page_errors: errors, horizontal_overflow: false, requested_periods: 30, listed_periods: 2, jackpot_money_unknown: true, pending_gap_unknown: true, recovery_rows_match_api: true, fixed_assisted_rows: fixedAssistedRows, identity_checked: true, export_matches_report: true, keyboard_table_scroll: true, read_only_data_unchanged: true};
    await fs.writeFile(path.join(output, "verification.json"), JSON.stringify(result, null, 2) + "\n");
    console.log(JSON.stringify(result, null, 2));
  } finally {
    if (browser) await browser.close();
    if (child.exitCode === null && child.signalCode === null) {const stopped = once(child, "exit"); child.kill("SIGINT"); await stopped;}
    await fs.rm(scratch, {recursive: true, force: true});
  }
}
main().catch(error => {console.error(error); process.exitCode = 1;});
