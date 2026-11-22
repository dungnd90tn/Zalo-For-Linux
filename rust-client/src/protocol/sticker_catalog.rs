//! Source-backed keyword search and public sticker pack discovery.
use super::{
    client::ClientType,
    messaging::{self as wire, WireError},
};
use crate::domain::{StickerDescriptor, MAX_STICKER_ID};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use url::Url;

pub(crate) const SEARCH_PATH: &str = "/api/message/sticker/search";
pub(crate) const DETAIL_PATH: &str = "/api/message/sticker/sticker_detail";
pub(crate) const PACK_LIST_PATH: &str = "/api/message/sticker/category/list/v2";
pub(crate) const PACK_DETAIL_PATH: &str = "/api/message/sticker/category/sticker_detail";
const MAX_QUERY_BYTES: usize = 256;
const MAX_QUERY_CHARS: usize = 64;
const MAX_RESULTS: usize = 10;
const MAX_PACK_ROWS: usize = 1_000;
pub(crate) const MAX_PACKS: usize = 100;
pub(crate) const MAX_PACK_ITEMS: usize = 200;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StickerPack {
    pub category_id: u64,
    pub name: String,
}

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
    valid_pack_id(id).then_some(id)
}

pub(crate) fn valid_pack_id(id: u64) -> bool {
    (1..=MAX_STICKER_ID).contains(&id)
}

fn pack_name(value: &Value) -> Option<&str> {
    let name = value.as_str()?.trim();
    (!name.is_empty()
        && name.len() <= 256
        && name.chars().count() <= 80
        && !name.chars().any(|c| {
            c.is_control() || matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
        }))
    .then_some(name)
}

pub(crate) fn packs(payload: &Value) -> Result<Vec<StickerPack>, WireError> {
    wire::service_code(payload)?;
    // The PC client accepts authenticated inline rows when the CDN manifest
    // cannot be used. A URL-only response is unavailable in this client.
    let list = payload.get("data").ok_or(WireError::Invalid)?;
    let rows = list
        .get("data")
        .and_then(Value::as_array)
        .ok_or(WireError::Invalid)?;
    let mut excluded = HashSet::new();
    if let Some(ids) = list.get("exCateIds").filter(|ids| !ids.is_null()) {
        let ids = ids.as_array().ok_or(WireError::Invalid)?;
        if ids.len() > MAX_PACK_ROWS {
            return Err(WireError::Oversize);
        }
        for id in ids {
            if id.as_u64() == Some(0) || id.as_str() == Some("0") {
                continue; // The built-in pack never receives a catalog token.
            }
            excluded.insert(known_id(id).ok_or(WireError::Invalid)?);
        }
    }
    if rows.len() > MAX_PACK_ROWS {
        return Err(WireError::Oversize);
    }
    for row in rows {
        if ["is_hidden", "isHidden"]
            .iter()
            .any(|field| row.get(field).is_some_and(|value| !unset(value)))
        {
            excluded.extend(row.get("id").and_then(known_id));
        }
    }
    Ok(rows
        .iter()
        .filter_map(|row| {
            let category_id = known_id(row.get("id")?)?;
            let name = pack_name(row.get("name")?)?;
            excluded.insert(category_id).then(|| StickerPack {
                category_id,
                name: name.to_owned(),
            })
        })
        .take(MAX_PACKS)
        .collect())
}

pub(crate) fn pack_parameters(category_id: u64) -> Result<Value, WireError> {
    if !valid_pack_id(category_id) {
        return Err(WireError::Invalid);
    }
    Ok(json!({"cid":category_id.to_string()}))
}

pub(crate) fn pack_stickers(
    payload: &Value,
    category_id: u64,
) -> Result<Vec<StickerDescriptor>, WireError> {
    wire::service_code(payload)?;
    if !valid_pack_id(category_id) {
        return Err(WireError::Invalid);
    }
    let rows = payload
        .get("data")
        .and_then(Value::as_array)
        .ok_or(WireError::Invalid)?;
    if rows.len() > MAX_PACK_ITEMS {
        return Err(WireError::Oversize);
    }
    let mut order = Vec::with_capacity(rows.len());
    let mut by_id = HashMap::with_capacity(rows.len());
    let mut correlated = false;
    for row in rows {
        if row.get("cateId").and_then(known_id) != Some(category_id) {
            continue;
        }
        let Some(sticker_id) = row.get("id").and_then(known_id) else {
            continue;
        };
        let parsed = ordinary_sticker(
            row,
            Address {
                sticker_id,
                category_id,
            },
        );
        correlated |= parsed.is_ok();
        let sticker = parsed.ok().flatten();
        by_id
            .entry(sticker_id)
            .and_modify(|earlier| {
                if *earlier != sticker {
                    *earlier = None;
                }
            })
            .or_insert_with(|| {
                order.push(sticker_id);
                sticker
            });
    }
    if !correlated && !rows.is_empty() {
        return Err(WireError::Invalid);
    }
    Ok(order
        .into_iter()
        .filter_map(|id| by_id.remove(&id).flatten())
        .collect())
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
    ordinary_sticker(row, requested)
}

fn ordinary_sticker(
    row: &Value,
    requested: Address,
) -> Result<Option<StickerDescriptor>, WireError> {
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
        "is_hidden",
        "isHidden",
    ]
    .iter()
    .all(|key| row.get(key).is_none_or(unset));
    Ok((ordinary && sticker.is_catalog()).then_some(sticker))
}

fn unset(value: &Value) -> bool {
    value.is_null()
        || value.as_bool() == Some(false)
        || value.as_u64() == Some(0)
        || matches!(value.as_str(), Some("" | "0"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_use_only_authenticated_inline_public_metadata() {
        let frame = |data| json!({"error_code":0,"data":data});
        let pack = |id, name| StickerPack {
            category_id: id,
            name: String::from(name),
        };
        assert_eq!(
            packs(&frame(json!({
                "dataUrl":"https://discard.invalid/catalog",
                "exCateIds":[0,"0",11],
                "data":[
                    {"id":10,"name":" Chào ","iconUrl":"https://discard.invalid/icon"},
                    {"id":"10","name":"Duplicate"},
                    {"id":11,"name":"Excluded"},
                    {"id":12,"name":"Hidden","is_hidden":5},
                    {"id":12,"name":"Conflicting public entry"},
                    {"id":13,"name":"Private","isHidden":6},
                    {"id":14,"name":"Bad\u{202e}Label"},
                    {"id":15,"name":"Cảm ơn"},
                    {"id":0,"name":"Local built-in"}
                ]
            }))),
            Ok(vec![pack(10, "Chào"), pack(15, "Cảm ơn")]),
        );
        assert_eq!(packs(&frame(json!({"data":[]}))), Ok(vec![]));
        assert_eq!(
            packs(&frame(json!({"dataUrl":"https://discard.invalid/catalog"}))),
            Err(WireError::Invalid),
        );
    }

    #[test]
    fn pack_stickers_keep_correlated_ordinary_addresses_only() {
        let frame = |data| json!({"error_code":0,"data":data});
        let sticker = StickerDescriptor {
            sticker_id: 20695,
            category_id: 10047,
            sticker_type: 3,
        };
        assert_eq!(pack_parameters(10047), Ok(json!({"cid":"10047"})));
        assert_eq!(pack_parameters(0), Err(WireError::Invalid));
        assert_eq!(
            pack_stickers(
                &frame(json!([
                    {"id":20695,"cateId":10047,"type":3,"stickerUrl":"https://discard.invalid/asset"},
                    {"id":20695,"cateId":10047,"type":3},
                    {"id":20696,"cateId":10048,"type":3},
                    {"id":20697,"cateId":10047,"type":3},
                    {"id":20697,"cateId":10047,"type":3,"status":1},
                    {"id":20698,"cateId":10047,"type":3,"extInfo":{"payload":"discard"}},
                    {"id":20699,"cateId":10047,"type":7}
                ])),
                10047
            ),
            Ok(vec![sticker]),
        );
        assert_eq!(pack_stickers(&frame(json!([])), 10047), Ok(vec![]));
        assert_eq!(
            pack_stickers(&frame(json!([{"id":20695,"cateId":10048,"type":3}])), 10047),
            Err(WireError::Invalid),
        );
        assert_eq!(
            pack_stickers(&frame(json!(vec![Value::Null; MAX_PACK_ITEMS + 1])), 10047),
            Err(WireError::Oversize),
        );
    }

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
