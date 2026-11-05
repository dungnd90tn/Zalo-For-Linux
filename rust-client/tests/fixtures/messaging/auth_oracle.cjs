// Execute the repository's exact original socket AUTH decoder, offline only.
const fs=require('fs'),path=require('path'),vm=require('vm'),assert=require('assert/strict');
const crypto=require('crypto'),zlib=require('zlib');
const root=path.resolve(__dirname,'../../../..');
const worker=fs.readFileSync(path.join(root,'pc-dist/lazy/default-login-main-startup-shared-worker-znotification.9e3e92e88644da772301.js'),'utf8');
const vendor=fs.readFileSync(path.join(root,'pc-dist/lazy/vendors-login-main-startup-shared-worker-znotification.9115fa5ca1824a42c293.js'),'utf8');
const between=(source,start,end)=>{const a=source.indexOf(start),b=source.indexOf(end,a+start.length);assert(a>=0&&b>a);return source.slice(a+start.length,b);};
const pako={};vm.runInNewContext('(function(e,t,r){'+between(vendor,'        rP9a: function(e, t, r) {','\n        },\n        rSVQ:')+'})')({},pako,()=>{throw Error('unexpected pako dependency')});
const classBody=between(worker,'            n.a = class {\n                constructor() {','\n        },\n        "0/Mm":');
const klass=vm.runInNewContext('(class { constructor() {'+classBody+')',{
 window:{atob:x=>Buffer.from(x,'base64').toString('binary')},TextDecoder,
 W:e=>new TextDecoder().decode(new Uint8Array(e)),Q:e=>Uint8Array.from(Buffer.from(e,'base64')),
 M:{a:{}},P:{k:{AUTHEN:1,PING:2},f:[4],e:new Proxy({},{get:(_,p)=>p}),i:{INTERNAL:1},a:{NOT_AUTHENTICATED:1},d:{RESPONSE:1}},
 A:{d:{verifyResponseData(){},getSrcRequest:()=>({SOCKET:1})}},i:pako,k:crypto.webcrypto,
 V:0,j:1,G:2,B:3,z:true,clearTimeout,Date,Uint8Array,DataView,ArrayBuffer,decodeURIComponent,
 m:{b:{onServerPong(){}}},c:{default:{increaseSuccess(){}}},d:{a:{now:()=>0}},
});
const authenticatedHandler=vm.runInNewContext('({onAuthenticated(e) { if (this.initSignal(),'+between(worker,'                onAuthenticated(e) {\n                    if (this.initSignal(),','\n                beforeClose(e) {').trim()+'})',{
 w:{a:e=>e&&Object.hasOwn(e,'cmd')&&Object.hasOwn(e,'subCmd')},m:{a:{init(){}}},S:{a:{instance:()=>({sendVerify(){}})}},b:{e:{INVALID_GET_OFFLINE:5010}}
});
const key=Buffer.from(Array.from({length:32},(_,i)=>i));
const inner={error_code:0,data:{qCmds:[{cmd:510,subCmd:0},{cmd:511,subCmd:0}]}};
function envelope(mode,compression='zlib',payload=inner){
 let bytes=Buffer.from(JSON.stringify(payload));if(mode!==3)bytes=compression==='gzip'?zlib.gzipSync(bytes):zlib.deflateSync(bytes);
 if(mode!==1){const iv=Buffer.from(Array.from({length:16},(_,i)=>i+1)),aad=Buffer.from(Array.from({length:16},(_,i)=>i+17));const aes=crypto.createCipheriv('aes-256-gcm',key,iv);aes.setAAD(aad);bytes=Buffer.concat([iv,aad,aes.update(bytes),aes.final(),aes.getAuthTag()]);}
 return {error_code:0,key:key.toString('base64'),encrypt:mode,data:mode===1?bytes.toString('base64'):encodeURIComponent(bytes.toString('base64'))};
}
async function execute(outer,version){
 const socket=Object.create(klass.prototype),errors=[];
 Object.assign(socket,{_authenticated:false,_cipherKey:null,_pendingSend:[],_pendingReceive:[],_startAt:null,
 logStaging(){},logError(){},_submitSentry(){},_handleError:e=>errors.push(e),_onDataError:(...args)=>errors.push(args[1]),onErrorRecovery(){},_doAfterAuth:data=>{socket.auth=JSON.parse(JSON.stringify(data));socket.queueAccepted=authenticatedHandler.onAuthenticated.call({initSignal(){},E2eeQueueOfflineController:{init(){},reorderQueueCmdToRequestPushOffline:x=>x},offlineCmds:new Map(),checkSwapToBUQueue(){},onMessage:false,logger:{zsymb(){}},socket:{_handleError:e=>errors.push(e)}},data)},_onData(){}});
 const frame=Buffer.concat([Buffer.from([version,1,0,0]),Buffer.from(JSON.stringify(outer))]);
 socket._processEventBlob(frame.buffer.slice(frame.byteOffset,frame.byteOffset+frame.byteLength));
 for(let i=0;i<200&&!socket._authenticated&&!errors.length;i++)await new Promise(r=>setTimeout(r,1));
 return {authenticated:socket._authenticated,errors,auth:socket.auth,queueAccepted:socket.queueAccepted,frame:frame.toString('base64')};
}
module.exports={worker,between,klass,pako,key,envelope};
if(require.main===module)(async()=>{
 const cases=[];
 for(const [name,version,mode,compression] of [['zlib_v1',1,1,'zlib'],['gzip_v0',0,1,'gzip'],['gzip_v2',2,1,'gzip'],['gcm_gzip_v2',2,2,'gzip'],['gcm_v3',1,3,'none']]){
  const result=await execute(envelope(mode,compression),version);assert(result.authenticated&&result.queueAccepted,name);assert.deepEqual(JSON.parse(JSON.stringify(result.auth)),inner);cases.push({name,frame:result.frame,expected:inner});
 }
 for(const outer of [{...envelope(1),key:undefined},envelope(1,'zlib',{error_code:102,data:{}}),{...envelope(1),error_code:102},{error_code:0,key:key.toString('base64'),encrypt:0,data:inner}])assert.equal((await execute(outer,1)).authenticated,false);
 const queueFailures=[];
 for(const [name,payload] of [['missing', {error_code:0,data:{}}],['empty',{error_code:0,data:{qCmds:[]}}],['wrong_type',{error_code:0,data:{qCmds:{}}}]]){
  const result=await execute(envelope(1,'gzip',payload),2);assert.equal(result.authenticated,true);assert.equal(result.queueAccepted,false);queueFailures.push({name,frame:result.frame});
 }
 const output=JSON.stringify({queueFailures,provenance:'exact original _processEventBlob/_onRawData/_decrypt/_decompress/_processAuthData and bundled pako; synthetic key only',cases},null,2)+'\n';
 if(process.argv.includes('--write'))fs.writeFileSync(path.join(__dirname,'auth_frames.json'),output);else assert.equal(fs.readFileSync(path.join(__dirname,'auth_frames.json'),'utf8'),output);
 console.log('Original AUTH oracle: 5 positive header/compression cases, 4 rejected authentication cases, 3 rejected queue schemas');
})().catch(e=>{console.error(e);process.exitCode=1});
