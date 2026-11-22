//! Bounded ordinary-group roster and profile wire contracts from the PC client.
use super::messaging::{id_string, numeric_id, service_code, Target, ThreadKind, WireError};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const ROSTER_PATH: &str = "/api/group/getmg-v2";
pub(crate) const PROFILE_PATH: &str = "/api/social/group/members";
pub(crate) const PAGE_SIZE: usize = 50;
const MAX_MEMBERS: usize = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    Owner,
    Deputy,
    Member,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    pub id: String,
    pub name: Option<String>,
    pub role: Option<Role>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
    pub members: Vec<Member>,
    pub total: Option<u32>,
    pub restricted: bool,
    pub next: Option<String>,
    pub permissions: GroupPermissions,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GroupPermissions {
    pub invite: bool,
    pub leave: bool,
}

pub(crate) struct VerifiedGroup {
    versions: BTreeMap<String, u64>,
    owner: String,
    admins: BTreeSet<String>,
    restricted: bool,
    pub(crate) can_pin: bool,
    pub(crate) can_forward: bool,
    pub permissions: GroupPermissions,
}

impl VerifiedGroup {
    pub(crate) fn contains(&self, member: &str) -> bool {
        self.versions.contains_key(member)
    }

    pub(crate) fn can_view(&self, account: &str, member: &str) -> bool {
        !self.restricted
            || member == account
            || member == self.owner
            || self.admins.contains(member)
    }
}

pub(crate) fn roster_parameters(target: &Target) -> Result<Value, WireError> {
    if target.kind != ThreadKind::Group {
        return Err(WireError::Unsupported);
    }
    numeric_id(&target.remote_id)?;
    // Zero requests fresh data and never authorizes reuse of an unchanged row.
    Ok(json!({"gridVerMap": json!({(target.remote_id.clone()): 0}).to_string()}))
}

pub(crate) fn profile_parameters(members: &[Member]) -> Result<Value, WireError> {
    if members.is_empty() || members.len() > PAGE_SIZE {
        return Err(WireError::Invalid);
    }
    let mut seen = BTreeSet::new();
    let versions = members
        .iter()
        .map(|member| {
            numeric_id(&member.id)?;
            if !seen.insert(&member.id) {
                return Err(WireError::Invalid);
            }
            // With no cached profiles the source asks for version zero.
            Ok(format!("{}_0", member.id))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(json!({"friend_pversion_map": versions}))
}

fn number(value: &Value) -> Result<u64, WireError> {
    value
        .as_u64()
        .or_else(|| {
            let text = value.as_str()?;
            if text.is_empty() || text.len() > 20 || !text.bytes().all(|byte| byte.is_ascii_digit())
            {
                return None;
            }
            text.parse().ok()
        })
        .ok_or(WireError::Invalid)
}

fn field_id(value: &Value, field: &str) -> Result<String, WireError> {
    id_string(value.get(field).ok_or(WireError::Invalid)?)
}

fn member_versions(row: &Value) -> Result<BTreeMap<String, u64>, WireError> {
    let list = row.get("memVerList").filter(|value| !value.is_null());
    let map = row.get("memberVerMap").filter(|value| !value.is_null());
    let mut versions = BTreeMap::new();
    let mut push = |id: String, version| {
        numeric_id(&id)?;
        if versions
            .insert(id, version)
            .is_some_and(|old| old != version)
        {
            return Err(WireError::Invalid);
        }
        Ok(())
    };
    if let Some(list) = list {
        let list = list.as_array().ok_or(WireError::Invalid)?;
        if list.len() > MAX_MEMBERS {
            return Err(WireError::Oversize);
        }
        for entry in list {
            let (id, version) = entry
                .as_str()
                .filter(|text| text.len() <= 45)
                .and_then(|text| text.split_once('_'))
                .ok_or(WireError::Invalid)?;
            if version.is_empty() || !version.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(WireError::Invalid);
            }
            push(
                id.to_owned(),
                version.parse().map_err(|_| WireError::Invalid)?,
            )?;
        }
    } else if let Some(map) = map {
        let map = map.as_object().ok_or(WireError::Invalid)?;
        if map.len() > MAX_MEMBERS {
            return Err(WireError::Oversize);
        }
        for (id, version) in map {
            push(id.clone(), number(version)?)?;
        }
    } else {
        return Err(WireError::Invalid);
    }
    Ok(versions)
}

fn flags_clear(data: &Value, field: &str, target: &Target) -> Result<(), WireError> {
    let Some(entries) = data.get(field).filter(|value| !value.is_null()) else {
        return Ok(());
    };
    let entries = entries.as_array().ok_or(WireError::Invalid)?;
    if entries.len() > MAX_MEMBERS {
        return Err(WireError::Oversize);
    }
    for entry in entries {
        if id_string(entry)? == target.remote_id {
            return Err(WireError::Invalid);
        }
    }
    Ok(())
}

pub(crate) fn verify_group(
    payload: &Value,
    account: &str,
    target: &Target,
) -> Result<VerifiedGroup, WireError> {
    service_code(payload)?;
    numeric_id(account)?;
    if target.kind != ThreadKind::Group {
        return Err(WireError::Unsupported);
    }
    numeric_id(&target.remote_id)?;
    let data = payload.get("data").ok_or(WireError::Invalid)?;
    flags_clear(data, "removedsGroup", target)?;
    flags_clear(data, "unchangedsGroup", target)?;
    let row = data
        .get("gridInfoMap")
        .and_then(Value::as_object)
        .filter(|rows| rows.len() == 1)
        .and_then(|rows| rows.get(&target.remote_id))
        .ok_or(WireError::Invalid)?;
    if field_id(row, "groupId")? != target.remote_id {
        return Err(WireError::Invalid);
    }
    let total = number(row.get("totalMember").ok_or(WireError::Invalid)?)?;
    if number(row.get("type").ok_or(WireError::Invalid)?)? != 1
        || !(1..=MAX_MEMBERS as u64).contains(&total)
        || row
            .get("e2ee")
            .is_some_and(|value| value.as_bool() != Some(false) && number(value) != Ok(0))
    {
        return Err(WireError::Unsupported);
    }
    let versions = member_versions(row)?;
    let owner = field_id(row, "creatorId")?;
    if versions.len() as u64 != total
        || !versions.contains_key(account)
        || !versions.contains_key(&owner)
    {
        return Err(WireError::Invalid);
    }
    let admins = row
        .get("adminIds")
        .and_then(Value::as_array)
        .filter(|ids| ids.len() <= MAX_MEMBERS)
        .ok_or(WireError::Invalid)?
        .iter()
        .map(id_string)
        .collect::<Result<BTreeSet<_>, _>>()?;
    if admins.iter().any(|id| !versions.contains_key(id)) {
        return Err(WireError::Invalid);
    }
    // Unknown or missing privacy settings limit ordinary members to the
    // source's restricted projection: owner, deputies and the viewer.
    let disabled = |field| {
        row.get("setting")
            .and_then(|settings| settings.get(field))
            .is_some_and(|value| value.as_bool() == Some(false) || number(value) == Ok(0))
    };
    let privileged = account == owner || admins.contains(account);
    let restricted = !disabled("lockViewMember") && !privileged;
    // The board uses a strict numeric zero for the ordinary member allowance.
    let can_pin = row
        .get("setting")
        .and_then(Value::as_object)
        .is_some_and(|settings| {
            privileged || settings.get("setTopicOnly").and_then(Value::as_u64) == Some(0)
        });
    let permissions = GroupPermissions {
        invite: total < MAX_MEMBERS as u64
            && (privileged || (disabled("addMemberOnly") && disabled("joinAppr"))),
        leave: account != owner,
    };
    // The source's ordinary multi-forward path falls back at 100 members.
    // An absent send lock defaults to open in the original settings reader.
    let can_forward = total < MAX_MEMBERS as u64
        && (privileged || row.pointer("/setting/lockSendMsg").is_none() || disabled("lockSendMsg"));
    Ok(VerifiedGroup {
        versions,
        owner,
        admins,
        restricted,
        can_pin,
        can_forward,
        permissions,
    })
}

#[cfg(test)]
pub(crate) fn roster_page(
    payload: &Value,
    account: &str,
    target: &Target,
    after: Option<&str>,
) -> Result<Page, WireError> {
    project_page(
        &verify_group(payload, account, target)?,
        account,
        target,
        after,
    )
}

pub(crate) fn project_page(
    group: &VerifiedGroup,
    account: &str,
    target: &Target,
    after: Option<&str>,
) -> Result<Page, WireError> {
    let total = group.versions.len() as u32;
    let mut selected: Vec<_> = group
        .versions
        .iter()
        .filter_map(|(id, version)| {
            let role = if *id == group.owner {
                Role::Owner
            } else if group.admins.contains(id) {
                Role::Deputy
            } else {
                Role::Member
            };
            if group.restricted && role == Role::Member && id != account {
                None
            } else {
                Some((id, version, role))
            }
        })
        .collect();
    selected.sort_unstable_by(|a, b| a.2.cmp(&b.2).then_with(|| a.0.cmp(b.0)));
    let mut fingerprint = Sha256::new();
    fingerprint.update(account.as_bytes());
    fingerprint.update(b":");
    fingerprint.update(target.remote_id.as_bytes());
    fingerprint.update(total.to_le_bytes());
    fingerprint.update([u8::from(group.restricted)]);
    for (id, version, role) in &selected {
        fingerprint.update(id.as_bytes());
        fingerprint.update(b":");
        fingerprint.update(version.to_le_bytes());
        fingerprint.update([*role as u8]);
    }
    // Ordinary groups have at most two local pages. Re-fetching the full
    // authoritative roster prevents old cursors surviving permission changes.
    let next_token = format!("50:{:x}", fingerprint.finalize());
    let start = match after {
        None => 0,
        Some(value) if value == next_token && selected.len() > PAGE_SIZE => PAGE_SIZE,
        _ => return Err(WireError::Invalid),
    };
    let next = (start + PAGE_SIZE < selected.len()).then_some(next_token);
    let members = selected
        .into_iter()
        .skip(start)
        .take(PAGE_SIZE)
        .map(|(id, _, role)| Member {
            id: id.clone(),
            name: None,
            role: Some(role),
        })
        .collect();
    Ok(Page {
        members,
        total: Some(total),
        restricted: group.restricted,
        next,
        permissions: group.permissions,
    })
}

pub(crate) fn apply_profiles(payload: &Value, members: &mut [Member]) -> Result<(), WireError> {
    service_code(payload)?;
    let profiles = payload
        .pointer("/data/profiles")
        .and_then(Value::as_object)
        .filter(|entries| entries.len() <= PAGE_SIZE)
        .ok_or(WireError::Invalid)?;
    for member in members {
        member.name = profiles
            .get(&member.id)
            .filter(|profile| field_id(profile, "id").as_ref() == Ok(&member.id))
            .and_then(|profile| profile.get("displayName"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|name| {
                !name.is_empty()
                    && name.len() <= 512
                    && !name.chars().any(char::is_control)
                    && !name.bytes().all(|byte| byte.is_ascii_digit())
            })
            .map(str::to_owned);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const GROUP: &str = "900719925474099123";

    fn request_fixture() -> Value {
        serde_json::from_str(include_str!(
            "../../tests/fixtures/messaging/group_roster.json"
        ))
        .unwrap()
    }

    fn response(row: Value) -> Value {
        json!({"error_code":0, "data":{"gridInfoMap":{GROUP:row}}})
    }

    fn locked_row() -> Value {
        json!({
            "groupId": GROUP, "type": 1, "totalMember": 4,
            "creatorId": "88", "adminIds": ["33"],
            "setting": {"lockViewMember": 1},
            "memVerList": request_fixture()["sourceMembers"]
        })
    }

    #[test]
    fn forwarding_requires_send_permission_and_fewer_than_a_hundred_members() {
        let target = Target::new(ThreadKind::Group, GROUP).unwrap();
        let mut row = locked_row();
        row["setting"]["lockSendMsg"] = json!(true);
        let payload = response(row);
        assert!(!verify_group(&payload, "22", &target).unwrap().can_forward);
        assert!(verify_group(&payload, "88", &target).unwrap().can_forward);

        for (total, permitted) in [(99, true), (100, false)] {
            let mut row = locked_row();
            row["totalMember"] = json!(total);
            row["memVerList"] = json!((1..=total).map(|id| format!("{id}_1")).collect::<Vec<_>>());
            assert_eq!(
                verify_group(&response(row), "22", &target)
                    .unwrap()
                    .can_forward,
                permitted
            );
        }
    }

    #[test]
    fn exact_source_requests_and_permission_filtered_profiles() {
        let target = Target::new(ThreadKind::Group, GROUP).unwrap();
        let source = request_fixture();
        assert_eq!(ROSTER_PATH, source["roster"]["path"].as_str().unwrap());
        assert_eq!(PROFILE_PATH, source["profiles"]["path"].as_str().unwrap());
        assert_eq!(
            roster_parameters(&target).unwrap(),
            source["roster"]["payload"]
        );
        let payload = response(locked_row());
        let mut page = roster_page(&payload, "22", &target, None).unwrap();
        assert!(page.restricted);
        assert_eq!(page.total, Some(4));
        assert_eq!(page.members[0].role, Some(Role::Owner));
        assert_eq!(page.members[1].role, Some(Role::Deputy));
        assert_eq!(
            profile_parameters(&page.members).unwrap(),
            source["profiles"]["payload"]
        );
        apply_profiles(
            &json!({"error_code":0, "data":{"profiles":{
                "88":{"id":"88","displayName":"Chủ nhóm"},
                "33":{"id":"24","displayName":"Sai tài khoản"},
                "24":{"id":"24","displayName":"Ẩn khỏi trang"},
                "22":{"id":"22","displayName":"22"}
            }}}),
            &mut page.members,
        )
        .unwrap();
        assert_eq!(page.members.len(), 3);
        assert_eq!(page.members[0].name.as_deref(), Some("Chủ nhóm"));
        assert!(page.members[1..].iter().all(|member| member.name.is_none()));

        let admin = roster_page(&payload, "33", &target, None).unwrap();
        assert!(!admin.restricted);
        assert_eq!(admin.members.len(), 4);
        let ordinary = verify_group(&payload, "22", &target).unwrap();
        assert!(ordinary.contains("24") && !ordinary.can_view("22", "24"));
        assert!(ordinary.can_view("22", "33"));
        assert_eq!(
            ordinary.permissions,
            GroupPermissions {
                invite: false,
                leave: true
            }
        );
        let owner = verify_group(&payload, "88", &target).unwrap();
        assert_eq!(
            owner.permissions,
            GroupPermissions {
                invite: true,
                leave: false
            }
        );
    }

    #[test]
    fn unbound_group_nonmember_incomplete_data_and_community_fail_closed() {
        let target = Target::new(ThreadKind::Group, GROUP).unwrap();
        let row = locked_row();
        assert!(roster_page(&response(row.clone()), "55", &target, None).is_err());
        for (field, value) in [
            ("type", json!(2)),
            ("groupId", json!("99")),
            ("totalMember", json!(5)),
            ("adminIds", json!(["55"])),
            ("memVerList", json!(["88_1", "88_2", "22_1", "33_0"])),
        ] {
            let mut untrusted = row.clone();
            untrusted[field] = value;
            assert!(roster_page(&response(untrusted), "22", &target, None).is_err());
        }
        let mut payload = response(row);
        payload["data"]["unchangedsGroup"] = json!([GROUP]);
        assert!(roster_page(&payload, "22", &target, None).is_err());
    }

    #[test]
    fn local_cursor_binds_current_roster_and_permission() {
        let target = Target::new(ThreadKind::Group, GROUP).unwrap();
        let map = (11..=81)
            .map(|id| (id.to_string(), json!(1)))
            .collect::<serde_json::Map<_, _>>();
        let mut row = json!({
            "groupId": GROUP, "type": 1, "totalMember": 71,
            "creatorId": "11", "adminIds": ["12"],
            "setting": {"lockViewMember": false},
            "memberVerMap": map
        });
        let payload = response(row.clone());
        let first = roster_page(&payload, "13", &target, None).unwrap();
        assert_eq!(first.members.len(), PAGE_SIZE);
        let cursor = first.next.as_deref();
        assert!(cursor.is_some());
        let last = roster_page(&payload, "13", &target, cursor).unwrap();
        assert_eq!(last.members.len(), 21);
        assert!(last.next.is_none());
        assert!(roster_page(&payload, "14", &target, cursor).is_err());
        row["setting"]["lockViewMember"] = json!(true);
        assert!(roster_page(&response(row.clone()), "13", &target, cursor).is_err());
        row["setting"] = json!({});
        let hidden_by_default = roster_page(&response(row), "13", &target, None).unwrap();
        assert!(hidden_by_default.restricted);
        assert_eq!(hidden_by_default.members.len(), 3);
    }
}
