'use strict';
// Same trusted workload as QuickJS; Node remains an offline comparison oracle.
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm');
const crypto=require('node:crypto'),{performance}=require('node:perf_hooks');
const root=path.resolve(__dirname,'../../..');
const started=performance.now();
const context={crypto:crypto.webcrypto,console:{log(){},warn(){},error(){}},
    setTimeout(){throw new Error('timers unavailable in isolated engine')},clearTimeout(){},
    ArrayBuffer,Uint8Array,Uint16Array,Uint32Array,Int8Array,Int16Array,Int32Array,
    Float32Array,Float64Array,DataView,TextEncoder,TextDecoder,Promise};
context.window=context;context.__now=()=>performance.now();
context.__engine=`node-${process.version}/v8-${process.versions.v8}`;
vm.createContext(context,{codeGeneration:{strings:false,wasm:false}});
const library=fs.readFileSync(path.join(root,'pc-dist/libs/libsignal-protocol.static.js'),'utf8');
if(crypto.createHash('sha256').update(library).digest('hex')!=='abbb3f578ede0d16c57b792a1c1793eb221dda07cf6f94919a3e76ee8a047966')throw new Error('trusted script digest mismatch');
vm.runInContext(library,context,{timeout:10000});
(function loadGroup(){
const groupSource=fs.readFileSync(path.join(root,'pc-dist/lazy/default-login-main-startup-shared-worker-znotification.9e3e92e88644da772301.js'),'utf8');
const markers=[['            class Ot {','            var wt = new class {'],['            class Ft extends St {','            const Xt ='],['            class Jt {','            var Zt = Jt,'],['            class en {','            var tn = en;']];
const snippets=markers.map(([start,end])=>{const offset=groupSource.indexOf(start);if(offset<0||groupSource.indexOf(start,offset+1)!==-1)throw new Error('source anchor');const finish=groupSource.indexOf(end,offset);if(finish<=offset)throw new Error('end anchor');return groupSource.slice(offset,finish)});
vm.runInContext(`(function(){const i={a:Object.assign},Xt={zsymb(){}};const wt={queueJob(name,job){return Promise.resolve().then(job)}};class we extends Error{constructor(code){super(code);this.code=code}}const Me={RECEIVE_MESSAGE_WITH_OLD_COUNTER:'old-counter',OVER_MESSAGE_KEYS:'future-counter',SENDER_KEY_STATE_NOT_FOUND:'missing-state',NO_SENKER_KEY_FOUND:'missing-key'};${snippets.join('\n')};globalThis.__Group={Chain:Qt,Message:xt,Cipher:Jt,Record:en};})()`,context,{timeout:10000});
})();
if(typeof global.gc==='function')global.gc();
context.__coldMs=performance.now()-started;
context.__idleRssKiB=Math.ceil(process.memoryUsage().rss/1024);
context.__jsHeapBytes=process.memoryUsage().heapUsed;
context.__fixtureJson=fs.readFileSync(path.join(root,'rust-client/tests/fixtures/e2ee/session.json'),'utf8');
context.__vectorsJson=fs.readFileSync(path.join(root,'rust-client/tests/fixtures/e2ee/vectors.json'),'utf8');
vm.runInContext(fs.readFileSync(path.join(__dirname,'src/workload.js'),'utf8'),context,{timeout:10000});
const rejections=[];process.on('unhandledRejection',error=>rejections.push(error));
const timeout=setTimeout(()=>{process.stderr.write('oracle deadline exceeded\n');process.exit(2)},20000);
vm.runInContext('runWorkload(JSON.parse(__fixtureJson),JSON.parse(__vectorsJson))',context,{timeout:10000}).then(async result=>{
    await new Promise(resolve=>setImmediate(resolve));
    for(const error of rejections)if(!/Message key not found|Bad MAC|storage-failure|Identity key changed/.test(error.message))throw error;
    clearTimeout(timeout);process.stdout.write(result+'\n');
}).catch(error=>{clearTimeout(timeout);console.error(error);process.exitCode=1});
