use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use hmac::{Hmac, Mac};
use rquickjs::{ArrayBuffer, Ctx, Exception};
use sha2::{Digest, Sha256, Sha512};
pub(crate) const LIBRARY: &str =
    include_str!("../../../../pc-dist/libs/libsignal-protocol.static.js");
pub(crate) const LIBRARY_SHA256: &str =
    "abbb3f578ede0d16c57b792a1c1793eb221dda07cf6f94919a3e76ee8a047966";
// 64KiB application text needs additional CBC padding/protobuf/MAC framing.
const MAX_INPUT: usize = 128 * 1024;
const MAX_RANDOM: usize = 65536;

fn copied_buffer(ctx: &Ctx<'_>, value: &ArrayBuffer<'_>) -> rquickjs::Result<Vec<u8>> {
    // No JS is re-entered while the slice is held; copy immediately so subsequent
    // allocations/calls cannot detach or resize a borrowed JS ArrayBuffer.
    let value =
        unsafe { value.as_bytes() }.ok_or_else(|| Exception::throw_type(ctx, "detached buffer"))?;
    if value.len() > MAX_INPUT {
        return Err(Exception::throw_range(ctx, "bridge input exceeds limit"));
    }
    Ok(value.to_vec())
}

pub(crate) fn bridge_crypto<'js>(
    ctx: Ctx<'js>,
    op: String,
    key: ArrayBuffer<'js>,
    data: ArrayBuffer<'js>,
    iv: ArrayBuffer<'js>,
) -> rquickjs::Result<ArrayBuffer<'js>> {
    let key = copied_buffer(&ctx, &key)?;
    let data = copied_buffer(&ctx, &data)?;
    let iv = copied_buffer(&ctx, &iv)?;
    let invalid = || Exception::throw_type(&ctx, "invalid cryptographic operation");
    let result = match op.as_str() {
        "sha256" if key.is_empty() && iv.is_empty() => Sha256::digest(&data).to_vec(),
        "sha512" if key.is_empty() && iv.is_empty() => Sha512::digest(&data).to_vec(),
        "hmac256" if !key.is_empty() && key.len() <= 128 && iv.is_empty() => {
            let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&key).map_err(|_| invalid())?;
            mac.update(&data);
            mac.finalize().into_bytes().to_vec()
        }
        "aes-encrypt" if key.len() == 32 && iv.len() == 16 => {
            cbc::Encryptor::<aes::Aes256>::new_from_slices(&key, &iv)
                .map_err(|_| invalid())?
                .encrypt_padded_vec_mut::<Pkcs7>(&data)
        }
        "aes-decrypt"
            if key.len() == 32 && iv.len() == 16 && !data.is_empty() && data.len() % 16 == 0 =>
        {
            cbc::Decryptor::<aes::Aes256>::new_from_slices(&key, &iv)
                .map_err(|_| invalid())?
                .decrypt_padded_vec_mut::<Pkcs7>(&data)
                .map_err(|_| invalid())?
        }
        _ => return Err(invalid()),
    };
    ArrayBuffer::new_copy(ctx, result)
}

pub(crate) fn bridge_random<'js>(
    ctx: Ctx<'js>,
    count: usize,
) -> rquickjs::Result<ArrayBuffer<'js>> {
    if count > MAX_RANDOM {
        return Err(Exception::throw_range(&ctx, "random request exceeds limit"));
    }
    let mut result = vec![0; count];
    getrandom::getrandom(&mut result)
        .map_err(|_| Exception::throw_internal(&ctx, "OS randomness unavailable"))?;
    ArrayBuffer::new_copy(ctx, result)
}

pub(crate) fn bridge_curve<'js>(
    ctx: Ctx<'js>,
    op: String,
    first: ArrayBuffer<'js>,
    second: ArrayBuffer<'js>,
    third: ArrayBuffer<'js>,
) -> rquickjs::Result<ArrayBuffer<'js>> {
    use libsignal_core::curve::{PrivateKey, PublicKey};
    use rand::SeedableRng;
    let first = copied_buffer(&ctx, &first)?;
    let second = copied_buffer(&ctx, &second)?;
    let third = copied_buffer(&ctx, &third)?;
    let invalid = || Exception::throw_type(&ctx, "invalid Curve input");
    let public = |raw: &[u8]| {
        if raw.len() != 32 {
            return Err(invalid());
        }
        let mut tagged = Vec::with_capacity(33);
        tagged.push(5);
        tagged.extend_from_slice(raw);
        PublicKey::deserialize(&tagged).map_err(|_| invalid())
    };
    let private = |raw: &[u8]| PrivateKey::deserialize(raw).map_err(|_| invalid());
    let output = match op.as_str() {
        "keypair" if first.len() == 32 && second.is_empty() && third.is_empty() => {
            let key = private(&first)?;
            let mut output = key.serialize();
            output.extend_from_slice(&key.public_key().map_err(|_| invalid())?.serialize());
            output
        }
        "agree" if first.len() == 32 && second.len() == 32 && third.is_empty() => private(&first)?
            .calculate_agreement(&public(&second)?)
            .map_err(|_| invalid())?
            .into_vec(),
        "sign" if first.len() == 32 && third.is_empty() => {
            let mut entropy = [0; 32];
            getrandom::getrandom(&mut entropy)
                .map_err(|_| Exception::throw_internal(&ctx, "OS randomness unavailable"))?;
            let mut rng = rand::rngs::StdRng::from_seed(entropy);
            private(&first)?
                .calculate_signature(&second, &mut rng)
                .map_err(|_| invalid())?
                .into_vec()
        }
        "verify" if first.len() == 32 && third.len() == 64 => {
            vec![u8::from(public(&first)?.verify_signature(&second, &third))]
        }
        _ => return Err(invalid()),
    };
    ArrayBuffer::new_copy(ctx, output)
}
