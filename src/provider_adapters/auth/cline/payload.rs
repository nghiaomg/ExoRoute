use super::*;

pub(super) fn decode_embedded_payload(code: &str) -> Option<Value> {
    // Cline's loopback callback contains a base64-encoded JSON token payload.
    // Some Cline versions append an opaque signature after the JSON object;
    // decode the URI component first and parse only the JSON prefix, matching
    // the behavior of the Cline OAuth client used by OmniRoute.
    let code = if code.as_bytes().contains(&b'%') {
        decode_uri_component(code).unwrap_or_else(|| code.to_owned())
    } else {
        code.to_owned()
    };
    let padding = (4 - code.len() % 4) % 4;
    let mut padded_code = code;
    if padding > 0 {
        padded_code.extend(std::iter::repeat_n('=', padding));
    }

    for decoder in [
        &general_purpose::URL_SAFE_NO_PAD,
        &general_purpose::URL_SAFE,
        &general_purpose::STANDARD_NO_PAD,
        &general_purpose::STANDARD,
    ] {
        let Ok(decoded) = decoder.decode(padded_code.as_bytes()) else {
            continue;
        };
        if decoded.len() > MAX_OAUTH_BODY_BYTES {
            return None;
        }
        let decoded = String::from_utf8_lossy(&decoded);
        let Some(last_brace) = decoded.rfind('}') else {
            continue;
        };
        let json = &decoded[..=last_brace];
        if let Ok(value) = serde_json::from_str::<Value>(json)
            && value
                .get("accessToken")
                .or_else(|| value.get("access_token"))
                .is_some()
        {
            return Some(value);
        }
    }
    None
}

pub(super) fn decode_uri_component(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            decoded.push(bytes[index]);
            index += 1;
            continue;
        }
        let high = bytes.get(index + 1).and_then(|byte| hex_value(*byte))?;
        let low = bytes.get(index + 2).and_then(|byte| hex_value(*byte))?;
        decoded.push((high << 4) | low);
        index += 3;
    }
    String::from_utf8(decoded).ok()
}

pub(super) fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

pub(crate) fn is_embedded_callback_code(code: &str) -> bool {
    decode_embedded_payload(code).is_some_and(|payload| ClineAccount::parse(&payload).is_ok())
}
