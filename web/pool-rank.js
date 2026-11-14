"use strict";
function poolRankLabel(pool) {
  if (pool.label) return pool.label;
  return `${names[pool.strategy] || pool.strategy} · ${names[pool.mode] || pool.mode}`;
}
function renderNumberPools() {
  const revision = state.renderRevision;
  content.innerHTML = title("NUMBER POOLS", "号码池与排序", "先看本期开奖前的号码顺序，再打开已经冻结的号码池。排序按更短的全覆盖号码池调整，不是中奖概率。") + `<div id="pool-rank" class="muted">正在计算排序…</div>`;
  const query = state.poolIssue ? `?issue=${encodeURIComponent(state.poolIssue)}` : "";
  return api("pools/rank" + query).then(data => {
    if (revision !== state.renderRevision) return;
    state.poolView = data;
    state.poolIssue = data.issue;
    state.poolPanel = state.poolPanel || "rank";
    state.poolSort = state.poolSort || "size";
    state.poolQuery = state.poolQuery || "";
    state.poolOpen = state.poolOpen || "";
    state.rankSpan = state.rankSpan || {front: 15, back: 12};
    paintPoolRank();
  });
}
function paintPoolRank() {
  const data = state.poolView;
  if (!data || !$("pool-rank")) return;
  const issues = data.issues.map(item => `<option value="${escapeHTML(item.issue)}" ${item.issue === data.issue ? "selected" : ""}>${escapeHTML(item.issue)}${item.settled ? "" : " · 待开奖"}</option>`).join("");
  const index = data.issues.findIndex(item => item.issue === data.issue);
  $("pool-rank").innerHTML = `<section class="card" id="issue-picker"><div class="row"><label class="field">期号<select id="pool-issue">${issues}</select></label><button class="button" id="pool-prev" type="button" ${index >= data.issues.length - 1 ? "disabled" : ""}>上一期</button><button class="button" id="pool-next" type="button" ${index <= 0 ? "disabled" : ""}>下一期</button></div><p class="muted">数据截止 ${escapeHTML(data.data_cutoff || "无")} · ${data.status === "settled" ? "已开奖" : "待开奖，名次覆盖在开奖后显示"}</p>${data.draw ? balls(data.draw.front, data.draw.back, data.draw) : ""}<p class="muted">${escapeHTML(data.note)}</p></section><div class="review-tasks" role="tablist"><button type="button" id="tab-rank" aria-selected="${state.poolPanel === "rank"}">本期排序</button><button type="button" id="tab-pools" aria-selected="${state.poolPanel === "pools"}">冻结号码池 · ${data.pools.length}</button></div>${state.poolPanel === "rank" ? rankPanel(data) : frozenPanel(data)}`;
  $("pool-issue").onchange = () => { state.poolIssue = $("pool-issue").value; renderNumberPools(); };
  $("pool-prev").onclick = () => { const next = data.issues[index + 1]; if (next) { state.poolIssue = next.issue; renderNumberPools(); } };
  $("pool-next").onclick = () => { const next = data.issues[index - 1]; if (next) { state.poolIssue = next.issue; renderNumberPools(); } };
  $("tab-rank").onclick = () => { state.poolPanel = "rank"; paintPoolRank(); };
  $("tab-pools").onclick = () => { state.poolPanel = "pools"; paintPoolRank(); };
  if (state.poolPanel === "rank") wireRank(data);
  else wirePools(data);
}
function rankNeed(algorithm, count) {
  const pair = (algorithm.need || []).find(item => item[0] === count);
  return pair ? pair[1] : null;
}
function shownAlgorithms(item) {
  const list = item.algorithms.slice();
  const full = item.targets[item.targets.length - 1];
  const value = algorithm => {
    if (state.rankSort === "top") return -(algorithm.top_hits ?? -1);
    if (state.rankSort === "history") return algorithm.history_mean_full ?? Number.POSITIVE_INFINITY;
    return rankNeed(algorithm, full) ?? Number.POSITIVE_INFINITY;
  };
  if (state.rankSort && state.rankSort !== "default") list.sort((a, b) => value(a) - value(b));
  return list;
}
function rankPanel(data) {
  if (!data.draw && (state.rankSort === "need" || state.rankSort === "top")) state.rankSort = "default";
  state.rankSort = state.rankSort || "default";
  state.rankFocus = state.rankFocus || {};
  return ["front", "back"].map(zone => {
    const item = data.zones[zone];
    const choices = zone === "front" ? [[15, "前15名"], [35, "全部35名"]] : [[12, "全部12名"]];
    const span = choices.some(choice => choice[0] === state.rankSpan[zone]) ? state.rankSpan[zone] : choices[0][0];
    state.rankSpan[zone] = span;
    const title = zone === "front" ? "前区" : "后区";
    const full = item.targets[item.targets.length - 1];
    const algorithms = shownAlgorithms(item);
    if (!algorithms.length) return `<section class="card" id="rank-${zone}"><h2>${title}排序</h2><p class="notice">这一区还没有排序。</p></section>`;
    const focus = algorithms.some(algorithm => algorithm.name === state.rankFocus[zone]) ? state.rankFocus[zone] : algorithms[0].name;
    state.rankFocus[zone] = focus;
    const guides = [1, 2].map(part => `<i class="dist-guide" style="left:${(part / 3) * 100}%"></i>`).join("");
    const distribution = data.draw ? `<h3>开奖号在各排序中的位置</h3><p class="muted">从左到右是第1名到第${item.size}名，红点是开奖号，灰线把区间分成三段。右侧是盖住全部${full}个开奖号的名次。</p><div class="dist-board">${algorithms.map(algorithm => {
      const marks = algorithm.ranking.map((number, index) => data.draw[zone].includes(number) ? `<i class="dist-hit" style="left:${(index / (item.size - 1)) * 100}%"></i>` : "").join("");
      const need = rankNeed(algorithm, full);
      return `<button type="button" class="dist-row${algorithm.name === focus ? " selected" : ""}" data-zone="${zone}" data-algorithm="${escapeHTML(algorithm.name)}"><span class="rank-name">${escapeHTML(algorithm.label)}</span><span class="dist-track">${guides}${marks}</span><span class="dist-need">${need == null ? "—" : need}</span></button>`;
    }).join("")}</div>` : "";
    const scale = Array.from({length: span}, (_, index) => `<span class="rank-num">${(index + 1) % 5 === 0 ? index + 1 : ""}</span>`).join("");
    const numbers = algorithms.map(algorithm => `<button type="button" class="rank-line${algorithm.name === focus ? " selected" : ""}" data-zone="${zone}" data-algorithm="${escapeHTML(algorithm.name)}"><span class="rank-name">${escapeHTML(algorithm.label)}</span><span class="rank-nums">${algorithm.ranking.slice(0, span).map(number => `<span class="rank-num${data.draw && data.draw[zone].includes(number) ? " hit" : ""}">${String(number).padStart(2, "0")}</span>`).join("")}</span></button>`).join("");
    const head = data.draw ? item.targets.map(count => `<th>覆盖${count}个</th>`).join("") + `<th>前${item.drawn}名含开奖号</th>` : "";
    const body = algorithms.map(algorithm => {
      const cells = data.draw ? item.targets.map(count => `<td class="num">${rankNeed(algorithm, count) == null ? "—" : `前${rankNeed(algorithm, count)}名`}</td>`).join("") + `<td class="num">${algorithm.top_hits == null ? "—" : `${algorithm.top_hits} / ${item.drawn}`}</td>` : "";
      return `<tr class="${algorithm.name === focus ? "selected" : ""}" data-zone="${zone}" data-algorithm="${escapeHTML(algorithm.name)}"><td>${escapeHTML(algorithm.label)}</td>${cells}<td class="num">${algorithm.history_mean_full == null ? "—" : Number(algorithm.history_mean_full).toFixed(2)}</td></tr>`;
    }).join("");
    const uniform = data.draw ? item.uniform_expected_prefix.map(pair => `<td class="num">${Number(pair[1]).toFixed(2)}</td>`).join("") + `<td class="num">${Number(item.uniform_top_hits).toFixed(2)} / ${item.drawn}</td>` : "";
    const selected = algorithms.find(algorithm => algorithm.name === focus);
    const detail = selected ? `<div class="rank-grid" id="rank-detail-${zone}">${selected.ranking.map((number, index) => `<span class="rank-cell${data.draw && data.draw[zone].includes(number) ? " hit" : ""}"><strong>${String(number).padStart(2, "0")}</strong><small>第${index + 1}名</small></span>`).join("")}</div>` : "";
    const expected = item.uniform_expected_prefix.map(pair => `盖住${pair[0]}个约前${Number(pair[1]).toFixed(2)}名`).join(" · ");
    return `<section class="card" id="rank-${zone}"><h2>${title}排序</h2><p class="muted">从左到右是第1名起。已开奖时红色是开奖号。热号、冷号、遗漏、跟随、避开和分区仍按原方向，但会把曾经拖长全覆盖前缀的号码提前。覆盖瓶颈、同出补齐、全覆盖融合和近期全覆盖只追求更短的号码池。均匀随机下${expected}，这一行不参与调整。点一行查看全部号码。名次不是中奖概率。</p><div class="row"><label class="field">排序方式<select id="sort-${zone}"><option value="default" ${state.rankSort === "default" ? "selected" : ""}>按算法分组</option>${data.draw ? `<option value="need" ${state.rankSort === "need" ? "selected" : ""}>全覆盖所需名次从少到多</option><option value="top" ${state.rankSort === "top" ? "selected" : ""}>前${item.drawn}名含开奖号从多到少</option>` : ""}<option value="history" ${state.rankSort === "history" ? "selected" : ""}>历史平均全覆盖从少到多</option></select></label><label class="field">号码显示<select id="span-${zone}">${choices.map(choice => `<option value="${choice[0]}" ${span === choice[0] ? "selected" : ""}>${choice[1]}</option>`).join("")}</select></label></div>${distribution}<h3>排序后的号码</h3><div class="rank-board"><div class="rank-line rank-scale" aria-hidden="true"><span class="rank-name"></span><span class="rank-nums">${scale}</span></div>${numbers}</div><div class="table-wrap"><table><thead><tr><th>算法</th>${head}<th>历史平均全覆盖</th></tr></thead><tbody>${body}<tr class="muted"><td>均匀理论期望</td>${uniform}<td class="num">${Number(item.uniform_expected_prefix[item.uniform_expected_prefix.length - 1][1]).toFixed(2)}</td></tr></tbody></table></div><h3>${escapeHTML(selected.label)}的全部号码</h3><p class="muted">${data.draw ? "红色是开奖号。" : "待开奖，开奖号位置在开奖后显示。"}历史平均只统计更早开奖已经存在的期，待开奖不记成 0。</p>${detail}</section>`;
  }).join("");
}
function wireRank() {
  for (const zone of ["front", "back"]) {
    if (!$(`sort-${zone}`)) continue;
    $(`sort-${zone}`).onchange = () => { state.rankSort = $(`sort-${zone}`).value; paintPoolRank(); };
    $(`span-${zone}`).onchange = () => { state.rankSpan[zone] = Number($(`span-${zone}`).value); paintPoolRank(); };
  }
  document.querySelectorAll("[data-algorithm]").forEach(row => row.onclick = () => { state.rankFocus[row.dataset.zone] = row.dataset.algorithm; paintPoolRank(); });
}
function shownPools(data) {
  const query = state.poolQuery.trim().toLowerCase();
  const list = data.pools.filter(pool => !query || poolRankLabel(pool).toLowerCase().includes(query));
  const sort = data.draw || state.poolSort !== "covered" ? state.poolSort : "size";
  const covered = pool => (pool.front_covered ?? 0) + (pool.back_covered ?? 0);
  const by = {size: (a, b) => a.front.length + a.back.length - (b.front.length + b.back.length), covered: (a, b) => covered(b) - covered(a), name: (a, b) => poolRankLabel(a).localeCompare(poolRankLabel(b), "zh")}[sort];
  return list.sort(by);
}
function frozenPanel(data) {
  const rows = shownPools(data);
  const opened = rows.find(pool => pool.id === state.poolOpen) || rows[0];
  if (opened) state.poolOpen = opened.id;
  const table = rows.length ? `<div class="table-wrap"><table id="pool-list"><thead><tr><th>方案</th><th>前区 / 后区</th>${data.draw ? "<th>覆盖开奖号</th>" : ""}<th>注数 / 成本</th></tr></thead><tbody>${rows.map(pool => `<tr data-pool="${escapeHTML(pool.id)}" class="${opened && opened.id === pool.id ? "selected" : ""}"><td>${escapeHTML(poolRankLabel(pool))}</td><td class="num">${pool.front.length} / ${pool.back.length}</td>${data.draw ? `<td class="num">${pool.front_covered} / 5 · ${pool.back_covered} / 2</td>` : ""}<td>${pool.bets}注 / ${money(pool.cost_fen)}</td></tr>`).join("")}</tbody></table></div>` : `<p class="notice">这一期没有冻结的号码池。</p>`;
  const detail = opened ? `<article class="card" id="opened-pool"><h3>${escapeHTML(poolRankLabel(opened))}</h3><p class="muted">${opened.bets}注 / ${money(opened.cost_fen)} · 前区 ${opened.front.length} 个 · 后区 ${opened.back.length} 个${data.draw ? ` · 覆盖前区 ${opened.front_covered}/5、后区 ${opened.back_covered}/2` : ""}</p>${balls(opened.front, opened.back, data.draw)}</article>` : "";
  return `<section class="card" id="frozen-pools"><h2>本期实际冻结号码池</h2><div class="row"><label class="field">筛选<input id="pool-search" value="${escapeHTML(state.poolQuery)}" placeholder="方案名"></label><label class="field">排序<select id="pool-sort"><option value="size" ${state.poolSort === "size" ? "selected" : ""}>号码数从少到多</option>${data.draw ? `<option value="covered" ${state.poolSort === "covered" ? "selected" : ""}>覆盖开奖号从多到少</option>` : ""}<option value="name" ${state.poolSort === "name" ? "selected" : ""}>方案名</option></select></label></div><p class="muted">共 ${rows.length} 个方案。号码按池内从小到大排列，点一行查看。</p>${table}${detail}</section>`;
}
function wirePools(data) {
  if ($("pool-search")) $("pool-search").oninput = () => {
    state.poolQuery = $("pool-search").value;
    const caret = $("pool-search").selectionStart;
    paintPoolRank();
    const field = $("pool-search");
    if (field) { field.focus(); field.setSelectionRange(caret, caret); }
  };
  if ($("pool-sort")) $("pool-sort").onchange = () => { state.poolSort = $("pool-sort").value; paintPoolRank(); };
  document.querySelectorAll("#pool-list tbody tr").forEach(row => row.onclick = () => { state.poolOpen = row.dataset.pool; paintPoolRank(); });
}
