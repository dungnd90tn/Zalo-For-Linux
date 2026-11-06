// Execute the exact original E2EE-list pagination reader with synthetic pages.
// No Signal engine, account, socket, key management or server is started.
const assert = require('assert/strict');
const fs = require('fs');
const path = require('path');
const vm = require('vm');
const root = path.resolve(__dirname, '../../../..');
const source = fs.readFileSync(path.join(root, 'pc-dist/compact-app-pc.e08d0d44f38873747a6b.js'), 'utf8');
const commandMatch = source.match(/GET_LIST_USER_E2EE:\s*(\d+)/);
assert(commandMatch, 'original eligibility command is missing');
const command = Number(commandMatch[1]);
assert.equal(command, 563);
const start = source.indexOf('                    async getListUserE2eeWithPagination(e) {');
const end = source.indexOf('                    async getPrekeyBundle(', start);
assert(start >= 0 && end > start, 'original eligibility pagination anchors changed');
const original = vm.runInNewContext('({' + source.slice(start, end).trim() + '})', {
    s: {i: {SignalCommands: {INIT: {GET_LIST_USER_E2EE: command}}}},
    i: {a: Object.assign},
});
const empty = () => ({err: 0, threads: [], waitUids: [], uids: [], pendingUids: []});
const absent = key => { const page = empty(); delete page[key]; return page; };
const inputs = [
    ['terminal_flag_absent', [empty()]],
    ['terminal_flag_null', [{...empty(), hasMore: null}]],
    ['terminal_flag_zero', [{...empty(), hasMore: 0}]],
    ['terminal_flag_false', [{...empty(), hasMore: false}]],
    ['first_uids_absent', [absent('uids')]],
    ['first_uids_null', [{...empty(), uids: null}]],
    ['first_pending_absent', [absent('pendingUids')]],
    ['first_pending_null', [{...empty(), pendingUids: null}]],
    ['second_optional_absent', [{...empty(), hasMore: 1, lastId: 22}, {err: 0, threads: [], waitUids: []}]],
    ['two_pages_target_retained', [{...empty(), hasMore: true, lastId: 22, threads: [{uid: '22'}]}, {...empty(), hasMore: 0}]],
    ['string_false_is_truthy', [{...empty(), hasMore: 'false', lastId: 22}, {...empty(), hasMore: 0}]],
];
(async () => {
    const cases = [];
    for (const [name, pages] of inputs) {
        let index = 0, requestId = 10;
        const requests = [];
        const reader = Object.assign({}, original, {
            Logger: {zsymb() {}},
            _generateNextReqId() { return ++requestId; },
            async _requestSocketAsync(id, request) {
                requests.push(request);
                assert(index < pages.length, 'unexpected extra page');
                return {data: {data: structuredClone(pages[index++])}};
            },
        });
        let result = null, failure = null;
        try { result = JSON.parse(JSON.stringify(await reader.getListUserE2eeWithPagination())); }
        catch (error) { failure = error.name; }
        cases.push({name, pages, accepted: failure === null, requests: JSON.parse(JSON.stringify(requests)), result, failure});
    }
    assert(cases.slice(0, 4).every(test => test.accepted));
    assert(cases.slice(4, 8).every(test => !test.accepted && test.failure === 'TypeError'));
    assert(cases[8].accepted && cases[8].result.uids.length === 0);
    assert(cases[9].accepted && cases[9].result.threads[0].uid === '22');
    assert(cases[10].accepted && cases[10].requests.length === 2);
    const output = JSON.stringify({provenance: 'Exact original getListUserE2eeWithPagination and source-extracted command 563 with synthetic responses. Logger arguments remain evaluated: missing/null first-page uids/pendingUids throw; later-page merge supplies defaults. Native strict type/resource/correlation rules are separate safety tests.', cases}, null, 2) + '\n';
    const destination = path.join(__dirname, 'eligibility.json');
    if (process.argv.includes('--write')) fs.writeFileSync(destination, output);
    else assert.equal(fs.readFileSync(destination, 'utf8'), output);
    console.log('Original eligibility oracle: ' + cases.length + ' pagination/optional-field cases');
})().catch(error => { console.error(error); process.exitCode = 1; });
