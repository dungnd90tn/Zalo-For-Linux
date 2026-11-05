(function() {
    const native=globalThis.__nativeCurve;
    delete globalThis.__nativeCurve;
    const empty=()=>new ArrayBuffer(0);
    Internal.curve25519.keyPair=function(seed){
        const wire=native('keypair',seed,empty(),empty());
        return {privKey:wire.slice(0,32),pubKey:wire.slice(33)};
    };
    Internal.curve25519.sharedSecret=function(publicKey,privateKey){return native('agree',privateKey,publicKey,empty())};
    Internal.curve25519.sign=function(privateKey,message){return native('sign',privateKey,message,empty())};
    Internal.curve25519.verify=function(publicKey,message,signature){
        // Existing sync leaf API returns FALSE on valid and TRUE on invalid.
        // The existing async wrapper converts invalid into rejected Promise.
        return new Uint8Array(native('verify',publicKey,message,signature))[0]!==1;
    };
})();
