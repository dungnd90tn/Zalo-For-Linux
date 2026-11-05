use std::{env, fs, path::PathBuf};

fn main() {
    if env::var_os("CARGO_FEATURE_EMBEDDED").is_none() {
        return;
    }
    let source_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../pc-dist/lazy/default-login-main-startup-shared-worker-znotification.9e3e92e88644da772301.js");
    println!("cargo:rerun-if-changed={}", source_path.display());
    let source = fs::read_to_string(source_path).expect("read repository group implementation");
    let markers = [
        ("            class Ot {", "            var wt = new class {"),
        (
            "            class Ft extends St {",
            "            const Xt =",
        ),
        ("            class Jt {", "            var Zt = Jt,"),
        ("            class en {", "            var tn = en;"),
    ];
    let mut output = String::from("(function(){ const i={a:Object.assign},Xt={zsymb(){}}; const wt={queueJob(name,job){return Promise.resolve().then(job)}}; class we extends Error{constructor(code){super(code);this.code=code}} const Me={RECEIVE_MESSAGE_WITH_OLD_COUNTER:'old-counter',OVER_MESSAGE_KEYS:'future-counter',SENDER_KEY_STATE_NOT_FOUND:'missing-state',NO_SENKER_KEY_FOUND:'missing-key'};\n");
    for (start, end) in markers {
        assert_eq!(
            source.matches(start).count(),
            1,
            "unique original group source anchor"
        );
        let offset = source.find(start).unwrap();
        let finish = offset + source[offset..].find(end).expect("group end anchor");
        output.push_str(&source[offset..finish]);
        output.push('\n');
    }
    output.push_str(
        "globalThis.__Group={Chain:Qt,Message:xt,Cipher:Jt,Record:en,Distribution:Mt};})()",
    );
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("group.js"),
        output,
    )
    .unwrap();
}
