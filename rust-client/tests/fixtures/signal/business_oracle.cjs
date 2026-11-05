'use strict';
// PUBLIC SYNTHETIC identities/RNG. No network/account/storage access.
// Exercises original business sender -> original SessionCipher -> receiver codec.
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),crypto=require('node:crypto'),assert=require('node:assert/strict');
const root=path.resolve(__dirname,'../../../..');
const source=fs.readFileSync(path.join(root,'pc-dist/lazy/default-login-main-startup-shared-worker-znotification.9e3e92e88644da772301.js'),'utf8');
const helper=fs.readFileSync(path.join(__dirname,'../e2ee/contract_test.cjs'),'utf8');
function slice(start,end){const a=source.indexOf(start);assert(a>=0&&source.indexOf(start,a+1)<0,'unique method');const b=source.indexOf(end,a);assert(b>a);return source.slice(a,b).trim();}
const bytes=x=>Uint8Array.from(Buffer.from(x,'hex')).buffer;
const hex=x=>Buffer.from(x,typeof x==='string'?'binary':undefined).toString('hex');
let counter=0;
const context={crypto:{subtle:crypto.webcrypto.subtle,getRandomValues(target){const raw=new Uint8Array(target.buffer,target.byteOffset,target.byteLength);for(let offset=0;offset<raw.length;offset+=32){const block=crypto.createHash('sha256').update('public-signal-business-'+counter++).digest();raw.set(block.subarray(0,Math.min(32,raw.length-offset)),offset)}return target}},console:{log(){},warn(){},error(){}},setTimeout,clearTimeout,ArrayBuffer,Uint8Array,Uint16Array,Uint32Array,Int8Array,Int16Array,Int32Array,Float32Array,Float64Array,DataView,TextEncoder,TextDecoder,Promise};
context.window=context;vm.createContext(context,{codeGeneration:{strings:false,wasm:false}});
vm.runInContext(fs.readFileSync(path.join(root,'pc-dist/libs/libsignal-protocol.static.js'),'utf8'),context,{timeout:10000});
const signal=context.libsignal;
const Store=vm.runInNewContext('('+helper.slice(helper.indexOf('class Store {'),helper.indexOf('\nasync function main()'))+')',{hex,assert});
const log={zsymb(){}};const tracking={getLogger(){return log},setLogData(){return {}},getLogData(){return {}}};
const senderSource=slice('async sendZText(e, t, n, a, o, r, l, d) {','                async _processAttachQuoteMsg(');
const sender=vm.runInNewContext('({'+senderSource+'})',{y:{a:{getInstance:()=>tracking}},N:{d:{}},D:{a:{once(){}}},M:{a:{},b:{}},w:{c:x=>x},s:{a:Object.assign},A:{i:{VERSION_MSG:{TEXT_MSG:{version:0,compat_version:0}}}},i:{default:{sendMsgE2ee:async(_,records)=>records}}});
// Original ByteBuffer calls backed by Node's UTF-8/binary codec in this oracle.
const ByteBuffer={wrap:function(value,encoding){const b=Buffer.from(value,typeof value==='string'?(encoding==='binary'?'latin1':encoding):undefined);return {buffer:b,toArrayBuffer(){return Uint8Array.from(b).buffer},toString(format){return b.toString(format)}}},concat(parts){const b=Buffer.concat(parts.map(x=>x.buffer));return {toString(format){return b.toString(format)}}}};
const utilSource=slice('utf8toBuffer: function(e) {','                splitTypeAndMsg: function(');
const util=vm.runInNewContext('({'+utilSource+'})',{o:ArrayBuffer.prototype,dcodeIO:{ByteBuffer},Uint8Array});
const encryptSource=slice('async encryptMessageAsync(e, t, n) {','                async decryptMessageAsync(');
const loopSource=slice('async encrypt(e, t, ...n) {','                extractTextContentFromServer(');
const runtime=vm.runInNewContext('({'+encryptSource+','+loopSource+'})',{o:{a:util},on:{a:{SESSION_LOCK:'session'}},ma:log,A:{default:{e2ee:{wait_to_save_db_session_prekey:false}}},K:{GROUPID_PREFIX:'g'},Uint8Array});
const decodeSource=slice('static getTextFromContent(e) {','                        static isRTF(').replace(/^static /,'');
const decode=vm.runInNewContext('({'+decodeSource+'})');
(async()=>{
 const alice=await signal.KeyHelper.generateIdentityKeyPair(bytes('77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a'));
 const bob=await signal.KeyHelper.generateIdentityKeyPair(bytes('5dab087e624a8a4b79e17f8b83800ee66f3bb1292618b6fd1c2f8b27ff88e0eb'));
 const as=new Store(alice,1001),bs=new Store(bob,2002),signed=await signal.KeyHelper.generateSignedPreKey(bob,7),prekey=await signal.KeyHelper.generatePreKey(9);
 bs.signedPrekeys.set(7,signed.keyPair);bs.prekeys.set(9,prekey.keyPair);
 const ba=new signal.SignalProtocolAddress('22',0),aa=new signal.SignalProtocolAddress('11',2);
 await new signal.SessionBuilder(as,ba).processPreKey({registrationId:2002,identityKey:bob.pubKey,signedPreKey:{keyId:7,publicKey:signed.keyPair.pubKey,signature:signed.signature},preKey:{keyId:9,publicKey:prekey.keyPair.pubKey}},100);
 const a=new signal.SessionCipher(as,ba),b=new signal.SessionCipher(bs,aa);let calls=0;
 runtime._createSessionCipher=()=>[a,'22.0'];runtime.Lock={acquire:async(_,__,job)=>job()};runtime._store={markStartEncrypting(){calls++},markEndEncrypting(){},isWaitingSaveSessionPrekeyToDb(){return false}};
 sender.signalProtocolManager={async encryptSendMsg(thread,values){const output=await runtime.encrypt(thread,0,...values);return [{userId:thread,deviceId:0,ciphers:output.ciphers,registrationId:output.registrationId}]}};
 sender._enrichE2eeVersion=(_,version)=>({e2ee:version});sender._enrichAdditionalParams=async x=>x;sender.addE2eeHash=()=>{};sender.buildMsgParams=(_,record)=>record;
 const cases=[{text:'Xin chào 🦀 "\\\n',styles:[]},{text:'Đậm',styles:[{start:0,len:3,st:'b'}]},{text:'x'.repeat(65536),styles:[]}];
 async function decrypt(value){const raw=Buffer.from(value,'base64');return Buffer.from(await (raw[0]===3?b.decryptPreKeyWhisperMessage(raw.subarray(1),'binary',100):b.decryptWhisperMessage(raw.subarray(1),'binary',100))).toString('utf8')}
 const output=[];
 for(let index=0;index<cases.length;index++){
  const input=cases[index],before=calls,records=await sender.sendZText('22',input.text,false,1700000000000+index,0,{styles:input.styles,ver:0},null,{});
  const body=decode.getTextFromContent(records[0].message),plain=await decrypt(body);assert.equal(plain,input.text);
  assert.equal(decode.getTextFromContent({action:'rtf',title:body}),body);
  const properties=records[0].textProperties?await decrypt(records[0].textProperties):null;
  assert.equal(calls-before,input.styles.length?2:1,'empty styles skip encryption');
  if(properties)assert.deepEqual(JSON.parse(properties),{styles:input.styles,ver:0});
  output.push({bytes:Buffer.byteLength(plain),sha256:crypto.createHash('sha256').update(plain).digest('hex'),preview:index<2?plain:null,encrypted_fields:calls-before,properties});
 }
 const result={provenance:'Exact original sendZText/encrypt/encryptMessageAsync/getTextFromContent plus bundled SessionCipher roundtrip; fixture-only Node ByteBuffer UTF8 bridge and synthetic RNG',cases:output};
 const text=JSON.stringify(result,null,2)+'\n',file=path.join(__dirname,'business.json');if(process.argv.includes('--write'))fs.writeFileSync(file,text);else assert.equal(fs.readFileSync(file,'utf8'),text);
 console.log('Original business text/Signal roundtrip: Unicode+escaping, separate styles, full65536byte rawUTF8, no empty-property encryption');
})().catch(e=>{console.error(e.stack);process.exitCode=1});
