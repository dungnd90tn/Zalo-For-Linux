'use strict';
// Synthetic AES256CBC oracle, independent of Rust. Never use account captures.
const crypto = require('node:crypto');
const fs = require('node:fs');
const key = Buffer.from(Array.from({length: 32}, (_, i) => i));
const iv = Buffer.alloc(16);
const input = fs.readFileSync(0, 'utf8');
if (process.argv[2] === 'decrypt') {
    const decipher = crypto.createDecipheriv('aes-256-cbc', key, iv);
    const bytes = Buffer.concat([decipher.update(input, 'base64'), decipher.final()]);
    process.stdout.write(bytes);
} else if (process.argv[2] === 'encrypt') {
    const data = JSON.parse(input);
    const cipher = crypto.createCipheriv('aes-256-cbc', key, iv);
    const ciphertext = Buffer.concat([cipher.update(JSON.stringify({error_code: 0, data})), cipher.final()]);
    process.stdout.write(JSON.stringify({error_code: 0, data: ciphertext.toString('base64')}));
} else {
    throw new Error('Expected explicit synthetic fixture mode');
}
