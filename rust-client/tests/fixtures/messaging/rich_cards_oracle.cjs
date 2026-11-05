// Execute original bundled readers against synthetic payloads only. No account,
// remote template, poll-detail lookup, action handler or network is started.
const fs = require('fs');
const path = require('path');
const vm = require('vm');
const assert = require('assert/strict');
const {between} = require('./auth_oracle.cjs');
const root = path.resolve(__dirname, '../../../..');
const main = fs.readFileSync(path.join(root, 'pc-dist/compact-app-pc.e08d0d44f38873747a6b.js'), 'utf8');
const lazy = fs.readFileSync(path.join(root, 'pc-dist/lazy/main-startup.171f144ab54f0da65752.js'), 'utf8');
const subtypes = between(main, '                _normalizeRecommendMsgSubType(e) {', '\n                transformMessageFromServer(e, t) {');
const normalize = vm.runInNewContext('(function(e){' + subtypes.slice(0, subtypes.lastIndexOf('}')) + '})', {
    s: {MSG_CONTACT: 'contact'}, r: {default: {logCoreError() {}}},
});
const start = main.indexOf('if (r.href = t.message.href, r.thumb = t.message.thumb, r.title = t.message.title,');
assert(start > 0);
const end = main.indexOf('\n                                        }\n                                        r.extra =', start);
assert(end > start);
const link = vm.runInNewContext('(function(content){const t={message:content},r={};' + main.slice(start, end) + ';return r;})');
const phone = between(main, '                                        let i = "";\n                                        try {\n                                            const e = JSON.parse(t.message.description);', '\n                                        n = `${j.a.str("STR_CONTACT_MSG")}`');
const contactPhone = vm.runInNewContext('(function(content){const t={message:content};let i="";try{const e=JSON.parse(t.message.description);' + phone + ';return i;})');
const pollParams = between(lazy, '                getPollParams(e) {', '\n                getLastMessagePoll(e, t, s) {');
const parsePoll = vm.runInNewContext('(function(e){' + pollParams.slice(0, pollParams.lastIndexOf('}')) + '})');
const notice = between(main, '                    let e = r.msg[A];\n                    return e = e.replace("%1$s", ""),', '\n                }), [t, r, A]), f =');
const readNotice = vm.runInNewContext('(function(r){const A="vi";let e=r.msg[A];return e=e.replace("%1$s", ""),' + notice + '})', {
    j: {a: {trans: () => 'Hành động ẩn danh'}}, s: {a: {createElement: (_, __, text) => text}},
});
// Options and votes are deliberately separate fetched data in the original UI.
assert(main.includes('this.GroupPollManager.getPollDetail(this.props.pollId, !0)'));
const cases = [];
for (const title of ['', 'Tiêu đề nguồn']) {
    const content = {action: 'recommened.link', title, description: 'Mô tả liên kết', thumb: 'https://f12-zpcloud.zdn.vn/card.jpg', href: 'https://example.test/article', params: JSON.stringify({mediaTitle: 'Tiêu đề dự phòng', unknown: 'retain'}), unknown: {retained: true}};
    assert.equal(normalize(content.action), 'contact');
    const actual = link(content);
    cases.push({name: 'recommended_link_' + (title ? 'title' : 'fallback'), kind: 'chat.recommended', content, expected: {title: actual.title, description: content.description, image_url: actual.thumb, url: actual.href}});
}
for (const action of ['recommened.user', 'recommened.vip']) {
    const content = {action, title: 'Tên danh thiếp', description: JSON.stringify({phone: '0900000000', privateUnknown: 'retain'}), thumb: 'https://f12-zpcloud.zdn.vn/contact.jpg', href: '12345', params: '23456'};
    assert.equal(normalize(action), 'contact');
    const actual = link(content);
    cases.push({name: action, kind: 'chat.recommended', content, expected: {title: actual.title, description: contactPhone(content), image_url: actual.thumb, url: null}});
}
for (const anonymous of [false, true]) {
    const content = {params: JSON.stringify({msg: {vi: '%1$s đã tạo bình chọn %2$s', en: 'Poll created'}, question: 'Nhóm chọn ngày nào?', isAnonymous: anonymous, pollId: 'synthetic-poll', unknown: {retained: true}})};
    const params = parsePoll({message: content});
    cases.push({name: 'poll_' + anonymous, kind: 'group.poll', content, expected: {title: readNotice(params), question: params.question, anonymous: params.isAnonymous}});
}
const output = JSON.stringify({provenance: 'Exact original _normalizeRecommendMsgSubType, shared-contact projection, contact phone reader, GroupPollInfoManager.getPollParams and poll notice renderer; synthetic offline data', cases}, null, 2) + '\n';
const destination = path.join(__dirname, 'rich_cards.json');
if (process.argv.includes('--write')) fs.writeFileSync(destination, output);
else assert.equal(fs.readFileSync(destination, 'utf8'), output);
console.log('Original recommended/poll oracle: ' + cases.length + ' synthetic cases');
