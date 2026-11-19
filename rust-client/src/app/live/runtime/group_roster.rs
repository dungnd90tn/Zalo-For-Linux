//! Authenticated read-only group member pages. Run on a background worker task.
use super::*;
use crate::protocol::group_roster as roster;

const DEADLINE: Duration = Duration::from_secs(10);
const CURSOR_BYTES: usize = 67;

pub(super) struct Request {
    pub thread: String,
    pub privacy_revision: u64,
    pub after: Option<String>,
}

impl Request {
    pub(super) fn target(&self) -> Result<Target, WireError> {
        let target = Target::from_thread(&self.thread)?;
        if target.kind != ThreadKind::Group {
            return Err(WireError::Unsupported);
        }
        if self.after.as_deref().is_some_and(|cursor| {
            !cursor.strip_prefix("50:").is_some_and(|hex| {
                cursor.len() == CURSOR_BYTES && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
        }) {
            return Err(WireError::Invalid);
        }
        Ok(target)
    }

    /// Hidden groups remain excluded even when unlocked for chat display.
    pub(super) fn permits(&self, policy: &std::sync::RwLock<privacy::Projection>) -> bool {
        policy.read().is_ok_and(|scope| {
            scope.confirmed
                && scope.status == LivePrivacyStatus::Ready
                && scope.revision == self.privacy_revision
                && !scope.hidden.contains(&self.thread)
        })
    }
}

pub(super) async fn post(
    client: &reqwest::Client,
    session: &VerifiedSession,
    service: &str,
    path: &str,
    parameters: Value,
) -> Result<Value> {
    let mut url = root_for(session, service)?;
    wire::validate_origin(&url, "https")?;
    url.set_path(path);
    url.query_pairs_mut()
        .append_pair("zpw_ver", wire::API_VERSION)
        .append_pair("zpw_type", session.client_type().api_type());
    let cipher = session.encrypt_payload(&serde_json::to_vec(&parameters)?)?;
    let body = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("params", &cipher)
        .finish();
    tokio::time::timeout(
        DEADLINE,
        authenticated_request(client, session, url, Some(body)),
    )
    .await?
}

pub(super) async fn load_page(
    client: &reqwest::Client,
    session: &VerifiedSession,
    policy: &std::sync::RwLock<privacy::Projection>,
    request: &Request,
) -> Result<roster::Page> {
    let target = request.target()?;
    if !request.permits(policy) {
        return Err(WireError::Unsupported.into());
    }
    let payload = post(
        client,
        session,
        "group",
        roster::ROSTER_PATH,
        roster::roster_parameters(&target)?,
    )
    .await?;
    let mut page = roster::roster_page(
        &payload,
        session.account_id(),
        &target,
        request.after.as_deref(),
    )?;
    if !request.permits(policy) {
        return Err(WireError::Unsupported.into());
    }
    if !page.members.is_empty() {
        let parameters = roster::profile_parameters(&page.members)?;
        // Optional photos and names are deliberately not persisted. If the
        // profile endpoint is unavailable the verified roster is usable.
        let profiles = post(client, session, "profile", roster::PROFILE_PATH, parameters).await;
        if let Ok(profiles) = profiles {
            let _ = roster::apply_profiles(&profiles, &mut page.members);
        }
    }
    if !request.permits(policy) {
        return Err(WireError::Unsupported.into());
    }
    Ok(page)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_roster_permission_requires_current_normal_scope() {
        let request = Request {
            thread: "g:77".into(),
            privacy_revision: 3,
            after: None,
        };
        let policy = std::sync::RwLock::new(privacy::Projection {
            revision: 3,
            ..Default::default()
        });
        assert!(request.target().is_ok());
        assert!(!request.permits(&policy));
        policy.write().unwrap().confirmed = true;
        assert!(request.permits(&policy));
        policy.write().unwrap().revision = 4;
        assert!(!request.permits(&policy));
        {
            let mut state = policy.write().unwrap();
            state.revision = 3;
            state.hidden.insert("g:77".into());
            state.unlocked = Some("g:77".into());
        }
        assert!(!request.permits(&policy));
    }
}
