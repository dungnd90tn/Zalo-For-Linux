// Execute the exact original HTTP preview caller, its decrypt/preprocessor chain,
// and conversation-key projection. Synthetic plaintext replaces AES only; no I/O.
const fs = require('fs');
const path = require('path');
const vm = require('vm');
const assert = require('assert/strict');
const {between} = require('../messaging/auth_oracle.cjs');
const root = path.resolve(__dirname, '../../../..');
const source = fs.readFileSync(path.join(root, 'pc-dist/compact-app-pc.e08d0d44f38873747a6b.js'), 'utf8');
const preprocessBody = between(source, '                preProcessChatMsgs(e, t = !1, n = !1) {', '\n                processForPreviewMessages(e, t) {');
const original = vm.runInNewContext('({preProcessChatMsgs(e,t=false,n=false){' + preprocessBody + '})', {
    a: {default: {decodeAES: value => value, preParse: value => value}},
    s: {GROUPID_PREFIX: 'g:'}, T: {zsymb() {}},
    r: {a: {}, b: {alert() {}}},
});
original.userId = '11';
original.processForPreviewMessages = (message, preview) => {if (preview) message.src = 'GET_PREVIEW';};
const callerBody = between(source, '                getLastMessagesForPreview(e) {', '\n                changeGroupOwner(e, t) {');
const keyBody = between(source, '(i || (i = {})).toConvId = function(e) {', '\n            }\n        },\n        ZaGb:');
const conversationKey = vm.runInNewContext('(function(e){' + keyBody + '})', {a: {GROUPID_PREFIX: 'g:'}});
let input;
let calls = 0;
const caller = vm.runInNewContext('({getLastMessagesForPreview(e){' + callerBody + '})', {
    o: {default: {getLastMessagesForPreview: async () => {calls++; return {data: {data: JSON.stringify({error_code:0, data: input})}};}}},
    Ge: {default: original},
});
const row = (uidFrom, idTo, msgType='webchat') => ({msgId:'841', uidFrom, idTo, msgType, ts:1700000000000, content:msgType === 'chat.photo' ? {title:'Ảnh',href:'https://f12-zpcloud.zdn.vn/synthetic.jpg'} : msgType === 'chat.webcontent' ? {params:{customMsg:{msg:{vi:'Thông báo dịch vụ'}}}} : 'synthetic'});
const cases = [
    {name:'empty_object', data:{}},
    {name:'control_only', data:{clearUnreads:[{idTo:'22',lastMsgId:'841'}]}},
    {name:'nullable_collections', data:{msgs:null,groupMsgs:null,pageMsgs:null,eeMsgs:null}},
    {name:'peer_addressed_direct', data:{msgs:[row('22','33','chat.photo')]}},
    {name:'peer_addressed_page', data:{pageMsgs:[row('44','55','chat.webcontent')]}},
    {name:'self_sentinel', data:{msgs:[row('0','22')]}},
    {name:'group', data:{groupMsgs:[row('22','90')]}},
    {name:'same_thread_both_peer_destinations', data:{msgs:[row('22','33'),{...row('22','44'),msgId:'842'}]}},
];
(async () => {
    for (const item of cases) {
        input = structuredClone(item.data);
        const response = await caller.getLastMessagesForPreview({synthetic:true});
        item.threads = Array.from(response.allmsgs, message => {
            assert.equal(message.src, 'GET_PREVIEW');
            const key = conversationKey(message);
            return message.isGroup ? key : 'd:' + key;
        });
    }
    assert.equal(calls, cases.length);
    assert.deepEqual(cases[3].threads, ['d:22']);
    assert.deepEqual(cases[4].threads, ['d:44']);
    assert.deepEqual(cases[5].threads, ['d:22']);
    assert.deepEqual(cases[6].threads, ['g:90']);
    const output = JSON.stringify({provenance:'Exact original getLastMessagesForPreview HTTP caller, preProcessChatMsgs and toConvId; synthetic plaintext/decrypt boundary, no network or account data',cases},null,2)+'\n';
    const destination = path.join(__dirname, 'previews.json');
    if (process.argv.includes('--write')) fs.writeFileSync(destination,output);
    else assert.equal(fs.readFileSync(destination,'utf8'),output);
    console.log('Original HTTP preview oracle: '+cases.length+' empty/control/null/peer/page/self/group cases');
})().catch(error => {console.error(error);process.exitCode=1;});
