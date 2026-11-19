// Execute the original ordinary quote builders with synthetic IDs/content only.
const assert = require('assert/strict');
const fs = require('fs');
const path = require('path');
const vm = require('vm');
const source = fs.readFileSync(path.resolve(__dirname,
    '../../../../pc-dist/lazy/default-login-main-startup-shared-worker-znotification.9e3e92e88644da772301.js'), 'utf8');
const start = source.indexOf('                static transformQuoteMessage(');
const end = source.indexOf('                static sendLinkMessage(', start);
assert(start >= 0 && end > start, 'Original quote builder anchors changed');
const ttlStart = source.indexOf('                d = (e, t = {}) => {', source.indexOf('        iiq4: function('));
const quoteTtlStart = source.indexOf('                A = (e, t = {}) => {', ttlStart);
const ttlEnd = source.indexOf('                u = new Set(', quoteTtlStart);
assert(ttlStart > 0 && quoteTtlStart > ttlStart && ttlEnd > quoteTtlStart, 'Original TTL anchors changed');
const expression = (from, to, prefix) => source.slice(from, to).trim()
    .replace(prefix, '').replace(/,$/, '');
const helpers = {a: {a: Object.assign}};
const ttl = vm.runInNewContext('(' + expression(ttlStart, quoteTtlStart, /^d = /) + ')', helpers);
const quoteTtl = vm.runInNewContext('(' + expression(quoteTtlStart, ttlEnd, /^A = /) + ')', helpers);
const Builder = vm.runInNewContext('(class {' + source.slice(start, end) + '})', {
    r: {MSG_TEXT: 1}, a: {a: Object.assign},
    y: {c: quoteTtl, f: ttl},
    D: {a: {getZaloClientID: () => 'synthetic-device'}},
    N: {a: {getVisibility: () => 0}},
    i: {default: {getGroupIdFromConversationId: id => id.replace(/^group_/, ''), encodeAES: text => text}},
    _: {a: {CHAT: 'chat', GROUP: 'group'}},
    w: {a: {getCurrentLanguageName: () => 'VN'}},
});
Builder._getCommonParams = () => ({zpw_ver: 800, zpw_type: 24});
Builder._postSendMsg = (endpoint, body, retry, metric) => ({
    endpoint, payload: JSON.parse(decodeURIComponent(body.slice('params='.length))), metric,
});
const base = {
    fromUid: '22', msgId: '9001', cliMsgId: '8001', cliMsgType: 1,
    ts: 1700000000123, textParams: ['Tin gốc tiếng Việt\n"trích dẫn" 😀', ''],
};
const inputs = [
    ['direct_text', false, base, null, true],
    ['group_text', true, base, null, true],
    ['direct_missing_client_id', false, {...base, cliMsgId: undefined}, null, false],
    ['group_missing_client_id', true, {...base, cliMsgId: undefined}, null, false],
    ['direct_numeric_ids', false, {...base, fromUid: 22, msgId: 9001, cliMsgId: 8001}, null, true],
    ['group_own_message', true, {...base, fromUid: '11', cliMsgId: '8002'}, null, true],
    ['direct_ephemeral_target', false, {...base, ttl: 60000}, null, false],
    ['group_ttl_options', true, {...base, ttl: 60000}, {ttl: 1000}, false],
];
const cases = inputs.map(([name, group, quote, options, native_supported]) => {
    const output = group
        ? Builder.sendGroupQuoteMessage('group_77', 'Trả lời tổng hợp', 7001, null, quote, 0, null, options)
        : Builder.sendQuoteMessage('22', 'Trả lời tổng hợp', 7001, quote, 0, null, options);
    return {name, group, quote, native_supported, output};
});
assert.equal(cases[0].output.endpoint.path, '/api/message/quote');
assert.equal(cases[1].output.endpoint.path, '/api/group/quote');
assert.equal(cases[0].output.payload.imei, 'synthetic-device');
assert(!('imei' in cases[1].output.payload));
assert(!('qmsgCliId' in cases[2].output.payload));
assert(!('qmsgCliId' in cases[3].output.payload));
assert.equal(cases[0].output.payload.qmsgTTL, 0);
assert.equal(cases[0].output.payload.ttl, 0);
assert.equal(cases[6].output.payload.qmsgTTL, 60000);
assert.equal(cases[7].output.payload.ttl, 1000);
const output = JSON.stringify({
    provenance: 'Exact original ordinary DM/group quote builders, transformQuoteMessage and TTL helpers; synthetic IDs/text, identity AES stand-in. Native deliberately requires real source cliMsgId and rejects ephemeral sources rather than fabricating metadata. Transport/acceptance is not established.',
    cases,
}, null, 2) + '\n';
const destination = path.join(__dirname, 'reply.json');
if (process.argv.includes('--write')) fs.writeFileSync(destination, output);
else assert.equal(fs.readFileSync(destination, 'utf8'), output);
console.log('Original ordinary quote oracle: ' + cases.length + ' cases');
