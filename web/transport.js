"use strict";
(function(root){
  function readTimeout(path){
    const endpoint=path.split("?")[0];
    if(["schedules","operations"].includes(endpoint))return 5000;
    if(["recommendations/live-review","export"].includes(endpoint)||endpoint.startsWith("jobs/"))return 60000;
    return 15000;
  }
  async function request(url,options={},timeoutMs=15000){
    const read=["GET","HEAD"].includes((options.method||"GET").toUpperCase());
    const controller=read?new AbortController():null;
    let timer;
    const operation=(async()=>{
      let response,body;
      try{
        response=await root.fetch(url,{...options,...(controller?{signal:controller.signal}:{})});
        body=await response.text();
      }catch{
        throw new Error(read?"网络读取中断，请检查连接后重试。":"连接中断，操作结果尚未确认，请先核对记录或运行状态。");
      }
      let value;
      try{value=JSON.parse(body);}catch{
        if(!response.ok)throw new Error(read?`请求失败（HTTP ${response.status}），请稍后重试。`:`服务返回 HTTP ${response.status}，操作结果尚未确认，请先核对记录或运行状态。`);
        throw new Error(read?"服务响应不是有效的 JSON，请重试读取。":"服务响应无法读取，操作结果尚未确认，请先核对记录或运行状态。");
      }
      if(!response.ok)throw new Error(typeof value?.error==="string"?value.error:read?`请求失败（HTTP ${response.status}），请稍后重试。`:`服务返回 HTTP ${response.status}，操作结果尚未确认，请先核对记录或运行状态。`);
      if(!value||typeof value!=="object")throw new Error(read?"服务响应格式无效，请重试读取。":"服务响应格式无效，操作结果尚未确认，请先核对记录或运行状态。");
      return value;
    })();
    // Bound the entire read, including a stalled response body. Writes have no
    // client deadline or replay: abandoning a response cannot undo a commit.
    try{
      if(!read)return await operation;
      return await Promise.race([operation,new Promise((_,reject)=>{
        timer=setTimeout(()=>{reject(new Error("读取超时，请重试；已保存记录不受影响。"));controller.abort();},timeoutMs);
      })]);
    }finally{clearTimeout(timer);}
  }
  const api={request,readTimeout};
  if(typeof module!=="undefined"&&module.exports)module.exports=api;else root.DltTransport=api;
})(typeof globalThis!=="undefined"?globalThis:this);
