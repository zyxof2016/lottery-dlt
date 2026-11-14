"use strict";
(function(root){
  function percent(value) {
    if(typeof value!=="number" || !Number.isFinite(value))return "—";
    if(value===0)return "0.00%";
    const magnitude=Math.abs(value*100);
    if(magnitude<0.01)return `${(value*100).toExponential(2)}%`;
    if(Math.abs(value)<1 && magnitude.toFixed(2)==="100.00")return value>0?">99.99%":"<-99.99%";
    return `${(value*100).toFixed(2)}%`;
  }
  function probability(value) {return typeof value==="number" && value>=0 && value<=1?percent(value):"—";}
  const api={percent,probability};
  if(typeof module!=="undefined" && module.exports)module.exports=api;else root.DltFormat=api;
})(typeof globalThis!=="undefined"?globalThis:this);
