// Independent source-derived plain-text boundary vectors. No account or network.
const fs = require('fs');
const path = require('path');
const vm = require('vm');
const crypto = require('crypto');
const assert = require('assert/strict');

const sourceName = 'pc-dist/compact-app-pc.e08d0d44f38873747a6b.js';
const source = fs.readFileSync(path.resolve(__dirname, '../../../../' + sourceName), 'utf8');
const begin = source.indexOf('                _sendPlainText(');
assert(begin >= 0, 'Original plain-text composer method must exist');
const method = source.slice(begin, begin + 18000);
const expression = method.match(/if \(p = (p\.trim\(\)), !p && !f\.length/);
assert(expression, 'Original boundary canonicalization must remain explicit');
assert(method.includes('this._processSendMessage(e, p, v,'), 'Canonical text must reach sending');
const canonical = vm.runInNewContext('(p) => ' + expression[1]);

const codepoints = [
    0x0009, 0x000a, 0x000b, 0x000c, 0x000d, 0x0020, 0x0085, 0x00a0,
    0x1680, 0x180e, ...Array.from({length: 12}, (_, i) => 0x2000 + i),
    0x2028, 0x2029, 0x202f, 0x205f, 0x2060, 0x3000, 0xfeff,
];
const inputs = codepoints.map(cp => ({
    name: 'boundary-U+' + cp.toString(16).toUpperCase().padStart(4, '0'),
    input: String.fromCodePoint(cp) + 'chào' + String.fromCodePoint(cp),
}));
inputs.push(
    {name: 'empty', input: ''},
    {name: 'ascii-whitespace-only', input: ' \t\r\n\v\f'},
    {name: 'telex-trailing-space', input: 'tiếng Việt '},
    {name: 'composer-trailing-newline', input: 'chào\n'},
    {name: 'mixed-boundaries', input: '\ufeff \tchào\r\n \u00a0'},
    {name: 'internal-spacing-and-newline', input: ' a  b\n c \t'},
    {name: 'internal-unicode-whitespace', input: ' a\u00a0b\ufeffc\u0085d '},
    {name: 'decomposed-preserved', input: '  cha\u0300o  '},
    {name: 'zero-width-space-preserved', input: '\u200bchào\u200b'},
);
const cases = inputs.map(({name, input}) => ({name, input, expected: canonical(input)}));
assert.equal(cases.find(c => c.name === 'boundary-U+0085').expected, '\u0085chào\u0085');
assert.equal(cases.find(c => c.name === 'boundary-U+FEFF').expected, 'chào');
assert.equal(cases.find(c => c.name === 'decomposed-preserved').expected, 'cha\u0300o');
const output = {
    provenance: {
        source: sourceName,
        sha256: crypto.createHash('sha256').update(source).digest('hex'),
        method: '_sendPlainText',
        expression: expression[1],
    },
    cases,
};
const target = path.join(__dirname, 'plain_text.json');
const serialized = JSON.stringify(output, null, 2) + '\n';
if (process.argv.includes('--write')) fs.writeFileSync(target, serialized);
else assert.equal(fs.readFileSync(target, 'utf8'), serialized);
console.log(`Original plain-text boundary oracle PASS (${cases.length} cases)`);
