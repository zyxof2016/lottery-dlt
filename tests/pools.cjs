const {test}=require("node:test");
const assert=require("node:assert/strict");
const P=require("../web/pools.js");
const pool=(id,front,back,extra={})=>({id,front,back,...extra});
const evidence=(extra={})=>({status:"recorded",content_hash:"a".repeat(64),front_score_status:"recorded_scores",back_score_status:"recorded_scores",front_all_scores_equal:true,back_all_scores_equal:false,front_candidate_basis:"rank_prefix",back_candidate_basis:"rank_prefix",...extra});
test("per-zone content voting retains all sources and uses the effective denominator",()=>{
  const input=[pool("a",[1,2],[1]),pool("b",[1,2],[2]),pool("c",[2,3],[2])],original=JSON.stringify(input);
  const r=P.analyze(input,2,{deduplicate_content:true});
  assert.equal(r.zones.front.effective_pool_count,2);assert.equal(r.zones.back.effective_pool_count,2);
  const row=r.zones.front.rows.find(r=>r.number===1);assert.equal(row.count,1);assert.equal(row.source_count,2);assert.equal(row.frequency,.5);
  assert.deepEqual(row.included_pool_ids,["a","b"]);assert.deepEqual(r.zones.front.voting_groups,[["a","b"],["c"]]);
  assert.deepEqual(r.zones.back.voting_groups,[["a"],["b","c"]]);assert.deepEqual(r.zones.front.high_frequency.numbers,[2]);
  assert.equal(r.pools.length,3);assert.equal(JSON.stringify(input),original);assert.match(P.csv(r),/本区有效计票池数/);
});
test("flat exclusion is per-zone and preserves unknown unscored and manually chosen pools",()=>{
  const scored=(id,e,source="personal")=>pool(id,[1,2],[1,2],{source,score_evidence:e});
  const r=P.analyze([scored("flat",evidence()),scored("old",null),scored("random",evidence({front_score_status:"not_scored"})),scored("manual",evidence(),"manual"),scored("manual-candidates",evidence({front_candidate_basis:"manual_pool"})),scored("no-hash",{...evidence(),content_hash:"bad"})],2,{exclude_flat_scores:true,deduplicate_content:true});
  assert.deepEqual(r.zones.front.excluded_flat_pool_ids,["flat"]);assert.deepEqual(r.zones.back.excluded_flat_pool_ids,[]);
  assert.equal(r.zones.front.active_source_count,5);assert.equal(r.zones.front.effective_pool_count,1);
  assert.deepEqual(r.zones.front.preserved_manual_pool_ids,["manual-candidates"]);
  assert.deepEqual(r.zones.front.unknown_score_pool_ids,["old","random","manual","no-hash"]);
  assert.equal(r.pools.length,6);assert.equal(r.pools.find(p=>p.id==="random").score_evidence.front_all_scores_equal,null);
  assert.match(P.csv(r),/flat/);assert.equal(r.options.exclude_flat_scores,true);
});
test("excluding every confirmed flat source produces empty sets and preserves pending unknowns",()=>{
  const input=[pool("flat",[1,2],[1,2],{source:"recommendation",score_evidence:evidence({back_all_scores_equal:true})})];
  const pending=P.analyze(input,3,{exclude_flat_scores:true});
  assert.equal(pending.zones.front.effective_pool_count,0);assert.deepEqual(pending.zones.front.rows,[]);assert.deepEqual(pending.zones.front.thresholds,[]);
  assert.deepEqual(pending.zones.front.common.numbers,[]);assert.equal(pending.zones.front.common.hit_count,null);
  const drawn=P.analyze(input,3,{exclude_flat_scores:true,issue:"26030",draw:{issue:"26030",front:[1,2,3,4,5],back:[1,2]}});
  assert.equal(drawn.zones.front.common.hit_count,0);assert.equal(drawn.zones.front.common.selected_hit_fraction,null);assert.equal(drawn.zones.front.common.draw_coverage_fraction,0);
  assert.equal(drawn.zones.front.common.lost_hit_count,2);assert.equal(drawn.pools[0].id,"flat");assert.equal(drawn.schema_version,3);
});
test("both zones count each pool once and keep strict intersection separate",()=>{
  const r=P.analyze([pool("a",[1,1,2,3],[1,2]),pool("b",[2,3,4],[2,3]),pool("c",[3,4,5],[2,4])],2);
  assert.deepEqual(r.zones.front.common.numbers,[3]);assert.deepEqual(r.zones.back.common.numbers,[2]);
  assert.deepEqual(r.zones.front.rows.map(x=>[x.number,x.count]),[[3,3],[2,2],[4,2],[1,1],[5,1]]);
  assert.deepEqual(r.zones.front.high_frequency.numbers,[2,3,4]);
  assert.deepEqual(r.zones.front.rows[1].included_pool_ids,["a","b"]);
  assert.deepEqual(r.zones.front.rows[1].excluded_pool_ids,["c"]);
  assert.equal(r.zones.front.rows[0].draw_hit,null);
});
test("empty, single and disjoint pools have explicit intersections",()=>{
  assert.deepEqual(P.analyze([]).zones.front.common.numbers,[]);
  const one=P.analyze([pool("a",[2,1],[1,2])]);assert.equal(one.minimum_count,1);assert.deepEqual(one.zones.front.common.numbers,[1,2]);
  assert.deepEqual(P.analyze([pool("a",[1],[1]),pool("b",[2],[2])]).zones.front.common.numbers,[]);
});
test("same identity is refused while distinct identical content is disclosed",()=>{
  assert.throws(()=>P.analyze([pool("a",[1],[1]),pool("a",[2],[2])]));
  const r=P.analyze([pool("a",[1],[1]),pool("b",[1],[1])]);assert.deepEqual(r.identical_content_groups,[["a","b"]]);
  assert.equal(r.zones.front.rows[0].count,2);
});
test("manual parsing is atomic and accepts common separators",()=>{
  assert.deepEqual(P.numbers("01、02，03; 04；05\n05",35),[1,2,3,4,5]);
  for(const input of ["","1 36","1 -2","1 2.5","1e1","1 xxx","1 0"])assert.throws(()=>P.numbers(input,35));
  assert.throws(()=>P.pool(pool("a",[1],[13])));assert.throws(()=>P.pool(pool("a",[1],[1],{issue:"2603"})));
  for(const n of [0,1.5,201,NaN])assert.throws(()=>P.analyze([],n));
});
test("cross issue marks compare only with the selected draw and pending stays unknown",()=>{
  const pools=[pool("a",[1,2,3],[1,2],{issue:"26030"}),pool("b",[2,4],[2,3],{issue:"26031"})];
  const r=P.analyze(pools,2,{issue:"26031",draw:{issue:"26031",date:"2026-03-03",front:[1,2,6,7,8],back:[2,5]}});
  assert.equal(r.draw_comparison.cross_issue_pool_count,1);
  assert.equal(r.zones.front.common.hit_count,1);assert.equal(r.zones.front.union.hit_count,2);
  assert.equal(r.zones.back.common.hit_count,1);assert.equal(r.zones.front.rows.find(n=>n.number===4).draw_hit,false);
  assert.equal(P.analyze(pools,2,{issue:"26032"}).zones.front.common.hit_count,null);
  assert.throws(()=>P.analyze(pools,2,{issue:"26032",draw:r.draw_comparison.draw}));
});
test("exports include all union rows and safe source text regardless of threshold",()=>{
  const pools=[pool("a",[1,2],[1],{label:'=SUM(1,2)\n"quoted"'}),pool("b",[2,3],[2],{label:"+formula"})];
  const r=P.analyze(pools,2),csv=P.csv(r);
  assert.equal(r.zones.front.rows.length,3);assert.equal(r.pools[0].label,pools[0].label);
  assert.ok(csv.includes('"前区","01"'));assert.ok(csv.includes("'=SUM"));assert.ok(csv.includes("'+formula"));
  assert.ok(csv.includes('"待开奖"'));assert.deepEqual(P.analyze(JSON.parse(JSON.stringify(r.pools)),2),r);
});
test("actual expanded ticket unions retain original source identities",()=>{
  const r=P.fromRecord({id:"abc",kind:"personal",label:"saved",created_at:"2026-03-01T10:00Z",plan:{request:{issue:"26030"},tickets:[{front:[1,2,3,4,5],back:[1,2]},{front:[2,3,4,5,6],back:[1,2]}]}});
  assert.equal(r.id,"record:abc");assert.deepEqual(r.front,[1,2,3,4,5,6]);assert.deepEqual(r.back,[1,2]);
});
test("threshold curves show which reference hits were removed",()=>{
  const input=[pool("a",[1,2,3],[1,2]),pool("b",[2,4],[2,3]),pool("c",[2,5],[3,4])];
  const comparison={issue:"26030",reference_pool_id:"a",draw:{issue:"26030",date:"2026-03-01",front:[1,2,6,7,8],back:[1,2]}};
  const r=P.analyze(input,2,comparison),curve=r.zones.front.thresholds;
  assert.equal(curve.length,3);assert.deepEqual(curve[0].numbers,[1,2,3,4,5]);
  assert.deepEqual(curve[1].removed_from_reference,[1,3]);assert.deepEqual(curve[1].removed_hits_from_reference,[1]);
  assert.equal(curve[1].hit_count,1);assert.equal(curve[1].lost_hit_count,1);
  assert.equal(r.zones.front.high_frequency.selected_hit_fraction,1);assert.equal(r.zones.front.high_frequency.draw_coverage_fraction,0.2);
  assert.equal(P.analyze(input,2,{...comparison,draw:null}).zones.front.thresholds[1].lost_hit_count,null);
  assert.deepEqual(P.analyze(input,2,{...comparison,reference_pool_id:"b"}).zones.front.thresholds[1].removed_from_reference,[4]);
});
test("source searches include names and explicit zone sizes",()=>{
  const p=pool("a",[1,2,3],[1,2],{label:"均衡方案"});
  assert.equal(P.matches(p,"均衡"),true);assert.equal(P.matches(p,"前区3"),true);assert.equal(P.matches(p,"后区2"),true);assert.equal(P.matches(p,"前区5"),false);
});
