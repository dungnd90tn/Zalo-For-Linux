// Execute the original PC preview methods over synthetic information events.
// No application startup, real account data, network, actions, or URLs are used.
const fs = require('fs');
const path = require('path');
const vm = require('vm');
const assert = require('assert/strict');
const {between} = require('./auth_oracle.cjs');
const root = path.resolve(__dirname, '../../../..');
const source = fs.readFileSync(path.join(root, 'pc-dist/compact-app-pc.e08d0d44f38873747a6b.js'), 'utf8');
const previewOffset = source.indexOf('                getPreviewMsgInfoChat(e) {');
const replacementOffset = source.lastIndexOf('                _renderItemReplace(e) {', previewOffset);
assert(previewOffset >= 0 && replacementOffset >= 0);
const replacement = between(source.slice(replacementOffset),
    '                _renderItemReplace(e) {', '\n                getPreviewMsgZNS(e) {');
const preview = between(source.slice(previewOffset),
    '                getPreviewMsgInfoChat(e) {', '\n                isStickerMessage(e) {');
const original = vm.runInNewContext('({_renderItemReplace(e) {' + replacement +
    ', getPreviewMsgInfoChat(e) {' + preview + '})', {
    r: {default: {valueValid: value => value !== null && value !== undefined}},
    l: {default: {getUidMe: () => '11', getDName: () => undefined}},
    c: {a: {currentLanguage: () => 0, str: key => key === 'STR_ME_DEL' ? 'Bạn' : '[Tin nhắn chưa hỗ trợ ở phiên bản hiện tại]'}},
    s: {LANG_INDEX_EN: 1},
});
const content = (params, title = 'Thông báo nhóm') => ({
    action: 'msginfo.actionlist', title,
    params: typeof params === 'string' ? params : JSON.stringify(params),
    unknownNested: {preserve: ['synthetic', {version: 7}]},
});
const cases = [
    {name: 'localized_text', content: content({msg: {vi: 'Đã cập nhật thông tin nhóm', en: 'Group updated'}})},
    {name: 'self_and_named_member', content: content({
        msg: {vi: '%1$s đã thêm %2$s vào nhóm', en: '%1$s added %2$s'},
        highLightsV2: [{type: 0, uid: '11', dpn: 'Tên tài khoản'}, {type: 0, uid: '22', dpn: 'Đặng Minh'}],
        actions: [{actionType: 'action.open.profile', actionData: 'https://invalid.example/synthetic'}],
    })},
    {name: 'sequential_reordered_markers', content: content({
        msg: {vi: '%2$s và %1$s'},
        highLightsV2: [{type: 0, uid: '22', dpn: 'An'}, {type: 0, uid: '33', dpn: 'Bình'}],
    })},
    {name: 'replace_one_occurrence', content: content({msg: {vi: '%1$s gọi %1$s'}, highLightsV2: [{type: 0, uid: '11'}]})},
    {name: 'missing_name_retains_template', content: content({msg: {vi: '%1$s đã tham gia'}, highLightsV2: [{type: 0, uid: '44'}]})},
    {name: 'provided_time_label', content: content({msg: {vi: 'Đã hẹn vào %1$s'}, highLightsV2: [{type: 1, dpn: 'Ngày mai'}]})},
    {name: 'title_fallback', content: content('not-json', 'Nhóm đã thay đổi')},
    {name: 'missing_vi_uses_title', content: content({msg: {en: 'English only'}}, 'Thông tin nhóm')},
    {name: 'literal_text_and_inert_actions', content: content({
        msg: {vi: '%1$s <b>đã thay đổi nhóm</b>'},
        highLightsV2: [{type: 0, uid: '22', dpn: '<script>literal</script>'}],
        actions: [{actionType: 'action.open.outapp', actionData: 'javascript:synthetic_action'}],
    })},
];
for (const item of cases) {
    // Same VM realm matters because the original checks value.constructor.
    const message = vm.runInNewContext('(' + JSON.stringify({message: item.content}) + ')');
    // JSON.parse is performed by the original function in its own VM realm.
    item.text = original.getPreviewMsgInfoChat(message);
}
const output = JSON.stringify({
    provenance: 'Exact original getPreviewMsgInfoChat/_renderItemReplace; Vietnamese, provided display names, synthetic identities; native UTC date fallback is tested separately',
    cases,
}, null, 2) + '\n';
const destination = path.join(__dirname, 'info.json');
if (process.argv.includes('--write')) fs.writeFileSync(destination, output);
else assert.equal(fs.readFileSync(destination, 'utf8'), output);
console.log('Original info preview oracle: ' + cases.length + ' synthetic text/name/fallback cases');
