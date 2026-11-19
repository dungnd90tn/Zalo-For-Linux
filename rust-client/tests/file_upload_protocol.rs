#![cfg(feature = "messaging")]
//! Compare the native raw-file handler against executed original builders.
use serde_json::Value;
use zalo_rust::protocol::{
    client::ClientType,
    messaging::{Target, ThreadKind},
    upload,
};

#[test]
fn upload_chunk_and_final_message_match_the_original_file_builders() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/messaging/file_upload.json")).unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        let group = case["group"].as_bool().unwrap();
        let target = Target::new(
            if group {
                ThreadKind::Group
            } else {
                ThreadKind::Direct
            },
            if group { "33" } else { "22" },
        )
        .unwrap();
        let name = case["name"].as_str().unwrap();
        for (index, chunk) in case["chunks"].as_array().unwrap().iter().enumerate() {
            assert_eq!(
                upload::chunk_parameters(
                    &target,
                    name,
                    6,
                    7001,
                    "synthetic-device",
                    index as u64 + 1,
                    2
                )
                .unwrap(),
                chunk["payload"]
            );
            let url = upload::endpoint(
                &url::Url::parse("https://wpa.zaloapp.com").unwrap(),
                target.kind,
                ClientType::PcLinux24,
                true,
            )
            .unwrap();
            let original = url::Url::parse(chunk["url"].as_str().unwrap()).unwrap();
            assert_eq!(url.path(), original.path());
            assert_eq!(
                url.query_pairs().find(|(k, _)| k == "type").unwrap().1,
                original.query_pairs().find(|(k, _)| k == "type").unwrap().1
            );
        }
        let parameters = upload::message_parameters(
            &target,
            name,
            &upload::source_extension(name),
            6,
            case["checksum"].as_str().unwrap(),
            7001,
            "synthetic-device",
            "synthetic-file",
            "https://files.example.test/resource",
        )
        .unwrap();
        assert_eq!(
            parameters, case["final"]["payload"],
            "Original final file contract for {name}"
        );
        let final_url = upload::endpoint(
            &url::Url::parse("https://wpa.zaloapp.com").unwrap(),
            target.kind,
            ClientType::PcLinux24,
            false,
        )
        .unwrap();
        assert_eq!(
            final_url.path(),
            url::Url::parse(case["final"]["url"].as_str().unwrap())
                .unwrap()
                .path()
        );
        assert!(!final_url.query_pairs().any(|(key, _)| key == "type"));
    }
}

#[test]
fn unknown_controls_cannot_authorize_a_file_url_and_completion_is_bounded() {
    use serde_json::json;
    let controls=(0..100).map(|n|json!({"content":{"act_type":"file_done","fileId":n+1,"data":{"url":format!("https://files.example.test/{n}")}}})).collect::<Vec<_>>();
    let parsed = upload::file_done(&json!({"error_code":0,"data":{"controls":controls}}));
    assert_eq!(parsed.len(), 64);
    for value in [
        json!({"url":"http://files.example.test/f"}),
        json!({"url":"https://user:secret@files.example.test/f"}),
        json!({"url":"https://files.example.test:444/f"}),
        json!({"url":"https://files.example.test/f#fragment"}),
        json!(false),
    ] {
        assert!(upload::file_done(&json!({"error_code":0,"data":{"controls":[{"content":{"act_type":"file_done","fileId":"7","data":value}}]}})).is_empty());
    }
    assert!(upload::file_done(&json!({"error_code":0,"data":{"controls":[{"content":{"act_type":"not_file_done","fileId":"7","data":{"url":"https://files.example.test/f"}}}]}})).is_empty());
    assert!(upload::chunk_result(&json!({"error_code":1,"data":{"fileId":"7"}})).is_err());
    assert!(
        upload::chunk_result(&json!({"error_code":2,"data":{"fileId":"7","finished":1}})).is_err()
    );
}
