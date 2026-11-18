//! Signed impersonation tokens (feature `impersonation`).
//!
//! Token format: `hex(payload_json) + "." + hex(hmac_sha256(secret, payload_json))`.

use std::time::Duration;

use chrono::Utc;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::error::TenancyError;
use crate::ports::{ScopedSession, SessionStore};
use crate::tenant::TenantId;

const SESSION_KEY: &str = "impersonation";

/// Claims carried by an impersonation token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImpersonationClaims {
    /// Tenant being impersonated.
    pub tenant_id: TenantId,
    /// End user acting inside the tenant.
    pub user_id: i64,
    /// Expiry as a Unix timestamp in seconds.
    pub expires_at: i64,
}

/// Issues a signed token valid for `ttl`.
pub fn issue_token(
    secret: &[u8],
    tenant_id: TenantId,
    user_id: i64,
    ttl: Duration,
) -> Result<String, TenancyError> {
    let claims = ImpersonationClaims {
        tenant_id,
        user_id,
        expires_at: Utc::now().timestamp() + ttl.as_secs() as i64,
    };
    let payload =
        serde_json::to_vec(&claims).map_err(|error| TenancyError::Crypto(error.to_string()))?;
    let signature = sign(secret, &payload)?;
    Ok(format!(
        "{}.{}",
        hex::encode(payload),
        hex::encode(signature)
    ))
}

/// Verifies the signature and expiry, returning the claims.
pub fn verify_token(secret: &[u8], token: &str) -> Result<ImpersonationClaims, TenancyError> {
    let (payload, signature) = token
        .split_once('.')
        .ok_or_else(|| TenancyError::Crypto("malformed token".to_string()))?;
    let payload = hex::decode(payload).map_err(|error| TenancyError::Crypto(error.to_string()))?;
    let signature =
        hex::decode(signature).map_err(|error| TenancyError::Crypto(error.to_string()))?;

    let mut mac = Hmac::<Sha256>::new_from_slice(secret)
        .map_err(|error| TenancyError::Crypto(error.to_string()))?;
    mac.update(&payload);
    mac.verify_slice(&signature)
        .map_err(|_| TenancyError::Crypto("invalid token signature".to_string()))?;

    let claims: ImpersonationClaims = serde_json::from_slice(&payload)
        .map_err(|error| TenancyError::Crypto(error.to_string()))?;
    if claims.expires_at <= Utc::now().timestamp() {
        return Err(TenancyError::Crypto(
            "impersonation token expired".to_string(),
        ));
    }
    Ok(claims)
}

fn sign(secret: &[u8], payload: &[u8]) -> Result<Vec<u8>, TenancyError> {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret)
        .map_err(|error| TenancyError::Crypto(error.to_string()))?;
    mac.update(payload);
    Ok(mac.finalize().into_bytes().to_vec())
}

/// Stores `token` under the `impersonation` key of the active tenant session.
pub async fn start_impersonation(
    sessions: &ScopedSession,
    token: &str,
) -> Result<(), TenancyError> {
    sessions.set(SESSION_KEY, token.as_bytes().to_vec()).await
}

/// Removes the `impersonation` key from the active tenant session.
pub async fn stop_impersonation(sessions: &ScopedSession) -> Result<(), TenancyError> {
    sessions.delete(SESSION_KEY).await
}
