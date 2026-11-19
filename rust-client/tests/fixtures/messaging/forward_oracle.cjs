// Execute the original ordinary direct text forwarding builders with synthetic IDs.
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const source = fs.readFileSync(path.resolve(__dirname,
    '../../../../pc-dist/lazy/default-login-main-startup-shared-worker-znotification.9e3e92e88644da772301.js'), 'utf8');

function section(start, end, offset = 0) {
    const from = source.indexOf(start, offset);
    const to = source.indexOf(end, from);
    assert(from >= 0 && to > from, 'Missing original source anchor: ' + start);
    return source.slice(from, to);
}

const groupPrefix = source.match(/\bje = "([^"]+)",/)[1];
const types = vm.runInNewContext('(' + section('fo = {\n                    TEXT:', 'Io = 100,').trim()
    .replace(/^fo = /, '').replace(/,$/, '') + ')');
const constants = {};
const exportsOf = (text, name, exports) => {
    const define = (target, key, getter) => Object.defineProperty(target, key, {get: getter});
    vm.runInNewContext('({' + text.trim().replace(/,$/, '') + '})')[name]({}, exports, {d: define});
};
exportsOf(section('Ubns: function(', 'UcEE: function('), 'Ubns', constants);

const refModule = source.indexOf('e2PW: function(');
const refs = vm.runInNewContext('(() => { const ' +
    section('h = e => o.default.md5(e)', '            function f(t)', refModule) +
    'return {p, E}; })()', {
    s: {a: Object.assign}, i: constants, o: {default: {
        md5: value => crypto.createHash('md5').update(value).digest('hex'),
    }},
    r: {a: id => id.replace(/^g/, ''), b: () => false},
    l: {ModuleContainer: {resolve: () => ({getSession: () => ({userId: '11'})})}},
    c: {a: {}}, d: {GROUPID_PREFIX: groupPrefix}, A: {q: {isCommunity: () => false}},
    u: {default: {sendToMeId: '999'}},
});

const ttlModule = source.indexOf('iiq4: function(');
const ttlHelpers = vm.runInNewContext('(() => { const ' +
    section('c = (e = []) => e.map', '                A = (e, t = {}) => {', ttlModule)
        .trim().replace(/,$/, ';') + 'return {e:c, f:d}; })()', {
    a: {a: Object.assign}, i: {a: id => id === '44' ? 3600 : 0},
});
const methods = section('static transformForwardMessage(', 'static getImportantMsg(');
const getRefParam = section('static getFwAdditionalParams(e) {', 'static getPhoneNumberPro(e) {');
const RefHelper = vm.runInNewContext('(class {' + getRefParam + '})', {G: constants});
const Api = vm.runInNewContext('(class {' + methods + '})', {
    a: {a: Object.assign},
    y: ttlHelpers,
    r: {GROUPID_PREFIX: groupPrefix},
    D: {a: {getZaloClientID: () => 'synthetic-device'}},
    i: {default: {
        encodeAES: text => text,
        getFwAdditionalParams: RefHelper.getFwAdditionalParams,
    }},
    _: {a: {FILE: 'file'}},
    s: {default: {multifw_timeout_per_uid: 10000, multifw_min_timeout: 60000}},
});
Api._getCommonParams = () => 'zpw_ver=681&zpw_type=24';
Api.transformParams = (_name, clientId) => assert.equal(clientId, null);
Api._postSendMsg = (endpoint, form, _retry, metric) => ({
    endpoint, payload: JSON.parse(decodeURIComponent(form.slice('params='.length))), metric,
});

const base = {
    msgId: '9001', cliMsgId: '8001', sendDttm: 1700000000123,
    message: 'Chuyển tiếp tiếng Việt\n"nội dung" 😀',
    fromUid: '22', toUid: '22',
};
const cases = [
    ['incoming_dm', base, '33', true],
    ['own_dm', {...base, fromUid: '0'}, '33', true],
    ['incoming_group_to_dm', {...base, fromUid: '55', toUid: groupPrefix + '77'}, '33', true],
    ['missing_source_client_id', {...base, cliMsgId: undefined}, '33', false],
    ['ephemeral_destination', base, '44', false],
].map(([name, original, toUid, native_supported]) => {
    const forwardRefLog = refs.p(original, {send2Me: false});
    const decorLog = refs.E(forwardRefLog);
    const wire = Api.forwardMultiMessage([{clientId: '7001', toUid}], types.TEXT, {
        message: original.message,
    }, false, null, {forwardRefLog, decorLog});
    assert.equal(wire.endpoint.path, '/api/message/mforward');
    assert.equal(wire.endpoint.domainType, 'file');
    assert.equal(wire.payload.msgType, '1');
    assert.equal(wire.payload.totalIds, 1);
    assert.equal(wire.payload.ttl, 0);
    assert.equal(wire.payload.toIds[0].ttl, toUid === '44' ? 3600 : 0);
    assert.equal('reference' in JSON.parse(wire.payload.msgInfo), Boolean(original.cliMsgId));
    return {name, original, toUid, native_supported, wire};
});
const output = JSON.stringify({
    provenance: 'Original forwardMultiMessage, reference/decor and TTL builders with synthetic IDs, text and identity AES. Fixtures exercise inner request shape only.',
    cases,
}, null, 2) + '\n';
const destination = path.join(__dirname, 'forward.json');
if (process.argv.includes('--write')) fs.writeFileSync(destination, output);
else assert.equal(fs.readFileSync(destination, 'utf8'), output);
console.log('Original ordinary text forward oracle: ' + cases.length + ' cases');
