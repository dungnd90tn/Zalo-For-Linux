// Execute the original response normalizer using synthetic metadata only.
const assert = require('assert/strict');
const fs = require('fs');
const path = require('path');
const vm = require('vm');
const root = path.resolve(__dirname, '../../../..');
const source = fs.readFileSync(path.join(root, 'pc-dist/lazy/default-login-main-startup-shared-worker-znotification.9e3e92e88644da772301.js'), 'utf8');
const start = source.indexOf('                        static parseE2eeResp(e) {');
const end = source.indexOf('                        static isImportantMsg(e)', start);
assert(start >= 0 && end > start, 'Original response reader anchors changed');
const Reader = vm.runInNewContext('(class {' + source.slice(start, end) + '})', {s: {a: Object.assign}});
const inputs = [
    ['encrypted_string_ids', {clientId:'101', minGlobalMsgId:'201', ts:123, data:[{error_code:9}]}],
    ['encrypted_numeric_ids', {clientId:101, minGlobalMsgId:201, ts:'123'}],
    ['explicit_msg_id_overrides_normalization', {clientId:'101', minGlobalMsgId:'201', msgId:'999', ts:123}],
    ['plain_passthrough', {clientId:'101', msgId:'201', ts:123}],
    ['missing_client_passthrough', {minGlobalMsgId:'201', data:[]}],
    ['missing_server_passthrough', {clientId:'101', data:[]}],
    ['zero_client_passthrough', {clientId:0, minGlobalMsgId:'201', data:[]}],
    ['null_minimum_passthrough', {clientId:'101', minGlobalMsgId:null, msgId:'201'}],
    ['no_timestamp', {clientId:'101', minGlobalMsgId:'201'}],
    ['unknown_fields_retained', {clientId:'101', minGlobalMsgId:'201', extra:{retained:true}, data:[]}],
];
const cases = inputs.map(([name, input]) => ({name, input, output: JSON.parse(JSON.stringify(Reader.parseE2eeResp(structuredClone(input))))}));
assert.equal(cases[0].output.msgId, '201');
assert(!('data' in cases[0].output));
assert.equal(cases[2].output.msgId, '999');
assert('data' in cases[4].output);
const output = JSON.stringify({provenance:'Exact original parseE2eeResp with synthetic response metadata. This normalizer does not establish HTTP success, safe IDs, request correlation or durable commit; native checks test those separately.', cases}, null, 2) + '\n';
const destination = path.join(__dirname, 'send_ack.json');
if (process.argv.includes('--write')) fs.writeFileSync(destination, output);
else assert.equal(fs.readFileSync(destination, 'utf8'), output);
console.log('Original encrypted send-response oracle: ' + cases.length + ' cases');
