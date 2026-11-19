// Run the original prekey-session transition with synthetic storage and crypto.
const assert = require('assert/strict');
const fs = require('fs');
const path = require('path');
const vm = require('vm');
const source = fs.readFileSync(path.resolve(__dirname, '../../../../pc-dist/libs/libsignal-protocol.static.js'), 'utf8');
const start = source.indexOf('processPreKey: function(e, t) {');
const end = source.indexOf('\n                processV3:', start);
assert(start >= 0 && end > start, 'Original SessionBuilder anchors changed');
const method = source.slice(start, end).trim().replace(/,$/, '');

async function run({timestamp = 100, requested = 100, trusted = true, signature = true}) {
    let archived = 0;
    const writes = [];
    const old = {registrationId: 77, indexInfo: {closed: -1}};
    const record = {
        getOpenSession: () => old,
        archiveCurrentState() { archived++; old.indexInfo.closed = 999; },
        updateSessionState(value) { this.added = value; },
        serialize() { return JSON.stringify({old, added: this.added}); },
    };
    const builder = vm.runInNewContext('({' + method + '})', {
        Promise, Date: {now: () => 999},
        Internal: {
            SessionLock: {queueJobForNumber: (_, work) => work()},
            crypto: {
                Ed25519Verify: async () => { if (!signature) throw Error('Invalid signature'); },
                createKeyPair: async () => ({pubKey: 'fresh-base-key'}),
            },
            SessionRecord: {deserialize: () => record},
        },
    });
    Object.assign(builder, {
        remoteAddress: {toString: () => '22.0', getName: () => '22'},
        logger: {info() {}},
        initSession: async () => ({registrationId: 77, indexInfo: {closed: -1, remoteIdentityKey: 'same-identity'}}),
        storage: {
            Direction: {SENDING: 1}, isTrustedIdentity: async () => trusted,
            loadSession: async () => 'existing-session', getSessionTs: () => timestamp,
            storeSession: async (_, bytes) => writes.push(JSON.parse(bytes)),
            saveIdentity: async () => {},
        },
    });
    let error;
    try {
        await builder.processPreKey({identityKey: 'same-identity', registrationId: 77,
            signedPreKey: {publicKey: 'signed-key', signature: 'signature', keyId: 78}}, requested);
    } catch (failure) { error = failure.message; }
    return {archived, writes, error};
}
(async () => {
    const equal = await run({});
    assert.equal(equal.archived, 1, 'Same trusted timestamp must replace the open session');
    assert.equal(equal.writes[0].old.indexInfo.closed, 999, 'The old session is retained closed');
    assert.equal(equal.writes[0].added.indexInfo.closed, -1, 'The fresh session becomes open');
    const older = await run({timestamp: 101});
    assert.equal(older.archived, 0);
    assert.equal(older.writes[0].added.indexInfo.closed, 999, 'A genuinely older prekey cannot replace the open session');
    for (const input of [{trusted: false}, {signature: false}]) {
        const rejected = await run(input);
        assert(rejected.error);
        assert.equal(rejected.archived, 0);
        assert.equal(rejected.writes.length, 0, 'Trust/signature failures leave sessions untouched');
    }
    console.log('Original session renewal oracle: equal/older timestamps and trust/signature refusal pass');
})().catch(error => { console.error(error); process.exitCode = 1; });
