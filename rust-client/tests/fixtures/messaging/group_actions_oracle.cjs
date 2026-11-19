// Execute the original group invitation and leave request builders on synthetic IDs.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const source = fs.readFileSync(path.resolve(__dirname,
    '../../../../pc-dist/lazy/default-login-main-startup-shared-worker-znotification.9e3e92e88644da772301.js'), 'utf8');
const start = source.indexOf('static inviteMemberV2({');
const end = source.indexOf('static disperseGroup(e) {', start);
assert(start >= 0 && end > start, 'Group action source anchors changed');
const group = '900719925474099123';
const friend = '22';
const device = 'synthetic-device';
const Api = vm.runInNewContext('(class {' + source.slice(start, end) + '})', {
    _: {b: {getGroupDomain: () => 'https://group.example.test'}},
    D: {a: {getZaloClientID: () => device}},
    i: {default: {
        getGroupIdFromConversationId: thread => thread.slice(1),
        encodeAES: text => text,
    }},
    w: {a: {getCurrentLanguageName: () => 'VN'}},
});
Api._getCommonParams = () => 'zpw_ver=681&zpw_type=24';
Api._post = (url, body, _request, metric) => {
    const request = new URL(url);
    return {
        path: request.pathname,
        query: Object.fromEntries(request.searchParams),
        payload: JSON.parse(new URLSearchParams(body).get('params')),
        metric,
    };
};

const result = {
    provenance: 'Original inviteMemberV2 and leaveGroup builders with synthetic identities and identity AES.',
    group, friend, device,
    invite: Api.inviteMemberV2({grid: 'g' + group, members: [friend], memberTypes: [-1]}),
    leave: Api.leaveGroup('g' + group, 0),
};
assert.equal(result.invite.path, '/api/group/invite/v2');
assert.equal(result.invite.metric, 12444);
assert.equal(result.leave.path, '/api/group/leave');
assert.equal(result.leave.metric, 11189);
assert.equal(result.leave.payload.silent, 0);
const output = JSON.stringify(result, null, 2) + '\n';
const destination = path.join(__dirname, 'group_actions.json');
if (process.argv.includes('--write')) fs.writeFileSync(destination, output);
else assert.equal(fs.readFileSync(destination, 'utf8'), output);
console.log('Original group action oracle: 2 cases');
