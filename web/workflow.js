"use strict";
(function(root){
  const enums={mode:["single","compound","dantuo"],strategy:["random","coverage","structural","hot","cold","omission"]};
  const numeric=["bets","front_pool","back_pool","front_banker_count","back_banker_count","window","multiplier"],arrays=["front_numbers","back_numbers","front_bankers","back_bankers"];
  function draft(value){
    if(!value||value.schema_version!==1||!value.request)throw new Error("草稿格式无效");
    const r=value.request,request={};
    if(typeof r.issue!=="string"||!/^\d{0,5}$/.test(r.issue))throw new Error("草稿期号无效");request.issue=r.issue;
    for(const [key,values] of Object.entries(enums)){if(!values.includes(r[key]))throw new Error("草稿选项无效");request[key]=r[key];}
    for(const key of numeric){if(!Number.isSafeInteger(r[key])||r[key]<0||r[key]>1000000)throw new Error("草稿参数无效");request[key]=r[key];}
    for(const key of arrays){if(!Array.isArray(r[key])||r[key].length>35||r[key].some(n=>!Number.isSafeInteger(n)||Math.abs(n)>1000000))throw new Error("草稿号码无效");request[key]=[...r[key]];}
    if(typeof r.additional!=="boolean")throw new Error("草稿追加选项无效");request.additional=r.additional;
    const label=value.label??"",deadline=value.deadline??"";
    if(typeof label!=="string"||label.length>80||typeof deadline!=="string"||deadline&&!/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}$/.test(deadline))throw new Error("草稿保存信息无效");
    return {schema_version:1,request,label,deadline};
  }
  function parseDraft(raw){if(typeof raw!=="string"||raw.length>16000)throw new Error("草稿体积无效");return draft(JSON.parse(raw));}
  function gate(request,overview,token){
    if(overview?.admin_token_required&&!token)return {ready:false,message:"生成和保存需要管理员访问，请先到系统设置验证令牌。",href:"#settings",action:"验证管理员访问"};
    if(!/^\d{5}$/.test(request.issue))return {ready:false,message:"请填写已核实的五位目标期号。",href:"#issue",action:"填写目标期号"};
    if(overview?.latest_draw&&request.issue<=overview.latest_draw.issue)return {ready:false,message:"目标期已开奖或早于当前缓存，请核对未来期号。",href:"#issue",action:"核对目标期号"};
    if(["hot","cold","omission"].includes(request.strategy)&&!overview?.draw_count)return {ready:false,message:"本实验策略需要目标期之前的开奖数据，请先同步或导入；随机与均衡无需历史。",href:"#draws",action:"准备开奖数据"};
    return {ready:true,message:""};
  }
  function activeSchedule(schedules,now=Date.now()){
    return [...schedules].filter(s=>/^\d{5}$/.test(s.issue)&&Date.parse(s.deadline)>now&&Date.parse(s.deadline)<=now+30*86400000&&Date.parse(s.verified_at)>=now-2*3600000&&Date.parse(s.verified_at)<=now).sort((a,b)=>Date.parse(a.deadline)-Date.parse(b.deadline))[0]||null;
  }
  async function loadSnapshot(api){
    const core=Promise.all(["overview","config"].map(key=>api(key)));
    const optional=Promise.allSettled(["schedules","operations"].map(key=>api(key)));
    const [[overview,config],results]=await Promise.all([core,optional]);
    const warnings={};for(const [i,key] of ["schedules","operations"].entries())if(results[i].status==="rejected")warnings[key]=String(results[i].reason?.message||"暂时不可用");
    return {overview,config,schedules:results[0].status==="fulfilled"?results[0].value.schedules:[],operations:results[1].status==="fulfilled"?results[1].value:null,warnings};
  }
  const api={draft,parseDraft,gate,activeSchedule,loadSnapshot};
  if(typeof module!=="undefined"&&module.exports)module.exports=api;else root.DltWorkflow=api;
})(typeof globalThis!=="undefined"?globalThis:this);
