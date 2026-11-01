// Independent Node/OpenSSL AES fixture. Synthetic local TLS tests only.
const crypto = require('crypto');
let input = '';
process.stdin.on('data', chunk => { input += chunk; if(input.length > 32768) process.exit(2); });
process.stdin.on('end', () => {
    const {query, uid, response} = JSON.parse(input);
    const p = new URLSearchParams(query);
    const zcid = p.get('zcid');
    const hash = crypto.createHash('md5').update(p.get('zcid_ext')).digest('hex').toUpperCase();
    const even = x => [...x].filter((_, i) => i % 2 === 0).join('');
    const odd = x => [...x].filter((_, i) => i % 2 === 1).join('');
    const key = Buffer.from(even(hash).slice(0, 8) + even(zcid).slice(0, 12) + [...odd(zcid)].reverse().join('').slice(0, 12));
    const plaintext = JSON.stringify(response || {error_code:0, data:{uid,dkey:'synthetic-storage',zpw_enk:'AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=',zpw_service_map_v3:{chat:['https://chat-wpa.zaloapp.com/']},zpw_ws:['wss://ws1.zalo.me/']}});
    const cipher = crypto.createCipheriv('aes-256-cbc', key, Buffer.alloc(16));
    const data = Buffer.concat([cipher.update(plaintext), cipher.final()]).toString('base64');
    process.stdout.write(JSON.stringify({error_code:0,data}));
});
