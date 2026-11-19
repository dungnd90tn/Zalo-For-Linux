// Execute the current PC group roster and profile request builders on synthetic IDs.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const source = fs.readFileSync(path.resolve(__dirname,
    '../../../../pc-dist/lazy/default-login-main-startup-shared-worker-znotification.9e3e92e88644da772301.js'), 'utf8');

function excerpt(begin, end) {
    const start = source.indexOf(begin);
    const finish = source.indexOf(end, start);
    assert(start >= 0 && finish > start, 'Group roster source anchors changed');
    return source.slice(start, finish);
}

const methods = excerpt('static getInfoMultiGroup(e) {', 'static getGroupInfo(e) {');
const Api = vm.runInNewContext('(class {' + methods + '})', {
    _: {b: {
        getGroupDomain: () => 'https://group.example.test',
        getProfileDomain: () => 'https://profile.example.test',
    }},
    i: {default: {encodeAES: text => text}},
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

const format = vm.runInNewContext('(class {' +
    excerpt('_sortMemberIds(e, t, n) {', '_formatDataAGroup(e) {') + '})');
const group = '900719925474099123';
const sourceMembers = ['24_0', '88_4', '22_1', '24_0', '33_2'];
const normalizer = new format();
const parsed = normalizer._reformatMemberIds(sourceMembers);
const roleOrder = normalizer._sortMemberIds('88', ['33'], parsed.memberIds);
const result = {
    provenance: 'Original getInfoMultiGroup, getMemberInGroup and member normalization helpers with synthetic IDs and identity AES.',
    group,
    roster: Api.getInfoMultiGroup({[group]: 0}),
    profiles: Api.getMemberInGroup(['88_0', '33_0', '22_0']),
    sourceMembers,
    normalized: parsed,
    roleOrder,
};
assert.equal(result.roster.path, '/api/group/getmg-v2');
assert.equal(result.roster.metric, 12028);
assert.equal(result.profiles.path, '/api/social/group/members');
assert.equal(result.profiles.metric, 12030);
assert.deepEqual(Array.from(roleOrder), ['88', '33', '24', '22']);
const output = JSON.stringify(result, null, 2) + '\n';
const destination = path.join(__dirname, 'group_roster.json');
if (process.argv.includes('--write')) fs.writeFileSync(destination, output);
else assert.equal(fs.readFileSync(destination, 'utf8'), output);
console.log('Original group roster oracle: 3 cases');
