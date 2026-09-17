//! Offline application entitlements, independent of inference engines and runtimes.
//!
//! Only the public verification key is compiled into the application. The signing
//! key belongs to a distributor-controlled issuer and must never ship with Sculpt.

use base64::{engine::general_purpose::STANDARD, Engine};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};

const LICENSE_VERSION: u32 = 1;
const PRODUCT: &str = "sculpt-desktop";
pub const MAX_LICENSE_BYTES: usize = 16 * 1024;
const MAX_PAYLOAD_BYTES: usize = 4 * 1024;
const MAX_PAYLOAD_BASE64_BYTES: usize = 4 * MAX_PAYLOAD_BYTES.div_ceil(3);

/// Constructed by the native application, never from frontend request fields.
#[derive(Clone, Debug)]
pub struct LicensePolicy {
    pub development: bool,
    pub public_key: Option<String>,
}

impl LicensePolicy {
    pub fn configured() -> Self {
        Self {
            development: cfg!(debug_assertions),
            public_key: option_env!("SCULPT_LICENSE_PUBLIC_KEY")
                .map(str::trim)
                .filter(|key| !key.is_empty())
                .map(str::to_owned),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LicenseClaims {
    pub version: u32,
    pub product: String,
    pub license_id: String,
    /// Issuance time in Unix seconds, not an expiry or a trusted machine clock.
    pub issued_at: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SignedLicense {
    version: u32,
    /// Base64 of the exact UTF-8 JSON bytes that were signed.
    payload: String,
    signature: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AccessMode {
    Development,
    Trial,
    Paid,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AccessStatus {
    pub mode: AccessMode,
    pub can_generate: bool,
    /// None means unlimited; a trial reports either one or zero.
    pub free_generations_remaining: Option<u32>,
    pub activation_available: bool,
    pub message: String,
}

fn verifying_key(encoded: &str) -> Result<VerifyingKey, String> {
    // Check length before decoding, including for a malformed distributor build.
    if encoded.len() != 44 {
        return Err("The application license verification key is invalid.".into());
    }
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|_| "The application license verification key is invalid.")?;
    let key_bytes: [u8; 32] = bytes
        .try_into()
        .map_err(|_| "The application license verification key is invalid.")?;
    let key = VerifyingKey::from_bytes(&key_bytes)
        .map_err(|_| "The application license verification key is invalid.")?;
    if key.is_weak() {
        return Err("The application license verification key is invalid.".into());
    }
    Ok(key)
}

/// Verify before persisting an activation, and again when reading stored access.
/// No network request, model load, or file-system mutation happens here.
pub fn verify_license(envelope: &str, public_key: &str) -> Result<LicenseClaims, String> {
    if envelope.is_empty() || envelope.len() > MAX_LICENSE_BYTES {
        return Err("The license is empty or exceeds the 16 KB size limit.".into());
    }
    let signed: SignedLicense =
        serde_json::from_str(envelope).map_err(|_| "The license envelope is not valid JSON.")?;
    if signed.version != LICENSE_VERSION {
        return Err("This license version is not supported by this version of Sculpt.".into());
    }
    if signed.payload.is_empty() || signed.payload.len() > MAX_PAYLOAD_BASE64_BYTES {
        return Err("The license payload exceeds the supported size.".into());
    }
    if signed.signature.len() != 88 {
        return Err("The license signature is invalid.".into());
    }
    let payload = STANDARD
        .decode(&signed.payload)
        .map_err(|_| "The license payload is not valid base64.")?;
    if payload.len() > MAX_PAYLOAD_BYTES {
        return Err("The license payload exceeds the supported size.".into());
    }
    let signature_bytes = STANDARD
        .decode(&signed.signature)
        .map_err(|_| "The license signature is invalid.")?;
    let signature =
        Signature::from_slice(&signature_bytes).map_err(|_| "The license signature is invalid.")?;
    verifying_key(public_key)?
        .verify_strict(&payload, &signature)
        .map_err(|_| "The license signature could not be verified.")?;

    // Parse only the exact bytes whose signature was verified. Re-serialization
    // must not be used as signing input because whitespace and key order vary.
    let claims: LicenseClaims =
        serde_json::from_slice(&payload).map_err(|_| "The signed license claims are invalid.")?;
    if claims.version != LICENSE_VERSION || claims.product != PRODUCT {
        return Err("This license is not valid for this version of Sculpt Desktop.".into());
    }
    if claims.license_id.is_empty()
        || claims.license_id.len() > 128
        || !claims
            .license_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.:".contains(&byte))
        || claims.issued_at == 0
        || claims.issued_at > 253_402_300_799
    {
        return Err("The signed license claims are invalid.".into());
    }
    Ok(claims)
}

pub fn validate_activation(
    policy: &LicensePolicy,
    envelope: &str,
) -> Result<LicenseClaims, String> {
    let public_key = policy.public_key.as_deref().ok_or(
        "License activation is not configured in this build. Contact the Sculpt distributor.",
    )?;
    verify_license(envelope, public_key)
}

/// The store supplies `trial_used` only after a successful real asset is durably
/// committed. Demos, failed jobs, and cancelled jobs must not consume the trial.
pub fn access_status(
    policy: &LicensePolicy,
    stored_license: Option<&str>,
    trial_used: bool,
) -> AccessStatus {
    let activation_available = policy
        .public_key
        .as_deref()
        .is_some_and(|key| verifying_key(key).is_ok());
    if policy.development {
        return AccessStatus {
            mode: AccessMode::Development,
            can_generate: true,
            free_generations_remaining: None,
            activation_available,
            message: "Development build · unlimited local generation.".into(),
        };
    }
    if stored_license.is_some_and(|license| validate_activation(policy, license).is_ok()) {
        return AccessStatus {
            mode: AccessMode::Paid,
            can_generate: true,
            free_generations_remaining: None,
            activation_available,
            message: "Sculpt is activated · unlimited local generation.".into(),
        };
    }
    let message = if trial_used && activation_available {
        "Your free reconstruction is complete. Activate Sculpt to continue generating locally."
    } else if trial_used {
        "Your free reconstruction is complete. License activation is not configured in this build."
    } else {
        "One free local reconstruction. Only a successfully saved 3D asset uses your trial."
    };
    AccessStatus {
        mode: AccessMode::Trial,
        can_generate: !trial_used,
        free_generations_remaining: Some(u32::from(!trial_used)),
        activation_available,
        message: message.into(),
    }
}

/// Call inside the same application operation lock used to admit a new job, so
/// two concurrent requests cannot both claim the final free reconstruction.
pub fn authorize_generation(
    policy: &LicensePolicy,
    stored_license: Option<&str>,
    trial_used: bool,
    simulated: bool,
) -> Result<(), String> {
    if simulated {
        return Ok(());
    }
    let status = access_status(policy, stored_license, trial_used);
    if status.can_generate {
        Ok(())
    } else {
        Err(status.message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use serde_json::json;

    // Deterministic test-only key. Production signing keys are never bundled.
    fn signing_key() -> SigningKey {
        SigningKey::from_bytes(&[42; 32])
    }

    fn public_key() -> String {
        STANDARD.encode(signing_key().verifying_key().as_bytes())
    }

    fn claims() -> LicenseClaims {
        LicenseClaims {
            version: 1,
            product: "sculpt-desktop".into(),
            license_id: "test-license-123".into(),
            issued_at: 1_789_430_400,
        }
    }

    fn sign_bytes(payload: &[u8]) -> String {
        json!({
            "version": 1,
            "payload": STANDARD.encode(payload),
            "signature": STANDARD.encode(signing_key().sign(payload).to_bytes()),
        })
        .to_string()
    }

    fn signed_license() -> String {
        sign_bytes(&serde_json::to_vec(&claims()).unwrap())
    }

    fn release_policy() -> LicensePolicy {
        LicensePolicy {
            development: false,
            public_key: Some(public_key()),
        }
    }

    #[test]
    fn verifies_exact_signed_payload_and_perpetual_paid_access() {
        let bytes = serde_json::to_vec_pretty(&claims()).unwrap();
        let envelope = sign_bytes(&bytes);
        assert_eq!(verify_license(&envelope, &public_key()).unwrap(), claims());
        let status = access_status(&release_policy(), Some(&envelope), true);
        assert_eq!(status.mode, AccessMode::Paid);
        assert!(status.can_generate);
        assert!(status.activation_available);
        assert_eq!(status.free_generations_remaining, None);
    }

    #[test]
    fn rejects_tampered_payload_signature_and_wrong_key() {
        let license = signed_license();
        let mut envelope: serde_json::Value = serde_json::from_str(&license).unwrap();
        let mut changed = claims();
        changed.license_id = "tampered".into();
        envelope["payload"] = json!(STANDARD.encode(serde_json::to_vec(&changed).unwrap()));
        assert!(verify_license(&envelope.to_string(), &public_key()).is_err());

        envelope = serde_json::from_str(&license).unwrap();
        envelope["signature"] = json!(STANDARD.encode([0; 64]));
        assert!(verify_license(&envelope.to_string(), &public_key()).is_err());
        let other_key = SigningKey::from_bytes(&[73; 32]);
        assert!(verify_license(
            &license,
            &STANDARD.encode(other_key.verifying_key().as_bytes())
        )
        .is_err());
    }

    #[test]
    fn rejects_wrong_product_versions_and_invalid_claims() {
        let mut wrong_product = claims();
        wrong_product.product = "another-product".into();
        let mut wrong_version = claims();
        wrong_version.version = 2;
        let mut empty_id = claims();
        empty_id.license_id.clear();
        let mut long_id = claims();
        long_id.license_id = "x".repeat(129);
        let mut invalid_id = claims();
        invalid_id.license_id = "license\n123".into();
        let mut missing_time = claims();
        missing_time.issued_at = 0;
        for invalid in [
            wrong_product,
            wrong_version,
            empty_id,
            long_id,
            invalid_id,
            missing_time,
        ] {
            assert!(verify_license(
                &sign_bytes(&serde_json::to_vec(&invalid).unwrap()),
                &public_key()
            )
            .is_err());
        }
        let mut envelope: serde_json::Value = serde_json::from_str(&signed_license()).unwrap();
        envelope["version"] = json!(2);
        assert!(verify_license(&envelope.to_string(), &public_key()).is_err());
    }

    #[test]
    fn rejects_unknown_fields_duplicate_fields_and_oversized_inputs() {
        let mut value = serde_json::to_value(claims()).unwrap();
        value["admin"] = json!(true);
        assert!(verify_license(
            &sign_bytes(&serde_json::to_vec(&value).unwrap()),
            &public_key()
        )
        .is_err());
        let mut envelope: serde_json::Value = serde_json::from_str(&signed_license()).unwrap();
        envelope["canGenerate"] = json!(true);
        assert!(verify_license(&envelope.to_string(), &public_key()).is_err());
        assert!(verify_license(
            &signed_license().replacen("{", "{\"version\":1,", 1),
            &public_key()
        )
        .is_err());
        assert!(verify_license(&" ".repeat(MAX_LICENSE_BYTES + 1), &public_key()).is_err());
        assert!(verify_license(
            &sign_bytes(&vec![b' '; MAX_PAYLOAD_BYTES + 1]),
            &public_key()
        )
        .is_err());
    }

    #[test]
    fn missing_or_invalid_build_key_never_grants_paid_access() {
        for key in [None, Some("invalid".into()), Some(STANDARD.encode([0; 32]))] {
            let policy = LicensePolicy {
                development: false,
                public_key: key,
            };
            let status = access_status(&policy, Some(&signed_license()), true);
            assert_eq!(status.mode, AccessMode::Trial);
            assert!(!status.can_generate);
            assert!(!status.activation_available);
            assert!(validate_activation(&policy, &signed_license()).is_err());
        }
    }

    #[test]
    fn release_trial_gates_real_jobs_but_preserves_demo_access() {
        let policy = release_policy();
        let available = access_status(&policy, None, false);
        assert_eq!(available.mode, AccessMode::Trial);
        assert_eq!(available.free_generations_remaining, Some(1));
        assert!(authorize_generation(&policy, None, false, false).is_ok());
        let exhausted = access_status(&policy, None, true);
        assert_eq!(exhausted.free_generations_remaining, Some(0));
        assert!(authorize_generation(&policy, None, true, false).is_err());
        assert!(authorize_generation(&policy, None, true, true).is_ok());
    }

    #[test]
    fn development_is_unlimited_without_weakening_activation_verification() {
        let policy = LicensePolicy {
            development: true,
            public_key: None,
        };
        let status = access_status(&policy, None, true);
        assert_eq!(status.mode, AccessMode::Development);
        assert_eq!(status.free_generations_remaining, None);
        assert!(authorize_generation(&policy, None, true, false).is_ok());
        assert!(validate_activation(&policy, "{}").is_err());
    }

    #[test]
    fn invalid_stored_license_does_not_create_paid_access_or_spend_trial() {
        let policy = release_policy();
        let status = access_status(&policy, Some("{}"), false);
        assert_eq!(status.mode, AccessMode::Trial);
        assert_eq!(status.free_generations_remaining, Some(1));
        assert!(status.can_generate);
        assert!(authorize_generation(&policy, Some("{}"), true, false).is_err());
    }
}
