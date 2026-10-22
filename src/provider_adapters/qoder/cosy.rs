//! Qoder COSY (hybrid RSA + AES + MD5) request signing.
//!
//! Qoder requires COSY authentication on all inference and catalog endpoints
//! under `/algo/api/v2/...`.
//!
//! Every signed request carries:
//! - An AES-128-CBC encrypted payload of user credentials, with the AES key
//!   encrypted via Qoder's RSA-1024 public key (`Cosy-Key`).
//! - An MD5 signature over `payloadB64 || \n || cosyKey || \n || timestamp || \n || body || \n || sigPath`.
//! - Client fingerprint headers (`Cosy-Machineid`, `Cosy-Version`, etc.).

use aes::cipher::{BlockEncryptMut, KeyIvInit, block_padding::Pkcs7};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use http::{HeaderMap, HeaderName, HeaderValue};
use md5::{Digest, Md5};
use rsa::{Pkcs1v15Encrypt, RsaPublicKey, pkcs8::DecodePublicKey};
use serde_json::json;

type Aes128CbcEnc = cbc::Encryptor<aes::Aes128>;

// Base64-encoded DER representation of Qoder's RSA-1024 public key.
pub(crate) const QODER_RSA_PUBLIC_KEY_B64: &str = "\
MIGfMA0GCSqGSIb3DQEBAQUAA4GNADCBiQKBgQDA8iMH5c02LilrsERw9t6Pv5Nc\
4k6Pz1EaDicBMpdpxKduSZu5OANqUq8er4GM95omAGIOPOh+Nx0spthYA2BqGz+l\
6HRkPJ7S236FZz73In/KVuLnwI8JJ2CbuJap8kvheCCZpmAWpb/cPx/3Vr/J6I17\
XcW+ML9FoCI6AOvOzwIDAQAB";

pub(crate) const QODER_IDE_VERSION: &str = "1.0.0";
pub(crate) const QODER_CLIENT_TYPE: &str = "5";
pub(crate) const QODER_DATA_POLICY: &str = "disagree";
pub(crate) const QODER_LOGIN_VERSION: &str = "v2";
pub(crate) const QODER_MACHINE_OS: &str = "x86_64_windows";
pub(crate) const QODER_MACHINE_TYPE: &str = "5";

#[derive(Clone, Debug)]
pub(crate) struct QoderCosyCredentials {
    pub(crate) user_id: String,
    pub(crate) auth_token: String,
    pub(crate) machine_id: String,
    pub(crate) name: Option<String>,
    pub(crate) email: Option<String>,
}

fn md5_hex(input: &[u8]) -> String {
    let mut hasher = Md5::new();
    hasher.update(input);
    format!("{:x}", hasher.finalize())
}

/// Computes sigPath from the request URL by stripping the leading `/algo` prefix.
pub(crate) fn compute_sig_path(request_url: &str) -> String {
    let path = match reqwest::Url::parse(request_url) {
        Ok(url) => url.path().to_owned(),
        Err(_) => request_url.split('?').next().unwrap_or("").to_owned(),
    };
    if let Some(stripped) = path.strip_prefix("/algo") {
        stripped.to_owned()
    } else {
        path
    }
}

/// Encrypts user information using AES-128-CBC and RSA-1024.
fn encrypt_user_info(creds: &QoderCosyCredentials) -> Result<(String, String), String> {
    // Generate fresh 16-byte AES key (first 16 chars of UUID v4 canonical string).
    let raw_uuid = uuid::Uuid::new_v4().to_string();
    let aes_key_str = &raw_uuid[..16];
    let aes_key_bytes = aes_key_str.as_bytes();

    let user_info_json = json!({
        "uid": creds.user_id,
        "security_oauth_token": creds.auth_token,
        "name": creds.name.as_deref().unwrap_or(""),
        "aid": "",
        "email": creds.email.as_deref().unwrap_or(""),
    })
    .to_string();

    // AES-128-CBC with key = IV = aes_key_bytes, PKCS7 padding
    let mut key_arr = [0u8; 16];
    let mut iv_arr = [0u8; 16];
    key_arr.copy_from_slice(aes_key_bytes);
    iv_arr.copy_from_slice(aes_key_bytes);

    let cipher = Aes128CbcEnc::new(&key_arr.into(), &iv_arr.into());
    let pt_bytes = user_info_json.as_bytes();
    let mut buf = vec![0u8; pt_bytes.len() + 16];
    buf[..pt_bytes.len()].copy_from_slice(pt_bytes);

    let encrypted = cipher
        .encrypt_padded_mut::<Pkcs7>(&mut buf, pt_bytes.len())
        .map_err(|e| format!("AES encryption padding failed: {e:?}"))?;
    let info_b64 = BASE64.encode(encrypted);

    // RSA-1024 encrypt AES key using PKCS1v15 padding
    let der_bytes = BASE64
        .decode(QODER_RSA_PUBLIC_KEY_B64)
        .map_err(|e| format!("invalid Qoder RSA key base64: {e}"))?;
    let public_key = RsaPublicKey::from_public_key_der(&der_bytes)
        .map_err(|e| format!("invalid Qoder RSA public key DER: {e}"))?;

    let mut rng = rand::rngs::OsRng;
    let encrypted_aes_key = public_key
        .encrypt(&mut rng, Pkcs1v15Encrypt, aes_key_bytes)
        .map_err(|e| format!("RSA encryption failed: {e}"))?;
    let cosy_key_b64 = BASE64.encode(encrypted_aes_key);

    Ok((cosy_key_b64, info_b64))
}

/// Builds all required COSY headers for a Qoder upstream request.
pub(crate) fn build_cosy_headers(
    body: &[u8],
    request_url: &str,
    creds: &QoderCosyCredentials,
    model_key: &str,
) -> Result<HeaderMap, String> {
    if creds.user_id.trim().is_empty() {
        return Err("Qoder user ID is missing for COSY signing".to_owned());
    }
    if creds.auth_token.trim().is_empty() {
        return Err("Qoder auth token is missing for COSY signing".to_owned());
    }

    let (cosy_key, info) = encrypt_user_info(creds)?;
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string();

    let request_id = uuid::Uuid::new_v4().to_string();
    let payload_json = json!({
        "version": "v1",
        "requestId": request_id,
        "info": info,
        "cosyVersion": QODER_IDE_VERSION,
        "ideVersion": "",
    })
    .to_string();
    let payload_b64 = BASE64.encode(payload_json.as_bytes());

    let sig_path = compute_sig_path(request_url);
    let body_str = String::from_utf8_lossy(body);

    let sig_input = format!("{payload_b64}\n{cosy_key}\n{timestamp}\n{body_str}\n{sig_path}");
    let sig = md5_hex(sig_input.as_bytes());

    let machine_id = if creds.machine_id.trim().is_empty() {
        uuid::Uuid::new_v4().to_string()
    } else {
        creds.machine_id.clone()
    };
    let body_hash = md5_hex(body);
    let body_length = body.len().to_string();

    let mut headers = HeaderMap::new();

    let set_header = |map: &mut HeaderMap, name: &'static str, val: &str| -> Result<(), String> {
        let h_name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|e| format!("invalid header name {name}: {e}"))?;
        let h_val = HeaderValue::from_str(val)
            .map_err(|e| format!("invalid header value for {name}: {e}"))?;
        map.insert(h_name, h_val);
        Ok(())
    };

    let auth_value = format!("Bearer COSY.{payload_b64}.{sig}");
    set_header(&mut headers, "authorization", &auth_value)?;
    set_header(&mut headers, "cosy-key", &cosy_key)?;
    set_header(&mut headers, "cosy-user", &creds.user_id)?;
    set_header(&mut headers, "cosy-date", &timestamp)?;
    set_header(&mut headers, "cosy-version", QODER_IDE_VERSION)?;
    set_header(&mut headers, "cosy-machineid", &machine_id)?;
    set_header(&mut headers, "cosy-machinetoken", &machine_id)?;
    set_header(&mut headers, "cosy-machinetype", QODER_MACHINE_TYPE)?;
    set_header(&mut headers, "cosy-machineos", QODER_MACHINE_OS)?;
    set_header(&mut headers, "cosy-clienttype", QODER_CLIENT_TYPE)?;
    set_header(&mut headers, "cosy-clientip", "127.0.0.1")?;
    set_header(&mut headers, "cosy-bodyhash", &body_hash)?;
    set_header(&mut headers, "cosy-bodylength", &body_length)?;
    set_header(&mut headers, "cosy-sigpath", &sig_path)?;
    set_header(&mut headers, "cosy-data-policy", QODER_DATA_POLICY)?;
    set_header(&mut headers, "cosy-organization-id", "")?;
    set_header(&mut headers, "cosy-organization-tags", "")?;
    set_header(&mut headers, "login-version", QODER_LOGIN_VERSION)?;
    set_header(
        &mut headers,
        "x-request-id",
        &uuid::Uuid::new_v4().to_string(),
    )?;
    set_header(&mut headers, "x-model-key", model_key)?;
    set_header(&mut headers, "x-model-source", "system")?;
    set_header(&mut headers, "accept-encoding", "identity")?;

    Ok(headers)
}
