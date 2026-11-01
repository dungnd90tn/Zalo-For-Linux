'use strict';
async function runWorkload(fixture, vectors) {
    globalThis.__phase='hkdf';
    let checks = 0;
    const timings = {};
    const data = value => Uint8Array.from(value.match(/../g).map(x=>parseInt(x,16))).buffer;
    const hex = value => typeof value === 'string' ? Array.from(value, x=>x.charCodeAt(0).toString(16).padStart(2,'0')).join('') : Array.from(new Uint8Array(value), x=>x.toString(16).padStart(2,'0')).join('');
    const utf8 = value => dcodeIO.ByteBuffer.wrap(value,'utf8').toArrayBuffer();
    const text = value => dcodeIO.ByteBuffer.wrap(value).toString('utf8');
    function assert(value, message) { if (!value) throw new Error(message); }
    async function rejects(job, reason) { let rejected=false; try { await job(); } catch(error) { rejected=true; if(reason) assert(String(error).includes(reason), `unexpected rejection: ${reason}`); } assert(rejected, 'expected rejection'); }
    for (const entry of vectors.hkdf) {
        const actual = await libsignal.HKDF.deriveSecrets(data(entry.ikm), data(entry.salt), entry.label);
        assert(actual.map(hex).join('') === entry.output, 'HKDF independent fixture mismatch'); checks++;
    }
    const started = __now();
    globalThis.__phase='known-answer';
    const identity = value => ({pubKey:data(fixture[value+'_public']),privKey:data(fixture[value+'_private'])});
    class Store {
        constructor(key, id) { this.identity=key;this.id=id;this.sessions=new Map;this.identities=new Map;this.prekeys=new Map;this.signed=new Map;this.Direction={SENDING:1,RECEIVING:2};this.fail=false; }
        async getIdentityKeyPair(){return this.identity}
        async getLocalRegistrationId(){return this.id}
        getSessionTs(){return undefined}
        async loadSession(name){return this.sessions.get(name)}
        async storeSession(name,value){if(this.fail)throw new Error('storage-failure');this.sessions.set(name,value)}
        async isTrustedIdentity(name,key){const old=this.identities.get(name.split('.')[0]);return old===undefined||old===hex(key)}
        async saveIdentity(name,key){assert(await this.isTrustedIdentity(name,key),'identity-change');this.identities.set(name.split('.')[0],hex(key));return false}
        async loadPreKey(id){return this.prekeys.get(id)}
        async loadSignedPreKey(id){return this.signed.get(id)}
        async removePreKey(id){this.prekeys.delete(id)}
    }
    const bobFixture = new Store(identity('bob_identity'),2002);
    bobFixture.signed.set(7,identity('bob_signed_prekey'));
    bobFixture.prekeys.set(9,identity('bob_one_time'));
    const aliceAddress = new libsignal.SignalProtocolAddress('alice',2);
    const bobAddress = new libsignal.SignalProtocolAddress('bob',2);
    const fixtureCipher = new libsignal.SessionCipher(bobFixture,aliceAddress);
    const rawFixture = data(fixture.initial_prekey_message);
    assert(text(await fixtureCipher.decryptPreKeyWhisperMessage(rawFixture,undefined,100))===fixture.plaintext,'independent prekey fixture');
    assert(!bobFixture.prekeys.has(9),'one-time key consumption');checks++;
    timings.known_answer_ms=__now()-started;

    const randomStart=__now();
    globalThis.__phase='identity-generation';
    const keyA=await libsignal.KeyHelper.generateIdentityKeyPair();
    const keyB=await libsignal.KeyHelper.generateIdentityKeyPair();
    assert(hex(keyA.privKey)!==hex(keyB.privKey) && hex(keyA.privKey)!==fixture.alice_identity_private,'CSPRNG identities must differ');checks++;
    timings.two_identity_ms=__now()-randomStart;
    const signedStart=__now();
    globalThis.__phase='signed-prekey';
    const signed=await libsignal.KeyHelper.generateSignedPreKey(keyB,17);
    const oneTime=await libsignal.KeyHelper.generatePreKey(19);
    await libsignal.Curve.async.verifySignature(keyB.pubKey,signed.keyPair.pubKey,signed.signature);
    const corruptSignature=Uint8Array.from(new Uint8Array(signed.signature));corruptSignature[0]^=1;
    await rejects(()=>libsignal.Curve.async.verifySignature(keyB.pubKey,signed.keyPair.pubKey,corruptSignature.buffer));checks++;
    timings.signed_prekey_verify_ms=__now()-signedStart;
    const aStore=new Store(keyA,1001),bStore=new Store(keyB,2002);
    bStore.signed.set(17,signed.keyPair);bStore.prekeys.set(19,oneTime.keyPair);
    const bundle={registrationId:2002,identityKey:keyB.pubKey,signedPreKey:{keyId:17,publicKey:signed.keyPair.pubKey,signature:signed.signature},preKey:{keyId:19,publicKey:oneTime.keyPair.pubKey}};
    const sessionStart=__now();
    globalThis.__phase='session-establishment';
    await new libsignal.SessionBuilder(aStore,bobAddress).processPreKey(bundle,200);
    const a=new libsignal.SessionCipher(aStore,bobAddress),b=new libsignal.SessionCipher(bStore,aliceAddress);
    const hello=await a.encrypt(utf8('Xin chào public session'));
    assert(hello.type===3,'prekey type');
    assert(text(await b.decryptPreKeyWhisperMessage(hello.body,'binary',200))==='Xin chào public session','session establish');checks++;
    const reply=await b.encrypt(utf8('Phản hồi public reply'));
    assert(text(await a.decryptWhisperMessage(reply.body,'binary',201))==='Phản hồi public reply','bidirectional session');checks++;
    timings.session_establish_ms=__now()-sessionStart;

    const pending=[];
    globalThis.__phase='personal-edge-cases';
    for(let index=0;index<4;index++)pending.push(await a.encrypt(utf8(`out of order ${index}`)));
    for(const index of [3,0,2,1])assert(text(await b.decryptWhisperMessage(pending[index].body,'binary',210+index))===`out of order ${index}`,'out of order');checks++;
    const beforeReplay=bStore.sessions.get('alice.2');
    await rejects(()=>b.decryptWhisperMessage(pending[1].body,'binary',211),'Message key not found');
    assert(bStore.sessions.get('alice.2')===beforeReplay,'replay changed committed snapshot');checks++;
    const tamper=await a.encrypt(utf8('tamper protected'));
    const corrupt=new Uint8Array(dcodeIO.ByteBuffer.wrap(tamper.body,'binary').toArrayBuffer());corrupt[corrupt.length-1]^=1;
    await rejects(()=>b.decryptWhisperMessage(corrupt.buffer,undefined,220));
    assert(text(await b.decryptWhisperMessage(tamper.body,'binary',220))==='tamper protected','MAC failure lost valid ratchet');checks++;
    const restarted=new Store(keyB,2002);restarted.sessions=new Map(JSON.parse(JSON.stringify([...bStore.sessions])));restarted.identities=new Map(bStore.identities);
    const bRestarted=new libsignal.SessionCipher(restarted,aliceAddress);
    const storage=await a.encrypt(utf8('retry storage'));
    const beforeFailure=restarted.sessions.get('alice.2');restarted.fail=true;
    await rejects(()=>bRestarted.decryptWhisperMessage(storage.body,'binary',221),'storage-failure');
    assert(restarted.sessions.get('alice.2')===beforeFailure,'storage failure committed a ratchet');restarted.fail=false;
    assert(text(await bRestarted.decryptWhisperMessage(storage.body,'binary',221))==='retry storage','serialized restart/storage retry');checks++;
    const badBundle={...bundle,identityKey:keyA.pubKey};
    await rejects(()=>new libsignal.SessionBuilder(aStore,bobAddress).processPreKey(badBundle,222),'Identity key changed');checks++;

    const messageCount=50,payload='m'.repeat(1024);
    const messageStart=__now();
    globalThis.__phase='personal-benchmark';
    for(let index=0;index<messageCount;index++){
        const encrypted=await a.encrypt(utf8(payload));
        assert(text(await bRestarted.decryptWhisperMessage(encrypted.body,'binary',230+index))===payload,'message benchmark plaintext');
    }
    timings.personal_1k_roundtrip_ms=(__now()-messageStart)/messageCount;
    const maximumText='x'.repeat(65536);
    const maximumEncrypted=await a.encrypt(utf8(maximumText));
    assert(text(await bRestarted.decryptWhisperMessage(maximumEncrypted.body,'binary',300))===maximumText,'64KiB domain text plus framing');checks++;
    const snapshotStart=__now();
    for(let index=0;index<100;index++)JSON.parse(JSON.stringify([...restarted.sessions]));
    timings.snapshot_serialize_ms=(__now()-snapshotStart)/100;

    const groupStart=__now(),g=__Group;
    globalThis.__phase='group-edge-cases';
    const chain=new g.Chain(0,data(vectors.group.chain_seed));
    const key=await chain.getSenderMessageKey();
    assert(hex(key.getIv())===vectors.group.iv && hex(key.getCipherKey())===vectors.group.cipher_key,'independent group KDF');checks++;
    const sender=new g.Record;sender.setSenderKeyState(77,0,data(vectors.group.chain_seed),keyA);
    const receiver=new g.Record;receiver.addSenderKeyState(77,0,data(vectors.group.chain_seed),keyA.pubKey);
    function groupStore(record){return {snapshot:record.serialize(),loads:0,fail:false,async loadSenderKey(){this.loads++;return new g.Record(g.Record.deserialize(this.snapshot).senderKeyStates)},storeSenderKey(name,next){if(this.fail)throw new Error('group-storage-failure');this.snapshot=next.serialize()}}}
    const gs=groupStore(sender),gr=groupStore(receiver),name={toString:()=> 'public-group::alice::2'};
    const groupCipher=new g.Cipher(gs,name),groupReceiver=new g.Cipher(gr,name);
    async function decryptGroup(wire){assert(wire.byteLength>65&&new Uint8Array(wire)[0]===0x33,'group envelope');await libsignal.Curve.async.verifySignature(keyA.pubKey,wire.slice(0,wire.byteLength-64),wire.slice(wire.byteLength-64));return groupReceiver.decrypt(wire)}
    const groupHello=await groupCipher.encrypt(utf8('public group hello'));
    const forged=Uint8Array.from(new Uint8Array(groupHello));forged[forged.length-1]^=1;
    const beforeForged=hex(gr.snapshot);await rejects(()=>decryptGroup(forged.buffer));
    assert(gr.loads===0&&hex(gr.snapshot)===beforeForged,'forged group touched ratchet');checks++;
    assert(text(await decryptGroup(groupHello))==='public group hello','valid group decrypt');checks++;
    const gp=[];for(let index=0;index<4;index++)gp.push(await groupCipher.encrypt(utf8(`group order ${index}`)));
    for(const index of [3,0,2,1])assert(text(await decryptGroup(gp[index]))===`group order ${index}`,'group out of order');checks++;
    const beforeGroupReplay=hex(gr.snapshot);await rejects(()=>decryptGroup(gp[1]),'old-counter');assert(hex(gr.snapshot)===beforeGroupReplay,'group replay committed');checks++;
    const groupStorage=await groupCipher.encrypt(utf8('group storage retry'));
    gr.fail=true;await rejects(()=>decryptGroup(groupStorage),'group-storage-failure');gr.fail=false;
    assert(text(await decryptGroup(groupStorage))==='group storage retry','group storage rollback/retry');checks++;
    timings.group_setup_checks_ms=__now()-groupStart;
    const groupMessageStart=__now();
    globalThis.__phase='group-benchmark';
    for(let index=0;index<20;index++){const encrypted=await groupCipher.encrypt(utf8(payload));assert(text(await decryptGroup(encrypted))===payload,'group benchmark plaintext')}
    timings.group_1k_roundtrip_ms=(__now()-groupMessageStart)/20;
    const maximumGroup=await groupCipher.encrypt(utf8(maximumText));
    assert(text(await decryptGroup(maximumGroup))===maximumText,'64KiB group domain text plus framing');checks++;
    const totalWorkload=__now()-started;
    if(globalThis.__nativeCurveMode){
        globalThis.__phase='native-asm-cross-verification';
        const crossStart=__now(),asm=__AsmCurve;
        const seed=data('77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a');
        const known=await libsignal.Curve.async.createKeyPair(seed);
        assert(hex(known.pubKey)==='058520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a','native RFC7748 public');checks++;
        const bobKnown=await libsignal.Curve.async.createKeyPair(data('5dab087e624a8a4b79e17f8b83800ee66f3bb1292618b6fd1c2f8b27ff88e0eb'));
        const expectedSecret='4a5d9d5ba4ce2de1728e3bf480350f25e07e21c947d19e3376f09b3c1e161742';
        assert(hex(await libsignal.Curve.async.calculateAgreement(bobKnown.pubKey,known.privKey))===expectedSecret,'native RFC7748 agreement');
        assert(hex(asm.sharedSecret(bobKnown.pubKey.slice(1),known.privKey))===expectedSecret,'asm RFC7748 agreement');checks++;
        for(const key of [known,keyA,keyB]){
            const message=utf8('cross-engine public XEdDSA fixture');
            const generated=await libsignal.Curve.async.calculateSignature(key.privKey,message);
            assert(asm.verify(key.pubKey.slice(1),message,generated)===false,'native signature rejected by original asm');checks++;
            const existing=asm.sign(key.privKey,message);
            await libsignal.Curve.async.verifySignature(key.pubKey,message,existing);checks++;
            const corrupt=Uint8Array.from(new Uint8Array(existing));corrupt[0]^=1;
            await rejects(()=>libsignal.Curve.async.verifySignature(key.pubKey,message,corrupt.buffer));
            assert(asm.verify(key.pubKey.slice(1),message,corrupt.buffer)===true,'asm accepted tampered signature');checks++;
        }
        await rejects(()=>libsignal.Curve.async.calculateAgreement(new Uint8Array(33).buffer,known.privKey));checks++;
        const lowOrder=new Uint8Array(33);lowOrder[0]=5;
        await rejects(()=>libsignal.Curve.async.calculateAgreement(lowOrder.buffer,known.privKey));checks++;
        await rejects(()=>libsignal.Curve.async.verifySignature(known.pubKey,utf8('x'),new Uint8Array(63).buffer));checks++;
        timings.native_asm_crosscheck_ms=__now()-crossStart;
    }
    return JSON.stringify({engine:__engine,cold_load_ms:__coldMs,idle_rss_kib:__idleRssKiB,js_heap_bytes:__jsHeapBytes,checks,timings,total_workload_ms:totalWorkload});
}
