const {test}=require("node:test"),assert=require("node:assert/strict"),fs=require("node:fs"),vm=require("node:vm");
function context(){
  const c={state:{poolQuery:"",poolSort:"size",poolOpen:"",rankSpan:{front:15,back:12},poolPanel:"rank"},names:{random:"均匀随机",dantuo:"胆拖"},escapeHTML:v=>String(v??"").replaceAll("&","&amp;").replaceAll("<","&lt;").replaceAll(">","&gt;"),money:fen=>fen==null?"待核实":`¥${(fen/100).toFixed(2)}`,balls:()=>""};
  vm.createContext(c);vm.runInContext(fs.readFileSync(require("node:path").join(__dirname,"../web/pool-rank.js"),"utf8"),c);return c;
}
test("frozen pool names are escaped and a pending ranking is not shown as zero",()=>{
  const c=context();
  const data={status:"pending",draw:null,note:"说明",zones:{front:{size:35,drawn:5,targets:[3,4,5],uniform_expected_prefix:[[5,30]],uniform_top_hits:0.71,algorithms:[{name:"hot",label:"热号",ranking:[35],need:null,top_hits:null,history_mean_full:null}]},back:{size:12,drawn:2,targets:[1,2],uniform_expected_prefix:[[2,8.67]],uniform_top_hits:0.33,algorithms:[]}}};
  const rank=c.rankPanel(data);
  assert.match(rank,/待开奖/);assert.ok(!rank.includes("历史全覆盖 0"));
  const pools=c.frozenPanel({draw:null,pools:[{id:"1",label:"<script>",kind:"recommendation",mode:"dantuo",strategy:"random",bets:1,cost_fen:200,front:[1],back:[2],front_covered:null,back_covered:null}]});
  assert.ok(!pools.includes("<script>"));assert.match(pools,/&lt;script&gt;/);
});
