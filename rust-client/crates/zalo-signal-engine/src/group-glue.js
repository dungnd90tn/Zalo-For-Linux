'use strict';
// The source wrapper has an inverted boolean and off-by-one signed slice.
// Correct only that glue. The mature Curve implementation does all cryptography.
__Group.Message.prototype.verifySignature=function(publicKey){
    const raw=this.serialize();
    if(raw.byteLength<=65||new Uint8Array(raw)[0]!==0x33||
       libsignal.Curve.verifySignature(publicKey,raw.slice(0,raw.byteLength-64),raw.slice(raw.byteLength-64))!==false){
        throw new Error('Invalid signature');
    }
};
