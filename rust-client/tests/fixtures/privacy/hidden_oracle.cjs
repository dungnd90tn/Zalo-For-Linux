// Execute original readers with synthetic inputs only. No manager, storage,
// account, request, PIN update/reset or remote connection is started.
const assert = require('assert/strict');
const crypto = require('crypto');
const fs = require('fs');
const path = require('path');
const vm = require('vm');

const root = path.resolve(__dirname, '../../../..');
const source = fs.readFileSync(path.join(root, 'pc-dist/compact-app-pc.e08d0d44f38873747a6b.js'), 'utf8');
function method(name, next) {
    const marker = '                ' + name + '(e) {';
    const start = source.indexOf(marker);
    const end = source.indexOf('\n                ' + next, start);
    assert(start >= 0 && end > start, 'original hidden method anchors changed');
    const body = source.slice(start + marker.length, end);
    return '(function(e){' + body.slice(0, body.lastIndexOf('}')) + '})';
}
const environment = {
    s: {GROUPID_PREFIX: 'g'},
    r: {default: {
        valueValid: value => value !== null && value !== undefined,
        md5: value => crypto.createHash('md5').update(value, 'utf8').digest('hex'),
    }},
};
const replaceHidden = vm.runInNewContext(method('_updateAllDataHidden', '_clearAllDataHidden()'), environment);
const verifyPin = vm.runInNewContext(method('isKeyPIN', 'handleRemoveGroupInCacheHidden('), environment);
assert(source.includes('commandId: 10048,\n                        path: "/api/hiddenconvers/get-all"'));
const snapshots = [];
for (const threads of [[], [{thread_id: '12', is_group: false}, {thread_id: '12', is_group: true}, {thread_id: '90', is_group: true}]]) {
    const store = {
        _threadsHidden: {obsolete: 1},
        _addItemHidden(id) { this._threadsHidden[id] = 1; },
        _saveIdsHiddenToLocalStorage() {},
    };
    assert(replaceHidden.call(store, threads));
    snapshots.push({threads, original_ids: Object.keys(store._threadsHidden).sort()});
}
const syntheticDigest = crypto.createHash('md5').update('1234').digest('hex');
const pinCases = [];
for (const pin of ['1234', '0000', '', '123', '12345', null]) {
    pinCases.push({pin, accepted: verifyPin.call({getPIN: () => syntheticDigest}, pin)});
}
assert(pinCases[0].accepted && pinCases.slice(1).every(test => !test.accepted));
const output = JSON.stringify({
    provenance: 'Exact original _updateAllDataHidden and isKeyPIN methods; synthetic 1234 PIN only; no original persistence/request code executed. Native fail-closed parsing, four-ASCII-digit input and leases are separate policy tests.',
    endpoint: '/api/hiddenconvers/get-all',
    snapshots,
    synthetic_pin_digest: syntheticDigest,
    pin_cases: pinCases,
}, null, 2) + '\n';
const destination = path.join(__dirname, 'hidden.json');
if (process.argv.includes('--write')) fs.writeFileSync(destination, output);
else assert.equal(fs.readFileSync(destination, 'utf8'), output);
console.log('Original hidden-chat oracle: ' + snapshots.length + ' replacement snapshots, ' + pinCases.length + ' synthetic PIN cases');
