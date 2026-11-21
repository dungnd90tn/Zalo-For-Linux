//! Read-only, account-bound entitlement for ordinary Original JPEG upload.
use super::*;
use crate::protocol::original_photo as wire_original;

pub(super) async fn entitled(client: &reqwest::Client, session: &VerifiedSession) -> Result<bool> {
    if session.config().original_photo_limits.is_none() {
        return Ok(false);
    }
    let viewer_url = wire_original::viewer_url(session.client_type())?;
    let viewer_cookie = session
        .cloud_viewer_cookie(&viewer_url)
        .ok_or(WireError::Unsupported)?;
    let plan_url = wire_original::plan_url(
        &root_for(session, wire_original::PLAN_SERVICE)?,
        session.client_type(),
    )?;
    let plan_cookie = session
        .authorization_cookie(&plan_url)
        .ok_or(WireError::Unsupported)?;

    let outer = bounded_json(
        client
            .get(viewer_url)
            .header("Cookie", viewer_cookie)
            .send()
            .await
            .map_err(|_| WireError::Unsupported)?,
    )
    .await?;
    wire::service_code(&outer)?;
    let encrypted = outer
        .get("data")
        .and_then(Value::as_str)
        .ok_or(WireError::Invalid)?;
    let viewer_body = session.decrypt_payload(encrypted)?;
    let payload: Value = serde_json::from_slice(&viewer_body).map_err(|_| WireError::Invalid)?;
    let viewer = wire_original::Viewer::parse(&payload)?;
    let plan = bounded_json(
        client
            .get(plan_url)
            .header("Cookie", plan_cookie)
            .header("cloud-viewer-key", viewer.header.clone())
            .send()
            .await
            .map_err(|_| WireError::Unsupported)?,
    )
    .await?;
    Ok(viewer.paid_plan(&plan)?)
}

async fn bounded_json(mut response: reqwest::Response) -> Result<Value, WireError> {
    let max = wire_original::MAX_VIEWER_RESPONSE;
    if response.status() != reqwest::StatusCode::OK
        || response
            .content_length()
            .is_some_and(|length| length > max as u64)
    {
        return Err(WireError::Unsupported);
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| WireError::Unsupported)? {
        if bytes.len() + chunk.len() > max {
            return Err(WireError::Oversize);
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| WireError::Invalid)
}
