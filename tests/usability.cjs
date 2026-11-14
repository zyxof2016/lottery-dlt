const {test}=require("node:test"),assert=require("node:assert/strict");
const U=require("../web/usability.js");
const base={mode:"single",strategy:"random",bets:5,front_pool:7,back_pool:3,front_banker_count:2,back_banker_count:1,multiplier:1,additional:false};
test("estimates match the real single, compound and double-zone banker presets",()=>{
  assert.deepEqual([U.estimate(base).bets,U.estimate(base).cost_fen],[5,1000]);
  const compound=U.estimate({...base,mode:"compound",front_pool:6,back_pool:2});assert.equal(compound.bets,6);assert.equal(compound.cost_fen,1200);
  const banker=U.estimate({...base,mode:"dantuo"});assert.equal(banker.bets,20);assert.equal(banker.cost_fen,4000);
  const additional=U.estimate({...base,mode:"dantuo",additional:true,multiplier:3});assert.equal(additional.bets,20);assert.equal(additional.cost_fen,18000);
});
test("expansion and purchase limits are explained before generation",()=>{
  const huge=U.estimate({...base,mode:"compound",front_pool:35,back_pool:12});assert.equal(huge.bets,21425712);assert.equal(huge.valid,false);assert.match(huge.message,/1000注上限/);
  assert.equal(U.estimate({...base,bets:1000,multiplier:99}).valid,false);
  assert.equal(U.estimate({...base,bets:0}).valid,false);assert.equal(U.estimate({...base,multiplier:0}).valid,false);
});
test("banker and experimental candidate constraints do not silently change the request",()=>{
  assert.equal(U.estimate({...base,mode:"dantuo",front_banker_count:0,back_banker_count:0}).valid,false);
  assert.equal(U.estimate({...base,mode:"dantuo",front_pool:5}).valid,false);
  assert.equal(U.estimate({...base,strategy:"hot",front_pool:5,back_pool:2,bets:2}).valid,false);
  assert.equal(U.estimate({...base,front_pool:0,back_pool:0,front_banker_count:NaN}).valid,true);
});
test("saved links select the original personal or recommendation archive under a proxy prefix",()=>{
  const record={id:"original-id",kind:"personal",plan:{request:{issue:"26120"}}};
  assert.equal(U.savedHref("/dlt",record),"/dlt/?issue=26120#saved-original-id");
  assert.equal(U.savedHref("",{...record,kind:"recommendation"}),"/?issue=26120#record-original-id");
});
