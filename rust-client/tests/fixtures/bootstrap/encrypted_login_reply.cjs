// Independent Node/OpenSSL AES fixture. Synthetic local TLS tests only.
const crypto = require('crypto');
let input = '';
process.stdin.on('data', chunk => { input += chunk; if(input.length > 32768) process.exit(2); });
process.stdin.on('end', () => {
    const {query, uid, response} = JSON.parse(input);
    const p = new URLSearchParams(query);
    const zcid = p.get('zcid');
    // Independently check the *desktop* identity encoded inside zcid, not just
    // the query value. A type-24 query with the old type-30 key must fail.
    if (p.get('type') !== '24' || p.get('client_version') !== '681') throw new Error('Wrong synthetic desktop identity');
    const deviceCipher = crypto.createDecipheriv('aes-256-cbc', Buffer.from('3FC4F0D2AB50057BCE0D90D9187A22B1'), Buffer.alloc(16));
    const device = Buffer.concat([deviceCipher.update(zcid, 'hex'), deviceCipher.final()]).toString('utf8').split(',');
    if (device.length !== 3 || device[0] !== '24') throw new Error('Wrong synthetic parameter identity');
    const hash = crypto.createHash('md5').update(p.get('zcid_ext')).digest('hex').toUpperCase();
    const even = x => [...x].filter((_, i) => i % 2 === 0).join('');
    const odd = x => [...x].filter((_, i) => i % 2 === 1).join('');
    const key = Buffer.from(even(hash).slice(0, 8) + even(zcid).slice(0, 12) + [...odd(zcid)].reverse().join('').slice(0, 12));
    const requestCipher = crypto.createDecipheriv('aes-256-cbc', key, Buffer.alloc(16));
    const payload = JSON.parse(Buffer.concat([requestCipher.update(p.get('params'), 'base64'), requestCipher.final()]).toString('utf8'));
    if (payload.imei !== device[1]) throw new Error('Synthetic device mismatch');
    const command = Object.hasOwn(payload, 'self_g') ? 'reqqr' : 'getlogininfo';
    const signed = [...p.keys()].filter(k => k !== 'signkey').sort().map(k => p.get(k)).join('');
    const signature = crypto.createHash('md5').update('zsecure' + command + signed).digest('hex');
    if (signature !== p.get('signkey')) throw new Error('Synthetic request signature mismatch');
    const reply = response || {error_code:0, data:{uid,dkey:'synthetic-storage',zpw_enk:'AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=',zpw_service_map_v3:{chat:['https://chat-wpa.zaloapp.com/'],group:['https://group-wpa.zaloapp.com/']},zpw_ws:['wss://ws1.zalo.me/']}};
    if (!response) {
        // Independent realistic map: unused CDN/route/extension schemas are not
        // messaging capabilities and must not make valid login fail.
        for (let i = 0; i < 100; i++) reply.data.zpw_service_map_v3['unused-cdn-' + i] = {future_schema:true, url:'http://cdn.invalid/asset/path'};
        reply.data.zpw_service_map_v3.analytics = ['https://metrics.invalid/collect'];
        reply.data.zpw_service_map_v3.profile = ['https://profile-wpa.zaloapp.com/'];
    }
    const plaintext = JSON.stringify(reply);
    const cipher = crypto.createCipheriv('aes-256-cbc', key, Buffer.alloc(16));
    const data = Buffer.concat([cipher.update(plaintext), cipher.final()]).toString('base64');
    process.stdout.write(JSON.stringify({error_code:0,data}));
});
