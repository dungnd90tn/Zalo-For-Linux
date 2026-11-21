#![cfg(feature = "messaging")]
//! Independent original-source wire oracle; no image or network IO.
use serde_json::{json, Value};
use zalo_rust::protocol::{
    bootstrap::PhotoUploadRoute,
    client::ClientType,
    messaging::{Target, ThreadKind},
    photo::{self, PhotoUrls},
    upload,
};

#[test]
fn ordinary_hd_photo_matches_executed_source_routes_chunks_resource_and_caption_submission() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/messaging/photo_upload.json")).unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        let group = case["group"].as_bool().unwrap();
        let kind = if group {
            ThreadKind::Group
        } else {
            ThreadKind::Direct
        };
        let target = Target::new(kind, if group { "33" } else { "22" }).unwrap();
        let route = if case["route"] == "photo" {
            PhotoUploadRoute::Photo
        } else {
            PhotoUploadRoute::File
        };
        let root = url::Url::parse("https://wpa.zaloapp.com").unwrap();
        let url = photo::endpoint(&root, kind, ClientType::PcLinux24, Some(route)).unwrap();
        for (index, chunk) in case["chunks"].as_array().unwrap().iter().enumerate() {
            assert_eq!(
                upload::chunk_parameters(
                    &target,
                    "photo.jpg",
                    6,
                    7001,
                    "synthetic-device",
                    index as u64 + 1,
                    2
                )
                .unwrap(),
                chunk["payload"]
            );
            let original = url::Url::parse(chunk["url"].as_str().unwrap()).unwrap();
            assert_eq!(url.path(), original.path());
            assert_eq!(
                url.query_pairs().find(|(key, _)| key == "type").unwrap().1,
                original
                    .query_pairs()
                    .find(|(key, _)| key == "type")
                    .unwrap()
                    .1
            );
        }
        let u = &case["urls"];
        let urls = PhotoUrls::from_value(&json!({
            "org":u["org"],"thumb":u["thumb"],"hd":u["hd"],"thumbSize":1,"fileSize":6,"hdSize":4
        }))
        .unwrap();
        assert_eq!(
            photo::message_parameters(
                &target,
                "synthetic-photo",
                7001,
                case["caption"].as_str().unwrap(),
                640,
                480,
                "synthetic-device",
                &urls
            )
            .unwrap(),
            case["final"]["payload"]
        );
        let final_url = photo::endpoint(&root, kind, ClientType::PcLinux24, None).unwrap();
        assert_eq!(
            final_url.path(),
            url::Url::parse(case["final"]["url"].as_str().unwrap())
                .unwrap()
                .path()
        );
        assert!(!final_url.query_pairs().any(|(k, _)| k == "type"));
    }
}

#[test]
fn album_members_match_the_bundled_picker_and_per_photo_submission() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/messaging/photo_album_upload.json")).unwrap();
    let urls = PhotoUrls::from_value(&json!({
        "org":"https://photos.example.test/original","thumb":"https://photos.example.test/thumb",
        "hd":"https://photos.example.test/hd","thumbSize":1,"fileSize":6,"hdSize":4
    }))
    .unwrap();
    let base_media = photo::media_content("", 640, 480, &urls).unwrap();
    for case in fixture["albums"].as_array().unwrap() {
        let group = case["group"].as_bool().unwrap();
        let kind = if group {
            ThreadKind::Group
        } else {
            ThreadKind::Direct
        };
        let target = Target::new(kind, if group { "33" } else { "22" }).unwrap();
        let layout_id = case["layoutId"].as_str().unwrap().parse().unwrap();
        let extra = case["extraMetadata"].as_bool().unwrap();
        for (index, final_request) in case["finals"].as_array().unwrap().iter().enumerate() {
            let expected = &final_request["payload"];
            let client_id = expected["clientId"].as_u64().unwrap();
            let mut request = photo::message_parameters(
                &target,
                "synthetic-photo",
                client_id,
                "",
                640,
                480,
                "synthetic-device",
                &urls,
            )
            .unwrap();
            let album = photo::AlbumMember::new(layout_id, index as u32, 3).unwrap();
            album
                .apply_to_message_parameters(&mut request, extra)
                .unwrap();
            assert_eq!(&request, expected);
            assert_eq!(
                photo::AlbumMember::from_message_parameters(&request),
                Some(album)
            );
            let content = album.apply_to_media_content(&base_media).unwrap();
            assert_eq!(content.album_member(), Some(album));
            let mut wrong = request;
            wrong["idInGroup"] = json!(3);
            assert!(photo::has_album_message_fields(&wrong));
            assert_eq!(photo::AlbumMember::from_message_parameters(&wrong), None);
            wrong["idInGroup"] = json!(index);
            wrong["extMsgProp"] = json!("{\"groupMediaMsg\":{\"groupLayoutId\":\"999\"}}");
            assert_eq!(photo::AlbumMember::from_message_parameters(&wrong), None);
        }
    }
}

#[test]
fn authenticated_album_config_uses_source_defaults_and_honors_a_smaller_count() {
    use zalo_rust::protocol::bootstrap::ServerConfig;
    let defaults = json!({"zpw_service_map_v3":{"chat":["https://wpa.zaloapp.com"]}});
    let config = ServerConfig::parse(&defaults).unwrap();
    assert_eq!(config.photo_album_max_count, 9);
    assert!(config.photo_album_extra_metadata);
    let mut selected = defaults;
    selected["setttings"] = json!({
        "sharefile":{"max_file":3},"features":{"should_attach_extraMsgProp":0}
    });
    let config = ServerConfig::parse(&selected).unwrap();
    assert_eq!(config.photo_album_max_count, 3);
    assert!(!config.photo_album_extra_metadata);
    selected["settings"]["sharefile"]["max_file"] = json!(4);
    assert_eq!(
        ServerConfig::parse(&selected)
            .unwrap()
            .photo_album_max_count,
        0
    );
    selected["setttings"]["sharefile"]["max_file"] = json!("malformed");
    assert_eq!(
        ServerConfig::parse(&selected)
            .unwrap()
            .photo_album_max_count,
        0
    );
}

#[test]
fn photo_completion_requires_matching_action_and_safe_urls_with_source_fallbacks() {
    for data in [
        json!({"thumb":"https://photos.example.test/t"}),
        json!({"hd":"https://photos.example.test/h"}),
        json!({"org":"https://photos.example.test/o"}),
    ] {
        let payload = json!({"error_code":0,"data":{"controls":[{"content":{"act_type":"photo_success","act":"photo","fileId":"7","data":data}}]}});
        let parsed = photo::photo_success(&payload);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].1.org, parsed[0].1.thumb);
        assert_eq!(parsed[0].1.org, parsed[0].1.hd);
        let mut wrong = payload.clone();
        wrong["data"]["controls"][0]["content"]["act"] = json!("file");
        assert!(photo::photo_success(&wrong).is_empty());
        let mut string = payload;
        string["data"]["controls"][0]["content"]["data"] = json!(data.to_string());
        assert_eq!(photo::photo_success(&string), parsed);
    }
    for raw in [
        "http://photos.example.test/o",
        "https://user:secret@photos.example.test/o",
        "https://photos.example.test:444/o",
        "https://photos.example.test/o#f",
    ] {
        assert!(photo::photo_success(&json!({"error_code":0,"data":{"controls":[{"content":{"act_type":"photo_success","act":"photo","fileId":"7","data":{"org":raw}}}]}})).is_empty());
    }
    assert!(photo::chunk_result(
        &json!({"error_code":1,"data":{"photoId":"7","normalUrl":"https://photos.example.test/o"}})
    )
    .is_err());
    assert!(
        photo::chunk_result(&json!({"error_code":0,"data":{"finished":1,"fileId":"7"}})).is_err()
    );
}

#[test]
fn real_feature_settings_bound_photo_without_breaking_other_messaging_capabilities() {
    use zalo_rust::protocol::bootstrap::ServerConfig;
    let base = json!({"zpw_service_map_v3":{"chat":["https://wpa.zaloapp.com"],"group":["https://wpa.zaloapp.com"],"file":["https://wpa.zaloapp.com"]}});
    for spelling in ["settings", "setttings"] {
        let mut value = base.clone();
        value[spelling] = json!({"features":{"file_photo_limit":{"resolutionLimit":{"hd":{"width":80,"height":40,"size":12345,"maxInputWidth":160,"maxInputHeight":80,"maxInputSize":10000}}}}});
        let c = ServerConfig::parse(&value).unwrap();
        assert_eq!(c.photo_upload_route, Some(PhotoUploadRoute::File));
        assert_eq!(
            (
                c.photo_limits.width,
                c.photo_limits.height,
                c.photo_limits.size
            ),
            (80, 40, 12345)
        );
        assert_eq!(
            (
                c.photo_limits.input_width,
                c.photo_limits.input_height,
                c.photo_limits.input_size
            ),
            (160, 80, 10000)
        );
        for malformed in [
            json!("unexpected"),
            json!([]),
            json!({"resolutionLimit":"unexpected"}),
            json!({"resolutionLimit":{"hd":[]}}),
            json!({"enable_new_endpoint_upload_photo":9}),
            json!({"enable_new_endpoint_upload_photo":1}),
        ] {
            let mut unavailable = base.clone();
            unavailable[spelling] = json!({"features":{"file_photo_limit":malformed}});
            let c = ServerConfig::parse(&unavailable)
                .expect("Optional photo configuration cannot reject authentication");
            assert!(c.photo_upload_route.is_none());
            for service in ["chat", "group", "file"] {
                assert!(c.services.contains_key(service));
            }
        }
        let mut defaults = base.clone();
        defaults[spelling] = json!({"features":{"file_photo_limit":null}});
        let c = ServerConfig::parse(&defaults).unwrap();
        assert_eq!(c.photo_upload_route, Some(PhotoUploadRoute::File));
        assert_eq!((c.photo_limits.width, c.photo_limits.height), (2560, 2560));
    }
    for bad in [json!(0), json!(-1), json!("80"), json!({})] {
        let mut value = base.clone();
        value["settings"] =
            json!({"features":{"file_photo_limit":{"resolutionLimit":{"hd":{"width":bad}}}}});
        let c = ServerConfig::parse(&value).unwrap();
        assert!(c.photo_upload_route.is_none());
        assert!(c.services.contains_key("file"));
    }
}

#[test]
fn original_nested_file_handler_and_chunk_settings_are_not_ignored_by_photo_or_file() {
    use zalo_rust::protocol::bootstrap::ServerConfig;
    let base = json!({"zpw_service_map_v3":{"chat":["https://wpa.zaloapp.com"],"group":["https://wpa.zaloapp.com"],"file":["https://wpa.zaloapp.com"]}});
    for spelling in ["settings", "setttings"] {
        let mut raw = base.clone();
        raw[spelling] = json!({"features":{"sharefile":{"chunk_size_file":128000},"multi_site_upload":{"enable_multi_site_api":false,"enable_my_cloud":false}}});
        let c = ServerConfig::parse(&raw).unwrap();
        assert_eq!(c.file_chunk_size, 128000);
        assert!(c.services.contains_key("file"));
        assert_eq!(c.photo_upload_route, Some(PhotoUploadRoute::File));
        for flag in ["enable_multi_site_api", "enable_my_cloud"] {
            let mut unsupported = raw.clone();
            unsupported[spelling]["features"]["multi_site_upload"][flag] = json!(true);
            let c = ServerConfig::parse(&unsupported).unwrap();
            assert!(!c.services.contains_key("file"));
            assert!(c.photo_upload_route.is_none());
            assert!(c.services.contains_key("chat"));
            assert!(c.services.contains_key("group"));
        }
    }
}
