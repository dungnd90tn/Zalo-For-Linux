//! Source-backed Zalo catalog keyword search and sticker-detail correlation.
use super::{
    client::ClientType,
    messaging::{self as wire, WireError},
};
use crate::domain::{StickerDescriptor, MAX_STICKER_ID};
use serde_json::{json, Value};
use std::collections::HashSet;
use url::Url;

pub(crate) const SEARCH_PATH: &str = "/api/message/sticker/search";
pub(crate) const DETAIL_PATH: &str = "/api/message/sticker/sticker_detail";
const MAX_QUERY_BYTES: usize = 256;
const MAX_QUERY_CHARS: usize = 64;
const MAX_RESULTS: usize = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Address {
    sticker_id: u64,
    category_id: u64,
}

pub(crate) fn valid_query(query: &str) -> bool {
    !query.is_empty()
        && query == query.trim()
        && query.len() <= MAX_QUERY_BYTES
        && query.chars().count() <= MAX_QUERY_CHARS
        && !query.chars().any(char::is_control)
}

pub(crate) fn url(
    root: &Url,
    path: &'static str,
    client_type: ClientType,
    encrypted_params: &str,
) -> Result<Url, WireError> {
    wire::validate_origin(root, "https")?;
    let mut url = root.clone();
    url.set_path(path);
    url.query_pairs_mut()
        .append_pair("zpw_ver", wire::API_VERSION)
        .append_pair("zpw_type", client_type.api_type())
        .append_pair("params", encrypted_params);
    Ok(url)
}

pub(crate) fn search_parameters(query: &str, imei: &str) -> Result<Value, WireError> {
    if !valid_query(query) || imei.is_empty() || imei.len() > 256 {
        return Err(WireError::Invalid);
    }
    // The ordinary picker uses srcType=0. This scope excludes Tenor and photo stickers.
    Ok(json!({"keyword":query,"limit":MAX_RESULTS,"srcType":0,"imei":imei}))
}

fn known_id(value: &Value) -> Option<u64> {
    let id: u64 = wire::id_string(value).ok()?.parse().ok()?;
    (id <= MAX_STICKER_ID).then_some(id)
}

pub(crate) fn search_results(payload: &Value) -> Result<Vec<Address>, WireError> {
    wire::service_code(payload)?;
    let rows = payload
        .get("data")
        .and_then(Value::as_array)
        .ok_or(WireError::Invalid)?;
    if rows.len() > MAX_RESULTS {
        return Err(WireError::Oversize);
    }
    let mut seen = HashSet::new();
    Ok(rows
        .iter()
        .filter_map(|row| {
            let address = Address {
                sticker_id: known_id(row.get("sticker_id")?)?,
                category_id: known_id(row.get("cate_id")?)?,
            };
            seen.insert(address).then_some(address)
        })
        .collect())
}

pub(crate) fn detail_parameters(address: Address) -> Value {
    json!({"sid":address.sticker_id})
}

pub(crate) fn detail(
    payload: &Value,
    requested: Address,
) -> Result<Option<StickerDescriptor>, WireError> {
    wire::service_code(payload)?;
    let row = payload.get("data").ok_or(WireError::Invalid)?;
    if row.is_null() {
        return Ok(None);
    }
    let id = row.get("id").and_then(known_id).ok_or(WireError::Invalid)?;
    let category = row
        .get("cateId")
        .and_then(known_id)
        .ok_or(WireError::Invalid)?;
    if (id, category) != (requested.sticker_id, requested.category_id) {
        return Err(WireError::Invalid);
    }
    let kind = row
        .get("type")
        .and_then(known_id)
        .ok_or(WireError::Invalid)?;
    let sticker = StickerDescriptor {
        sticker_id: id,
        category_id: category,
        sticker_type: kind.try_into().map_err(|_| WireError::Invalid)?,
    };
    // The picker filters hidden entries. Effect and personalized payloads use
    // extra metadata or separate submission paths. Asset URLs are discarded.
    let ordinary = [
        "status",
        "effectId",
        "fssInfo",
        "extInfo",
        "isPhotoSticker",
        "isAISticker",
    ]
    .iter()
    .all(|key| {
        row.get(key).is_none_or(|value| {
            value.is_null()
                || value.as_bool() == Some(false)
                || value.as_u64() == Some(0)
                || matches!(value.as_str(), Some("" | "0"))
        })
    });
    Ok((ordinary && sticker.is_catalog()).then_some(sticker))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_supported_search_and_detail_filtering_only_admit_correlated_catalog_stickers() {
        assert_eq!(
            search_parameters("chào", "fixture").unwrap(),
            json!({"keyword":"chào","limit":10,"srcType":0,"imei":"fixture"})
        );
        assert!(!valid_query(" chào") && !valid_query("a\tb") && !valid_query(&"á".repeat(65)));
        let addresses = search_results(&json!({"error_code":0,"data":[
            {"cate_id":"10047","sticker_id":"20695"},
            {"cate_id":10047,"sticker_id":20695},
            {"cate_id":10047,"sticker_id":0},
            {"cate_id":10047,"sticker_id":"012"},
            {"cate_id":0,"sticker_id":1},
            {"cate_id":10047,"sticker_id":9_007_199_254_740_992_u64},
            {"cate_id":10047,"sticker_id":"https://outside.invalid"}
        ]}))
        .unwrap();
        assert_eq!(addresses.len(), 1);
        assert_eq!(detail_parameters(addresses[0]), json!({"sid":20695}));
        let frame = |data| json!({"error_code":0,"data":data});
        assert!(detail(
            &frame(json!({"id":20695,"cateId":10048,"type":3})),
            addresses[0]
        )
        .is_err());
        assert_eq!(
            detail(
                &frame(json!({"id":20695,"cateId":10047,"type":2})),
                addresses[0]
            ),
            Ok(None)
        );
        assert_eq!(
            detail(
                &frame(json!({"id":20695,"cateId":10047,"type":3,"status":1})),
                addresses[0]
            ),
            Ok(None)
        );
        assert_eq!(
            detail(
                &frame(json!({"id":20695,"cateId":10047,"type":3,"fssInfo":"custom"})),
                addresses[0]
            ),
            Ok(None)
        );
        assert_eq!(
            detail(
                &frame(
                    json!({"id":20695,"cateId":"10047","type":"3","stickerUrl":"https://outside.invalid"})
                ),
                addresses[0]
            ),
            Ok(Some(StickerDescriptor {
                sticker_id: 20695,
                category_id: 10047,
                sticker_type: 3
            }))
        );
        assert!(search_results(&frame(json!({}))).is_err());
        assert_eq!(search_results(&frame(json!([]))), Ok(vec![]));
    }
}
