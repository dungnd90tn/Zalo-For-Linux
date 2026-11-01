// Offline execution of exact original post-AUTH raw/decrypt/decompress/dispatch methods.
const fs=require('fs'),path=require('path'),vm=require('vm'),assert=require('assert/strict');
const {worker,between,klass,key,envelope}=require('./auth_oracle.cjs');
const dispatch=vm.runInNewContext('({async _onData(e,t,n,a) {'+between(worker,'                async _onData(e, t, n, a) {','\n                _onLightweightData(e, t, n) {').trim()+',_onLightweightData(e,t,n) {const a=this._getDataHanlder(e,t);'+between(worker,'                _onLightweightData(e, t, n) {\n                    const a = this._getDataHanlder(e, t);','\n                _doAfterAuth(e) {').trim()+'})',{_:{d:{INNER_PAYLOAD:2},c:{CANNOT_PARSE:1}}});
const message={error_code:0,data:{msgs:[{uidFrom:'22',idTo:'11',msgId:'903',cliMsgId:'1700000000001',ts:'1700000000001',msgType:'webchat',content:'Synthetic encrypted transport message'}]}};
const api={error_code:0,data:{reqId:'signal_req_1',data:{err:0,hasMore:false,threads:[],uids:[],waitUids:[],pendingUids:[]}}};
async function run(command,outer,sub=0){
 const socket=Object.create(klass.prototype),data=[],errors=[];let lightweight=false;
 const handler={preParseData:JSON.parse,onData:(cmd,sub,payload)=>data.push({cmd,sub,payload})};
 Object.assign(socket,{_authenticated:true,_cipherKey:key.toString('base64'),_lastPing:1700000000000,_pingTimer:1,_lastPong:0,
 _pendingReceive:[],logStaging(){},logError(){},_submitSentry(){},_handleError:e=>errors.push(e),_onDataError:(_,code)=>errors.push(code??null),onErrorRecovery(){},
 _getDataHanlder:()=>handler,_onData:dispatch._onData,_onLightweightData(...args){lightweight=true;dispatch._onLightweightData.apply(this,args)}});
 const frame=Buffer.concat([Buffer.from([2,command&255,command>>8,sub]),Buffer.from(JSON.stringify(outer))]);
 socket._processEventBlob(frame.buffer.slice(frame.byteOffset,frame.byteOffset+frame.byteLength));
 for(let i=0;i<20;i++)await new Promise(r=>setTimeout(r,1));
 return {frame:frame.toString('base64'),route:socket._lastPong?'pong':data.length?lightweight?'lightweight':'data':'rejected',payload:data[0]?.payload??null,hasError:errors.length>0};
}
(async()=>{
 const cases=[];
 for(const status of ['absent',null,0]){
  const outer={eventId:1700000000000};if(status!=='absent')outer.error_code=status;
  const result=await run(2,outer,1);assert.equal(result.route,'pong');assert.equal(result.hasError,false);cases.push({name:'pong_'+status,...result});
  const active={data:{reqId:'cmd_4',at:1700000000000}};if(status!=='absent')active.error_code=status;
  const light=await run(4,active);assert.equal(light.route,'lightweight');assert.equal(light.hasError,false);cases.push({name:'active_'+status,...light});
 }
 for(const [name,cmd,inner,mode,compression] of [['personal',501,message,2,'gzip'],['group',521,{error_code:0,data:{groupMsgs:[]}},2,'gzip'],['control',601,{error_code:0,data:{controls:[]}},3,'none'],['capability_plain',563,api,0,'none'],['capability_gzip',563,api,1,'gzip']]){
  const outer=mode===0?{error_code:0,encrypt:0,data:JSON.stringify(inner)}:envelope(mode,compression,inner);
  delete outer.key; // Only AUTH distributes a key; post-AUTH uses the committed socket key.
  const result=await run(cmd,outer);assert.equal(result.route,'data',name);assert.equal(result.hasError,false);assert.deepEqual(JSON.parse(JSON.stringify(result.payload)),inner);cases.push({name,...result});
 }
 for(let i=1;i<=3;i++){
  const inner=JSON.parse(JSON.stringify(message));inner.data.msgs[0].msgId=String(902+i);inner.data.msgs[0].cliMsgId=String(1700000000010+i);
  const outer=envelope(2,'gzip',inner);delete outer.key;const result=await run(501,outer);assert.equal(result.route,'data');cases.push({name:'after_heartbeat_'+i,...result});
 }
 for(const [name,outer] of [['missing',{encrypt:0,data:JSON.stringify(message)}],['null',{error_code:null,encrypt:0,data:JSON.stringify(message)}],['outer_error',{error_code:7,encrypt:0,data:JSON.stringify(message)}],['inner_error',{error_code:0,encrypt:0,data:JSON.stringify({error_code:7,data:{msgs:[]}})}]]){
  const result=await run(501,outer);assert.equal(result.route,'rejected',name);cases.push({name:'invalid_message_'+name,...result});
 }
 const output=JSON.stringify({provenance:'exact own socket raw/decrypt/decompress and subclass _onData/_onLightweightData, bundled Pako; synthetic fixed key',cases},null,2)+'\n';
 if(process.argv.includes('--write'))fs.writeFileSync(path.join(__dirname,'post_auth_frames.json'),output);else assert.equal(fs.readFileSync(path.join(__dirname,'post_auth_frames.json'),'utf8'),output);
 console.log('Original post-AUTH oracle: '+cases.length+' control/API/encrypted-message acceptance and rejection cases');
})().catch(e=>{console.error(e);process.exitCode=1});
