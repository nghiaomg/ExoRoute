import type { Locale } from '../../lib/i18n';
import type { DocsPage } from '../../lib/navigation';
import { localizeDocsCopy } from './docsTranslations';

export interface DocsCodeSample {
  label: string;
  language: string;
  code: string;
}

export interface DocsStep {
  number: string;
  title: string;
  body: string;
  code?: DocsCodeSample;
  note?: string;
}

export interface IntegrationGuide {
  id: string;
  title: string;
  badge: string;
  summary: string;
  fields: Array<{ label: string; value: string }>;
  steps: string[];
  code?: DocsCodeSample;
  note?: string;
}

export interface DocsCopy {
  shell: {
    documentation: string;
    backToDashboard: string;
    signIn: string;
    menu: string;
    closeMenu: string;
    themeLight: string;
    themeDark: string;
    localFirst: string;
    publicDocs: string;
    copy: string;
    copied: string;
    copyFailed: string;
  };
  nav: Record<DocsPage, { label: string; description: string }>;
  home: {
    eyebrow: string;
    title: string;
    intro: string;
    primaryCta: string;
    secondaryCta: string;
    badges: string[];
    cards: Array<{ page: DocsPage; title: string; description: string; cta: string }>;
    architectureTitle: string;
    architectureIntro: string;
    flow: Array<{ label: string; description: string }>;
    securityTitle: string;
    securityBody: string;
  };
  quickstart: {
    eyebrow: string;
    title: string;
    intro: string;
    steps: DocsStep[];
    requestTitle: string;
    requestBody: string;
    request: DocsCodeSample;
    powershell: DocsCodeSample;
    checklistTitle: string;
    checklist: string[];
    nextTitle: string;
    nextBody: string;
  };
  integrations: {
    eyebrow: string;
    title: string;
    intro: string;
    connectionTitle: string;
    connectionBody: string;
    connectionFields: Array<{ label: string; value: string }>;
    compatibilityNote: string;
    guides: IntegrationGuide[];
    troubleshootingTitle: string;
    troubleshooting: Array<{ title: string; body: string }>;
  };
  reference: {
    eyebrow: string;
    title: string;
    intro: string;
    endpointTitle: string;
    endpointHeaders: string[];
    endpoints: Array<{ method: string; path: string; protocol: string; description: string }>;
    authTitle: string;
    authBody: string;
    authCode: DocsCodeSample;
    modelTitle: string;
    modelBody: string;
    modelCode: DocsCodeSample;
    environmentTitle: string;
    environmentBody: string;
    environmentRows: Array<{ name: string; value: string; description: string }>;
    safetyTitle: string;
    safetyItems: string[];
  };
}

const english: DocsCopy = {
  shell: {
    documentation: 'Documentation',
    backToDashboard: 'Back to dashboard',
    signIn: 'Admin sign in',
    menu: 'Documentation navigation',
    closeMenu: 'Close documentation navigation',
    themeLight: 'Switch to light mode',
    themeDark: 'Switch to dark mode',
    localFirst: 'Local-first gateway',
    publicDocs: 'Public documentation',
    copy: 'Copy',
    copied: 'Copied',
    copyFailed: 'Could not copy',
  },
  nav: {
    docs: { label: 'Overview', description: 'What ExoRoute does and how the pieces fit.' },
    'docs-quickstart': { label: 'Quickstart', description: 'Install, configure, and send your first request.' },
    'docs-integrations': { label: 'Integrations', description: 'Connect SDKs, coding tools, and clients.' },
    'docs-reference': { label: 'API reference', description: 'Endpoints, authentication, models, and limits.' },
  },
  home: {
    eyebrow: 'EXOROUTE DOCUMENTATION',
    title: 'One local gateway for every model you use.',
    intro: 'Route OpenAI and Anthropic-compatible traffic through one controlled endpoint, combine providers with fallbacks, and keep provider credentials behind your own gateway.',
    primaryCta: 'Start the quickstart',
    secondaryCta: 'Configure an integration',
    badges: ['OpenAI-compatible', 'Anthropic-compatible', 'Local-first', 'Failover ready'],
    cards: [
      { page: 'docs-quickstart', title: 'Quickstart', description: 'Get from a fresh install to a verified gateway request in a few minutes.', cta: 'Install and run' },
      { page: 'docs-integrations', title: 'Integrations', description: 'Copy working settings for SDKs, Cursor, Continue, Cline, and Roo Code.', cta: 'Connect a tool' },
      { page: 'docs-reference', title: 'API reference', description: 'Use the supported endpoints, protocols, auth headers, and model aliases correctly.', cta: 'Read the reference' },
    ],
    architectureTitle: 'The request path stays simple',
    architectureIntro: 'Your client only needs one base URL and one ExoRoute key. ExoRoute selects the configured combo target, converts protocols when needed, and records the request locally.',
    flow: [
      { label: 'Your tool', description: 'SDK, editor, CLI, or curl sends a standard API request.' },
      { label: 'ExoRoute', description: 'Authenticates the gateway key, resolves the combo, and applies bounded retries.' },
      { label: 'Providers', description: 'Configured upstream credentials and models receive the translated request.' },
    ],
    securityTitle: 'Keep the boundary intentional',
    securityBody: 'The gateway listener is loopback-only by default. For remote access, terminate TLS at a reverse proxy and forward to ExoRoute over loopback. Never put provider credentials or EXOROUTE_MASTER_KEY in a client tool.',
  },
  quickstart: {
    eyebrow: 'QUICKSTART',
    title: 'Install once, route everywhere.',
    intro: 'This path creates the smallest useful setup: one ExoRoute process, one provider, one combo alias, one gateway key, and one verified request.',
    steps: [
      {
        number: '01',
        title: 'Install ExoRoute',
        body: 'Use the npm launcher when Node.js 18+ is available. It downloads the matching Rust release binary and verifies its SHA-256 checksum.',
        code: { label: 'Terminal', language: 'sh', code: 'npm install -g exoroute\nexoroute' },
        note: 'The npm launcher starts the Rust binary; it does not compile Rust on the client machine.',
      },
      {
        number: '02',
        title: 'Open the local dashboard',
        body: 'Browse to http://127.0.0.1:8686. Complete the first-run password change, then open Providers and add an upstream credential.',
        note: 'The generated admin password and EXOROUTE_MASTER_KEY belong in the private ~/.exoroute/.env file. Do not paste either into an SDK or editor.',
      },
      {
        number: '03',
        title: 'Create a model alias',
        body: 'In Combos, create a stable alias such as coding-model and add one or more provider targets. Use priority order for deterministic fallback or round-robin for distribution.',
      },
      {
        number: '04',
        title: 'Create a gateway key',
        body: 'Open API keys, generate a client key, and copy it immediately. ExoRoute shows the secret only at creation time; this key is separate from every provider credential.',
      },
    ],
    requestTitle: 'Send the first request',
    requestBody: 'Use the combo alias from the previous step. The API key is an ExoRoute gateway key, not an OpenAI, Anthropic, or provider key.',
    request: {
      label: 'curl · macOS / Linux',
      language: 'sh',
      code: String.raw`export EXOROUTE_API_KEY='replace-with-your-gateway-key'
export EXOROUTE_BASE_URL='http://127.0.0.1:8686/v1'

curl "$EXOROUTE_BASE_URL/chat/completions" \
  -H "Authorization: Bearer $EXOROUTE_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "coding-model",
    "messages": [{"role": "user", "content": "Say hello in one sentence."}]
  }'`,
    },
    powershell: {
      label: 'PowerShell',
      language: 'powershell',
      code: String.raw`$env:EXOROUTE_API_KEY = 'replace-with-your-gateway-key'
$body = @{
  model = 'coding-model'
  messages = @(@{ role = 'user'; content = 'Say hello in one sentence.' })
} | ConvertTo-Json -Depth 5

Invoke-RestMethod 'http://127.0.0.1:8686/v1/chat/completions' -Method Post -Headers @{ Authorization = "Bearer $env:EXOROUTE_API_KEY" } -ContentType 'application/json' -Body $body`,
    },
    checklistTitle: 'Before connecting a tool',
    checklist: [
      'GET /v1/models returns the combo alias you plan to use.',
      'The client base URL ends in /v1, unless the tool asks for the gateway origin separately.',
      'The Authorization header contains the ExoRoute gateway key.',
      'The model field uses the combo alias, not an unconfigured upstream model ID.',
      'Remote deployments use HTTPS at the reverse proxy and keep ExoRoute bound to loopback.',
    ],
    nextTitle: 'Next: connect your tool',
    nextBody: 'Use the integration recipes for SDKs and coding tools, then use the API reference when you need streaming, Responses, Anthropic Messages, or stream continuity.',
  },
  integrations: {
    eyebrow: 'INTEGRATIONS',
    title: 'Connect the tools you already use.',
    intro: 'Every integration uses the same three values: an ExoRoute base URL, an ExoRoute gateway key, and a configured combo alias. Choose the protocol your tool speaks and copy the matching recipe.',
    connectionTitle: 'Shared connection settings',
    connectionBody: 'For a local process, use loopback. For a remote deployment, use the HTTPS reverse-proxy URL. Do not expose the plain HTTP ExoRoute listener directly to the internet.',
    connectionFields: [
      { label: 'OpenAI-compatible base URL', value: 'http://127.0.0.1:8686/v1' },
      { label: 'Anthropic SDK base URL', value: 'http://127.0.0.1:8686' },
      { label: 'Anthropic raw HTTP path', value: 'http://127.0.0.1:8686/v1/messages' },
      { label: 'API key', value: 'The key generated under Dashboard → API keys' },
      { label: 'Model', value: 'A combo alias returned by GET /v1/models' },
    ],
    compatibilityNote: 'If a client asks for “API type”, choose OpenAI-compatible for /v1/chat/completions or /v1/responses. Choose Anthropic-compatible for /v1/messages. The Anthropic Python SDK receives the origin http://127.0.0.1:8686 and appends /v1/messages; raw HTTP clients use the full path.',
    guides: [
      {
        id: 'curl',
        title: 'cURL',
        badge: 'HTTP',
        summary: 'The fastest way to validate a key, alias, streaming response, or reverse-proxy path.',
        fields: [
          { label: 'Base URL', value: 'http://127.0.0.1:8686/v1' },
          { label: 'Header', value: 'Authorization: Bearer <EXOROUTE_API_KEY>' },
          { label: 'Model', value: 'Your combo alias' },
        ],
        steps: [
          'Replace the key and model alias in the sample.',
          'Use /v1/models first if you are unsure which aliases are enabled.',
          'Add "stream": true to the JSON body to receive SSE events.',
        ],
        code: {
          label: 'Chat Completions',
          language: 'sh',
          code: String.raw`curl http://127.0.0.1:8686/v1/chat/completions \
  -H "Authorization: Bearer $EXOROUTE_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"model":"coding-model","messages":[{"role":"user","content":"Explain Rust ownership."}]}'`,
        },
      },
      {
        id: 'python-openai',
        title: 'OpenAI SDK · Python',
        badge: 'OPENAI',
        summary: 'Use the official OpenAI client for Chat Completions or Responses while keeping one gateway endpoint.',
        fields: [
          { label: 'Package', value: 'pip install openai' },
          { label: 'base_url', value: 'http://127.0.0.1:8686/v1' },
          { label: 'api_key', value: 'EXOROUTE_API_KEY' },
        ],
        steps: [
          'Install the OpenAI Python SDK in your virtual environment.',
          'Set the gateway key in an environment variable rather than source control.',
          'Pass the combo alias as model; the SDK does not need to know the upstream provider.',
        ],
        code: {
          label: 'Python',
          language: 'python',
          code: String.raw`import os
from openai import OpenAI

client = OpenAI(
    api_key=os.environ["EXOROUTE_API_KEY"],
    base_url="http://127.0.0.1:8686/v1",
)

response = client.chat.completions.create(
    model="coding-model",
    messages=[{"role": "user", "content": "Explain Rust ownership in one sentence."}],
)
print(response.choices[0].message.content)`,
        },
      },
      {
        id: 'node-openai',
        title: 'OpenAI SDK · Node.js',
        badge: 'OPENAI',
        summary: 'Use the official JavaScript client from a server, script, or TypeScript application.',
        fields: [
          { label: 'Package', value: 'npm install openai' },
          { label: 'baseURL', value: 'http://127.0.0.1:8686/v1' },
          { label: 'apiKey', value: 'EXOROUTE_API_KEY' },
        ],
        steps: [
          'Keep the gateway key in the process environment or a secret manager.',
          'Set baseURL, not a provider-specific URL, in the OpenAI client constructor.',
          'Use the same client for Chat Completions or Responses supported by your SDK version.',
        ],
        code: {
          label: 'Node.js / TypeScript',
          language: 'typescript',
          code: String.raw`import OpenAI from "openai";

const client = new OpenAI({
  apiKey: process.env.EXOROUTE_API_KEY,
  baseURL: "http://127.0.0.1:8686/v1",
});

const response = await client.chat.completions.create({
  model: "coding-model",
  messages: [{ role: "user", content: "List three Rust ownership rules." }],
});

console.log(response.choices[0]?.message?.content);`,
        },
      },
      {
        id: 'python-anthropic',
        title: 'Anthropic SDK · Python',
        badge: 'MESSAGES',
        summary: 'Send Anthropic Messages requests through ExoRoute when your combo routes to a Claude-compatible target.',
        fields: [
          { label: 'Package', value: 'pip install anthropic' },
          { label: 'base_url', value: 'http://127.0.0.1:8686' },
          { label: 'api_key', value: 'EXOROUTE_API_KEY' },
        ],
        steps: [
          'Use the Anthropic client and its Messages API, not an OpenAI Chat Completions payload.',
          'Set max_tokens because Anthropic Messages requires it.',
          'Preserve thinking blocks and tool history when continuing a Claude conversation unless your provider configuration explicitly handles them.',
        ],
        code: {
          label: 'Python',
          language: 'python',
          code: String.raw`import os
from anthropic import Anthropic

client = Anthropic(
    api_key=os.environ["EXOROUTE_API_KEY"],
    base_url="http://127.0.0.1:8686",
)

message = client.messages.create(
    model="claude-route",
    max_tokens=256,
    messages=[{"role": "user", "content": "Explain a Rust lifetime."}],
)
print(message.content[0].text)`,
        },
        note: 'The configured combo still decides the upstream model. The client-facing model name can be an alias such as claude-route.',
      },
      {
        id: 'cursor',
        title: 'Cursor',
        badge: 'EDITOR',
        summary: 'Point Cursor at the OpenAI-compatible gateway so model changes and fallback policy stay in ExoRoute.',
        fields: [
          { label: 'Provider', value: 'OpenAI / OpenAI-compatible' },
          { label: 'Base URL', value: 'http://127.0.0.1:8686/v1' },
          { label: 'API key', value: 'Your ExoRoute gateway key' },
          { label: 'Model', value: 'Your combo alias, for example coding-model' },
        ],
        steps: [
          'Open Cursor Settings → Models and add an OpenAI-compatible model.',
          'Paste the ExoRoute base URL including /v1 and enter the gateway key.',
          'Use the combo alias exactly as it appears in GET /v1/models.',
          'Run a small chat or edit request first; inspect Requests in the dashboard before enabling it for larger work.',
        ],
        note: 'Cursor labels and model settings can move between releases. The four values above are the contract; do not paste an upstream provider key into Cursor.',
      },
      {
        id: 'continue',
        title: 'Continue',
        badge: 'IDE',
        summary: 'Configure an OpenAI-compatible model in Continue using a project or user config YAML file.',
        fields: [
          { label: 'Provider', value: 'openai' },
          { label: 'apiBase', value: 'http://127.0.0.1:8686/v1' },
          { label: 'apiKey', value: 'Your ExoRoute gateway key' },
          { label: 'model', value: 'Your combo alias' },
        ],
        steps: [
          'Open Continue configuration and add a model entry.',
          'Keep the key outside a committed repository when the config supports environment interpolation.',
          'Set apiBase to the /v1 endpoint; do not append /chat/completions.',
        ],
        code: {
          label: 'config.yaml',
          language: 'yaml',
          code: String.raw`models:
  - name: ExoRoute coding
    provider: openai
    model: coding-model
    apiBase: http://127.0.0.1:8686/v1
    apiKey: \${EXOROUTE_API_KEY}`,
        },
        note: 'Continue configuration keys may vary by version. Keep the provider, base URL, key, and model mapping equivalent to this example.',
      },
      {
        id: 'cline-roo',
        title: 'Cline / Roo Code',
        badge: 'CODING AGENT',
        summary: 'Use the OpenAI-compatible provider option for agentic coding workflows and keep the gateway key scoped to the client.',
        fields: [
          { label: 'API provider', value: 'OpenAI Compatible' },
          { label: 'Base URL', value: 'http://127.0.0.1:8686/v1' },
          { label: 'API key', value: 'Your ExoRoute gateway key' },
          { label: 'Model ID', value: 'Your combo alias' },
        ],
        steps: [
          'Open the extension provider settings and select OpenAI Compatible.',
          'Enter the gateway base URL, key, and combo alias.',
          'Start with a bounded task and confirm the request appears in the ExoRoute dashboard.',
          'Keep backup, migration, deployment, and destructive tasks behind your normal review and approval process; routing through ExoRoute does not make them safe automatically.',
        ],
        note: 'If the extension asks for an organization or project ID, leave it empty unless your configured upstream specifically requires one.',
      },
    ],
    troubleshootingTitle: 'When an integration does not connect',
    troubleshooting: [
      { title: '401 or 403', body: 'Use the ExoRoute gateway key generated under API keys. Provider keys are only entered in Providers and must not be sent by the client.' },
      { title: '404 model or route', body: 'Call GET /v1/models and copy an enabled combo alias. An upstream model ID is not automatically a client-facing model name.' },
      { title: 'Connection refused', body: 'Confirm the process is running on the expected host and port. A loopback listener is intentionally unreachable from another machine without a reverse proxy.' },
      { title: 'Streaming looks wrong', body: 'Use the protocol-native endpoint and let the SDK parse SSE. Do not route an Anthropic Messages request through a Chat Completions-only adapter setting.' },
    ],
  },
  reference: {
    eyebrow: 'API REFERENCE',
    title: 'The contract between your client and ExoRoute.',
    intro: 'Use the gateway API key on every model request. The client-facing model namespace is made of configured combo aliases and provider/model aliases exposed by the gateway.',
    endpointTitle: 'Endpoints',
    endpointHeaders: ['Method', 'Path', 'Protocol', 'Use it for'],
    endpoints: [
      { method: 'POST', path: '/v1/chat/completions', protocol: 'OpenAI Chat', description: 'Chat requests, including SSE streaming when stream is true.' },
      { method: 'POST', path: '/v1/responses', protocol: 'OpenAI Responses', description: 'Responses API clients and tools that preserve response items.' },
      { method: 'POST', path: '/v1/messages', protocol: 'Anthropic Messages', description: 'Claude-compatible messages, tools, content blocks, and streaming.' },
      { method: 'GET', path: '/v1/models', protocol: 'Gateway catalog', description: 'List enabled combo aliases and provider/model aliases available to the key.' },
      { method: 'GET', path: '/v1/streams/{stream_id}', protocol: 'SSE replay', description: 'Resume a retained stream when stream continuity is enabled.' },
      { method: 'DELETE', path: '/v1/streams/{stream_id}', protocol: 'SSE control', description: 'Stop a retained background stream explicitly.' },
    ],
    authTitle: 'Authentication',
    authBody: 'Gateway API routes require an ExoRoute API key. Send it as a Bearer token. Admin sessions and provider credentials use separate authentication boundaries and must never be reused by a client tool.',
    authCode: {
      label: 'Required request headers',
      language: 'http',
      code: String.raw`Authorization: Bearer <EXOROUTE_API_KEY>
Content-Type: application/json`,
    },
    modelTitle: 'Model resolution',
    modelBody: 'Combos are the stable client contract. A combo can point to multiple provider targets with priority or round-robin strategy. Change provider credentials and fallbacks in the dashboard without changing every client configuration.',
    modelCode: {
      label: 'Discover enabled names',
      language: 'sh',
      code: String.raw`curl http://127.0.0.1:8686/v1/models \
  -H "Authorization: Bearer $EXOROUTE_API_KEY"`,
    },
    environmentTitle: 'Runtime configuration',
    environmentBody: 'These values configure the gateway process. They are not client integration settings and should stay on the host running ExoRoute.',
    environmentRows: [
      { name: 'EXOROUTE_HOST', value: '127.0.0.1', description: 'Listener bind address. Keep loopback unless a supported deployment boundary is in place.' },
      { name: 'EXOROUTE_PORT', value: '8686', description: 'HTTP listener port used by the dashboard and gateway.' },
      { name: 'EXOROUTE_DATA_DIR', value: '~/.exoroute', description: 'Directory for the private .env file and local application data.' },
      { name: 'EXOROUTE_DATABASE_PATH', value: 'exoroute.lmdb', description: 'LMDB environment path, resolved according to the data directory rules.' },
      { name: 'EXOROUTE_MASTER_KEY', value: 'generated / stable', description: 'Encryption key for stored provider credentials; preserve it with the installation.' },
    ],
    safetyTitle: 'Operational boundaries',
    safetyItems: [
      'Put ExoRoute behind a TLS reverse proxy for remote access; the built-in listener is plain HTTP and defaults to loopback.',
      'Store provider secrets only in the dashboard or its protected data directory. Never commit .env, provider keys, gateway keys, or EXOROUTE_MASTER_KEY.',
      'Keep one ExoRoute process on a local filesystem for an LMDB environment. Do not open the same environment from multiple processes or a network filesystem.',
      'Treat combo aliases as routing configuration, not access control. Use separate gateway keys and normal client-side authorization for different workloads.',
      'Streaming continuity is bounded. It can resume a retained stream after a client disconnect, but it cannot survive a gateway process restart.',
    ],
  },
};

const vietnamese: DocsCopy = {
  ...english,
  shell: {
    ...english.shell,
    documentation: 'Tài liệu',
    backToDashboard: 'Về dashboard',
    signIn: 'Đăng nhập admin',
    menu: 'Điều hướng tài liệu',
    closeMenu: 'Đóng điều hướng tài liệu',
    themeLight: 'Chuyển sang giao diện sáng',
    themeDark: 'Chuyển sang giao diện tối',
    localFirst: 'Gateway local-first',
    publicDocs: 'Tài liệu công khai',
    copy: 'Sao chép',
    copied: 'Đã sao chép',
    copyFailed: 'Không thể sao chép',
  },
  nav: {
    docs: { label: 'Tổng quan', description: 'ExoRoute làm gì và các thành phần kết nối ra sao.' },
    'docs-quickstart': { label: 'Bắt đầu nhanh', description: 'Cài đặt, cấu hình và gửi request đầu tiên.' },
    'docs-integrations': { label: 'Tích hợp', description: 'Kết nối SDK, editor, CLI và các client.' },
    'docs-reference': { label: 'API reference', description: 'Endpoint, xác thực, model và giới hạn.' },
  },
  home: {
    ...english.home,
    eyebrow: 'TÀI LIỆU EXOROUTE',
    title: 'Một gateway local cho mọi model bạn sử dụng.',
    intro: 'Định tuyến traffic tương thích OpenAI và Anthropic qua một endpoint có kiểm soát, kết hợp provider với fallback, và giữ credential phía sau gateway của bạn.',
    primaryCta: 'Bắt đầu nhanh',
    secondaryCta: 'Cấu hình tích hợp',
    badges: ['Tương thích OpenAI', 'Tương thích Anthropic', 'Local-first', 'Có fallback'],
    cards: [
      { page: 'docs-quickstart', title: 'Bắt đầu nhanh', description: 'Từ cài đặt mới đến request gateway được xác minh trong vài phút.', cta: 'Cài đặt và chạy' },
      { page: 'docs-integrations', title: 'Tích hợp', description: 'Cấu hình mẫu cho SDK, Cursor, Continue, Cline và Roo Code.', cta: 'Kết nối công cụ' },
      { page: 'docs-reference', title: 'API reference', description: 'Dùng đúng endpoint, protocol, header xác thực và model alias.', cta: 'Đọc reference' },
    ],
    architectureTitle: 'Đường đi của request luôn rõ ràng',
    architectureIntro: 'Client chỉ cần một base URL và một ExoRoute key. ExoRoute chọn target của combo, chuyển protocol khi cần và ghi log cục bộ.',
    flow: [
      { label: 'Công cụ của bạn', description: 'SDK, editor, CLI hoặc curl gửi request chuẩn.' },
      { label: 'ExoRoute', description: 'Xác thực gateway key, resolve combo và áp dụng retry có giới hạn.' },
      { label: 'Providers', description: 'Credential và model upstream đã cấu hình nhận request đã chuyển đổi.' },
    ],
    securityTitle: 'Giữ ranh giới bảo mật rõ ràng',
    securityBody: 'Listener mặc định chỉ bind loopback. Khi truy cập từ xa, terminate TLS tại reverse proxy rồi forward về ExoRoute qua loopback. Không đưa provider credential hoặc EXOROUTE_MASTER_KEY vào client tool.',
  },
  quickstart: {
    ...english.quickstart,
    eyebrow: 'BẮT ĐẦU NHANH',
    title: 'Cài một lần, định tuyến mọi nơi.',
    intro: 'Thiết lập nhỏ nhất nhưng đầy đủ: một process ExoRoute, một provider, một combo alias, một gateway key và một request được xác minh.',
    steps: [
      { ...english.quickstart.steps[0], title: 'Cài ExoRoute', body: 'Dùng npm launcher khi máy có Node.js 18+. Launcher tải binary Rust đúng nền tảng và kiểm tra SHA-256.', note: 'npm launcher khởi chạy binary Rust; không compile Rust trên máy client.' },
      { ...english.quickstart.steps[1], title: 'Mở dashboard local', body: 'Mở http://127.0.0.1:8686. Đổi mật khẩu lần đầu, sau đó vào Providers để thêm credential upstream.', note: 'Mật khẩu admin và EXOROUTE_MASTER_KEY nằm trong ~/.exoroute/.env. Không dán chúng vào SDK hoặc editor.' },
      { ...english.quickstart.steps[2], title: 'Tạo model alias', body: 'Trong Combos, tạo alias ổn định như coding-model rồi thêm một hoặc nhiều target provider. Dùng priority cho fallback xác định hoặc round-robin để chia tải.' },
      { ...english.quickstart.steps[3], title: 'Tạo gateway key', body: 'Vào API keys, tạo client key và copy ngay. Secret chỉ hiển thị lúc tạo; key này tách biệt với mọi provider credential.' },
    ],
    requestTitle: 'Gửi request đầu tiên',
    requestBody: 'Dùng combo alias ở bước trước. API key là gateway key của ExoRoute, không phải key OpenAI, Anthropic hay provider.',
    checklistTitle: 'Trước khi kết nối công cụ',
    checklist: [
      'GET /v1/models trả về combo alias bạn định dùng.',
      'Base URL của client kết thúc bằng /v1, trừ khi tool yêu cầu origin riêng.',
      'Authorization header chứa ExoRoute gateway key.',
      'Trường model dùng combo alias, không dùng upstream model ID chưa cấu hình.',
      'Deploy từ xa phải dùng HTTPS ở reverse proxy và giữ ExoRoute bind loopback.',
    ],
    nextTitle: 'Tiếp theo: kết nối công cụ',
    nextBody: 'Dùng công thức tích hợp cho SDK và coding tool, sau đó xem API reference khi cần streaming, Responses, Anthropic Messages hoặc stream continuity.',
  },
  integrations: {
    ...english.integrations,
    eyebrow: 'TÍCH HỢP',
    title: 'Kết nối các công cụ bạn đang dùng.',
    intro: 'Mọi tích hợp đều dùng ba giá trị: ExoRoute base URL, ExoRoute gateway key và combo alias đã cấu hình. Chọn protocol mà tool sử dụng rồi copy công thức tương ứng.',
    connectionTitle: 'Cấu hình kết nối dùng chung',
    connectionBody: 'Process local dùng loopback. Deploy từ xa dùng URL HTTPS của reverse proxy. Không expose listener HTTP thuần của ExoRoute trực tiếp ra internet.',
    connectionFields: [
      { label: 'OpenAI-compatible base URL', value: 'http://127.0.0.1:8686/v1' },
      { label: 'Anthropic SDK base URL', value: 'http://127.0.0.1:8686' },
      { label: 'Anthropic raw HTTP path', value: 'http://127.0.0.1:8686/v1/messages' },
      { label: 'API key', value: 'Key tạo tại Dashboard → API keys' },
      { label: 'Model', value: 'Combo alias trả về từ GET /v1/models' },
    ],
    compatibilityNote: 'Nếu tool hỏi “API type”, chọn OpenAI-compatible cho /v1/chat/completions hoặc /v1/responses; chọn Anthropic-compatible cho /v1/messages. Anthropic Python SDK nhận origin http://127.0.0.1:8686 và tự gọi /v1/messages; raw HTTP client mới ghép đầy đủ path.',
    guides: english.integrations.guides.map((guide) => ({ ...guide, fields: guide.fields.map((field) => field.label === 'API key' ? { ...field, value: 'ExoRoute gateway key của bạn' } : field) })),
    troubleshootingTitle: 'Khi tích hợp không kết nối được',
    troubleshooting: [
      { title: '401 hoặc 403', body: 'Dùng ExoRoute gateway key tạo trong API keys. Provider key chỉ nhập trong Providers và không được gửi từ client.' },
      { title: '404 model hoặc route', body: 'Gọi GET /v1/models và copy combo alias đang bật. Upstream model ID không tự động trở thành model name phía client.' },
      { title: 'Connection refused', body: 'Kiểm tra process chạy đúng host và port. Listener loopback cố ý không truy cập được từ máy khác nếu không có reverse proxy.' },
      { title: 'Streaming sai', body: 'Dùng endpoint native của protocol và để SDK parse SSE. Không gửi Anthropic Messages qua cấu hình chỉ hỗ trợ Chat Completions.' },
    ],
  },
  reference: {
    ...english.reference,
    eyebrow: 'API REFERENCE',
    title: 'Contract giữa client và ExoRoute.',
    intro: 'Dùng gateway API key cho mọi model request. Namespace model phía client gồm combo alias và provider/model alias được gateway expose.',
    endpointTitle: 'Endpoint',
    authTitle: 'Xác thực',
    authBody: 'Gateway API route yêu cầu ExoRoute API key. Gửi key dưới dạng Bearer token. Admin session và provider credential là các ranh giới xác thực riêng, không dùng lại cho client tool.',
    modelTitle: 'Resolve model',
    modelBody: 'Combo là contract ổn định phía client. Một combo có thể trỏ tới nhiều provider target theo priority hoặc round-robin. Thay credential và fallback trong dashboard mà không phải đổi cấu hình mọi client.',
    environmentTitle: 'Cấu hình runtime',
    environmentBody: 'Các giá trị này cấu hình process gateway. Chúng không phải setting tích hợp client và phải nằm trên host chạy ExoRoute.',
    safetyTitle: 'Ranh giới vận hành',
    safetyItems: [
      'Đặt ExoRoute sau TLS reverse proxy khi truy cập từ xa; listener tích hợp là HTTP thuần và mặc định bind loopback.',
      'Chỉ lưu provider secret trong dashboard hoặc data directory được bảo vệ. Không commit .env, provider key, gateway key hoặc EXOROUTE_MASTER_KEY.',
      'Giữ một process ExoRoute trên filesystem local cho LMDB. Không mở cùng environment từ nhiều process hoặc network filesystem.',
      'Combo alias là cấu hình routing, không phải access control. Dùng gateway key riêng và authorization phía client cho workload khác nhau.',
      'Stream continuity có giới hạn. Nó resume stream sau khi client disconnect nhưng không sống qua việc gateway restart.',
    ],
  },
};

export function docsCopy(locale: Locale): DocsCopy {
  if (locale === 'en') return english;
  if (locale === 'vi') return localizeDocsCopy(vietnamese, locale);
  return localizeDocsCopy(english, locale);
}
