// Synthetic offline oracle: execute the repository's original photo normalizer
// and ZInstant V1 metadata reader. No app, account, network or template runs.
const fs = require('fs');
const path = require('path');
const vm = require('vm');
const assert = require('assert/strict');
const {between} = require('./auth_oracle.cjs');
const root = path.resolve(__dirname, '../../../..');
const main = fs.readFileSync(path.join(root, 'pc-dist/compact-app-pc.e08d0d44f38873747a6b.js'), 'utf8');
const lazy = fs.readFileSync(path.join(root, 'pc-dist/lazy/18.2736ee958ea402bb72d6.js'), 'utf8');
const parse = between(main, '                _parsePhotoParams(e) {', '\n                _normalizeMessageJSON(e, t) {');
const normalize = between(main, '                _normalizeMessageJSON(e, t) {', '\n                        case s.MSG_VIDEO: {');
const photo = vm.runInNewContext('({_isHttp(e){return typeof e === "string" && e.toLowerCase().startsWith("http")},_parsePhotoParams(e){' + parse + ',_normalizeMessageJSON(e,t){' + normalize + '}}})', {
    d: {a: value => {try {return typeof value === 'object' ? value : JSON.parse(value);} catch {return undefined;}}},
    u: {f: () => undefined}, s: {MSG_E2EE_PENDING: 'pending', MSG_PHOTO: 'photo', MSG_GIF: 'gif'},
    A: {a: {transcode: value => value}}, o: {a: {get: () => undefined}},
});
const readerBody = between(lazy, 'o = class {', '\n            }, Object(d.a)(o.prototype');
const Reader = vm.runInNewContext('(class {' + readerBody + '})', {
    p: {g: {safeParseJson: value => {try {return [null, JSON.parse(value)];} catch (error) {return [error];}}}},
    zconsole: {zsymb() {}}, z: {MSG_ZINSTANT: 'v1', MSG_ZINSTANT_OLD: 'old'}, c: {a: Object.assign},
});
const reader = new Reader();
const preview = between(main, '                getPreviewMsgZNS(e) {', '\n                getPreviewMsgZNSDefault() {');
const projector = vm.runInNewContext('({getPreviewMsgZNS(e){' + preview + ',getPreviewMsgZNSDefault(){return "[Tin nhắn Media]"}})', {
    s: {}, i: {a: Object.assign}, c: {a: {getCurrentLanguageName: () => 'vi'}},
    w: {h: {Adapters: {MessageEntity: function (message) {this.message = message;}}, create: adapter => ({getZInstantMetadata: () => reader.getZInstantMetadata('synthetic', adapter.message.message)})}},
    zconsole: {zsymb() {}},
});
// The HTTP preview API calls this exact preprocessing method. Its optional
// message collections explicitly accept null, as does eeMsgs || [] downstream.
const preloadBody = between(main, '                preProcessChatMsgs(e, t = !1, n = !1) {', '\n                processForPreviewMessages(e, t) {');
const preload = vm.runInNewContext('({preProcessChatMsgs(e,t=false,n=false){' + preloadBody + '})', {
    a: {default: {decodeAES: value => value, preParse: value => value}}, s: {GROUPID_PREFIX: 'g:'},
});
const emptyPreload = preload.preProcessChatMsgs(JSON.stringify({data: {msgs:null, groupMsgs:null, pageMsgs:null, eeMsgs:null}}), false, true);
assert.equal(emptyPreload.allmsgs.length, 0);
assert(main.includes('Ge.default.preProcessChatMsgs(e.data.data, !1, !0)'), 'HTTP preview must use the audited preprocessor');
assert(main.includes('this._filterDuplicatedMessages(t.eeMsgs || [])'), 'null encrypted previews use empty fallback');
const cases = [];
for (const objectParams of [false, true]) {
    const params = {hd: 'https://f12-zpcloud.zdn.vn/synthetic-hd.jpg?sig=one%2Btwo', width: '640', height: 480, unknown: {retained: true}};
    const content = {title: 'Ảnh minh họa', description: 'Ảnh gửi trong nhóm', href: 'https://f12-zpcloud.zdn.vn/synthetic-normal.jpg', thumb: 'https://f12-zpcloud.zdn.vn/synthetic-thumb.jpg', params: objectParams ? params : JSON.stringify(params), unknown: ['full', 'content']};
    const normalized = photo._normalizeMessageJSON({content: structuredClone(content)}, 'photo');
    cases.push({name: 'photo_' + (objectParams ? 'object' : 'string'), kind: 'chat.photo', content, expected: {title: normalized.title, caption: normalized.description, thumbnail_url: normalized.thumbUrl, original_url: normalized.oriUrl, width: 640, height: 480}});
}
for (const objectParams of [false, true]) {
    const params = {customMsg: {msg: {vi: 'Đơn hàng đã được giao thành công', en: 'Delivered'}}, item: {bundle_data: JSON.stringify({remoteTemplateSpecific: {orderId: 'synthetic'}})}, pcItem: {layoutPC: 3}, unknown: {preserved: true}};
    const content = {title: 'Thông báo đơn hàng', description: 'Thông tin từ tài khoản dịch vụ', thumb: 'https://f14-zpcloud.zdn.vn/synthetic-card.jpg', params: objectParams ? params : JSON.stringify(params)};
    cases.push({name: 'webcontent_' + (objectParams ? 'object' : 'string'), kind: 'chat.webcontent', content, expected: {title: projector.getPreviewMsgZNS({msgType: 'v1', message: structuredClone(content)}), description: content.description, image_url: content.thumb}});
}
const output = JSON.stringify({provenance: 'Exact original photo _normalizeMessageJSON (identity transcode) and ZInstant V1 reader/getPreviewMsgZNS; synthetic read-only payloads, no remote templates', cases}, null, 2) + '\n';
const destination = path.join(__dirname, 'media.json');
if (process.argv.includes('--write')) fs.writeFileSync(destination, output);
else assert.equal(fs.readFileSync(destination, 'utf8'), output);
console.log('Original media oracle: ' + cases.length + ' synthetic photo/ZInstant cases');
