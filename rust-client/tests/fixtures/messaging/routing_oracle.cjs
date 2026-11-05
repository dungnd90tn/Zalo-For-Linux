// Execute original queue preprocessing and its exact default consumer key expression.
// Synthetic values only; no accounts, network, UI or mutable application state.
const fs = require('fs');
const path = require('path');
const vm = require('vm');
const assert = require('assert/strict');
const {worker, between} = require('./auth_oracle.cjs');

const chatHandlerOffset = worker.indexOf('const j = ["uidFrom", "uidTo", "uid", "ownerId"');
assert(chatHandlerOffset >= 0);
const preprocessBody = between(worker.slice(chatHandlerOffset),
    '                preProcessChatMsgs(e, t = !1, n = !1) {',
    '\n                initSignal() {');
const preprocess = vm.runInNewContext(
    '({preProcessChatMsgs(e, t = false, n = false) {' + preprocessBody + '})',
    {i: {GROUPID_PREFIX: 'g:'}}).preProcessChatMsgs;
const consumerExpression = between(worker,
    'default: {\n                                        let a;\n                                        a = ',
    ', at.a.onNewMsg(n, a);');
assert(consumerExpression.length < 200, 'consumer extraction must remain a single expression');
const consumerKey = vm.runInNewContext('(e, n) => (' + consumerExpression + ')',
    {P: {GROUPID_PREFIX: 'g:'}});

const cases = [
    ['peer_addressed_other', 'msgs', '22', '33', 'chat.photo', 'd:22'],
    ['peer_addressed_text', 'msgs', '22', '33', 'webchat', 'd:22'],
    ['page_peer_addressed', 'pageMsgs', '44', '55', 'webchat', 'd:44'],
    ['incoming_to_account', 'msgs', '22', '11', 'webchat', 'd:22'],
    ['incoming_to_zero', 'msgs', '22', '0', 'webchat', 'd:22'],
    ['self_sentinel', 'msgs', '0', '22', 'webchat', 'd:22'],
    ['self_chat', 'msgs', '0', '0', 'webchat', 'd:11'],
    ['group_collision', 'groupMsgs', '33', '22', 'webchat', 'g:22'],
];
const result = cases.map(([name, field, uidFrom, idTo, msgType, expected]) => {
    const context = {userId: '11', signalProtocolManager: {checkMsgE2ee() {}}, processForPreviewMessages() {}};
    const data = preprocess.call(context, {data: {[field]: [{uidFrom, idTo, msgType}]}});
    assert.equal(data.allmsgs.length, 1);
    const message = data.allmsgs[0];
    const key = consumerKey({}, message);
    const thread = message.isGroup ? key : 'd:' + key;
    assert.equal(thread, expected, name);
    return {name, field, uidFrom, idTo, msgType, thread};
});
const output = JSON.stringify({provenance: 'Exact original preProcessChatMsgs and _processChatMessages default consumer key expression; synthetic identities', cases: result}, null, 2) + '\n';
const destination = path.join(__dirname, 'routing.json');
if (process.argv.includes('--write')) fs.writeFileSync(destination, output);
else assert.equal(fs.readFileSync(destination, 'utf8'), output);
console.log('Original routing oracle: ' + result.length + ' peer/page/self/group cases');
