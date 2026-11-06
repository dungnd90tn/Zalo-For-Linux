// Execute only original file title/URL readers with synthetic payloads.
// No download, filesystem action, account, app or network code is executed.
const assert = require('assert/strict');
const fs = require('fs');
const path = require('path');
const vm = require('vm');
const root = path.resolve(__dirname, '../../../..');
const source = fs.readFileSync(path.join(root, 'pc-dist/compact-app-pc.e08d0d44f38873747a6b.js'), 'utf8');
function between(start, end) {
    const a = source.indexOf(start), b = source.indexOf(end, a + start.length);
    assert(a >= 0 && b > a, 'original file reader anchors changed');
    return source.slice(a + start.length, b);
}
const unsupported = JSON.parse(between('                    unsupportFile: ', ',\n'));
// The extracted prefix starts inside an if-condition: prepend its guard.
const title = vm.runInNewContext('(function(e){if (!(e && e.content && e.content.title))' + between('                        case s.MSG_FILE:\n                            if (!(e && e.content && e.content.title))', '\n                        case s.MSG_ZINSTANT_V2:') + '})', {
    c: {default: {unsupportFile: unsupported}}, i: {a: Object.assign},
});
const href = vm.runInNewContext('(function(e){' + between('                getFileMessageUrl: function(e) {', '\n                },\n                getFileMessageIndicatorState:') + '})');
const inputs = [
    ['append_extension', {title: 'Báo cáo quý', href: 'https://files.zdn.vn/synthetic-report?token=fake', params: JSON.stringify({fileSize: 12345, fileExt: 'PDF', unknown: {retained: true}})}],
    ['existing_extension', {title: 'Tài liệu.PDF', href: 'https://files.zdn.vn/synthetic.pdf', params: JSON.stringify({fileSize: '1048576', fileExt: 'pdf'})}],
    ['zero_size', {title: 'Rỗng', href: 'https://files.zdn.vn/empty.txt', params: JSON.stringify({fileSize: 0, fileExt: 'txt'})}],
    ['unsupported_extension', {title: 'Cài đặt', href: 'https://files.zdn.vn/setup', params: JSON.stringify({fileSize: 16, fileExt: 'EXE'})}],
    ['object_params', {title: 'Đã có tên.txt', href: 'https://files.zdn.vn/synthetic.txt', params: {fileSize: 512, fileExt: 'txt'}}],
    ['folder', {title: 'Thư mục', href: 'https://files.zdn.vn/folder.zip', params: JSON.stringify({fileSize: 2048, fileExt: 'zip', fType: 2})}],
    ['encrypted', {title: 'Mã hóa.pdf', href: 'https://files.zdn.vn/ciphertext', params: JSON.stringify({fileSize: 640, fileExt: 'pdf', secureKey: 'synthetic-not-a-key'})}],
    ['missing_url', {title: 'Tệp chưa khả dụng.docx', params: JSON.stringify({fileExt: 'docx'})}],
];
const cases = inputs.map(([name, content]) => {
    content.unrecognized = {keep: ['entire', 'source', 'payload']};
    const normalized = title({content: structuredClone(content)});
    return {name, kind: 'share.file', content, expected: {title: normalized.title, href: href({message: normalized}) || null}};
});
const output = JSON.stringify({provenance: 'Exact original MSG_FILE title normalizer and getFileMessageUrl, including original unsupported-extension list. Synthetic payloads only. Native URL policy, decoding limits, encrypted-file handling and metadata bounds are separate tests.', cases}, null, 2) + '\n';
const destination = path.join(__dirname, 'files.json');
if (process.argv.includes('--write')) fs.writeFileSync(destination, output);
else assert.equal(fs.readFileSync(destination, 'utf8'), output);
console.log('Original file oracle: ' + cases.length + ' synthetic title/URL cases');
