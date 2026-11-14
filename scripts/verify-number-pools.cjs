"use strict";
const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const os = require("node:os");
const path = require("node:path");
const {spawn} = require("node:child_process");
const {chromium} = require(process.env.PLAYWRIGHT_MODULE || "/home/administrator/playwright/node_modules/playwright");
const root = path.resolve(__dirname, "..");
const binary = path.join(root, "target/release/lottery-dlt");
const output = path.join(root, "test-results/number-pools-1.10.0");
const browserPath = "/home/administrator/.cache/ms-playwright/chromium-1234/chrome-linux64/chrome";

async function main() {
  const scratch = await fs.mkdtemp(path.join(os.tmpdir(), "dlt-pools-"));
  const token = "isolated-browser-test-token-123456";
  const child = spawn(binary, ["--db", path.join(scratch, "test.sqlite3"), "serve", "--bind", "127.0.0.1:0", "--base-path", "/dlt"], {
    env: {...process.env, LOTTERY_ADMIN_TOKEN: token, LOTTERY_REQUIRE_SCHEDULE: "0", TOKIO_WORKER_THREADS: "2"},
    stdio: ["ignore", "pipe", "pipe"],
  });
  let browser, stderr = "";
  child.stderr.on("data", data => {stderr += data.toString();});
  try {
    const url = await new Promise((resolve, reject) => {
      let out = "";
      const timer = setTimeout(() => reject(new Error(`Service start timed out: ${stderr}`)), 45000);
      child.on("error", error => {clearTimeout(timer); reject(error);});
      child.on("exit", code => {clearTimeout(timer); reject(new Error(`Service exited ${code}: ${stderr}`));});
      child.stdout.on("data", data => {
        out += data.toString();
        const match = out.match(/http:\/\/127\.0\.0\.1:\d+\/dlt\//);
        if (match) {clearTimeout(timer); resolve(match[0]);}
      });
    });
    const call = async (endpoint, body) => {
      const response = await fetch(url + "api/" + endpoint, {method: body ? "POST" : "GET", headers: {"Content-Type": "application/json", Authorization: `Bearer ${token}`}, body: body ? JSON.stringify(body) : undefined});
      const value = await response.json();
      assert.ok(response.ok, JSON.stringify(value));
      return value;
    };
    const draws = [
      {issue: "26001", date: "2026-03-01", front: [1, 2, 3, 4, 5], back: [1, 2]},
      {issue: "26002", date: "2026-03-02", front: [1, 2, 3, 4, 35], back: [1, 12]},
      {issue: "26003", date: "2026-03-03", front: [6, 7, 8, 9, 10], back: [3, 4]},
    ].map(draw => ({...draw, pool_before_fen: 0, source: "synthetic-isolated-browser-test"}));
    await call("draws/import", draws);
    const health = await (await fetch(url + "healthz")).json();
    assert.equal(health.version, "1.11.0");
    const rank = await call("pools/rank?issue=26003");
    assert.equal(rank.status, "settled");
    assert.equal(rank.data_cutoff, "26002");
    const named = name => rank.zones.front.algorithms.find(item => item.name === name);
    assert.equal(named("hot").ranking[0], 35);
    assert.equal(named("cover_partner").ranking[0], 35);
    assert.ok(named("cover_bottleneck").ranking.indexOf(35) < named("cover_bottleneck").ranking.indexOf(6));
    assert.equal(rank.zones.front.algorithms[0].top_hits !== 0 || rank.zones.front.algorithms[0].need !== null, true);
    assert.equal(rank.zones.front.uniform_expected_prefix[2][1], 30);
    assert.equal(rank.pools.length, 0);
    browser = await chromium.launch({headless: true, executablePath: browserPath, args: ["--no-sandbox", "--disable-dev-shm-usage"]});
    await fs.mkdir(output, {recursive: true});
    const errors = [];
    for (const width of [1440, 375]) {
      const context = await browser.newContext({viewport: {width, height: width === 1440 ? 1000 : 812}, locale: "zh-CN", timezoneId: "Asia/Shanghai"});
      await context.addInitScript(value => sessionStorage.setItem("dlt-admin-token", value), token);
      const page = await context.newPage();
      page.on("pageerror", error => errors.push(error.message));
      await page.goto(url + "#generator", {waitUntil: "networkidle"});
      await page.waitForSelector("#rank-front");
      const nav = await page.locator("#navigation").innerText();
      assert.match(nav, /号码池与排序/);
      assert.match(nav, /开奖历史/);
      assert.match(nav, /系统设置/);
      assert.ok(!nav.includes("复盘") && !nav.includes("回测") && !nav.includes("研究"));
      await page.waitForFunction(() => document.querySelector("#rank-front .rank-num.hit, #rank-front .rank-num"));
      const hot = await page.locator("#rank-front .rank-line", {hasText: "热号"}).locator(".rank-num").first().innerText();
      assert.equal(hot, "35");
      assert.match(await page.locator("#rank-front").innerText(), /30\.00/);
      assert.match(await page.locator("#rank-front").innerText(), /覆盖瓶颈/);
      assert.match(await page.locator("#rank-front").innerText(), /全覆盖融合/);
      assert.match(await page.locator("#rank-back").innerText(), /后区/);
      await page.locator("#tab-pools").click();
      await page.waitForSelector("#frozen-pools");
      assert.match(await page.locator("#frozen-pools").innerText(), /没有冻结的号码池/);
      await page.locator("#tab-rank").click();
      await page.locator("#sort-front").selectOption("history");
      await page.waitForFunction(() => document.querySelector("#sort-front").value === "history");
      if (width === 1440) {
        assert.equal(await page.locator("#pool-prev").isEnabled(), true);
        await page.locator("#pool-prev").click();
        await page.waitForFunction(() => document.querySelector("#pool-issue")?.value === "26002");
        await page.locator("#navigation button", {hasText: "开奖历史"}).click();
        await page.waitForSelector("table");
        await page.locator("#navigation button", {hasText: "系统设置"}).click();
        await page.waitForFunction(() => document.body.innerText.includes("lottery-dlt 1.11.0"));
        await page.locator("#navigation button", {hasText: "号码池与排序"}).click();
        await page.waitForSelector("#rank-front");
      }
      const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
      assert.ok(overflow <= 1, `overflow ${overflow} at ${width}`);
      const toast = page.locator("#toast");
      if (await toast.count()) await toast.evaluate(node => {node.hidden = true;});
      await page.screenshot({path: path.join(output, width === 1440 ? "01-rank-desktop.png" : "02-rank-mobile.png"), fullPage: true});
      await context.close();
    }
    assert.deepEqual(errors, []);
    console.log(JSON.stringify({version: health.version, hot: 35, cutoff: rank.data_cutoff, errors, screenshots: output}));
  } finally {
    child.kill("SIGTERM");
    if (browser) await browser.close();
  }
}
main().catch(error => {console.error(error); process.exit(1);});
