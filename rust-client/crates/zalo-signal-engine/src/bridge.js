'use strict';
globalThis.window = globalThis;
globalThis.console = Object.freeze({log(){},error(){},warn(){}});
globalThis.setTimeout = function() { throw new Error('timers unavailable in isolated engine'); };
globalThis.clearTimeout = function() {};
(function() {
    const native = __nativeCrypto, random = __nativeRandom;
    delete globalThis.__nativeCrypto;
    delete globalThis.__nativeRandom;
    function copy(value) {
        if (value instanceof ArrayBuffer) return value.slice(0);
        if (ArrayBuffer.isView(value)) return value.buffer.slice(value.byteOffset, value.byteOffset + value.byteLength);
        throw new TypeError('BufferSource required');
    }
    const empty = () => new ArrayBuffer(0);
    function name(value) { return typeof value === 'string' ? value : value.name; }
    const subtle = Object.freeze({
        async importKey(format, raw, algorithm, extractable, usages) {
            if (format !== 'raw' || extractable !== false || !Array.isArray(usages)) throw new TypeError('unsupported key import');
            const type = name(algorithm), bytes = copy(raw);
            if (!['AES-CBC', 'HMAC'].includes(type)) throw new TypeError('unsupported algorithm');
            if (type === 'HMAC' && name(algorithm.hash) !== 'SHA-256') throw new TypeError('unsupported HMAC');
            return Object.freeze({type, bytes, usages: usages.slice()});
        },
        async encrypt(algorithm, key, data) {
            if (name(algorithm) !== 'AES-CBC' || key.type !== 'AES-CBC' || !key.usages.includes('encrypt')) throw new TypeError('unsupported encryption');
            return native('aes-encrypt', key.bytes, copy(data), copy(algorithm.iv));
        },
        async decrypt(algorithm, key, data) {
            if (name(algorithm) !== 'AES-CBC' || key.type !== 'AES-CBC' || !key.usages.includes('decrypt')) throw new TypeError('unsupported decryption');
            return native('aes-decrypt', key.bytes, copy(data), copy(algorithm.iv));
        },
        async sign(algorithm, key, data) {
            if (name(algorithm) !== 'HMAC' || key.type !== 'HMAC' || !key.usages.includes('sign')) throw new TypeError('unsupported signing');
            return native('hmac256', key.bytes, copy(data), empty());
        },
        async digest(algorithm, data) {
            const type = name(algorithm);
            if (!['SHA-256','SHA-512'].includes(type)) throw new TypeError('unsupported hash');
            return native(type === 'SHA-512' ? 'sha512' : 'sha256', empty(), copy(data), empty());
        },
    });
    globalThis.crypto = Object.freeze({subtle, getRandomValues(target) {
        if (!ArrayBuffer.isView(target) || target instanceof DataView || target instanceof Float32Array || target instanceof Float64Array || target.byteLength > 65536) throw new TypeError('integer TypedArray <=64KiB required');
        new Uint8Array(target.buffer, target.byteOffset, target.byteLength).set(new Uint8Array(random(target.byteLength)));
        return target;
    }});
})();
globalThis.TextEncoder = class { encode(value) { return new Uint8Array(dcodeIO.ByteBuffer.wrap(String(value),'utf8').toArrayBuffer()); } };
