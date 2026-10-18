# Báo cáo tích hợp Muse (Meta Model API) vào ExoRoute

Nguồn tham chiếu đã đọc trực tiếp:

- `docs/9router` — gateway Next.js "9Router"
- `docs/OmniRoute` — gateway TypeScript "OmniRoute"
- Mã nguồn ExoRoute hiện tại (`src/`, `data/`, `web/src/`, `build.rs`)

Toàn bộ kết luận dưới đây dựa trên các tệp được trích dẫn kèm số dòng. Phần nào chưa xác minh được sẽ ghi rõ **"chưa xác minh"**.

---

## 1. Tóm tắt điều hành

Trong hai repo tham chiếu, từ "muse" dùng cho **hai thứ hoàn toàn khác nhau**:

| | `muse` / `muse-code` | `muse-spark-web` |
|---|---|---|
| Bản chất | Muse Code (Meta Model API) — API **chính thức** của Meta | Meta AI (meta.ai) — **đảo ngược giao thức web**, dùng cookie phiên |
| Xác thực | OAuth device grant (RFC 8628) + API key dán tay | Cookie `ecto_1_sess` + token WS `ecto1:...` |
| Wire format | OpenAI **Responses** (`POST /responses`) | GraphQL (warmup) + WebSocket nhị phân `wss://gateway.meta.ai/ws/clippy` |
| Base URL | `https://api.meta.ai/v1` | `https://www.meta.ai/api/graphql` |
| ToS | Hợp lệ (dùng subscription/API key của chính người dùng) | OmniRoute tự gắn nhãn `tos: "avoid"` và cảnh báo ToS của Meta |
| Khuyến nghị cho ExoRoute | **NÊN tích hợp** | **KHÔNG nên tích hợp** |

**Kết luận:** ExoRoute chỉ nên tích hợp **`muse` (= muse-code, Meta Model API chính thức)**.

**Tin tốt:** ExoRoute đã có sẵn *mọi* primitive cần thiết. Adapter `kilocode` là bản mẫu gần như trùng khớp (device authorization, token dài hạn không có refresh token). Khối lượng công việc ước tính:

- 1 thư mục adapter mới `src/provider_adapters/muse/` (~5–6 file)
- 1 thư mục lưu tài khoản `src/provider_adapters/auth/muse/` (~3 file)
- 1 preset trong `data/provider-presets.yaml`
- Sửa 4 file registry/allow-list (`build.rs`, `provider_adapters/mod.rs`, `registry.rs`, `auth/panels.rs`)
- 1 dòng frontend (`auth-panel.registry.ts`)

---

## 2. Cách 9Router tích hợp muse

### 2.1 Registry provider — `docs/9router/open-sse/providers/registry/muse.js` (67 dòng)

```js
id: "muse",
priority: 120,
alias: "muse",
aliases: ["muse-ai", "meta-model-api", "muse-code", "muse-subscription"],
uiAlias: "muse",
category: "oauth",
authModes: ["oauth", "apikey"],
hasOAuth: true,
```

Metadata hiển thị: `name: "Muse (Meta Model API)"`, `icon: "auto_awesome"`, `color: "#0866FF"`, `textIcon: "MU"`, `website: "https://muse.ai"`, `apiKeyUrl: "https://dev.meta.ai"`.

**Transport — hai wire format trên cùng một key** (`muse.js:32-53`):

```js
baseUrl: "https://api.meta.ai/v1/chat/completions",
validateUrl: "https://api.meta.ai/v1/models",
modelsUrl:   "https://api.meta.ai/v1/models",
// transport chính (mặc định):
auth: { combined: true, header: "Authorization", scheme: "bearer", hooks: ["museHeaders"] },
// transports[] bổ sung:
{ format: "openai",           baseUrl: "https://api.meta.ai/v1/chat/completions" },
{ format: "openai-responses", baseUrl: "https://api.meta.ai/v1/responses" },
```

Comment tại `muse.js:38-41` giải thích lý do: *"Muse Spark reasoning (incl. encrypted_content replay) chỉ round-trip được trên Responses"* — nên các model Muse Spark **pin** `targetFormat: "openai-responses"`.

**Danh mục model** (`muse.js:54-61`): `muse-spark-1.3`, `muse-spark-1.2`, `muse-spark-1.1`, `muse-spark-1.3-contributor`, `muse-spark-1.2-contributor` — tất cả `targetFormat: "openai-responses"`, `supportedFormats: ["openai-responses"]`. Có `passthroughModels: true` (model ngoài danh mục vẫn được chuyển tiếp).

**OAuth block** (`muse.js:62-66`):

```js
oauth: {
  clientId: "1031625952748946",
  deviceCodeUrl: "https://auth.meta.com/oidc/device/authorization/",
  tokenUrl:      "https://auth.meta.com/oidc/device/token/",
},
```

### 2.2 Hook `museHeaders` — `docs/9router/open-sse/executors/default.js:46`

```js
museHeaders: (h, c) => { if (c?.accessToken && !c?.apiKey) h["x-api-version"] = "1.0.0"; },
```

Nghĩa là: **chỉ** khi xác thực bằng OAuth (access token của tài khoản, không phải API key dán tay) thì thêm header `x-api-version: 1.0.0`. Comment đầu file `muse.js:4` xác nhận: *"oauth accounts get x-api-version via the museHeaders hook"*.

### 2.3 Luồng OAuth — `docs/9router/src/lib/oauth/providers/muse.js` (132 dòng)

`flowType: "device_code"`. Ba bước:

**(a) `requestDeviceCode`** — `POST` form-urlencoded tới `config.deviceCodeUrl`:

```js
headers: { "Content-Type": "application/x-www-form-urlencoded",
           Accept: "application/json",
           "x-api-version": API_VERSION },   // API_VERSION = "1.0.0"
body: new URLSearchParams({ client_id: config.clientId }),
```

**(b) `pollToken`** — `POST` form tới `config.tokenUrl`:

```js
body: new URLSearchParams({
  grant_type: "urn:ietf:params:oauth:grant-type:device_code",
  device_code: deviceCode,
  client_id: config.clientId,
}),
```

Xử lý pending (`muse.js:52-55`): coi `authorization_pending` và `slow_down` là *thành công một phần* để tiếp tục poll (`ok: response.ok || pending`).

**(c) `postExchange` — mint key** (`muse.js:57-112`), `POST https://api.meta.ai/muse-code/key`:

```js
body: JSON.stringify({ onboard: true }),
headers: { Accept, Authorization: `Bearer ${tokens.access_token}`,
           "Content-Type": "application/json", "x-api-version": "1.0.0" },
redirect: "error",
```

Retry có kiểm soát: `muse.js:62-77` thử lại tối đa **3 lần** khi gặp `429` hoặc `5xx`, backoff `5000 * (attempt + 1)` ms — vì device code là **one-shot** và endpoint mint bị rate-limit gắt.

Kiểm tra kết quả mint (`muse.js:100-109`):

| Điều kiện response | Hành vi |
|---|---|
| `payload.is_subs_active === false` | `throw` — *"Muse Code subscription is inactive — activate it on muse.ai first"* |
| `!payload.api_key && (payload.require_payment \|\| action_url)` | `throw` kèm `action_url` |
| `!payload.api_key` | `throw` — *"Muse Code key response is missing api_key"* |

Error envelope của Meta (`403` code `4705002`) được parse để lấy `title`/`detail`/`action_url` (`muse.js:81-89`).

**`mapTokens`** (`muse.js:113-129`) — điểm quan trọng nhất:

```js
return {
  accessToken: payload.api_key,     // chat dùng key đã mint, KHÔNG phải token Meta
  refreshToken: null,               // Meta không cấp refresh token
  expiresIn: null,
  email: payload.user_email?.trim().toLowerCase() || undefined,
  providerSpecificData: {
    authMethod: "device_code",
    oauthAccessToken: tokens.access_token,   // giữ lại để re-mint sau này
    subscriptionTier: payload.subs_tier_name || null,
  },
};
```

Chiến lược refresh của 9Router: **không có**. Comment `muse.js:115-116`: *"No expiry/refresh from Meta — a dead key means re-login."* Comment `oauth.js:130-131` xác nhận: *"Meta rejects refresh_token grants; the minted Model API key never expires."*

### 2.4 Alias

`docs/9router/tests/__baseline__/alias-baseline.json:121` → `"muse-ai": "muse"`; `tests/__baseline__/verify-alias.mjs:26` liệt kê `"muse", "muse-ai", "meta-model-api"`.

---

## 3. Cách OmniRoute tích hợp muse-code

### 3.1 Hằng số — `docs/OmniRoute/open-sse/config/museCode.ts` (40 dòng)

```ts
export const MUSE_CODE_USER_AGENT = "muse-code/1.0.2";
export const MUSE_CODE_INFERENCE_USER_AGENT =
  "muse-build/1.3.0 (interactive; macos-aarch64; build ac7280f2aca67769d1455a8847bb502b617d50f6)";
export const MUSE_CODE_AUTH_HOST = "https://auth.meta.com";
export const MUSE_CODE_DEVICE_CODE_URL = `${MUSE_CODE_AUTH_HOST}/oidc/device/authorization/`;
export const MUSE_CODE_TOKEN_URL       = `${MUSE_CODE_AUTH_HOST}/oidc/device/token/`;
export const MUSE_CODE_MINT_URL        = "https://api.meta.ai/muse-code/key";
export const MUSE_CODE_API_BASE_URL    = "https://api.meta.ai/v1";
export const MUSE_CODE_RESPONSES_URL   = `${MUSE_CODE_API_BASE_URL}/responses`;
export const MUSE_CODE_DEVICE_GRANT    = "urn:ietf:params:oauth:grant-type:device_code";
export const MUSE_CODE_DEFAULT_POLL_INTERVAL_SEC = 5;
export const MUSE_CODE_MAX_POLL_MS = 15 * 60 * 1000;

export function museCodeHeaders(extra?) {
  return { Accept: "application/json", "User-Agent": MUSE_CODE_USER_AGENT, ...extra };
}
export function isMuseDcaToken(token) { return token?.trim().startsWith("dca:"); }
```

Header doc (`museCode.ts:1-8`) nói rõ mục tiêu: **tương thích CLIProxyAPI** (`internal/auth/meta`, `internal/runtime/executor/meta_executor*`). Hai User-Agent khác nhau: `muse-code/1.0.2` cho auth, `muse-build/1.3.0 (...)` cho inference.

### 3.2 Mint key — `docs/OmniRoute/open-sse/services/museCodeAuth.ts` (80 dòng)

```ts
const response = await fetch(mintUrl, {
  method: "POST",
  headers: museCodeHeaders({ Authorization: `Bearer ${token}`, "Content-Type": "application/json" }),
  body: JSON.stringify({ dca_token: token }),     // ← khác 9Router
});
```

`MuseMintedKey` (`museCodeAuth.ts:7-18`) parse: `api_key` **hoặc** `apiKey`, `base_url`/`baseUrl`, `user_email`/`email`, `user_full_name`/`name`, `subs_tier_name`, `subs_tier_id`, `is_subs_active`, `has_payment_method`, `require_payment`, `can_subscribe`. Có `normalizeMuseBaseUrl` để strip dấu `/` cuối.

### 3.3 Luồng OAuth — `docs/OmniRoute/src/lib/oauth/providers/muse-code.ts` (136 dòng)

- `flowType: "device_code"`, không dùng PKCE (comment `constants/oauth.ts:176`: *"PKCE is not used on this grant"*)
- `requestDeviceCode`: `POST` form **chỉ** `{ client_id }`
- `pollToken`: `POST` form `{ client_id, device_code, grant_type }`
- **`postExchange` mint là best-effort** (`muse-code.ts:75-92`):

```ts
try {
  const minted = await mintMuseApiKey(dcaToken);
  return { minted, dcaToken };
} catch {
  return { dcaToken };     // vẫn lưu record DCA-only, remint ở request đầu tiên
}
```

- `mapTokens` (`muse-code.ts:93-135`):

```ts
const inferenceKey = minted?.apiKey || (isMuseDcaToken(dcaToken) ? "" : dcaToken);
return {
  accessToken: inferenceKey || dcaToken,
  refreshToken: dcaToken,          // ← DCA token đóng vai refresh token
  expiresIn: hasMintedKey ? undefined : tokens.expires_in,
  email: minted?.email,
  displayName: minted?.name,
  providerSpecificData: { dcaToken, authKind: "oauth", baseUrl, email, name,
    subsTierName, subsTierId, isSubsActive, hasPaymentMethod, requirePayment,
    canSubscribe, dcaExpiresAt, lastRefresh },
};
```

### 3.4 Remint khi 401 — `docs/OmniRoute/open-sse/services/tokenRefresh/providers/museCode.ts` (58 dòng)

Đây là **khác biệt cốt lõi** so với 9Router. Comment `:6-10`:

> *"Muse Code has no refresh-token grant. CLIProxyAPI remints the inference key from the durable `dca:` device token on 401 / missing API key. OmniRoute stores that DCA token as `refreshToken`."*

Hàm `refreshMuseCodeToken` tìm DCA token (ưu tiên `providerSpecificData.dcaToken`, fallback `refreshToken`), gọi lại `mintMuseApiKey`, và trả về `{ accessToken: minted.apiKey, refreshToken: dcaToken, expiresIn: undefined, providerSpecificData: {...} }`. Lỗi chỉ `log.warn` và trả `null` (không ném).

Wiring: `open-sse/services/tokenRefresh.ts:455` (`case "muse-code"`), `:497` (danh sách provider).

### 3.5 Registry provider — `docs/OmniRoute/open-sse/config/providers/registry/muse-code/index.ts` (148 dòng)

```ts
const MUSE_SPARK_THINKING = ["minimal","low","medium","high","xhigh","max"];
// mỗi model:
{ contextLength: 1048576, maxOutputTokens: 65536,
  supportsReasoning: true, supportsXHighEffort: true,
  supportedThinkingEfforts, toolCalling: true, supportsVision: true,
  targetFormat: "openai-responses",
  unsupportedParams: ["logprobs","topLogprobs","logitBias"] }
```

Base URL là `MUSE_CODE_RESPONSES_URL`; `extraHeaders: { "User-Agent": MUSE_CODE_INFERENCE_USER_AGENT }`; `passthroughModels: true`; `reasoningTransport: "opaque"`.

Danh mục: `muse-spark-1.3`, `-1.3-contributor`, `-1.2`, `-1.2-contributor`, `-1.1` (không vision, thinking `["low","medium","high","xhigh"]`), `llama-4-maverick`, `llama-4-scout`, `llama-3.3-70b`, `llama-3.1-405b/70b/8b`, `llama-3.2-90b-vision`, `llama-3.2-11b-vision`. `muse-spark-1.2*` chỉ có `minimal..xhigh` (không `max`).

### 3.6 Client ID công khai

`docs/OmniRoute/src/lib/oauth/constants/oauth.ts:175-182`:

```ts
export const MUSE_CODE_CONFIG = {
  clientId: resolvePublicCred("muse_id", "MUSE_CODE_OAUTH_CLIENT_ID"),
  deviceCodeUrl: "https://auth.meta.com/oidc/device/authorization/",
  tokenUrl:      "https://auth.meta.com/oidc/device/token/",
  mintUrl:       "https://api.meta.ai/muse-code/key",
};
```

`MASK = "omniroute-public-v1"` (`open-sse/utils/publicCreds.ts:27`), XOR từng byte. Giải mã `muse_id` tại `publicCreds.ts:193`:

```
[94,93,93,88,68,93,64,77,80,31,71,65,90,85,93,85]  XOR "omniroute-public-v1"
= "1031625952748946"
```

**Trùng khớp chính xác** với `clientId` literal trong 9Router (`muse.js:63`). Hai repo độc lập xác nhận cùng một public client id.

### 3.7 Dual auth & các touchpoint khác

- `src/shared/constants/providers.ts:46` — `DUAL_AUTH_PROVIDER_IDS` chứa `"muse-code"` (vừa OAuth vừa API key dán tay)
- `src/shared/constants/providers/oauth.ts:310` — entry OAuth đầy đủ
- `open-sse/config/providers/index.ts:241,518` — đăng ký registry
- `docs/openapi.yaml:12946` — route `GET /api/v1/muse-code/models`
- Test: `tests/unit/muse-code-oauth.test.ts` (tham chiếu trong `stryker.conf.json:527`)
- `docs/reference/PROVIDER_REFERENCE.md:74` — alias `mc`, mô tả: *"Wire format is OpenAI Responses (POST /responses)"*

---

## 4. muse-spark-web — vì sao ExoRoute KHÔNG nên tích hợp

Đây là provider **web cookie không chính thức**, vận hành meta.ai bằng cách đảo ngược giao thức trình duyệt.

**Credential** (`src/shared/providers/webSessionCredentials.ts:127-138`):

```
credentialName: "ecto_1_sess + ecto1: WS auth token"
placeholder: "ecto_1_sess=...; ecto1:... (WS auth token from meta.ai DevTools → Network → WS → clippy)"
storageKeys: ["cookie", "ecto_1_sess", "abra_sess"]
```

**Giao thức** (`open-sse/executors/muse-spark-web.ts`):

| Hằng số | Giá trị |
|---|---|
| `META_AI_GRAPHQL_API` | `https://www.meta.ai/api/graphql` (dòng 15) |
| `META_AI_DEFAULT_COOKIE` | `ecto_1_sess` (dòng 20) |
| `META_AI_WARMUP_DOC_ID` | `e7f802582dbfed8e181b012e010993eb` (dòng 27) |
| `META_AI_MODE_SWITCH_DOC_ID` | `c32bbe999c48e64e855dc63177d5153f` (dòng 28) |
| `META_WS_APP_ID` | `1522763855472543` (dòng 29) |
| `META_WS_AUTHTYPE` / `DGW_VERSION` / `TIER` | `15:0` / `5` / `prod` |
| WS endpoint | `wss://gateway.meta.ai/ws/clippy?...` (dòng 904) |
| Frame types | intro `0x0f`, prompt `0x0d` + flag `0x80` |
| `MODEL_MAP` | `muse-spark`→`think_fast`, `muse-spark-thinking`→`think_hard`, `muse-spark-contemplating`→`think_hard` |
| Cache hội thoại | `MUSE_CONV_CACHE_MAX = 5000`, TTL 30 phút (`:269-271`) |

**Cảnh báo ToS do chính OmniRoute ghi** (`docs/reference/FREE_TIERS.md:125`):

> *"Meta ToS explicitly prohibits automated access without prior permission, reverse engineering without written permission, and circumventing technologi…"*

Bảng xếp hạng `FREE_TIERS.md:267` cho `muse-spark-web`: `keyless`, ToS `avoid`, risk `3`. `open-sse/config/freeTierCatalog.ts:46` → `"muse-spark-web": "avoid"`.

**Vì sao không phù hợp ExoRoute:**

1. ExoRoute **không có** khái niệm "web cookie provider" — không có credential kind tương ứng, và `AdapterCapabilities` không mô hình hoá nó.
2. Đòi hỏi WebSocket **nhị phân** với framing tự chế — vượt ra ngoài mô hình HTTP/SSE của `ProviderAdapter` (`read_event_stream`, `prepare_upstream_request`).
3. Cần cache hội thoại có trạng thái (thread continuity) — ExoRoute giữ request stateless theo thiết kế, và AGENTS.md yêu cầu mọi cache khoá theo entity có thể xoá phải có chính sách dọn dẹp.
4. Rủi ro pháp lý/ToS và độ bền thấp: OmniRoute đã phải sửa nhiều lần (GraphQL→WS `#7528`, đổi tên cookie `abra_sess`→`ecto_1_sess` `#5449`, `ECT0`/Ecto-era query `#1668`).

**Nếu vẫn muốn**: phải là một nhánh thiết kế riêng (executor riêng + credential kind mới + chính sách ToS rõ ràng), không thể nhét vào `ProviderAdapter` OAuth hiện tại. Khuyến nghị tách thành quyết định sản phẩm riêng, không gộp vào công việc muse-code.

---

## 5. Bề mặt tích hợp sẵn có của ExoRoute

### 5.1 Preset khai báo bằng YAML, sinh lúc build

`data/provider-presets.yaml` được `build.rs` parse và sinh ra `YAML_PROVIDER_PRESETS` (`build.rs:307-320`), rồi `src/provider_adapters/presets.rs:37` `include!` file sinh ra.

`build.rs:85-266` `validate_provider_presets` áp các ràng buộc **cứng** sau — vi phạm là **build fail**:

| Ràng buộc | Vị trí |
|---|---|
| `AUTH_TYPES` whitelist: `none`, `bearer`, `header`, `codex_oauth`, `antigravity_oauth`, `kilocode_oauth` | `build.rs:94-101` |
| `auth_panel` whitelist: `openai_codex`, `command_code`, `cline`, `kilocode` | `build.rs:240-250` |
| `(oauth_accounts \|\| api_key_auth_assist) == auth_panel.is_some()` | `build.rs:232-239` |
| Phải có `api_keys` hoặc `oauth_accounts` | `build.rs:226-231` |
| `default_model_prefix` **duy nhất** toàn cục, regex `[a-z0-9][a-z0-9._-]*` | `build.rs:63-73`, `:124-133` |
| URL preset phải `https://` | `build.rs:163-173` |
| `default_auth_type` phải nằm trong `supported_auth_types` | `build.rs:174-187` |
| `default_preferred_protocol` phải nằm trong `default_supported_protocols` | `build.rs:188-201` |
| `supported_upstream_protocols` ⊆ `{chat_completions, responses, messages, google_generate_content}` | `build.rs:102-107`, `:214-225` |

→ **Bắt buộc sửa `build.rs`**: thêm auth type mới (ví dụ `muse_oauth`) và panel mới (`muse`).

### 5.2 Trait `ProviderAdapter` — `src/provider_adapters/mod.rs:315-670`

Các hook quan trọng cho muse:

| Hook | Dòng | Dùng cho muse |
|---|---|---|
| `endpoint` / `upstream_endpoint` | `:318`, `:325` | `…/v1/responses` hoặc `…/v1/chat/completions` |
| `apply_request_auth` / `apply_upstream_request_auth` | `:377`, `:407` | Bearer + User-Agent inference |
| `uses_device_authorization` | `:567` | `true` |
| `start_device_authorization` | `:577` | `POST auth.meta.com/oidc/device/authorization/` |
| `poll_device_authorization` | `:588` | Poll + mint |
| `resolve_oauth_request_auth` | `:617` | Trả `OAuthRequestAuth { access_token, … }` |
| `save_oauth_account` | `:658` | Lưu record mã hoá |
| `discover_oauth_models` | `:528` | `GET /v1/models` |
| `test_oauth_model` | `:538` | Probe model |
| `validate_config` | `:361` | Ràng buộc base URL/protocol |

Kiểu dữ liệu device flow đã định nghĩa sẵn (`mod.rs:164-191`): `AdapterDeviceAuthorization { device_code, user_code, verification_uri, expires_in, interval }`, `AdapterDevicePoll { Pending, SlowDown, Denied, Expired, Approved(AdapterOAuthAccount) }`.

### 5.3 Kilo Code = bản mẫu gần trùng khớp

`src/provider_adapters/kilocode/` là ví dụ **hoàn chỉnh** cần bắt chước:

- `mod.rs:132-191` — `uses_device_authorization` / `start` / `poll`
- `mod.rs:193-219` — `resolve_oauth_request_auth` + `save_oauth_account`
- `mod.rs:226-236` — `device_auth_url()` lấy **origin** từ base URL của row
- `device.rs` (209 dòng) — client có timeout chặn trần:
  - `MAX_DEVICE_BODY_BYTES = 64 * 1024`, `MAX_DEVICE_CODE_INPUT_BYTES = 512`
  - `DEVICE_CONNECT_TIMEOUT_CAP = 3s`, `DEVICE_REQUEST_TIMEOUT_CAP = 15s`
  - `device_client()` đi qua `egress::provider_client(...)` (`device.rs:199-207`) → giữ DNS pinning, chặn private IP, UA `ExoRoute/<version>`
  - Map HTTP status → poll state (`device.rs:114-120`): `202→Pending`, `403→Denied`, `410→Expired`, `429→SlowDown`
  - URL percent-encode device code bằng `path_segments_mut().push()` (`device.rs:91-93`)

Và module tài khoản:

- `auth/kilocode/mod.rs:4-7` — comment: *"Kilo Code issues a long-lived device-authorization token without a refresh token or an expiry, so there is nothing to refresh here. A token the gateway later rejects is marked invalid by the normal credential-rotation path and the operator reconnects."* → **mô tả này áp dụng gần như nguyên vẹn cho muse**
- `auth/kilocode/account.rs` — `from_device_approval`, `decode`, `display_name`, `identity()` (email, fallback `sha256(token)` 16 byte hex), `validate_token` (ASCII graphic, `MAX_TOKEN_BYTES = 8 * 1024`)
- `auth/kilocode/store.rs` — `save_kilocode_account` (mã hoá `security::encrypt_secret`, `credential_type = "oauth"`, `oauth_account_identity`, cập nhật thay vì tạo trùng qua secondary index `ProviderApiKeyIndex`), `account_for_use`, `provider_base_url`

### 5.4 Admin device flow đã tổng quát hoá

- `src/admin/provider_auth/mod.rs:88-98` — `start_provider_auth` tự nhánh sang device flow khi `provider_adapters::uses_device_authorization(&adapter_id)`
- `src/admin/provider_auth/device.rs:15-57` — `start_provider_device_auth` trả `{ flow_id, authorization_url, user_code, expires_in, interval, method: "device_code" }`
- `src/admin/provider_auth/device.rs:83-203` — `provider_auth_poll`, có chặn tần suất theo `interval` (`:123-128`), và chỉ lấy `base_url` từ row provider (`:61-75`)

**Nợ kỹ thuật cần xử lý:** `device.rs` hard-code chuỗi *"Kilo Code"* trong 4 thông báo (`:73`, `:154`, `:163`, `:171`). Khi thêm muse, các chuỗi này sẽ hiển thị sai tên provider. Cần refactor nhỏ để dùng tên provider/preset.

### 5.5 Frontend — panel đã tổng quát hoá

`web/src/features/providers/DeviceCodeOAuthPanel.svelte` (254 dòng) là panel **dùng chung**, nhận prop `providerName` (mặc định `'Kilo Code'`, `:14`) và hiển thị `user_code`, `verificationUrl`, poll theo `interval`. **Không cần viết component mới, không cần chuỗi i18n mới** — các key đã tồn tại (`web/src/lib/locales/en.ts:962-963`).

Chỉ cần thêm một dòng vào `web/src/features/providers/auth-panel.registry.ts:12` và `:22-43`.

### 5.6 Định tuyến protocol theo từng model

- `src/provider_adapters/runtime.rs:30-37` — `model_upstream_protocol()` chỉ hoạt động khi capability `model_protocol_routing = true`, và hiện chỉ delegate cho `opencode::model_protocol`
- `src/provider_adapters/opencode/routing.rs:78-119` — bản mẫu ánh xạ model → protocol
- `src/gateway/execution/target_preparation.rs:95-130` — thứ tự ưu tiên: `target.protocol` → `target.model_protocol` (override lưu theo từng model) → `model_upstream_protocol(adapter, model)` → `provider.preferred_protocol` (khi adapter *không* bật model routing)

### 5.7 ExoRoute đã chạm tới muse-spark qua các adapter khác

Quan trọng để không tạo trùng lặp:

| Adapter | Model | Protocol | Vị trí |
|---|---|---|---|
| `opencode_go` | `muse-spark-1.2-contributor`, `muse-spark-1.3-contributor` | Responses | `opencode/routing.rs:93-94` |
| `opencode_zen` | `muse-spark-1.3`, `muse-spark-1.2`, `muse-spark-1.3-contributor-free` | Responses | `opencode/routing.rs:151-153` |
| `freebuff` | `meta/muse-spark-1.2-contributor` → alias `base2-free-muse-spark` | Chat Completions | `freebuff/catalog.rs:28,41` |

**Khoảng trống so với OmniRoute:** ExoRoute **không** có `MUSE_SPARK_MIN_OUTPUT_TOKENS` floor. OmniRoute phải thêm vì muse-spark tiêu hết budget vào reasoning ẩn rồi trả `HTTP 200` với content rỗng (`CHANGELOG.md` `#11214`, `#12497`), và phải rewrite `finish_reason: "length"` sai. Cần kiểm tra riêng cho đường opencode hiện có của ExoRoute.

### 5.8 Ràng buộc layout nguồn

`scripts/check_source_boundaries.mjs`:
- Ngân sách **800 dòng production** mỗi file (`budget = 800`, dòng 5)
- Cấm tên `*_tests.rs`; test module phải là `tests.rs` hoặc thư mục `tests/` (dòng 46-49)
- Cấm vừa có `<name>.rs` vừa có `<name>/mod.rs` (dòng 55-61)
- Danh sách `providerModules` (dòng 32-43) bắt buộc mỗi provider có `mod.rs`

---

## 6. Kế hoạch tích hợp muse-code vào ExoRoute — checklist theo thứ tự phụ thuộc

> Nguyên tắc: giữ nguyên hành vi hiện có; chỉ thêm mới. Không đụng vào đường opencode/freebuff đang chạy.

### Bước 1 — `build.rs`

```rust
const AUTH_TYPES: &[&str] = &[
    "none", "bearer", "header", "codex_oauth", "antigravity_oauth",
    "kilocode_oauth",
    "muse_oauth",                       // ← thêm
];
```
và panel (`build.rs:240-250`):
```rust
if let Some(panel) = capabilities.auth_panel.as_deref()
    && !matches!(panel, "openai_codex" | "command_code" | "cline" | "kilocode" | "muse")
```

### Bước 2 — `src/provider_adapters/mod.rs`

```rust
pub const MUSE_ADAPTER_ID: &str = "muse";
mod muse;                    // thêm vào khối mod (dòng 20-39)
```
Giữ `mod.rs` dưới 800 dòng production (hiện 735 dòng — **sát trần**, cần theo dõi).

### Bước 3 — `data/provider-presets.yaml`

```yaml
  - id: muse
    adapter_id: muse
    name: Muse (Meta Model API)
    description: Sign in with a Meta account (Muse Code subscription) or paste a Model API key from dev.meta.ai. Muse Spark reasoning requires the Responses wire format.
    category: oauth
    labels: []
    default_base_url: https://api.meta.ai/v1
    default_logo_url: null
    default_model_prefix: muse
    default_models:
      - muse-spark-1.3
      - muse-spark-1.2
      - muse-spark-1.1
      - muse-spark-1.3-contributor
      - muse-spark-1.2-contributor
    default_auth_type: muse_oauth
    supported_auth_types:
      - muse_oauth
      - bearer
    default_preferred_protocol: responses
    default_supported_protocols:
      - responses
      - chat_completions
    protocol_selectable: true
    capabilities:
      api_keys: true
      oauth_accounts: true
      local_usage_meter: false
      local_quota_tracking: false
      model_discovery: true
      usage_limits: false
      api_key_usage: false
      api_key_usage_status: unsupported
      model_protocol_routing: true
      supported_upstream_protocols: [responses, chat_completions]
      api_key_auth_assist: false
      model_catalog_authoritative: false
      event_stream_response: false
      auth_panel: muse
```

Kiểm tra: `default_model_prefix: muse` chưa bị chiếm (hiện có `kilo, kc, cx, cmc, ocg, ocz, openrouter, fb, ag, cl, cp, nim`). `default_auth_type` ∈ `supported_auth_types` ✓. `preferred` ∈ `supported` ✓. `oauth_accounts=true` ⇒ `auth_panel` phải `Some` ✓. `api_key_usage_status` nhất quán với `api_key_usage=false` ✓.

### Bước 4 — `src/provider_adapters/muse/mod.rs`

```rust
impl ProviderAdapter for MuseAdapter {
    fn adapter_id(&self) -> &'static str { MUSE_ADAPTER_ID }

    fn endpoint(&self, base_url: &str, protocol: Protocol, _: Option<&str>) -> Result<Url, String> {
        muse_endpoint(base_url, UpstreamProtocol::from(protocol))
    }

    fn validate_config(...) -> Result<(), &'static str> { /* base_url https + host api.meta.ai */ }

    // Bearer + UA inference (chỉ khi có credential)
    fn apply_upstream_request_auth(&self, request, auth) -> Result<RequestBuilder, String> {
        let request = self.apply_request_auth(request, auth.auth_type, auth.auth_header,
                                             auth.secret, auth.oauth_auth, auth.session_id)?;
        Ok(request.header(http::header::USER_AGENT, MUSE_INFERENCE_USER_AGENT))
    }

    fn uses_device_authorization(&self) -> bool { true }
    fn start_device_authorization(...)  { /* → muse::device::start_authorization */ }
    fn poll_device_authorization(...)   { /* → poll + mint → Approved */ }
    fn resolve_oauth_request_auth(...)  { /* → auth::muse::account_for_use */ }
    fn save_oauth_account(...)          { /* → auth::muse::save_muse_account */ }
    fn discover_oauth_models(...)       { /* GET {base}/models, Bearer */ }
    fn test_oauth_model(...)            { /* probe nhỏ nhất */ }
}
```

Lưu ý `apply_request_auth` mặc định (`mod.rs:377-405`) **từ chối** `oauth_auth`, nên bắt buộc phải override cả `apply_upstream_request_auth` (giống `kilocode/mod.rs:89-101`).

### Bước 5 — `src/provider_adapters/muse/device.rs`

Cấu trúc theo `kilocode/device.rs`, nhưng RFC 8628 chuẩn:

- **Host auth là hằng số cố định**, KHÔNG lấy từ row: `https://auth.meta.com` (khác `api.meta.ai`). `kilocode/mod.rs:226-236` lấy origin từ row vì Kilo dùng cùng host — muse thì không, nên dùng hằng số.
- `start_authorization`: `POST` form `{ client_id }`, `Content-Type: application/x-www-form-urlencoded`, `Accept: application/json`, UA `muse-code/1.0.2`. Parse `device_code`, `user_code`, `verification_uri`, `verification_uri_complete`, `expires_in`, `interval` (mặc định 5s).
- `poll_authorization`: `POST` form `{ client_id, device_code, grant_type: "urn:ietf:params:oauth:grant-type:device_code" }`. Map lỗi OAuth2 → state:
  - `authorization_pending` → `Pending`
  - `slow_down` → `SlowDown`
  - `access_denied` → `Denied`
  - `expired_token` → `Expired`
  - `429`/`5xx` → `Err(...)` (caller giữ flow pending, `admin/provider_auth/device.rs:178-183`)
- `mint_api_key(dca_token)`: `POST https://api.meta.ai/muse-code/key`, `Authorization: Bearer <dca>`, UA `muse-code/1.0.2`, body JSON. **Xem mục 9 — hai repo dùng body khác nhau.**
- Bắt buộc: giới hạn body (`read_limited_response`, 64 KiB), validate `verification_uri` phải `https`, không userinfo/fragment (như `kilocode/device.rs:148-156`), percent-encode mọi tham số.
- Mọi HTTP đi qua `egress::provider_client(...)` với timeout chặn trần.

### Bước 6 — `src/provider_adapters/muse/routing.rs`

```rust
pub(super) fn muse_model_protocol(model: &str) -> Option<UpstreamProtocol> {
    let m = model.to_ascii_lowercase();
    if m.starts_with("muse-spark") || m.starts_with("llama-") {
        Some(UpstreamProtocol::Responses)      // toàn bộ danh mục Meta dùng Responses
    } else {
        None                                    // passthrough → dùng preferred_protocol
    }
}
```
Và đăng ký nhánh trong `runtime.rs:35`:
```rust
open_code::model_protocol(adapter_id, model)
    .or_else(|| muse::model_protocol(adapter_id, model))
```
(`model_protocol` trong `opencode/routing.rs:78` đã có nhánh `_ => None` nên an toàn.)

### Bước 7 — `src/provider_adapters/auth/muse/`

`account.rs`:
```rust
pub struct MuseAccount {
    pub dca_token: String,
    #[serde(default)] pub api_key: Option<String>,
    #[serde(default)] pub email: Option<String>,
    #[serde(default)] pub display_name: Option<String>,
    #[serde(default)] pub subscription_tier: Option<String>,
}
```
- `from_device_approval(&Value)` — parse `access_token` (phải bắt đầu `dca:`), `expires_in`
- `with_minted(minted)` — gắn `api_key` đã mint
- `identity()` — `email:<email>` nếu có, fallback `token:<sha256(api_key|dca)[0..16]>`
- `validate_token` — ASCII graphic, trần 8 KiB (như `kilocode/account.rs:78-89`)

`store.rs`: `save_muse_account` / `account_for_use` sao chép `kilocode/store.rs` (mã hoá `security::encrypt_secret`, `credential_type = "oauth"`, `oauth_account_identity`, cập nhật qua secondary index thay vì tạo trùng, kiểm tra `adapter_id` trong transaction).

`MAX_ACCOUNT_BYTES` cần lớn hơn Kilo vì có thêm `api_key`: `MAX_TOKEN_BYTES * 3 + 4096`.

### Bước 8 — `src/provider_adapters/registry.rs`

```rust
static ADAPTERS: [&'static dyn ProviderAdapter; 14] = [ /* … */ &muse::MUSE_ADAPTER ];
```
(cập nhật cả hằng số độ dài mảng.)

### Bước 9 — `src/provider_adapters/auth/panels.rs`

```rust
pub const MUSE: &str = "muse";
pub const KNOWN_PANELS: [&str; 5] = [OPENAI_CODEX, COMMAND_CODE, CLINE, KILOCODE, MUSE];
```
(cập nhật test `known_panels_are_unique_and_nonempty` về `5`.)

### Bước 10 — `src/admin/provider_auth/device.rs`

Thay 4 chuỗi hard-code `"Kilo Code"` (`:73`, `:154`, `:163`, `:171`) bằng tên provider lấy từ preset (`presets::preset_for_adapter(&adapter_id).map(|p| p.name)`), để thông báo đúng cho cả Kilo Code và Muse. Đây là refactor **bắt buộc** — nếu bỏ qua, người dùng muse sẽ thấy thông báo "Kilo Code".

### Bước 11 — `src/provider_adapters/tests/registry_catalog.rs`

Thêm assert cho `MUSE_ADAPTER_ID` (adapter có trong `adapter_ids`, preset có trong `preset_ids`) và cho capability `model_protocol_routing`.

### Bước 12 — `web/src/features/providers/auth-panel.registry.ts`

```ts
export const KNOWN_AUTH_PANELS = ['openai_codex','command_code','cline','kilocode','muse'] as const;
// …
  muse: { oauth: DeviceCodeOAuthPanel, accounts: CodexAccountsPanel, apiKey: null },
```
Không cần i18n mới; `DeviceCodeOAuthPanel` đã generic. `providerName` do trang cha truyền vào.

### Bước 13 — `scripts/check_source_boundaries.mjs`

Thêm `'muse'` vào mảng `providerModules` (dòng 32-43).

### Bước 14 — Test

- Unit: parse `start_authorization`/`poll_authorization` (dùng `src/support/mock_upstream.rs`), map lỗi OAuth2 → poll state, validate `verification_uri`
- Unit: `mint_api_key` parse `api_key`/`apiKey`/`base_url`, và các trường hợp `is_subs_active=false`, thiếu `api_key`
- Unit: account round-trip (encrypt→decrypt), `identity()` ổn định khi reconnect, cập nhật thay vì tạo trùng
- Unit: `muse_model_protocol` cho `muse-spark-*` / `llama-*` / model lạ
- Integration: preset vượt `build.rs` validation (đã được kiểm tra tự động khi build)
- Integration: `validate_config` từ chối base URL không https / sai host

---

## 7. Quyết định thiết kế quan trọng

### 7.1 Chiến lược refresh — hai lựa chọn

ExoRoute **không có** hook refresh token chung cho OAuth. `resolve_oauth_request_auth` được gọi **mỗi lần thử** trong `gateway/execution/credential_rotation.rs:52`. Kilo Code chọn "token chết thì đánh dấu invalid, người dùng kết nối lại" (`auth/kilocode/mod.rs:4-7`).

| | Phương án A — re-login khi 401 | Phương án B — remint khi 401 (CLIProxyAPI/OmniRoute parity) |
|---|---|---|
| Cách làm | Mint tại login, lưu `api_key`; 401 → đường rotation hiện có đánh dấu invalid → người dùng kết nối lại | Lưu `dca_token` + `api_key`; khi thiếu/hết hạn `api_key` thì gọi mint lại trong `resolve_oauth_request_auth` |
| Công sức | Thấp (giống kilocode gần như 1:1) | Trung bình |
| Ưu | Khớp kiến trúc ExoRoute, không network call trên hot path | Trải nghiệm tốt hơn: subscription không cần login lại khi key bị thu hồi |
| Nhược | Phải login lại khi key chết | Thêm network call vào `resolve_oauth_request_auth` → **phải** gate chặt: chỉ gọi khi `api_key` rỗng, timeout chặn trần, không gọi khi đang có key |
| Rủi ro | Không | Nếu gate sai → 1 request mạng mỗi lần inference; nếu mint fail giữa lúc retry → phải trả về `Err` để rotation chuyển credential kế tiếp, không nuốt lỗi |

**Đề xuất:** làm **A** trước (đúng kiến trúc, ít rủi ro), sau đó bổ sung **B** như cải tiến có test riêng. Lưu `dca_token` trong record ngay từ đầu (đằng nào cũng cần cho B), nhưng đừng gọi mint trên hot path ở phiên bản đầu.

### 7.2 Chỉ Responses hay cả Chat Completions?

Bằng chứng:
- Meta chấp nhận **cả hai** wire format trên cùng key: `muse.js:38-41` *"Meta accepts Chat Completions and Responses wire formats on the same key"*
- Nhưng Muse Spark reasoning (gồm `encrypted_content` replay) **chỉ** round-trip đúng trên Responses → `muse.js:55-59` pin `targetFormat: "openai-responses"`; OmniRoute cũng đặt `targetFormat: "openai-responses"` cho **mọi** model (`muse-code/index.ts:25,70,82,…`)

**Đề xuất:** khai `supported_upstream_protocols: [responses, chat_completions]`, `default_preferred_protocol: responses`, và để `muse_model_protocol()` trả `Responses` cho mọi model đã biết. Giữ `chat_completions` cho model passthrough/legacy và cho người dùng chủ động chọn.

### 7.3 Host auth cố định, không lấy từ row

`kilocode/mod.rs:226-236` lấy origin từ base URL của row vì Kilo Code dùng chung host cho auth và inference. Meta thì **khác host** (`auth.meta.com` vs `api.meta.ai`). Vì vậy muse phải dùng hằng số `https://auth.meta.com` và `https://api.meta.ai/muse-code/key`, vẫn đi qua `egress::provider_client` để giữ DNS pinning và chặn private IP.

### 7.4 Bảo mật

- `dca_token` **và** `api_key` đều là secret → mã hoá bằng `security::encrypt_secret`, không log, không trả trong error
- `EXOROUTE_MASTER_KEY` phải có trước khi kết nối tài khoản (`admin/provider_auth/mod.rs:80-85` đã chặn sẵn)
- Không đưa client_id vào export DB (nó là public, nhưng giữ nguyên quy ước)
- Thông báo lỗi mint phải redact: `fetch` error phải dùng `.without_url()` như `kilocode/device.rs:56`

### 7.5 Client ID

Meta public CLI client id = `1031625952748946` (xác nhận chéo bởi cả hai repo). ExoRoute **không** có cơ chế obfuscation như OmniRoute `resolvePublicCred`, và cũng không có tiền lệ hard-code client id public trong repo (antigravity/codex dùng env hoặc flow riêng).

**Đề xuất:** hằng số trong `src/provider_adapters/muse/mod.rs` + cho phép override qua env `EXOROUTE_MUSE_CLIENT_ID` (đọc trong `config`), và ghi rõ trong comment đây là public client id của Muse CLI, không phải secret. Nếu đội ngũ lo ngại scanner regex, có thể thêm cơ chế mask tương tự OmniRoute — nhưng đó là quyết định riêng.

### 7.6 Giới hạn kỹ thuật cần tôn trọng

- `src/provider_adapters/mod.rs` hiện **735 dòng** production; trần là 800 → chỉ còn ~65 dòng. Nên đặt hằng số và logic của muse trong thư mục riêng, chỉ thêm vài dòng vào `mod.rs`.
- `MAX_DEVICE_BODY_BYTES`, `MAX_TOKEN_BYTES` phải có trần rõ ràng.
- `poll_device_authorization` không được vượt `interval` — cơ chế chặn đã có ở `admin/provider_auth/device.rs:123-128`.
- Không giữ lock qua `.await` (AGENTS.md).

---

## 8. Những gì KHÔNG làm

- **Không** tích hợp `muse-spark-web` (xem mục 4).
- **Không** đụng vào đường `opencode_go` / `opencode_zen` / `freebuff` đang phục vụ `muse-spark-*` — chúng là provider khác, người dùng khác, credential khác.
- **Không** thêm `MUSE_SPARK_MIN_OUTPUT_TOKENS` vào adapter `muse` — đây là vấn đề của **upstream** muse-spark (reasoning ẩn tiêu hết budget), ảnh hưởng cả đường opencode hiện có. Nếu muốn xử lý, phải làm ở tầng chung (post-translate body shaping) và có test riêng, theo dõi riêng.
- **Không** thêm dependency mới — mọi thứ cần thiết (`reqwest`, `serde_json`, `sha2`, `uuid`) đã có.

---

## 9. Điểm chưa xác minh / cần kiểm chứng thực tế

| # | Vấn đề | Bằng chứng xung đột | Cách xử lý |
|---|---|---|---|
| 1 | **Body của request mint key** | 9Router gửi `{"onboard": true}` (`muse.js:71`); OmniRoute gửi `{"dca_token": token}` (`museCodeAuth.ts:38`) | Cả hai đều gửi `Authorization: Bearer <token>`. Cần thử với API thật (hoặc gửi cả hai trường) trước khi chốt. OmniRoute tuyên bố parity CLIProxyAPI, 9Router tuyên bố `onboard:true` để enroll lần đầu. **Chưa xác minh.** |
| 2 | **Header `x-api-version: 1.0.0`** | 9Router gửi trên **cả** request device/token/mint/museHeaders-khi-OAuth (`muse.js:17,34,69`; `default.js:46`); OmniRoute **không** gửi, chỉ dùng User-Agent (`museCodeHeaders`) | Gửi kèm là vô hại; nên gửi để tối đa tương thích. **Chưa xác minh** Meta có bắt buộc không. |
| 3 | **User-Agent bắt buộc** | `museCode.ts:7` nói *"The Muse CLI User-Agent is required by Meta"*; 9Router **không** đặt UA riêng | Nên đặt UA `muse-code/1.0.2` cho auth và `muse-build/1.3.0 (...)` cho inference. **Chưa xác minh** mức độ bắt buộc. |
| 4 | **Có scope OAuth không?** | Cả hai repo **không** gửi `scope` trong device authorization; **không** dùng PKCE | Coi như không có scope. **Chưa xác minh** Meta có scope tuỳ chọn. |
| 5 | **`GET /v1/models` có cần UA inference không?** | 9Router dùng `modelsUrl: https://api.meta.ai/v1/models`; OmniRoute không khai discovery cho muse-code | Thử với UA inference; nếu 403 thì fallback danh mục tĩnh trong preset (`default_models`). |
| 6 | **Muse Spark có cần floor output token không?** | OmniRoute phải thêm `MUSE_SPARK_MIN_OUTPUT_TOKENS = 512` (`CHANGELOG.md` `#11214`, `#12497`) | Kiểm tra riêng trên đường opencode của ExoRoute; **chưa đánh giá** vì nằm ngoài phạm vi tích hợp provider. |
| 7 | **Gói subscription có giới hạn quota?** | Không repo nào mô hình hoá quota của muse-code (9Router `refreshToken: null`, OmniRoute không có usage fetcher cho muse-code) | Mặc định `usage_limits: false`, `api_key_usage: false` trong preset. Nếu sau này phát hiện endpoint usage thì thêm sau. |

---

## 10. Kiểm chứng bắt buộc sau khi triển khai

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo check --workspace --all-targets --all-features
cargo check --release
node scripts/check_source_boundaries.mjs
```

Từ `web/`:

```powershell
npm run check
npm run build
```

Ghi chú:
- `build.rs` sẽ **fail build** nếu preset vi phạm whitelist auth type/panel, prefix trùng, hoặc URL không https — đây là lưới an toàn tự động cho bước 1–3.
- `cargo check --release` cần thiết vì binary nhúng `web/dist` (`RustEmbed`); phải build frontend trước.
- `scripts/check_source_boundaries.mjs` sẽ chặn nếu file nào vượt 800 dòng hoặc sai quy ước đặt tên module test.
- Việc xác minh OAuth thật với `auth.meta.com` **không thể** làm bằng unit test — cần một lần chạy tay có tài khoản Meta thật để chốt các điểm ở mục 9.

---

## 11. Bảng tổng hợp thay đổi

| Tệp | Hành động |
|---|---|
| `build.rs` | Thêm `muse_oauth` vào `AUTH_TYPES`; thêm `muse` vào whitelist panel |
| `data/provider-presets.yaml` | Thêm preset `muse` |
| `src/provider_adapters/mod.rs` | `MUSE_ADAPTER_ID`, `mod muse;` |
| `src/provider_adapters/muse/mod.rs` | **Mới** — `ProviderAdapter` impl |
| `src/provider_adapters/muse/device.rs` | **Mới** — RFC 8628 + mint |
| `src/provider_adapters/muse/routing.rs` | **Mới** — model → protocol |
| `src/provider_adapters/muse/catalog.rs` | **Mới** (tuỳ chọn) — discovery + probe |
| `src/provider_adapters/muse/tests.rs` | **Mới** — test |
| `src/provider_adapters/auth/muse/mod.rs` | **Mới** |
| `src/provider_adapters/auth/muse/account.rs` | **Mới** — record + `identity()` |
| `src/provider_adapters/auth/muse/store.rs` | **Mới** — lưu/mã hoá/tải |
| `src/provider_adapters/auth/mod.rs` | `pub(crate) mod muse;` |
| `src/provider_adapters/auth/panels.rs` | `MUSE` + `KNOWN_PANELS` 5 phần tử |
| `src/provider_adapters/registry.rs` | Đăng ký adapter (13 → 14) |
| `src/provider_adapters/runtime.rs` | Nhánh `muse_model_protocol` trong `model_upstream_protocol` |
| `src/admin/provider_auth/device.rs` | **Refactor** 4 chuỗi hard-code "Kilo Code" → tên provider |
| `src/provider_adapters/tests/registry_catalog.rs` | Assert adapter/preset muse |
| `web/src/features/providers/auth-panel.registry.ts` | Thêm panel `muse` |
| `scripts/check_source_boundaries.mjs` | Thêm `muse` vào `providerModules` |

---

## 12. Kết luận

ExoRoute **không cần** thay đổi kiến trúc để nhận muse. Adapter `kilocode` đã chứng minh toàn bộ đường đi (device authorization → record OAuth mã hoá → bearer theo từng request → panel dashboard dùng chung) hoạt động đúng với một provider có token dài hạn **không refresh token** — đúng tình huống của Meta Muse.

Ba điểm cần quyết định trước khi viết code:

1. **Phạm vi:** chỉ `muse-code` (khuyến nghị) hay thêm cả `muse-spark-web` (không khuyến nghị — ToS + WebSocket + cache trạng thái).
2. **Refresh:** phương án A (re-login khi 401, khuyến nghị làm trước) hay B (remint từ `dca_token`).
3. **Wire format:** mặc định Responses cho toàn bộ danh mục Muse Spark, vẫn khai Chat Completions cho passthrough.

Ba điểm phải xác minh bằng API thật trước khi chốt: body của request mint (mục 9.1), header `x-api-version` (9.2), và User-Agent bắt buộc (9.3).

---

## 13. Trạng thái triển khai (muse-code) — đã hoàn tất

Người dùng đã chọn **cả hai** muse và chiến lược refresh **"mint lúc login + tự remint từ `dca_token` khi key chết"**. Phần `muse-code` đã được triển khai xong; `muse-spark-web` **chưa triển khai** và cần một quyết định kiến trúc riêng (mục 14).

### 13.1 Những gì đã ship

| Tệp | Trạng thái |
|---|---|
| `src/provider_adapters/muse/mod.rs` | **Mới** — `ProviderAdapter` impl đầy đủ |
| `src/provider_adapters/muse/device.rs` | **Mới** — RFC 8628 + mint (có `parse_token_response` / `parse_mint_response` thuần để test) |
| `src/provider_adapters/muse/catalog.rs` | **Mới** — discovery `GET /v1/models` |
| `src/provider_adapters/muse/probe.rs` | **Mới** — probe `Responses` với `max_output_tokens = 512` |
| `src/provider_adapters/muse/tests.rs` | **Mới** — 8 test |
| `src/provider_adapters/auth/muse/mod.rs` | **Mới** |
| `src/provider_adapters/auth/muse/account.rs` | **Mới** — `MuseAccount` (DCA token + inference key + metadata + `last_mint_at`) |
| `src/provider_adapters/auth/muse/store.rs` | **Mới** — lưu/mã hoá/tải, `persist_account` |
| `src/provider_adapters/auth/muse/tests.rs` | **Mới** — 6 test |
| `src/provider_adapters/mod.rs` | `MUSE_ADAPTER_ID`, `mod muse;`, hook `repair_rejected_credential` + re-export ở `dispatch` |
| `src/provider_adapters/dispatch.rs` | `repair_rejected_credential(...)` |
| `src/provider_adapters/registry.rs` | Đăng ký adapter (13 → 14) |
| `src/provider_adapters/auth/mod.rs` | `pub(crate) mod muse;` |
| `src/provider_adapters/auth/panels.rs` | `MUSE` + `KNOWN_PANELS` (5 phần tử) |
| `src/provider_adapters/tests/registry_catalog.rs` | Assert adapter/preset muse |
| `build.rs` | Thêm `muse` vào whitelist panel (không thêm auth type) |
| `data/provider-presets.yaml` | Preset `muse` (mục 13.3) |
| `src/gateway/execution/classify.rs` | Gọi `repair_rejected_credential` trước khi `mark_provider_key_invalid` |
| `src/admin/provider_auth/device.rs` | Bỏ hard-code "Kilo Code" → tên provider |
| `web/src/features/providers/auth-panel.registry.ts` | Panel `muse` (device code + danh sách account) |
| `scripts/check_source_boundaries.mjs` | `muse` trong `providerModules` |

### 13.2 Sai lệch so với kế hoạch ban đầu (và lý do)

1. **Không thêm `muse_oauth` vào `AUTH_TYPES` của `build.rs`.** Dùng đúng khuôn `cline`/`clinepass`: `supported_auth_types: [bearer]`, `oauth_accounts: true` + `api_keys: true`, panel `muse` là **device panel**. OAuth hay API key được quyết định bởi `credential_type` của record (`"oauth"`), không phải bởi `auth_type`. Ít xâm lấn hơn và không phải sửa `build.rs` cho auth type.
2. **Không có `src/provider_adapters/muse/routing.rs` và không sửa `runtime.rs`.** Preset đặt `model_protocol_routing: false` + `default_preferred_protocol: responses`. Override `model_protocol` theo từng model vẫn hoạt động qua chuỗi fallback sẵn có (`src/gateway/execution/target_preparation.rs:95-103`), nên một nhánh `muse_model_protocol` là abstraction thừa.
3. **Remint khi 401 dùng hook trait mới thay vì sửa thẳng `classify.rs`.** `ProviderAdapter::repair_rejected_credential` mặc định trả `false`; `classify.rs:47-65` chỉ chèn một `&& !...await` vào let-chain 401 sẵn có. Không đổi hành vi của 13 adapter còn lại.
4. **Thêm cooldown remint 60 giây (`REMINT_COOLDOWN_SECONDS`).** Nếu Meta liên tục từ chối key vừa mint, mỗi request lỗi sẽ sinh một request mint → biến traffic của client thành traffic mint. `MuseAccount.last_mint_at` được lưu cùng record; trong cooldown, credential rơi về đường invalid hoá chuẩn (trả `false`) và operator phải đăng nhập lại. Đây là dữ liệu **có hạn** (một timestamp), không phải cache mới.
5. **`MINT_REQUEST_TIMEOUT_CAP` = 10s** (không phải 20s) vì mint cũng chạy trên đường request khi key bị từ chối; trần phải đủ nhỏ để không giữ một request đã lỗi quá lâu. Khi lỗi, request hiện tại **vẫn lỗi** (không retry trong cùng request) — chỉ request kế tiếp dùng key mới.
6. **`MUSE_BASE_URL` không còn là hằng số production.** Row provider giữ base URL (đúng như preset), nên hằng số này chỉ còn giá trị tài liệu; test dùng literal.

### 13.3 Preset `muse`

`id/adapter_id: muse`, `category: oauth`, `default_base_url: https://api.meta.ai/v1`, `default_model_prefix: muse`, `default_auth_type: bearer`, `supported_auth_types: [bearer]`, `default_preferred_protocol: responses`, `default_supported_protocols: [responses, chat_completions]`, `protocol_selectable: true`, `auth_panel: muse`, `api_keys: true`, `oauth_accounts: true`, `model_discovery: true`, `model_catalog_authoritative: false`, các cờ còn lại `false`/`unsupported`.

`default_models`: `muse-spark-1.3`, `-1.3-contributor`, `-1.2`, `-1.2-contributor`, `-1.1`, `llama-4-maverick`, `llama-4-scout`, `llama-3.3-70b`, `llama-3.1-405b`, `llama-3.1-70b`, `llama-3.1-8b`, `llama-3.2-90b-vision`, `llama-3.2-11b-vision`.

### 13.4 Kiểm chứng đã chạy trên snapshot này

| Lệnh | Kết quả |
|---|---|
| `cargo fmt --all -- --check` | **Pass** |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | **Pass** |
| `cargo test --workspace --all-features` | **Pass** — 605 passed, 0 failed (14 test muse) |
| `cargo check --workspace --all-targets --all-features` | **Pass** |
| `cargo check --release` | **Pass** |
| `node scripts/check_source_boundaries.mjs` | **Pass** (exit 0) |
| `npm run check` (trong `web/`) | **Pass** — 0 errors, 0 warnings |
| `npm run build` (trong `web/`) | **Pass** (cảnh báo chunk > 500 kB là có sẵn, không do thay đổi này) |

**Chưa chạy (không thể chạy trong môi trường này):** mọi lệnh gọi API thật tới `auth.meta.com` / `api.meta.ai`. Vì vậy ba điểm ở mục 9 vẫn **chưa được xác minh thực tế**:

- Body của mint: code hiện dùng `{"onboard": true}` (theo 9Router). OmniRoute dùng `{"dca_token": token}` — **hai tài liệu tham chiếu mâu thuẫn nhau**. Nếu mint trả lỗi khi chạy thật, đây là nghi phạm đầu tiên.
- Header `x-api-version: 1.0.0`: code **luôn gửi** (theo 9Router). OmniRoute không gửi. Nếu inference trả 4xx, thử bỏ header này.
- User-Agent: `muse-code/1.0.2` cho auth, `muse-build/1.3.0 (interactive; macos-aarch64; build ac7280f2aca67769d1455a8847bb502b617d50f6)` cho inference.

Cũng chưa xác minh: `client_id = 1031625952748946` có còn hiệu lực không. Có thể override bằng `EXOROUTE_MUSE_CLIENT_ID`.

### 13.5 Khoảng trống đã biết, ngoài phạm vi thay đổi này

ExoRoute **không có** sàn `max_output_tokens` cho request thường. OmniRoute đặt `MUSE_SPARK_MIN_OUTPUT_TOKENS = 512` vì muse-spark tiêu hết ngân sách output cho reasoning ẩn rồi trả HTTP 200 với `content` rỗng. Probe của muse đã dùng 512 (`probe.rs`), nhưng đường gateway thì chưa. Cần một bước shaping body sau khi translate, áp cho các model muse-spark và chỉ khi client không yêu cầu cao hơn.

### 13.6 Không đụng tới

Các route muse-spark gián tiếp đang có: `opencode_go`/`opencode_zen` (Responses) và `freebuff` (`meta/muse-spark-1.2-contributor` → `base2-free-muse-spark`), cùng `src/provider_adapters/freebuff/catalog.rs`, `freebuff/tests.rs`, `src/admin/model_probes.rs`.

---

## 14. `muse-spark-web` — chưa triển khai, cần quyết định

`muse-spark-web` **không thể** là một provider drop-in như `muse-code`. Đây là kết luận có bằng chứng từ chính code ExoRoute, không phải đánh giá chủ quan:

1. **Không có transport WebSocket.** `Cargo.toml` không có `tokio-tungstenite` hay crate WS nào. OmniRoute dùng `wss://gateway.meta.ai/ws/clippy` (`docs/OmniRoute/open-sse/executors/muse-spark-web.ts`).
2. **Kiểu lỗi của stream chặn WS.** `UpstreamChunkStream = Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>` (`src/gateway/streaming.rs:47-48`). Một nguồn WS không thể tạo ra `reqwest::Error`. Đổi kiểu này kéo theo `reader.rs`, `preflight.rs`, `extract.rs`, `finalize.rs`, `errors.rs` và worker continuity — tức là thay đổi xuyên suốt đường streaming.
3. **Không có hook thay transport.** `src/gateway/execution/send.rs` luôn `reqwest` POST rồi đưa `reqwest::Response` vào `classify_response`. Không có điểm chèn nào cho transport khác.
4. **Cần cache hội thoại có trạng thái.** Frame intro `0x0f`, prompt `0x0d`, cờ `0x80`; OmniRoute dùng `META_CONV_CACHE_MAX = 5000` entry với TTL 30 phút — tức phải có capacity + cleanup policy tường minh (AGENTS.md yêu cầu).
5. **Cần loại credential mới** (cookie web của Meta) kèm UI riêng — không dùng chung device flow hay API key.
6. **Rủi ro ToS rõ ràng.** Chính OmniRoute gắn nhãn `tos: "avoid"` cho provider này.

Để làm đúng, cần một thay đổi kiến trúc thật: tách một abstraction transport cho upstream (HTTP/WS), đổi kiểu lỗi của stream, thêm cache có bound + loại credential cookie + UI. Đây là công việc nhiều rủi ro hơn hẳn `muse-code` và ảnh hưởng tới mọi adapter khác qua đường streaming.

**Đề xuất:** giữ nguyên `muse-code` (đã hoàn tất, đã kiểm chứng), và quyết định riêng cho `muse-spark-web` — hoặc (a) bỏ qua, hoặc (b) làm như một dự án kiến trúc riêng với thiết kế transport tường minh.

**Quyết định của người dùng (đã chốt):** **bỏ qua `muse-spark-web`**. Vì vậy phạm vi hoàn tất của đợt tích hợp này là `muse-code`; mục 14 giữ lại như hồ sơ lý do kỹ thuật để lần sau không phải điều tra lại. Nếu về sau muốn làm, đây là danh sách việc phải giải quyết trước, theo đúng thứ tự phụ thuộc: (1) thêm dependency WebSocket và bọc nó sau một abstraction transport của upstream; (2) đổi kiểu lỗi của `UpstreamChunkStream` khỏi `reqwest::Error` và cập nhật mọi consumer; (3) cho `send.rs` một điểm chèn transport; (4) thiết kế cache hội thoại có capacity + TTL + cleanup; (5) thêm loại credential cookie web và UI; (6) xác nhận lại rủi ro ToS với người dùng trước khi phát hành.

