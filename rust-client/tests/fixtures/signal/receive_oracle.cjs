'use strict';
// Original offline-session dispatch, public synthetic server envelopes only.
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),assert=require('node:assert/strict');
const source=fs.readFileSync(path.resolve(__dirname,'../../../../pc-dist/lazy/default-login-main-startup-shared-worker-znotification.9e3e92e88644da772301.js'),'utf8');
const wire=JSON.parse(fs.readFileSync(path.join(__dirname,'wire.json'),'utf8'));
function method(start,end){const a=source.indexOf(start);assert(a>=0&&source.indexOf(start,a+1)<0,'unique method');const b=source.indexOf(end,a);assert(b>a);return source.slice(a,b).trim();}
const commands={INIT_SESSION:543,INIT_SESSION_MOBILE:545,ACK_INIT_SESSION:544,NOTIFY_DELETE_KEY:548,REQUEST_PREKEYS:549,INIT_SESSION_GROUP:561,DISABLE_E2EE:580,ENABLE_E2EE:581};
const snippet=method('async onReceiveSessionOffline({','                async onReceiveInitSession({');
const dispatched=[];
const adapter=vm.runInNewContext('({'+snippet+'})',{c:{i:{SignalCommands:{INIT:commands}}},ma:{zsymb(){}},console},{timeout:1000});
Object.assign(adapter,{_offlineQueue:[],_preprocessOffline(){},isBeforeNewInit(){return false},updatePendingOff(){},processInitAndAck(e){dispatched.push({kind:'init',event:e})},handleAckInit(e){dispatched.push({kind:'ack',event:e})},handleDeleteKey(e){dispatched.push({kind:'delete',event:e})},handleRequestPrekeys(e,offline){dispatched.push({kind:'prekeys',offline,event:e})}});
function event(id,cmd,extra){return {msgId:String(id),cmd,sessionData:{uidFrom:'22',uidTo:'11',deviceId:0,ts:1700000000000,...extra}};}
const first={error_code:0,data:{more:true,lastActionId:'100',eesession:[event(100,543,{data:wire.prekey})]}};
const last={error_code:0,data:{more:false,lastActionId:'104',eesession:[event(101,544,{type:1,data:wire.personal}),event(102,549,{type:1}),event(103,548,{deleteType:0}),event(104,549,{type:2})]}};
(async()=>{
 await adapter.onReceiveSessionOffline({payload:first});assert.equal(dispatched.length,0,'partial sessions never dispatched');
 await adapter.onReceiveSessionOffline({payload:last});assert.deepEqual(dispatched.map(x=>x.kind),['init','ack','prekeys','delete','prekeys']);assert.equal(adapter._offlineQueue.length,0);
 const group={error_code:0,data:{more:false,lastActionId:'105',eesession:[event(105,561,{threadId:'33',type:2,data:wire.personal})]}};
 const message={msgId:'106',uidFrom:'22',idTo:'11',ts:'1700000000000',msgType:'webchat',content:wire.personal,paramsExt:{platformType:0}};
 const messages={error_code:0,data:{msgs:[message],pageMsgs:[]}};
 const groupMessages={error_code:0,data:{groupMsgs:[{...message,msgId:'107',idTo:'33',content:wire.group}]}};
 const deliveries={error_code:0,data:{delivereds:[{msgId:'106',uid:'22',deviceId:0,err:0}]}};
 const output={provenance:'Original onReceiveSessionOffline dispatch for synthetic envelopes; message/group/delivery field shapes from N404551-404625/N162568/N163012/N167752',first,last,group,messages,groupMessages,deliveries,dispatch:dispatched};
 const text=JSON.stringify(output,null,2)+'\n',file=path.join(__dirname,'receive.json');
 if(process.argv.includes('--write'))fs.writeFileSync(file,text);else assert.equal(fs.readFileSync(file,'utf8'),text);
 console.log('Original session queue oracle: partial-page hold and five ordered session events; synthetic group/message/delivery descriptors');
})().catch(e=>{console.error(e.message);process.exitCode=1});
