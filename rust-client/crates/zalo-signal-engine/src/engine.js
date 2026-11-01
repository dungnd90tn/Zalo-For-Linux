'use strict';
globalThis.__engineExecute=async function(request){
    const fail=code=>{const error=new Error(code);error.engineCode=code;throw error};
    const ensure=(value,code='invalid_input')=>{if(!value)fail(code)};
    const binary=value=>{ensure(Array.isArray(value)&&value.length<=131072&&value.every(x=>Number.isInteger(x)&&x>=0&&x<=255),'invalid_record');return Uint8Array.from(value).buffer};
    const array=value=>Array.from(new Uint8Array(value));
    const equal=(a,b)=>a.length===b.length&&a.every((x,i)=>x===b[i]);
    const pair=value=>{ensure(value&&Array.isArray(value.publicKey)&&value.publicKey.length===33&&value.publicKey[0]===5&&Array.isArray(value.privateKey)&&value.privateKey.length===32,'invalid_record');return{pubKey:binary(value.publicKey),privKey:binary(value.privateKey)}};
    const packPair=value=>({publicKey:array(value.pubKey),privateKey:array(value.privKey)});
    const platform=value=>({Mobile:0,Web:1,Pc:2})[value];
    function token(key){
        const local=key.local.epoch+':'+platform(key.local.platform);
        const peer=value=>value.uid+':'+platform(value.platform)+':'+value.epoch;
        switch(key.kind){
            case'local_identity':return'L:'+local;
            case'pre_key':return'P:'+local+':'+key.id;
            case'signed_pre_key':return'K:'+local+':'+key.id;
            case'session':return'S:'+local+':'+peer(key.peer);
            case'peer_identity':return'I:'+local+':'+peer(key.peer);
            case'session_timestamp':return'T:'+local+':'+peer(key.peer);
            case'group_sender':return'G:'+local+':'+key.group+':'+peer(key.sender);
            default:fail('invalid_record');
        }
    }
    function safe(value,depth=0){
        ensure(depth<=64,'invalid_record');
        if(value&&typeof value==='object')for(const key of Object.keys(value)){ensure(!['__proto__','constructor','prototype'].includes(key),'invalid_record');safe(value[key],depth+1)}
    }
    const records=new Map,changes=new Map;
    for(const entry of request.records){safe(entry.value);const id=token(entry.key);ensure(!records.has(id),'invalid_record');records.set(id,entry)}
    const key=(kind,extra={})=>({kind,local:request.local,...extra});
    const get=key=>records.get(token(key))?.value;
    const put=(key,value)=>{safe(value);const entry={key,value};records.set(token(key),entry);changes.set(token(key),{put:entry})};
    const remove=key=>{records.delete(token(key));changes.set(token(key),{delete:key})};
    const localKey=key('local_identity');
    const local=()=>{const value=get(localKey);ensure(value&&Number.isInteger(value.registrationId)&&value.registrationId>=0&&value.registrationId<=16383,'missing_record');pair(value);return value};
    const trust=request.trust;
    function approved(){
        ensure(trust&&trust.peer&&trust.identityKey.length===33&&trust.identityKey[0]===5&&trust.registrationId>=0&&trust.registrationId<=16383,'trust_rejected');
        const old=get(key('peer_identity',{peer:trust.peer}));
        if(old)ensure(old.registrationId===trust.registrationId&&equal(old.identityKey,trust.identityKey),'identity_changed');
        return trust;
    }
    const address=()=>new libsignal.SignalProtocolAddress(trust.peer.uid,platform(trust.peer.platform));
    function store(){
        approved();
        const sessionKey=key('session',{peer:trust.peer}),identityKey=key('peer_identity',{peer:trust.peer}),tsKey=key('session_timestamp',{peer:trust.peer});
        const expected=trust.peer.uid+'.'+platform(trust.peer.platform);
        const checkName=name=>ensure(name===expected||name===trust.peer.uid,'trust_rejected');
        return{
            Direction:{SENDING:1,RECEIVING:2},
            async getIdentityKeyPair(){return pair(local())},
            async getLocalRegistrationId(){return local().registrationId},
            getSessionTs(name){checkName(name);const value=get(tsKey);if(value!==undefined){ensure(Number.isSafeInteger(value)&&value>=0,'invalid_record');return value}return undefined},
            async loadSession(name){checkName(name);const value=get(sessionKey);if(value===undefined)return undefined;ensure(typeof value==='string'&&value.length<=1048576,'invalid_record');safe(JSON.parse(value));return value},
            async storeSession(name,value){checkName(name);ensure(typeof value==='string'&&value.length<=1048576,'limit_exceeded');safe(JSON.parse(value));put(sessionKey,value);put(tsKey,Math.max(get(tsKey)||0,trust.sessionTimestamp))},
            async isTrustedIdentity(name,value){checkName(name);return equal(array(typeof value==='string'?dcodeIO.ByteBuffer.wrap(value,'binary').toArrayBuffer():value),trust.identityKey)},
            async saveIdentity(name,value){checkName(name);const actual=typeof value==='string'?array(dcodeIO.ByteBuffer.wrap(value,'binary').toArrayBuffer()):array(value);ensure(equal(actual,trust.identityKey),'identity_changed');put(identityKey,{registrationId:trust.registrationId,identityKey:trust.identityKey,sessionTimestamp:trust.sessionTimestamp,basis:trust.basis});return false},
            async loadPreKey(id){const value=get(key('pre_key',{id}));return value===undefined?undefined:pair(value)},
            async loadSignedPreKey(id){const value=get(key('signed_pre_key',{id}));return value===undefined?undefined:pair(value)},
            async removePreKey(id){remove(key('pre_key',{id}))},
        };
    }
    const prekeyProto=dcodeIO.ProtoBuf.loadProto('message EnginePreKey { optional uint32 preKeyId=1; optional bytes baseKey=2; optional bytes identityKey=3; optional bytes message=4; optional uint32 registrationId=5; optional uint32 signedPreKeyId=6; }').build('EnginePreKey');
    async function decryptPersonal(){
        approved();const raw=binary(request.message.body);
        ensure(raw.byteLength>9&&raw.byteLength<=131072&&new Uint8Array(raw)[0]===0x33,'invalid_ciphertext');
        if(request.message.messageType==='PreKey'){
            const header=prekeyProto.decode(raw.slice(1));
            ensure(header.registrationId===trust.registrationId&&equal(array(header.identityKey.toArrayBuffer()),trust.identityKey),'trust_rejected');
        }
        const cipher=new libsignal.SessionCipher(store(),address());
        const plaintext=request.message.messageType==='PreKey'?await cipher.decryptPreKeyWhisperMessage(raw,undefined,request.messageTimestamp):await cipher.decryptWhisperMessage(raw,undefined,request.messageTimestamp);
        ensure(plaintext.byteLength<=65536,'limit_exceeded');return plaintext;
    }
    function senderRecord(sender){return key('group_sender',{group:request.group,sender})}
    function groupName(sender){return{toString:()=>request.group+'::'+sender.uid+'::'+platform(sender.platform)}}
    function groupStore(sender,identity,registrationId){
        const recordKey=senderRecord(sender);
        return{
            async loadSenderKey(){
                const saved=get(recordKey);if(!saved)return new __Group.Record;
                ensure(saved.registrationId===registrationId&&equal(saved.identityKey,identity),'identity_changed');
                const record=new __Group.Record(__Group.Record.deserialize(binary(saved.record)).senderKeyStates);
                // Correct the original wrapper's falsey-ID shortcut: sender ID0
                // is a valid wire value and must select exact matching states.
                record.getSenderKeyStates=function(id){return id===undefined||id===null?this.senderKeyStates.slice(0,1):this.senderKeyStates.filter(state=>state.getKeyId()===id)};
                return record;
            },
            storeSenderKey(name,record){put(recordKey,{registrationId,identityKey:identity,record:array(record.serialize()),distributionTimestamp:get(recordKey)?.distributionTimestamp||0})},
        };
    }
    try{
        let output;
        switch(request.operation){
            case'create_bundle':{
                ensure(!get(localKey),'already_provisioned');
                const identity=await libsignal.KeyHelper.generateIdentityKeyPair();
                const registrationId=libsignal.KeyHelper.generateRegistrationId();
                const signed=await libsignal.KeyHelper.generateSignedPreKey(identity,registrationId+1);
                put(localKey,{...packPair(identity),registrationId});
                put(key('signed_pre_key',{id:signed.keyId}),packPair(signed.keyPair));
                const prekeys=[];
                for(let index=0;index<request.prekeyCount;index++){const item=await libsignal.KeyHelper.generatePreKey(registrationId+3+index);put(key('pre_key',{id:item.keyId}),packPair(item.keyPair));prekeys.push([item.keyId,array(item.keyPair.pubKey)])}
                output={kind:'bundle',registrationId,identityKey:array(identity.pubKey),signedPrekeyId:signed.keyId,signedPrekey:array(signed.keyPair.pubKey),signature:array(signed.signature),prekeys};break;
            }
            case'generate_prekeys':{
                const identity=local(),prekeys=[];
                for(const id of request.reservedIds)ensure(get(key('pre_key',{id}))===undefined,'already_provisioned');
                for(const id of request.reservedIds){const item=await libsignal.KeyHelper.generatePreKey(id);put(key('pre_key',{id}),packPair(item.keyPair));prekeys.push([id,array(item.keyPair.pubKey)])}
                output={kind:'prekeys',registrationId:identity.registrationId,prekeys};break;
            }
            case'process_bundle':{
                approved();const bundle=request.bundle;
                ensure(bundle.registrationId===trust.registrationId&&equal(bundle.identityKey,trust.identityKey),'trust_rejected');
                const value={registrationId:bundle.registrationId,identityKey:binary(bundle.identityKey),signedPreKey:{keyId:bundle.signedPrekeyId,publicKey:binary(bundle.signedPrekey),signature:binary(bundle.signature)}};
                if(bundle.prekey)value.preKey={keyId:bundle.prekey[0],publicKey:binary(bundle.prekey[1])};
                await new libsignal.SessionBuilder(store(),address()).processPreKey(value,trust.sessionTimestamp);output={kind:'session_ready'};break;
            }
            case'encrypt_personal':{
                approved();const value=await new libsignal.SessionCipher(store(),address()).encrypt(binary(request.plaintext));
                ensure(value.registrationId===trust.registrationId,'trust_rejected');
                output={kind:'personal_encrypted',messageType:value.type===3?'PreKey':'Whisper',body:array(dcodeIO.ByteBuffer.wrap(value.body,'binary').toArrayBuffer()),remoteRegistrationId:value.registrationId};break;
            }
            case'decrypt_personal':output={kind:'plaintext',body:array(await decryptPersonal())};break;
            case'create_group_distribution':{
                const identity=local(),sender=request.sender;
                const storage=groupStore(sender,identity.publicKey,identity.registrationId),record=await storage.loadSenderKey();
                if(record.isEmpty()){
                    const id=crypto.getRandomValues(new Uint16Array(1))[0];
                    record.setSenderKeyState(id,0,crypto.getRandomValues(new Uint8Array(32)).buffer,libsignal.Curve.generateKeyPair());storage.storeSenderKey(groupName(sender),record);
                }
                const state=record.getSenderKeyStates()[0];ensure(state,'missing_record');
                output={kind:'group_distribution',body:array(new __Group.Distribution(null,state.getKeyId(),state.getSenderChainKey().getIteration(),state.getSenderChainKey().getSeed(),state.getSigningKeyPublic()).serialize())};break;
            }
            case'process_group_distribution':{
                const plaintext=await decryptPersonal(),distribution=new __Group.Distribution(plaintext);
                ensure(distribution.serialize().byteLength>1&&new Uint8Array(plaintext)[0]===0x33,'invalid_ciphertext');
                ensure(distribution.getChainKey().byteLength===32&&distribution.getSignatureKey().byteLength===33,'invalid_ciphertext');
                const storage=groupStore(trust.peer,trust.identityKey,trust.registrationId),record=await storage.loadSenderKey();
                const previous=get(senderRecord(trust.peer));
                ensure(!previous||request.messageTimestamp>=previous.distributionTimestamp,'stale_operation');
                record.addSenderKeyState(distribution.getId(),distribution.getIteration(),distribution.getChainKey(),distribution.getSignatureKey());
                storage.storeSenderKey(groupName(trust.peer),record);
                const updated=get(senderRecord(trust.peer));put(senderRecord(trust.peer),{...updated,distributionTimestamp:request.messageTimestamp});output={kind:'group_ready'};break;
            }
            case'encrypt_group':{
                const identity=local(),storage=groupStore(request.sender,identity.publicKey,identity.registrationId);
                ensure(get(senderRecord(request.sender)),'missing_record');
                output={kind:'group_encrypted',body:array(await new __Group.Cipher(storage,groupName(request.sender)).encrypt(binary(request.plaintext)))};break;
            }
            case'decrypt_group':{
                approved();const raw=binary(request.ciphertext);ensure(raw.byteLength>65&&new Uint8Array(raw)[0]===0x33,'invalid_ciphertext');
                const storage=groupStore(trust.peer,trust.identityKey,trust.registrationId);
                const record=await storage.loadSenderKey(),message=new __Group.Message(raw);
                const states=record.getSenderKeyStates(message.getKeyId());ensure(states.length,'missing_record');
                let valid=false;
                for(const state of states)try{await libsignal.Curve.async.verifySignature(state.getSigningKeyPublic(),raw.slice(0,raw.byteLength-64),raw.slice(raw.byteLength-64));valid=true;break}catch(error){}
                ensure(valid,'invalid_ciphertext');
                // Authenticity is established above, before GroupCipher can advance
                // or save any chain. Its original unsafe check is not trusted.
                const plaintext=await new __Group.Cipher(storage,groupName(trust.peer)).decrypt(raw);
                ensure(plaintext.byteLength<=65536,'limit_exceeded');output={kind:'plaintext',body:array(plaintext)};break;
            }
            default:fail('invalid_input');
        }
        return JSON.stringify({account:request.account,generation:request.generation,operationId:request.operationId,expectedRevision:request.expectedRevision,output,delta:Array.from(changes.values())});
    }catch(error){
        let code=error.engineCode;
        if(!code){const message=String(error?.message||'');code=error?.name==='MessageCounterError'||message==='old-counter'?'replay':/Identity key changed|Unknown identity/.test(message)?'identity_changed':/No record|No session|Missing Signed|missing-state|missing-key/.test(message)?'missing_record':/future|future-counter/.test(message)?'limit_exceeded':/signature|MAC|version|decrypt|Invalid/.test(message)?'invalid_ciphertext':'runtime_failure'}
        return JSON.stringify({error:code});
    }
};
