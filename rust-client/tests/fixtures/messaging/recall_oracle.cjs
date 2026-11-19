// Execute the original ordinary recall request builder with synthetic IDs.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const source = fs.readFileSync(path.resolve(__dirname,
    '../../../../pc-dist/lazy/default-login-main-startup-shared-worker-znotification.9e3e92e88644da772301.js'), 'utf8');
const start = source.indexOf('static undoMessage(e, t, n, a) {');
const end = source.indexOf('static undoE2eeMessage(', start);
assert(start !== -1 && end > start, 'Original recall anchors changed');

const Api = vm.runInNewContext('(class {' + source.slice(start, end) + '})', {
    r: {GROUPID_PREFIX: 'g'},
    _: {b: {
        getChatDomain: () => 'https://chat.example.test',
        getGroupDomain: () => 'https://group.example.test',
    }},
    i: {default: {
        encodeAES: text => text,
        getGroupIdFromConversationId: thread => thread.slice(1),
    }},
    N: {a: {getVisibility: () => 0}},
    D: {a: {getZaloClientID: () => 'synthetic-device'}},
});
Api._getCommonParams = () => 'zpw_ver=681&zpw_type=24';
Api._post = (url, body, _request, metric) => {
    const endpoint = new URL(url);
    return {
        path: endpoint.pathname,
        host: endpoint.hostname,
        query: Object.fromEntries(endpoint.searchParams),
        payload: JSON.parse(new URLSearchParams(body).get('params')),
        metric,
    };
};

const direct = [
    {name: 'direct_own_text', peer:'22', msgId:'9001', clientId:7001, cliMsgIdUndo:'8001'},
    {name: 'direct_large_exact_message_id', peer:'22', msgId:'900719925474099123', clientId:7002, cliMsgIdUndo:'8002'},
].map(item => ({
    ...item,
    request: Api.undoMessage(item.msgId, item.peer, item.clientId, item.cliMsgIdUndo),
}));
for (const {request} of direct) {
    assert.equal(request.path, '/api/message/undo');
    assert.equal(request.metric, 11281);
    assert.equal(request.host, 'chat.example.test');
    assert(!('imei' in request.payload) && !('visibility' in request.payload));
}
const group = Api.undoMessage('9001', 'g77', 7001, '8001');
assert.equal(group.path, '/api/group/undomsg');
assert.equal(group.payload.imei, 'synthetic-device');

const output = JSON.stringify({
    provenance: 'Original undoMessage request builder with synthetic IDs and identity AES. Group method shown for contrast; native scope is ordinary direct text.',
    direct, group,
}, null, 2) + '\n';
const destination = path.join(__dirname, 'recall.json');
if (process.argv.includes('--write')) fs.writeFileSync(destination, output);
else assert.equal(fs.readFileSync(destination, 'utf8'), output);
console.log('Original ordinary recall oracle: ' + (direct.length + 1) + ' cases');
