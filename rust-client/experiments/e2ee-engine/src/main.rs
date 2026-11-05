use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use hmac::{Hmac, Mac};
use rquickjs::{
    context::EvalOptions, ArrayBuffer, Context, Ctx, Exception, Function, Promise, Runtime,
};
use sha2::{Digest, Sha256, Sha512};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};
use std::time::Instant;

const LIBRARY: &str = include_str!("../../../../pc-dist/libs/libsignal-protocol.static.js");
const LIBRARY_SHA256: &str = "abbb3f578ede0d16c57b792a1c1793eb221dda07cf6f94919a3e76ee8a047966";
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

fn bridge_crypto<'js>(
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

fn bridge_random<'js>(ctx: Ctx<'js>, count: usize) -> rquickjs::Result<ArrayBuffer<'js>> {
    if count > MAX_RANDOM {
        return Err(Exception::throw_range(&ctx, "random request exceeds limit"));
    }
    let mut result = vec![0; count];
    getrandom::getrandom(&mut result)
        .map_err(|_| Exception::throw_internal(&ctx, "OS randomness unavailable"))?;
    ArrayBuffer::new_copy(ctx, result)
}

#[cfg(feature = "native-curve")]
fn bridge_curve<'js>(
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

fn rss_kib() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines().find_map(|line| {
                line.strip_prefix("VmRSS:")
                    .and_then(|s| s.split_whitespace().next())
                    .and_then(|s| s.parse().ok())
            })
        })
        .unwrap_or(0)
}

fn run() -> Result<(), String> {
    let start = Instant::now();
    let sha = format!("{:x}", Sha256::digest(LIBRARY.as_bytes()));
    if sha != LIBRARY_SHA256 {
        return Err("trusted library digest mismatch".into());
    }
    let runtime = Runtime::new().map_err(|_| "runtime creation failed")?;
    runtime.set_memory_limit(64 * 1024 * 1024);
    runtime.set_max_stack_size(2 * 1024 * 1024);
    let cancelled = Arc::new(AtomicBool::new(false));
    // Whole offline comparison suite has many independent operations. Production
    // per-operation ceilings are tested separately; this is a finite suite cap.
    let deadline = Arc::new(AtomicU64::new(120_000));
    let interrupt_cancel = cancelled.clone();
    let interrupt_deadline = deadline.clone();
    runtime.set_interrupt_handler(Some(Box::new(move || {
        interrupt_cancel.load(Ordering::Relaxed)
            || start.elapsed().as_millis() as u64 > interrupt_deadline.load(Ordering::Relaxed)
    })));
    let context = Context::full(&runtime).map_err(|_| "context creation failed")?;
    let loaded = context.with(|ctx| -> rquickjs::Result<()> {
        ctx.globals()
            .set("__nativeCrypto", Function::new(ctx.clone(), bridge_crypto)?)?;
        ctx.globals()
            .set("__nativeRandom", Function::new(ctx.clone(), bridge_random)?)?;
        ctx.globals().set(
            "__now",
            Function::new(ctx.clone(), move || start.elapsed().as_secs_f64() * 1000.)?,
        )?;
        ctx.globals()
            .set("__engine", "rquickjs-0.14.0/quickjs-ng-0.16.2")?;
        #[cfg(feature = "native-curve")]
        {
            ctx.globals()
                .set("__nativeCurve", Function::new(ctx.clone(), bridge_curve)?)?;
            ctx.globals().set(
                "__engine",
                "rquickjs-0.14.0/quickjs-ng-0.16.2+signal-core-904c8dc",
            )?;
        }
        ctx.eval::<(), _>(include_str!("bridge.js"))?;
        let mut original_options = EvalOptions::default();
        original_options.strict = false;
        #[cfg(feature = "native-curve")]
        let source = {
            const ANCHOR: &str = "Internal.Curve = n(Internal.curve25519)";
            assert_eq!(
                LIBRARY.matches(ANCHOR).count(),
                1,
                "pinned primitive hook anchor"
            );
            LIBRARY.replacen(
                ANCHOR,
                &format!("{}\n{ANCHOR}", include_str!("native-curve-glue.js")),
                1,
            )
        };
        #[cfg(not(feature = "native-curve"))]
        let source = LIBRARY;
        ctx.eval_with_options::<(), _>(source, original_options)?;
        ctx.eval::<(), _>(include_str!(concat!(env!("OUT_DIR"), "/group.js")))?;
        Ok(())
    });
    runtime.run_gc();
    let cold_ms = start.elapsed().as_secs_f64() * 1000.;
    let idle_rss = rss_kib();
    let memory = runtime.memory_usage();
    let result = loaded.and_then(|()| {
        context.with(|ctx| -> rquickjs::Result<String> {
            ctx.globals().set("__coldMs", cold_ms)?;
            ctx.globals().set("__idleRssKiB", idle_rss)?;
            ctx.globals()
                .set("__jsHeapBytes", memory.memory_used_size)?;
            ctx.globals().set(
                "__fixtureJson",
                include_str!("../../../tests/fixtures/e2ee/session.json"),
            )?;
            ctx.globals().set(
                "__vectorsJson",
                include_str!("../../../tests/fixtures/e2ee/vectors.json"),
            )?;
            ctx.eval::<(), _>(include_str!("workload.js"))?;
            let promise: Promise =
                ctx.eval("runWorkload(JSON.parse(__fixtureJson), JSON.parse(__vectorsJson))")?;
            promise.finish()
        })
    });
    match result {
        Ok(value) => println!("{value}"),
        Err(error) => {
            eprintln!("research Rust/JS boundary error: {error:?}");
            context.with(|ctx| {
                eprintln!(
                    "research phase: {:?}; elapsed {:?}",
                    ctx.globals().get::<_, String>("__phase"),
                    start.elapsed()
                );
                let value = ctx.catch();
                eprintln!("research exception: {value:?}");
                if let Some(object) = value.as_object() {
                    if let Ok(message) = object.get::<_, String>("stack") {
                        eprintln!("{message}");
                    }
                }
            });
            return Err("isolated engine failed; runtime discarded".into());
        }
    }
    drop(context);
    drop(runtime);
    Ok(())
}

fn main() {
    // Runtime and callbacks are created/used/dropped on this one OS worker.
    let result = std::thread::Builder::new()
        .name("e2ee-experiment".into())
        .stack_size(4 * 1024 * 1024)
        .spawn(run)
        .expect("spawn isolated experiment worker")
        .join()
        .expect("worker panicked");
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    struct RestrictedEngine {
        context: Context,
        _runtime: Runtime,
        poisoned: bool,
    }

    impl RestrictedEngine {
        fn new(memory: usize, stack: usize, duration: Duration, cancel: Arc<AtomicBool>) -> Self {
            let runtime = Runtime::new().unwrap();
            runtime.set_memory_limit(memory);
            runtime.set_max_stack_size(stack);
            let until = Instant::now() + duration;
            runtime.set_interrupt_handler(Some(Box::new(move || {
                cancel.load(Ordering::Acquire) || Instant::now() >= until
            })));
            let context = Context::full(&runtime).unwrap();
            Self {
                context,
                _runtime: runtime,
                poisoned: false,
            }
        }
        fn execute(&mut self, trusted_source: &'static str) -> Result<(), &'static str> {
            if self.poisoned {
                return Err("poisoned");
            }
            let result = self.context.with(|ctx| ctx.eval::<(), _>(trusted_source));
            if result.is_err() {
                self.poisoned = true;
                self.context.with(|ctx| {
                    let _ = ctx.catch();
                });
                Err("discarded")
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn memory_limit_rejects_allocation_and_poisoned_runtime_rejects_reuse() {
        let mut engine = RestrictedEngine::new(
            64 * 1024 * 1024,
            2 * 1024 * 1024,
            Duration::from_secs(2),
            Arc::default(),
        );
        assert_eq!(
            engine.execute("new ArrayBuffer(96*1024*1024);"),
            Err("discarded")
        );
        assert_eq!(engine.execute("1+1;"), Err("poisoned"));
    }

    #[test]
    fn stack_limit_rejects_recursive_script() {
        // Rust's default test-thread stack is only 2MiB: equal to the JS ceiling
        // leaves no host frames. Match the actual 4MiB production-worker layout.
        std::thread::Builder::new()
            .stack_size(4 * 1024 * 1024)
            .spawn(|| {
                let mut engine = RestrictedEngine::new(
                    64 * 1024 * 1024,
                    2 * 1024 * 1024,
                    Duration::from_secs(2),
                    Arc::default(),
                );
                assert_eq!(
                    engine.execute("(function recurse(){return 1+recurse()})();"),
                    Err("discarded")
                );
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn deadline_interrupts_busy_script_and_discards_runtime() {
        let started = Instant::now();
        let mut engine = RestrictedEngine::new(
            64 * 1024 * 1024,
            2 * 1024 * 1024,
            Duration::from_millis(25),
            Arc::default(),
        );
        assert_eq!(engine.execute("while(true){}"), Err("discarded"));
        assert!(started.elapsed() < Duration::from_secs(1));
        assert_eq!(engine.execute("1+1;"), Err("poisoned"));
    }

    #[test]
    fn operator_cancel_interrupts_running_script() {
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        let mut engine = RestrictedEngine::new(
            64 * 1024 * 1024,
            2 * 1024 * 1024,
            Duration::from_secs(2),
            cancel,
        );
        let sender = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(25));
            stop.store(true, Ordering::Release)
        });
        let started = Instant::now();
        assert_eq!(engine.execute("while(true){}"), Err("discarded"));
        sender.join().unwrap();
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn no_filesystem_network_or_module_loader_bindings() {
        let mut engine = RestrictedEngine::new(
            64 * 1024 * 1024,
            2 * 1024 * 1024,
            Duration::from_secs(2),
            Arc::default(),
        );
        engine.execute("for(const name of ['require','process','std','os','fetch','WebSocket','XMLHttpRequest','Deno','Bun']){if(typeof globalThis[name]!=='undefined')throw new Error('unexpected host API')} ").unwrap();
    }

    #[test]
    fn json_values_are_data_not_evaluated_source() {
        let mut engine = RestrictedEngine::new(
            64 * 1024 * 1024,
            2 * 1024 * 1024,
            Duration::from_secs(2),
            Arc::default(),
        );
        engine.context.with(|ctx| {
            ctx.globals()
                .set(
                    "untrustedJson",
                    r#"{"message":"');globalThis.injected=true;//"}"#,
                )
                .unwrap();
        });
        engine.execute("const data=JSON.parse(untrustedJson);if(data.message!==\"');globalThis.injected=true;//\"||typeof injected!=='undefined')throw new Error('data executed');").unwrap();
    }

    #[test]
    fn promise_microtask_loop_is_interrupted() {
        let cancel = Arc::new(AtomicBool::new(false));
        let mut engine = RestrictedEngine::new(
            64 * 1024 * 1024,
            2 * 1024 * 1024,
            Duration::from_millis(25),
            cancel,
        );
        let started = Instant::now();
        let failed = engine.context.with(|ctx| {
            let promise: Promise = ctx
                .eval("(function spin(){return Promise.resolve().then(spin)})()")
                .unwrap();
            promise.finish::<()>().is_err()
        });
        assert!(failed);
        assert!(started.elapsed() < Duration::from_secs(1));
        engine.poisoned = true;
        assert_eq!(engine.execute("1+1;"), Err("poisoned"));
    }

    #[test]
    fn crypto_bridge_checks_typed_array_offsets_and_primitive_known_answers() {
        let mut engine = RestrictedEngine::new(
            64 * 1024 * 1024,
            2 * 1024 * 1024,
            Duration::from_secs(2),
            Arc::default(),
        );
        engine.context.with(|ctx|{
            ctx.globals().set("__nativeCrypto",Function::new(ctx.clone(),bridge_crypto).unwrap()).unwrap();
            ctx.globals().set("__nativeRandom",Function::new(ctx.clone(),bridge_random).unwrap()).unwrap();
            ctx.eval::<(),_>(include_str!("bridge.js")).unwrap();
            let promise:Promise=ctx.eval(r#"(async()=>{
                const hex=x=>Array.from(new Uint8Array(x),v=>v.toString(16).padStart(2,'0')).join('');
                const raw=new Uint8Array([99,97,98,99,99]);
                if(hex(await crypto.subtle.digest('SHA-256',raw.subarray(1,4)))!=='ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad')throw new Error('offset digest');
                const out=new Uint8Array(66);out.fill(7);crypto.getRandomValues(out.subarray(1,65));
                if(out[0]!==7||out[65]!==7||out.subarray(1,65).every(x=>x===7))throw new Error('random offsets');
                let rejected=false;try{crypto.getRandomValues(new Float64Array(4))}catch(e){rejected=true}if(!rejected)throw new Error('float RNG accepted');
                rejected=false;try{await crypto.subtle.digest('SHA-256',new ArrayBuffer(131073))}catch(e){rejected=true}if(!rejected)throw new Error('oversized native input accepted');
            })()"#).unwrap();
            promise.finish::<()>().unwrap();
        });
        engine.execute("if(typeof __nativeCrypto!=='undefined'||typeof __nativeRandom!=='undefined')throw new Error('native bridge leaked');").unwrap();
    }
}
