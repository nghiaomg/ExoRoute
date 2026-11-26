//! Hand-rolled protobuf and Connect framing for the Devin Desktop transport.
//!
//! The wire schema mirrors the field numbers OmniRoute verified against the
//! live `exa.api_server_pb` / `exa.auth_pb` services (2026-07). The protocol
//! is reverse-engineered and can break when Cognition changes their backend;
//! nothing here may invent a new dependency — encoding is explicit varint and
//! length-delimited field work over `Vec<u8>`.

use super::*;

pub(super) const BASE_URL: &str = "https://server.codeium.com";
pub(super) const CHAT_URL: &str =
    "https://server.codeium.com/exa.api_server_pb.ApiServerService/GetChatMessage";
pub(super) const AUTH_URL: &str = "https://server.codeium.com/exa.auth_pb.AuthService/GetUserJwt";

pub(super) const CONNECT_COMPRESSED_FLAG: u8 = 0x01;
pub(super) const CONNECT_END_STREAM_FLAG: u8 = 0x02;
pub(super) const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;
pub(super) const MAX_AUTH_RESPONSE_BYTES: usize = 1024 * 1024;
pub(super) const MAX_RESPONSE_BYTES: usize = 64 * 1024 * 1024;

const IDE_NAME: &str = "windsurf";
const LOCALE: &str = "en-US";
const CHAT_MESSAGE_REQUEST_TYPE_CASCADE: u64 = 5;
/// Stop reason the backend reports for tool-calling turns.
const STOP_REASON_TOOL_CALLS: u64 = 10;
const STOP_REASON_LENGTH: u64 = 3;
const STOP_REASON_CONTENT_FILTER: u64 = 11;

/// The Desktop bundle identifies the client as Windsurf; both version strings
/// are overridable so a backend change can be followed without a rebuild.
pub(super) fn resolve_ide_version() -> String {
    version_from_env("EXOROUTE_DEVIN_DESKTOP_VERSION", "3.6.27")
}

pub(super) fn resolve_extension_version() -> String {
    version_from_env("EXOROUTE_DEVIN_DESKTOP_EXTENSION_VERSION", "1.48.2")
}

fn version_from_env(key: &str, default: &str) -> String {
    match std::env::var(key) {
        Ok(value)
            if value.split('.').count() == 3
                && value.split('.').all(|part| {
                    !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit())
                }) =>
        {
            value
        }
        _ => default.to_owned(),
    }
}

// --- Encoding ---------------------------------------------------------------

fn encode_varint(mut value: u64, out: &mut Vec<u8>) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

fn encode_length_field(field: u32, payload: &[u8], out: &mut Vec<u8>) {
    encode_varint((u64::from(field) << 3) | 2, out);
    encode_varint(payload.len() as u64, out);
    out.extend_from_slice(payload);
}

fn encode_string_field(field: u32, value: &str, out: &mut Vec<u8>) {
    if !value.is_empty() {
        encode_length_field(field, value.as_bytes(), out);
    }
}

fn encode_varint_field(field: u32, value: u64, out: &mut Vec<u8>) {
    if value != 0 {
        encode_varint(u64::from(field) << 3, out);
        encode_varint(value, out);
    }
}

pub(super) struct MetadataRefs<'a> {
    pub(super) api_key: &'a str,
    pub(super) session_id: &'a str,
    pub(super) user_jwt: &'a str,
}

fn encode_metadata(metadata: &MetadataRefs<'_>, out: &mut Vec<u8>) {
    let extension_version = resolve_extension_version();
    let ide_version = resolve_ide_version();
    let mut metadata_bytes = Vec::new();
    encode_string_field(1, IDE_NAME, &mut metadata_bytes);
    encode_string_field(2, &extension_version, &mut metadata_bytes);
    encode_string_field(3, metadata.api_key, &mut metadata_bytes);
    encode_string_field(4, LOCALE, &mut metadata_bytes);
    encode_string_field(7, &ide_version, &mut metadata_bytes);
    encode_string_field(10, metadata.session_id, &mut metadata_bytes);
    encode_string_field(12, IDE_NAME, &mut metadata_bytes);
    encode_string_field(21, metadata.user_jwt, &mut metadata_bytes);
    out.extend_from_slice(&metadata_bytes);
}

/// Encodes the unary `GetUserJwtRequest`: field 1 wraps the Metadata message.
pub(super) fn encode_auth_request(metadata: &MetadataRefs<'_>) -> Vec<u8> {
    let mut metadata_bytes = Vec::new();
    encode_metadata(metadata, &mut metadata_bytes);
    let mut out = Vec::with_capacity(metadata_bytes.len() + 8);
    encode_length_field(1, &metadata_bytes, &mut out);
    out
}

pub(super) struct ToolCallRef<'a> {
    pub(super) id: &'a str,
    pub(super) name: &'a str,
    pub(super) arguments: &'a str,
}

pub(super) struct PromptRef<'a> {
    pub(super) message_id: &'a str,
    /// 1 = user, 2 = assistant, 4 = tool result.
    pub(super) source: u64,
    pub(super) prompt: &'a str,
    pub(super) tool_calls: &'a [ToolCallRef<'a>],
    pub(super) tool_call_id: &'a str,
}

pub(super) struct ToolDefRef<'a> {
    pub(super) name: &'a str,
    pub(super) description: &'a str,
    pub(super) schema_json: &'a str,
    pub(super) strict: bool,
}

pub(super) enum ToolChoiceRef<'a> {
    OptionName(&'a str),
}

pub(super) struct ChatRequestRefs<'a> {
    pub(super) metadata: MetadataRefs<'a>,
    pub(super) model: &'a str,
    pub(super) system_prompt: &'a str,
    pub(super) prompts: &'a [PromptRef<'a>],
    pub(super) cascade_id: &'a str,
    pub(super) tools: &'a [ToolDefRef<'a>],
    pub(super) disable_parallel_tool_calls: bool,
    pub(super) tool_choice: Option<ToolChoiceRef<'a>>,
}

/// Encodes the verified `GetChatMessageRequest` wire schema.
pub(super) fn encode_chat_request(request: &ChatRequestRefs<'_>) -> Vec<u8> {
    let mut out = Vec::new();
    let mut metadata_bytes = Vec::new();
    encode_metadata(&request.metadata, &mut metadata_bytes);
    encode_length_field(1, &metadata_bytes, &mut out);
    encode_string_field(2, request.system_prompt, &mut out);
    for prompt in request.prompts {
        let mut prompt_bytes = Vec::new();
        encode_string_field(1, prompt.message_id, &mut prompt_bytes);
        encode_varint_field(2, prompt.source, &mut prompt_bytes);
        encode_string_field(3, prompt.prompt, &mut prompt_bytes);
        for tool_call in prompt.tool_calls {
            let mut call_bytes = Vec::new();
            encode_string_field(1, tool_call.id, &mut call_bytes);
            encode_string_field(2, tool_call.name, &mut call_bytes);
            encode_string_field(3, tool_call.arguments, &mut call_bytes);
            encode_length_field(6, &call_bytes, &mut prompt_bytes);
        }
        encode_string_field(7, prompt.tool_call_id, &mut prompt_bytes);
        encode_length_field(3, &prompt_bytes, &mut out);
    }
    encode_varint_field(7, CHAT_MESSAGE_REQUEST_TYPE_CASCADE, &mut out);
    for tool in request.tools {
        let mut tool_bytes = Vec::new();
        encode_string_field(1, tool.name, &mut tool_bytes);
        encode_string_field(2, tool.description, &mut tool_bytes);
        encode_string_field(3, tool.schema_json, &mut tool_bytes);
        encode_varint_field(12, u64::from(tool.strict), &mut tool_bytes);
        encode_length_field(10, &tool_bytes, &mut out);
    }
    if request.disable_parallel_tool_calls {
        encode_varint_field(11, 1, &mut out);
    }
    match request.tool_choice {
        Some(ToolChoiceRef::OptionName(name)) => {
            let mut choice = Vec::new();
            encode_string_field(1, name, &mut choice);
            encode_length_field(12, &choice, &mut out);
        }
        None => {}
    }
    encode_string_field(14, request.model, &mut out);
    encode_string_field(16, request.cascade_id, &mut out);
    encode_string_field(21, request.model, &mut out);
    out
}

/// Connect streaming envelope: one flags byte plus a big-endian u32 length.
pub(super) fn encode_frame(payload: &[u8], flags: u8) -> Vec<u8> {
    let mut frame = Vec::with_capacity(5 + payload.len());
    frame.push(flags);
    frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    frame.extend_from_slice(payload);
    frame
}

// --- Decoding ---------------------------------------------------------------

pub(super) enum FieldValue {
    Varint(u64),
    /// Wire types we skip: the byte length is known, the payload unused.
    Fixed64,
    Fixed32,
    Bytes(Vec<u8>),
}

pub(super) struct ProtoField {
    pub(super) number: u32,
    pub(super) value: FieldValue,
}

fn read_varint(bytes: &[u8], offset: &mut usize) -> Result<u64, String> {
    let mut value: u64 = 0;
    let mut shift = 0u32;
    for _ in 0..10 {
        let Some(&byte) = bytes.get(*offset) else {
            return Err("truncated protobuf varint".to_owned());
        };
        *offset += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
        shift += 7;
        if shift >= 64 {
            return Err("protobuf varint exceeds 64 bits".to_owned());
        }
    }
    Err("truncated protobuf varint".to_owned())
}

pub(super) fn decode_fields(bytes: &[u8]) -> Result<Vec<ProtoField>, String> {
    let mut fields = Vec::new();
    let mut offset = 0usize;
    while offset < bytes.len() {
        let tag = read_varint(bytes, &mut offset)?;
        let number =
            u32::try_from(tag >> 3).map_err(|_| "invalid protobuf field number".to_owned())?;
        if number == 0 {
            return Err("invalid protobuf field number".to_owned());
        }
        let value = match tag & 0x07 {
            0 => FieldValue::Varint(read_varint(bytes, &mut offset)?),
            1 => {
                if bytes.len() - offset < 8 {
                    return Err("truncated protobuf fixed64".to_owned());
                }
                offset += 8;
                FieldValue::Fixed64
            }
            2 => {
                let length = read_varint(bytes, &mut offset)?;
                let length =
                    usize::try_from(length).map_err(|_| "protobuf field too long".to_owned())?;
                if length > bytes.len() - offset {
                    return Err("truncated protobuf field".to_owned());
                }
                let payload = bytes[offset..offset + length].to_vec();
                offset += length;
                FieldValue::Bytes(payload)
            }
            5 => {
                if bytes.len() - offset < 4 {
                    return Err("truncated protobuf fixed32".to_owned());
                }
                offset += 4;
                FieldValue::Fixed32
            }
            other => return Err(format!("unsupported protobuf wire type {other}")),
        };
        fields.push(ProtoField { number, value });
    }
    Ok(fields)
}

fn field_bytes(field: &ProtoField) -> Option<&[u8]> {
    match &field.value {
        FieldValue::Bytes(payload) => Some(payload),
        _ => None,
    }
}

fn field_varint(field: &ProtoField) -> Option<u64> {
    match &field.value {
        FieldValue::Varint(value) => Some(*value),
        _ => None,
    }
}

/// Extracts the user JWT from the unary `GetUserJwtResponse`. A returned
/// custom API server URL is deliberately ignored: following a
/// credential-derived URL would hand the upstream an SSRF path.
pub(super) fn decode_auth_response(bytes: &[u8]) -> Result<String, String> {
    for field in decode_fields(bytes)? {
        if field.number == 1
            && let Some(payload) = field_bytes(&field)
        {
            return String::from_utf8(payload.to_vec())
                .map_err(|_| "Devin Desktop auth response is not valid UTF-8".to_owned());
        }
    }
    Err("Devin Desktop authentication returned no user JWT".to_owned())
}

pub(super) struct Usage {
    pub(super) input_tokens: u64,
    pub(super) output_tokens: u64,
    pub(super) cache_write_tokens: u64,
    pub(super) cache_read_tokens: u64,
}

pub(super) struct ToolCallDelta {
    pub(super) id: String,
    pub(super) name: String,
    /// Argument fragments concatenate across frames.
    pub(super) arguments: String,
}

pub(super) struct ChatResponse {
    pub(super) text: String,
    pub(super) thinking: String,
    pub(super) stop_reason: u64,
    pub(super) usage: Option<Usage>,
    pub(super) tool_calls: Vec<ToolCallDelta>,
}

fn decode_usage(bytes: &[u8]) -> Usage {
    let mut usage = Usage {
        input_tokens: 0,
        output_tokens: 0,
        cache_write_tokens: 0,
        cache_read_tokens: 0,
    };
    for field in decode_fields(bytes).unwrap_or_default() {
        if let Some(value) = field_varint(&field) {
            match field.number {
                2 => usage.input_tokens = value,
                3 => usage.output_tokens = value,
                4 => usage.cache_write_tokens = value,
                5 => usage.cache_read_tokens = value,
                _ => {}
            }
        }
    }
    usage
}

fn decode_tool_call(bytes: &[u8]) -> ToolCallDelta {
    let mut call = ToolCallDelta {
        id: String::new(),
        name: String::new(),
        arguments: String::new(),
    };
    for field in decode_fields(bytes).unwrap_or_default() {
        if let Some(payload) = field_bytes(&field)
            && let Ok(text) = std::str::from_utf8(payload)
        {
            match field.number {
                1 => call.id = text.to_owned(),
                2 => call.name = text.to_owned(),
                3 => call.arguments.push_str(text),
                _ => {}
            }
        }
    }
    call
}

/// Decodes one `GetChatMessageResponse` message.
pub(super) fn decode_chat_response(bytes: &[u8]) -> Result<ChatResponse, String> {
    let mut response = ChatResponse {
        text: String::new(),
        thinking: String::new(),
        stop_reason: 0,
        usage: None,
        tool_calls: Vec::new(),
    };
    for field in decode_fields(bytes)? {
        match field.number {
            3 => {
                if let Some(payload) = field_bytes(&field) {
                    response.text.push_str(&String::from_utf8_lossy(payload));
                }
            }
            5 => response.stop_reason = field_varint(&field).unwrap_or(0),
            6 => {
                if let Some(payload) = field_bytes(&field) {
                    let call = decode_tool_call(payload);
                    // Argument fragments arrive as id-less submessages; the
                    // emit loop decides whether they continue a known call.
                    if !call.id.is_empty() || !call.name.is_empty() || !call.arguments.is_empty() {
                        response.tool_calls.push(call);
                    }
                }
            }
            7 => {
                if let Some(payload) = field_bytes(&field) {
                    response.usage = Some(decode_usage(payload));
                }
            }
            9 => {
                if let Some(payload) = field_bytes(&field) {
                    response
                        .thinking
                        .push_str(&String::from_utf8_lossy(payload));
                }
            }
            _ => {}
        }
    }
    Ok(response)
}

pub(super) fn finish_reason(stop_reason: u64, has_tool_calls: bool) -> &'static str {
    if has_tool_calls || stop_reason == STOP_REASON_TOOL_CALLS {
        "tool_calls"
    } else if stop_reason == STOP_REASON_LENGTH {
        "length"
    } else if stop_reason == STOP_REASON_CONTENT_FILTER {
        "content_filter"
    } else {
        "stop"
    }
}

// --- Connect frame parsing --------------------------------------------------

pub(super) struct Frame {
    pub(super) flags: u8,
    pub(super) payload: Vec<u8>,
}

/// Incremental Connect-stream framer. `push` consumes network bytes and
/// returns every complete frame; partial tails stay buffered between calls.
pub(super) struct FrameParser {
    pending: Vec<u8>,
}

impl FrameParser {
    pub(super) fn new() -> Self {
        Self {
            pending: Vec::new(),
        }
    }

    pub(super) fn push(&mut self, bytes: &[u8]) -> Result<Vec<Frame>, String> {
        if self.pending.len() + bytes.len() > MAX_FRAME_BYTES.saturating_add(MAX_RESPONSE_BYTES) {
            return Err("Devin Desktop response exceeds the safety limit".to_owned());
        }
        self.pending.extend_from_slice(bytes);
        let mut frames = Vec::new();
        loop {
            if self.pending.len() < 5 {
                break;
            }
            let length = u32::from_be_bytes([
                self.pending[1],
                self.pending[2],
                self.pending[3],
                self.pending[4],
            ]) as usize;
            if length > MAX_FRAME_BYTES {
                return Err("Devin Desktop Connect frame exceeds the safety limit".to_owned());
            }
            if self.pending.len() < 5 + length {
                break;
            }
            let flags = self.pending[0];
            let payload = self.pending[5..5 + length].to_vec();
            self.pending.drain(..5 + length);
            if flags & !(CONNECT_COMPRESSED_FLAG | CONNECT_END_STREAM_FLAG) != 0 {
                return Err("invalid Devin Desktop Connect frame flags".to_owned());
            }
            frames.push(Frame { flags, payload });
        }
        Ok(frames)
    }

    pub(super) fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }
}

/// Decodes a gzip-compressed Connect frame payload with a hard output bound.
pub(super) fn decode_gzip(payload: &[u8], limit: usize) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let mut decoder = flate2::read::GzDecoder::new(payload);
    let mut out = Vec::new();
    decoder
        .by_ref()
        .take(limit as u64 + 1)
        .read_to_end(&mut out)
        .map_err(|error| {
            format!("Devin Desktop Connect frame could not be decompressed: {error}")
        })?;
    if out.len() > limit {
        return Err("Devin Desktop Connect frame exceeds the decompression limit".to_owned());
    }
    Ok(out)
}

/// Parses a Connect end-stream trailer. `Ok(None)` is a successful trailer,
/// `Ok(Some((code, message)))` an upstream error, `Err` an unreadable one.
pub(super) fn parse_trailer(payload: &[u8]) -> Result<Option<(String, String)>, String> {
    let text = String::from_utf8_lossy(payload);
    let parsed: Value = serde_json::from_str(text.trim())
        .map_err(|_| "invalid Devin Desktop Connect trailer".to_owned())?;
    let Some(error) = parsed.get("error") else {
        return Ok(None);
    };
    if error.is_null() {
        return Ok(None);
    }
    let code = error
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or("upstream_error")
        .to_owned();
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("Connect stream failed")
        .to_owned();
    Ok(Some((code, message)))
}

pub(super) fn unix_timestamp() -> i64 {
    crate::support::time::unix_seconds()
}
