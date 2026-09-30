//! Invitation preview and authenticated public reads.

use super::*;

pub(crate) fn decode_join_control_pages(
    family_id: [u8; 16],
    control_pages: &[Vec<u8>],
) -> Result<Vec<(u64, Vec<u8>)>, BindingError> {
    let mut after = 0;
    let mut controls = Vec::new();
    let page_count = control_pages.len();
    if page_count == 0
        || page_count > 64
        || control_pages.iter().map(Vec::len).sum::<usize>() > 16 * 1024 * 1024
    {
        return Err(BindingError::InvalidBytes);
    }
    for (index, bytes) in control_pages.iter().enumerate() {
        let page = ControlPage::decode(bytes, family_id, after).map_err(rejected)?;
        if page.has_more && page.entries.is_empty() {
            return Err(BindingError::InvalidBytes);
        }
        if page.has_more != (index + 1 < page_count) {
            return Err(BindingError::InvalidBytes);
        }
        after = page.next_after;
        controls.extend(
            page.entries
                .into_iter()
                .map(|entry| (entry.cursor, entry.committed_bytes)),
        );
    }
    if controls.len() < 2 || controls[0].0 != 1 {
        return Err(BindingError::InvalidBytes);
    }
    Ok(controls)
}

#[uniffi::export]
pub fn preview_invitation(fragment: String) -> Result<InvitationPreviewRow, BindingError> {
    let bootstrap = InvitationBootstrap::from_fragment(&fragment).map_err(rejected)?;
    let path = format!(
        "/v1/families/{}/control?after=0",
        lower_hex(&bootstrap.family_id())
    );
    let read = bootstrap.sign_get(&path).map_err(rejected)?;
    Ok(InvitationPreviewRow {
        family_id: bootstrap.family_id().to_vec(),
        relay_origin: bootstrap.relay_origin().to_owned(),
        role: bootstrap.fixed_role(),
        control_path: path,
        read_auth: read.bytes,
    })
}

#[uniffi::export]
pub fn invitation_control_read(
    fragment: String,
    after: u64,
) -> Result<SignedReadRow, BindingError> {
    let bootstrap = InvitationBootstrap::from_fragment(&fragment).map_err(rejected)?;
    let path = format!(
        "/v1/families/{}/control?after={after}",
        lower_hex(&bootstrap.family_id())
    );
    let auth = bootstrap.sign_get(&path).map_err(rejected)?.bytes;
    Ok(SignedReadRow { path, auth, after })
}

#[uniffi::export]
pub fn invitation_status_read(fragment: String) -> Result<SignedReadRow, BindingError> {
    let bootstrap = InvitationBootstrap::from_fragment(&fragment).map_err(rejected)?;
    let path = format!(
        "/v1/families/{}/invitation-status/{}",
        lower_hex(&bootstrap.family_id()),
        lower_hex(&bootstrap.invitation_id())
    );
    let auth = bootstrap.sign_get(&path).map_err(rejected)?.bytes;
    Ok(SignedReadRow {
        path,
        auth,
        after: 0,
    })
}

#[uniffi::export]
pub fn verify_invitation_status(
    fragment: String,
    response: Vec<u8>,
) -> Result<InvitationStatusRow, BindingError> {
    let bootstrap = InvitationBootstrap::from_fragment(&fragment).map_err(rejected)?;
    let status = bootstrap.verify_status(&response).map_err(rejected)?;
    Ok(InvitationStatusRow {
        reason: status.reason,
        cursor: status.cursor,
        observed_ms: status.observed_ms,
    })
}

#[uniffi::export]
pub fn invitation_control_page_progress(
    fragment: String,
    control_page: Vec<u8>,
    after: u64,
) -> Result<ControlPageProgressRow, BindingError> {
    let bootstrap = InvitationBootstrap::from_fragment(&fragment).map_err(rejected)?;
    control_page_progress(bootstrap.family_id().to_vec(), control_page, after)
}

#[uniffi::export]
pub fn control_page_progress(
    family_id: Vec<u8>,
    control_page: Vec<u8>,
    after: u64,
) -> Result<ControlPageProgressRow, BindingError> {
    let page = ControlPage::decode(&control_page, fixed(&family_id)?, after).map_err(rejected)?;
    if page.has_more && page.entries.is_empty() {
        return Err(BindingError::InvalidBytes);
    }
    Ok(ControlPageProgressRow {
        next_after: page.next_after,
        has_more: page.has_more,
    })
}
