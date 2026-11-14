const {test}=require("node:test"),assert=require("node:assert/strict");
const F=require("../web/format.js");
test("small positive probabilities remain visibly nonzero",()=>{
  assert.equal(F.probability(0.000103),"0.01%");
  assert.equal(F.probability(1/21425712),"4.67e-6%");
  assert.equal(F.probability(0.009386),"0.94%");
});
test("rounding cannot turn a probability below one into certainty",()=>{
  assert.equal(F.probability(0.999999),">99.99%");assert.equal(F.probability(1),"100.00%");assert.equal(F.probability(0),"0.00%");
});
test("unknown and invalid probabilities are distinct from zero",()=>{
  for(const value of [null,undefined,NaN,Infinity,-1,1.1,"0.5"])assert.equal(F.probability(value),"—");
});
test("paired negative effects retain their sign",()=>{
  assert.equal(F.percent(-0.1),"-10.00%");assert.equal(F.percent(-0.999999),"<-99.99%");assert.equal(F.percent(-1e-7),"-1.00e-5%");
});
