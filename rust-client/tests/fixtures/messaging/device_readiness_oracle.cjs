// Exercise the original device-init refusal decision with synthetic identities.
const assert = require('assert/strict');
const fs = require('fs');
const path = require('path');
const vm = require('vm');
const root = path.resolve(__dirname, '../../../..');
const source = fs.readFileSync(path.join(root, 'pc-dist/lazy/default-login-main-startup-shared-worker-znotification.9e3e92e88644da772301.js'), 'utf8');
const start = source.indexOf('this._userId === t && "number" == typeof a.err && A.default.e2ee.bundle_err_stop_retry.includes(a.err)');
const terminal = 'Promise.resolve(c.i.SESSION_RESULT.ERROR)';
const end = source.indexOf(terminal, start);
assert(start >= 0 && end > start, 'Original init refusal anchors changed');
const expression = source.slice(start, end + terminal.length);
const inactive = Number(source.match(/DOES_NOT_ACCESS_LONG_TIME:\s*(-\d+)/)[1]);
const stopRetry = JSON.parse(source.match(/bundle_err_stop_retry:\s*(\[[^\]]+\])/)[1]);
const evaluate = vm.runInNewContext('(function(a,t,n,A,c,d){return ' + expression + ';})', {Promise});
const results = {ERROR_IGNORE:'ignored', ERROR_TIMEOUT:'timeout', ERROR:'error'};
const inputs = [
    ['peer_inactive', false, -7], ['peer_unsupported_device', false, -8],
    ['peer_client_unsupported', false, -3], ['peer_blacklist', false, -11],
    ['peer_own_bundle_missing', false, -9], ['peer_timeout', false, {code:'ERR_CONNECTION_TIMED_OUT'}],
    ['own_client_unsupported', true, -3], ['own_inactive', true, -7],
    ['own_unsupported_device', true, -8], ['own_blacklist', true, -11],
];
const mergeStart = source.indexOf('                async mergeMetaListFromServer(e, t, n) {');
const mergeEnd = source.indexOf('                async deleteMeta(e)', mergeStart);
assert(mergeStart >= 0 && mergeEnd > mergeStart, 'Original metadata merge anchors changed');
const Metadata = vm.runInNewContext('(class {' + source.slice(mergeStart, mergeEnd) + '})', {
    T:{a:{now:()=>100}}, d:{default:{getTimeNow:()=>100}},
    Qn:{a:{ENABLED:'enabled',NONE:'none'}}, i:{a:Object.assign},
});
(async () => {
    const cases = [];
    for (const [name, own, error] of inputs) {
        const updates = [];
        const context = {_userId:'11', MetadataStorageService:{updateMeta(...args){updates.push(args);}}};
        const outcome = await evaluate.call(context, {err:error}, own?'11':'22', 0,
            {default:{e2ee:{bundle_err_stop_retry:stopRetry}}},
            {i:{ERROR_INIT:{DOES_NOT_ACCESS_LONG_TIME:inactive}, SESSION_RESULT:results}},
            {default:{getTimeNow(){return 100;}}});
        cases.push({name, own, error, outcome, marked_inactive:updates.length > 0});
    }
    assert.equal(cases[0].outcome, 'ignored');
    assert.equal(cases[1].outcome, 'error');
    assert.equal(cases[6].outcome, 'ignored');
    assert.equal(cases[8].outcome, 'error');
    const metadataCases = [];
    for (const [name, devices, old] of [
        ['missing_iid', [{di:0}], []], ['zero_iid', [{di:0,iid:0}], []],
        ['null_iid', [{di:0,iid:null}], []], ['positive_iid', [{di:0,iid:123}], []],
        ['same_iid_keeps_session', [{di:0,iid:123}], [{deviceId:0,sessionKey:123,sessionTs:9,stale:0,inactive:1}]],
        ['changed_iid_marks_stale', [{di:0,iid:124}], [{deviceId:0,sessionKey:123,sessionTs:9,stale:0}]],
        ['missing_old_device_retained_stale', [{di:0,iid:123}], [{deviceId:1,sessionKey:456,sessionTs:9,stale:0}]],
    ]) {
        const previous = structuredClone(old);
        const record = {uid:'22',status:'enabled',devices:Object.fromEntries(previous.map(item=>[item.deviceId,item]))};
        const saved = [];
        const instance = new Metadata();
        Object.assign(instance, {
            TASK:{LoadEnabled:1,LoadUnEnabled:2},waitingTask:{isPending:()=>false},
            getUserId:()=> '11',getUserMeta:()=>previous.length?record:undefined,
            getDeviceMeta:(_,di)=>record.devices[di],getAllDeviceMeta:()=>Object.values(record.devices),
            getEnabledListChecksum:()=>0,calculateCheckSum:()=>1,saveEnabledListChecksum:()=>{},
            Logger:{zsymb:()=>{}},SignalProtocolManager:{E2eeEventHandlerManager:{initStatus:()=>{}}},
            repository:{getAllSync:()=>previous.length?[record]:[],upsert:async rows=>saved.push(...rows)},
            _logEnabledList:()=>{},
        });
        await instance.mergeMetaListFromServer([{uid:'22',devices}], [], false);
        const metadata = saved[0] || record;
        metadataCases.push({name,devices,previous:old,result:JSON.parse(JSON.stringify(metadata.devices))});
    }
    assert.equal(metadataCases[0].result[0].sessionKey, undefined);
    assert.equal(metadataCases[1].result[0].sessionKey, 0);
    assert.equal(metadataCases[4].result[0].stale, 0);
    assert.equal(metadataCases[6].result[1].stale, 100);
    const output = JSON.stringify({provenance:'Exact original initSessionWith refusal ternary and mergeMetaListFromServer. Peer inactivity is -7 only; own-device exclusions use the source-configured stop-retry table. Metadata projection does not establish transport completeness or identity replacement permission.', inactive, stop_retry:stopRetry, cases, metadata_cases:metadataCases}, null, 2) + '\n';
    const destination = path.join(__dirname, 'device_readiness.json');
    if (process.argv.includes('--write')) fs.writeFileSync(destination, output);
    else assert.equal(fs.readFileSync(destination, 'utf8'), output);
    console.log('Original device readiness oracle: ' + cases.length + ' refusal and ' + metadataCases.length + ' metadata cases');
})().catch(error => { console.error(error); process.exitCode = 1; });
