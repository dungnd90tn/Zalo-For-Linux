'use strict';

// OFFLINE RESEARCH ONLY. This VM is not a production security boundary. The
// subprocess has a memory limit and a watchdog; its host owns all file access.
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
// The existing SessionLock creates an unobserved cleanup promise on rejection.
// Record this source behavior explicitly; do not alter the original library.
const cleanupRejections = [];
process.on('unhandledRejection', reason => cleanupRejections.push(reason));
const root = path.resolve(__dirname, '../../../..');
const fixture = JSON.parse(fs.readFileSync(path.join(__dirname, 'vectors.json')));
const sourcePath = path.join(root, 'pc-dist/libs/libsignal-protocol.static.js');
const source = fs.readFileSync(sourcePath, 'utf8');
const workerSource = fs.readFileSync(path.join(root, 'pc-dist/lazy/default-login-main-startup-shared-worker-znotification.9e3e92e88644da772301.js'), 'utf8');
const watchdog = setTimeout(() => { process.stderr.write('E2EE fixture deadline exceeded\n'); process.exit(2); }, 20000);
watchdog.unref();

function bytes(value) { return Uint8Array.from(Buffer.from(value, 'hex')).buffer; }
function hex(value) { return Buffer.from(value, typeof value === 'string' ? 'binary' : undefined).toString('hex'); }
let randomCounter = 0;
const context = {
    // Deterministic randomness is exclusively for PUBLIC SYNTHETIC fixtures.
    crypto: { subtle: crypto.webcrypto.subtle, getRandomValues(target) {
        const raw = new Uint8Array(target.buffer, target.byteOffset, target.byteLength);
        for (let offset = 0; offset < raw.length; offset += 32) {
            const block = crypto.createHash('sha256').update(`zalo-public-e2ee-fixture-${randomCounter++}`).digest();
            raw.set(block.subarray(0, Math.min(32, raw.length - offset)), offset);
        }
        return target;
    } },
    console: { log() {}, warn() {}, error() {} }, setTimeout, clearTimeout,
    ArrayBuffer, Uint8Array, Uint16Array, Uint32Array, Int8Array, Int16Array,
    Int32Array, Float32Array, Float64Array, DataView, TextEncoder, TextDecoder, Promise,
};
context.window = context;
vm.createContext(context, { codeGeneration: { strings: false, wasm: false } });
vm.runInContext(source, context, { timeout: 10000, filename: 'existing-zalo-libsignal.js' });
const signal = context.libsignal;

function originalGroupClasses() {
    function slice(start, end) {
        const offset = workerSource.indexOf(start);
        assert(offset >= 0 && workerSource.indexOf(start, offset + 1) === -1, 'unique original source anchor');
        const finish = workerSource.indexOf(end, offset);
        assert(finish > offset, 'original source end anchor');
        return workerSource.slice(offset, finish);
    }
    const snippets = [
        slice('            class Ot {', '            var wt = new class {'),
        slice('            class Ft extends St {', '            const Xt ='),
        slice('            class Jt {', '            var Zt = Jt,'),
        slice('            class en {', '            var tn = en;'),
    ];
    // Only logging, object spread, error tagging and a serial fixture scheduler
    // are supplied. Crypto/ratchet/message implementations are untouched slices.
    return vm.runInContext(`(function() {
        const i = { a: Object.assign }, Xt = { zsymb() {} };
        const wt = { queueJob(name, job) { return Promise.resolve().then(job); } };
        class we extends Error { constructor(code) { super(code); this.code = code; } }
        const Me = { RECEIVE_MESSAGE_WITH_OLD_COUNTER: 'old-counter', OVER_MESSAGE_KEYS: 'future-counter', SENDER_KEY_STATE_NOT_FOUND: 'missing-state', NO_SENKER_KEY_FOUND: 'missing-key' };
        ${snippets.join('\n')}
        return { Chain: Qt, Message: xt, Cipher: Jt, Record: en };
    })()`, context, { timeout: 10000, filename: 'existing-zalo-group-extract.js' });
}

class Store {
    constructor(identity, registrationId) {
        this.identity = identity;
        this.registrationId = registrationId;
        this.sessions = new Map();
        this.prekeys = new Map();
        this.signedPrekeys = new Map();
        this.identities = new Map();
        this.sessionTimes = new Map();
        this.Direction = { SENDING: 1, RECEIVING: 2 };
        this.failWrites = false;
    }
    async getIdentityKeyPair() { return this.identity; }
    async getLocalRegistrationId() { return this.registrationId; }
    getSessionTs(name) { return this.sessionTimes.get(name); }
    async loadSession(name) { return this.sessions.get(name); }
    async storeSession(name, value) {
        if (this.failWrites) throw new Error('synthetic storage failure');
        this.sessions.set(name, value);
    }
    async isTrustedIdentity(name, key) {
        const previous = this.identities.get(name.split('.')[0]);
        return previous === undefined || previous === hex(key);
    }
    async saveIdentity(name, key) {
        assert(await this.isTrustedIdentity(name, key), 'fixture identity change requires explicit approval');
        this.identities.set(name.split('.')[0], hex(key));
        return false;
    }
    async loadPreKey(id) { return this.prekeys.get(id); }
    async loadSignedPreKey(id) { return this.signedPrekeys.get(id); }
    async removePreKey(id) { this.prekeys.delete(id); }
}

async function main() {
    let checks = 0;
    for (const entry of fixture.hkdf) {
        const result = await signal.HKDF.deriveSecrets(bytes(entry.ikm), bytes(entry.salt), entry.label);
        assert.equal(result.map(hex).join(''), entry.output);
        checks++;
    }
    assert.notEqual(fixture.hkdf[2].output, fixture.hkdf[3].output, 'stock Signal labels are incompatible');
    checks++;

    const group = originalGroupClasses();
    const groupChain = new group.Chain(0, bytes(fixture.group.chain_seed));
    const groupKey = await groupChain.getSenderMessageKey();
    assert.equal(hex(groupKey.getIv()), fixture.group.iv);
    assert.equal(hex(groupKey.getCipherKey()), fixture.group.cipher_key);
    assert.equal(hex((await groupChain.getNext()).getSeed()), fixture.group.next_chain);
    checks++;

    const groupSigning = signal.Curve.createKeyPair(new Uint8Array(32).fill(8).buffer);
    const groupRecord = new group.Record();
    groupRecord.setSenderKeyState(77, 0, bytes(fixture.group.chain_seed), groupSigning);
    const groupStore = { async loadSenderKey() { return groupRecord; }, storeSenderKey() {} };
    const groupWire = await new group.Cipher(groupStore, { toString: () => 'public-group::alice::2' }).encrypt(bytes(Buffer.from('public group fixture').toString('hex')));
    const signatureBody = groupWire.slice(0, groupWire.byteLength - 64);
    const signature = groupWire.slice(groupWire.byteLength - 64);
    await signal.Curve.async.verifySignature(groupSigning.pubKey, signatureBody, signature);
    checks++;
    const forgedGroup = Uint8Array.from(new Uint8Array(groupWire));
    forgedGroup[forgedGroup.length - 1] ^= 1;
    await assert.rejects(signal.Curve.async.verifySignature(groupSigning.pubKey,
        forgedGroup.buffer.slice(0, forgedGroup.length - 64), forgedGroup.buffer.slice(forgedGroup.length - 64)));
    // Explicit security regression in the bundled group wrapper: its sync Curve
    // return convention is inverted and the signed slice includes a signature
    // byte. Preserve evidence, NEVER enable this wrapper as a production engine.
    assert.doesNotThrow(() => new group.Message(forgedGroup.buffer).verifySignature(groupSigning.pubKey));
    checks++;
    const receiverRecord = new group.Record();
    receiverRecord.addSenderKeyState(77, 0, bytes(fixture.group.chain_seed), groupSigning.pubKey);
    let committedGroup = receiverRecord.serialize();
    let groupLoads = 0;
    const receiverStore = {
        async loadSenderKey() {
            groupLoads++;
            return new group.Record(group.Record.deserialize(committedGroup).senderKeyStates);
        },
        storeSenderKey(name, record) { committedGroup = record.serialize(); },
    };
    async function verifiedGroupDecrypt(wire) {
        assert(wire.byteLength > 65 && new Uint8Array(wire)[0] === 0x33);
        await signal.Curve.async.verifySignature(groupSigning.pubKey,
            wire.slice(0, wire.byteLength - 64), wire.slice(wire.byteLength - 64));
        return new group.Cipher(receiverStore, { toString: () => 'public-group::alice::2' }).decrypt(wire);
    }
    const beforeForgedGroup = hex(committedGroup);
    await assert.rejects(verifiedGroupDecrypt(forgedGroup.buffer));
    assert.equal(groupLoads, 0, 'invalid signature rejected before loading/mutating a ratchet');
    assert.equal(hex(committedGroup), beforeForgedGroup);
    assert.equal(Buffer.from(await verifiedGroupDecrypt(groupWire)).toString(), 'public group fixture');
    checks++;

    // RFC7748 section 6.1: independent published X25519 vector, with Zalo tag 05.
    const alicePrivate = bytes('77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a');
    const alice = await signal.KeyHelper.generateIdentityKeyPair(alicePrivate);
    assert.equal(hex(alice.pubKey), '058520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a');
    checks++;
    const bob = await signal.KeyHelper.generateIdentityKeyPair(bytes('5dab087e624a8a4b79e17f8b83800ee66f3bb1292618b6fd1c2f8b27ff88e0eb'));
    const secret = await signal.Curve.async.calculateAgreement(bob.pubKey, alice.privKey);
    assert.equal(hex(secret), '4a5d9d5ba4ce2de1728e3bf480350f25e07e21c947d19e3376f09b3c1e161742');
    checks++;

    const aliceStore = new Store(alice, 1001);
    const bobStore = new Store(bob, 2002);
    const signed = await signal.KeyHelper.generateSignedPreKey(bob, 7);
    const prekey = await signal.KeyHelper.generatePreKey(9);
    bobStore.signedPrekeys.set(7, signed.keyPair);
    bobStore.prekeys.set(9, prekey.keyPair);
    const bobAddress = new signal.SignalProtocolAddress('bob', 2);
    const aliceAddress = new signal.SignalProtocolAddress('alice', 2);
    const bundle = { registrationId: 2002, identityKey: bob.pubKey,
        signedPreKey: { keyId: 7, publicKey: signed.keyPair.pubKey, signature: signed.signature },
        preKey: { keyId: 9, publicKey: prekey.keyPair.pubKey } };
    await new signal.SessionBuilder(aliceStore, bobAddress).processPreKey(bundle, 100);
    const a = new signal.SessionCipher(aliceStore, bobAddress);
    const b = new signal.SessionCipher(bobStore, aliceAddress);
    const initial = await a.encrypt(bytes(Buffer.from('public synthetic hello').toString('hex')));
    assert.equal(initial.type, 3);
    const initialPlain = await b.decryptPreKeyWhisperMessage(initial.body, 'binary', 100);
    assert.equal(Buffer.from(initialPlain).toString(), 'public synthetic hello');
    assert(!bobStore.prekeys.has(9), 'successful prekey decrypt consumes one-time prekey');
    checks++;
    const reply = await b.encrypt(bytes(Buffer.from('public synthetic reply').toString('hex')));
    assert.equal(reply.type, 1);
    assert.equal(Buffer.from(await a.decryptWhisperMessage(reply.body, 'binary', 101)).toString(), 'public synthetic reply');
    checks++;

    const pending = [];
    for (let index = 0; index < 4; index++) pending.push(await a.encrypt(bytes(Buffer.from(`out of order ${index}`).toString('hex'))));
    for (const index of [3, 0, 2, 1]) {
        assert.equal(Buffer.from(await b.decryptWhisperMessage(pending[index].body, 'binary', 102 + index)).toString(), `out of order ${index}`);
    }
    checks++;
    const beforeReplay = bobStore.sessions.get('alice.2');
    await assert.rejects(b.decryptWhisperMessage(pending[1].body, 'binary', 103), error => error.name === 'MessageCounterError');
    assert.equal(bobStore.sessions.get('alice.2'), beforeReplay, 'replay must not commit a changed ratchet');
    checks++;

    const clean = await a.encrypt(bytes(Buffer.from('tamper protected').toString('hex')));
    const corrupt = Buffer.from(clean.body, 'binary');
    corrupt[corrupt.length - 1] ^= 1;
    await assert.rejects(b.decryptWhisperMessage(corrupt.toString('binary'), 'binary', 110));
    assert.equal(Buffer.from(await b.decryptWhisperMessage(clean.body, 'binary', 110)).toString(), 'tamper protected');
    checks++;

    // Store serialization is an actual persistence boundary: a fresh cipher uses
    // only reloaded JSON records, never the previous cipher's object cache.
    const restarted = new Store(bob, 2002);
    restarted.sessions = new Map(JSON.parse(JSON.stringify([...bobStore.sessions])));
    restarted.identities = new Map(bobStore.identities);
    const restartedCipher = new signal.SessionCipher(restarted, aliceAddress);
    const afterRestart = await a.encrypt(bytes(Buffer.from('after restart').toString('hex')));
    assert.equal(Buffer.from(await restartedCipher.decryptWhisperMessage(afterRestart.body, 'binary', 111)).toString(), 'after restart');
    checks++;

    const beforeFailure = restarted.sessions.get('alice.2');
    const retry = await a.encrypt(bytes(Buffer.from('retry after storage failure').toString('hex')));
    restarted.failWrites = true;
    await assert.rejects(restartedCipher.decryptWhisperMessage(retry.body, 'binary', 112), /synthetic storage failure/);
    assert.equal(restarted.sessions.get('alice.2'), beforeFailure);
    restarted.failWrites = false;
    assert.equal(Buffer.from(await restartedCipher.decryptWhisperMessage(retry.body, 'binary', 112)).toString(), 'retry after storage failure');
    checks++;

    const wrongIdentity = await signal.KeyHelper.generateIdentityKeyPair(new Uint8Array(32).fill(4).buffer);
    await assert.rejects(new signal.SessionBuilder(aliceStore, bobAddress).processPreKey({ ...bundle, identityKey: wrongIdentity.pubKey }), /Identity key changed/);
    checks++;

    // Export only deliberately public fixture secrets for the independent Python
    // X3DH/MAC/AES oracle. No profile, cookie, server or real account is involved.
    const sessionFixture = {
        description: 'PUBLIC SYNTHETIC KEYS ONLY. Never use fixture RNG or keys in production.',
        library_sha256: crypto.createHash('sha256').update(source).digest('hex'),
        alice_identity_private: hex(alice.privKey), alice_identity_public: hex(alice.pubKey),
        bob_identity_private: hex(bob.privKey), bob_identity_public: hex(bob.pubKey),
        bob_signed_prekey_private: hex(signed.keyPair.privKey), bob_signed_prekey_public: hex(signed.keyPair.pubKey),
        bob_one_time_private: hex(prekey.keyPair.privKey), bob_one_time_public: hex(prekey.keyPair.pubKey),
        initial_prekey_message: Buffer.from(initial.body, 'binary').toString('hex'),
        plaintext: 'public synthetic hello',
    };
    if (process.argv.includes('--write-session')) {
        fs.writeFileSync(path.join(__dirname, 'session.json'), JSON.stringify(sessionFixture, null, 2) + '\n');
    } else {
        assert.deepEqual(sessionFixture, JSON.parse(fs.readFileSync(path.join(__dirname, 'session.json'))));
        checks++;
    }
    await new Promise(resolve => setImmediate(resolve));
    assert(cleanupRejections.length >= 4, 'known library cleanup rejections remain visible');
    for (const error of cleanupRejections) {
        assert(error.name === 'MessageCounterError' || /Bad MAC|synthetic storage failure|Identity key changed/.test(error.message), 'unexpected cleanup rejection');
    }
    checks++;
    process.stdout.write(`PASS ${checks} offline E2EE contract checks; no production network or profiles\n`);
}
main().then(() => clearTimeout(watchdog)).catch(error => { console.error(error); process.exitCode = 1; });
