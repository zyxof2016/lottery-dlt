"use strict";
(function(root){
  function choose(n,k){if(!Number.isInteger(n)||!Number.isInteger(k)||n<0||k<0||k>n)return 0;let value=1;for(let i=1;i<=Math.min(k,n-k);i++)value=value*(n-i+1)/i;return Math.round(value);}
  function estimate(request){
    const integer=(value,min,max)=>Number.isInteger(value)&&value>=min&&value<=max;
    const invalid=message=>({valid:false,bets:null,cost_fen:null,message});
    if(!integer(request.multiplier,1,99))return invalid("倍数须为1～99。");
    let bets;
    if(request.mode==="single"){
      if(!integer(request.bets,1,1000))return invalid("单式注数须为1～1000。");
      bets=request.bets;
      if(["hot","cold","omission"].includes(request.strategy)){
        if(!integer(request.front_pool,5,35)||!integer(request.back_pool,2,12))return invalid("请检查前后区号码池大小。");
        if(choose(request.front_pool,5)*choose(request.back_pool,2)<bets)return invalid("候选池组合不足，请扩大号码池或减少注数。");
      }
    }else if(["compound","dantuo"].includes(request.mode)){
      const f=request.front_pool,b=request.back_pool;
      if(!integer(f,5,35)||!integer(b,2,12))return invalid("前区池须为5～35，后区池须为2～12。");
      const df=request.mode==="dantuo"?request.front_banker_count:0,db=request.mode==="dantuo"?request.back_banker_count:0;
      if(!integer(df,0,4)||!integer(db,0,1)||request.mode==="dantuo"&&df+db===0)return invalid("胆拖至少一区有胆；前区最多4胆，后区最多1胆。");
      if(df&&f<6||db&&b<3)return invalid("有胆的前区至少6个号码，后区至少3个号码（含胆）。");
      bets=choose(f-df,5-df)*choose(b-db,2-db);
    }else return invalid("请选择投注形态。");
    const cost=bets*(request.additional?300:200)*request.multiplier;
    const message=bets>1000?`将展开${bets.toLocaleString()}注，超过1000注上限，请缩小号码池。`:cost>(request.additional?3000000:2000000)?"金额超过单张限额，请减少注数或倍数。":"注数和金额预估，实际票组以生成结果为准。";
    return {valid:bets<=1000&&cost<=(request.additional?3000000:2000000),bets,cost_fen:cost,message};
  }
  function savedHref(base,record){return `${base}/?issue=${encodeURIComponent(record.plan.request.issue)}#${record.kind==="personal"?"saved":"record"}-${encodeURIComponent(record.id)}`;}
  const api={choose,estimate,savedHref};
  if(typeof module!=="undefined"&&module.exports)module.exports=api;else root.DltUsability=api;
})(typeof globalThis!=="undefined"?globalThis:this);
