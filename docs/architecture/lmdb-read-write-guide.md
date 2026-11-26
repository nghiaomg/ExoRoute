# ExoRoute LMDB Read/Write Engineering Guide

**Status:** normative for all code that touches the embedded database  
**Scope:** `Cargo.toml`, `src/infra/storage/`, `src/infra/db/`, and every domain caller of the storage facade  
**Audit snapshot:** branch `develop`, commit `25b0f16`  
**Last verified:** 2026-11-10

This guide is the single source of truth for safe LMDB access in ExoRoute. It is grounded in the
actual code at the snapshot above. Every file path, crate name, type, constant, and function name
appearing here was read from this repository. Where the guide prescribes a pattern the current code
does **not** yet follow, it is explicitly marked **REQUIRED (not yet implemented)** and cross-referenced
from Section 7.

---

## Table of contents

1. [Architecture & LMDB Environment Setup](#1-architecture-lmdb-environment-setup)
2. [Key Design & Value Serialization Standards](#2-key-design-value-serialization-standards)
3. [Safe Read Operations (`RoTxn`)](#3-safe-read-operations-rotxn)
4. [Safe Write Operations (`RwTxn`) & Concurrency](#4-safe-write-operations-rwtxn-concurrency)
5. [Security, Encryption & Export/Import Invariants](#5-security-encryption-exportimport-invariants)
6. [Error Handling, Recovery & Retention](#6-error-handling-recovery-retention)
7. [Audit Findings in Current Codebase](#7-audit-findings-in-current-codebase-anti-patterns-to-eliminate)
8. [Developer & Reviewer Checklist](#8-developer-reviewer-checklist)

---

## Conventions

- **MUST / MUST NOT** — enforced invariant; violating PRs do not merge.
- **SHOULD** — strong default; deviations require written justification in the PR.
- **CURRENT** — behavior verified in the audited snapshot.
- **REQUIRED (not yet implemented)** — prescribed target behavior; Section 7 lists the concrete debt.
- File references use repository-relative paths, e.g. `src/infra/storage/environment.rs:97-104`.

---

## 1. Architecture & LMDB Environment Setup

### 1.1 Crate surface (verified)

`Cargo.toml` declares the exact stack this guide targets:

| Concern | Crate / version | Notes |
|---|---|---|
| LMDB binding | `heed = { version = "0.22.1", default-features = false, features = ["serde", "serde-bincode"] }` | The only LMDB binding. No `lmdb`, `lmdb-rkv`, or `libmdbx` crate is present. |
| Serialization | `bincode = "1.3"`, `serde = { version = "1", features = ["derive"] }`, `serde_json = "1"` | All raw record values are bincode. |
| Compression | `zstd = { version = "0.13", default-features = false, features = ["zdict_builder"] }` | Only request-log values may be zstd frames. |
| AEAD | `chacha20poly1305 = "0.10"` | ChaCha20-Poly1305 for secrets at rest. |
| Hashing / comparison | `sha2 = "0.10"`, `subtle = "2"`, `argon2 = "0.5"` | Token digests, constant-time compare, password hashing. |
| Randomness | `rand = "0.8"` | Nonces, tokens, master key (`OsRng`). |
| Async runtime | `tokio = { version = "1", features = [...] }` | Includes `rt-multi-thread`, `sync`, `time`. |
| Process lock | `fs2 = "0.4"` | `try_lock_exclusive` on the sibling `.lock` file. |

The package is Rust edition **2024** with `rust-version = "1.88"` (read from `Cargo.toml`); do not
assume the older edition from prose in tickets.

### 1.2 Layering rule (MUST)

All raw LMDB access is confined to `src/infra/storage/`. The only places in the codebase that call
`heed` directly are `environment.rs`, `read_txn.rs`, `write_txn.rs`, `records.rs` (error mapping),
and `bootstrap_storage_version` in `environment.rs`. Domain callers MUST go through:

- `Database::read(|txn| ...)` and `Database::write(|txn| ...)` (`src/infra/storage/environment.rs:161-262`);
- the `ReadTxn` / `WriteTxn` typed views (`src/infra/storage/read_txn.rs`, `src/infra/storage/write_txn.rs`);
- domain helpers in `src/infra/db/` or the owning feature module.

**MUST NOT** create a second `heed::Env`, call `env.read_txn()`/`env.write_txn()` outside
`environment.rs`, or open a second handle to the same files. One `Database` handle is cloned
(`Arc<DatabaseInner>`) across handlers and workers.

### 1.3 Environment flags and sizing (CURRENT)

From `src/infra/storage/environment.rs:32-140`:

| Setting | Value | Rationale / rule |
|---|---|---|
| Options | `EnvOpenOptions::new().read_txn_without_tls()` | Read transactions carry no thread-local storage; the closure-based `read` API never leaks transaction references across threads. |
| `map_size` (initial) | `INITIAL_MAP_SIZE = 1024 * 1024 * 1024` (1 GiB) | Validated nonzero, page-aligned (4096), and `<= map_size_limit`. |
| `map_size` (hard limit) | `MAX_MAP_SIZE = 1 << 40` (1 TiB) | Open is rejected if `env.info().map_size` falls outside `[requested, limit]`. |
| `max_readers` | `MAX_READERS = 256` | Reader slots are also bounded in-process by the operation semaphore. |
| `max_dbs` | `MAX_NAMED_DATABASES = 64` | 42 named tables are currently created; the reserve is for additive migrations. |
| Operation budget | `MAX_DATABASE_OPERATIONS = 64` | A `tokio::sync::Semaphore` shared by reads and writes; `try_acquire_owned` returns `StorageError::Busy` when exhausted. |
| Single writer | `Arc<Mutex<()>>` | `write` takes `lock_owned().await` before opening `RwTxn`. |
| Resize gate | `Arc<RwLock<()>>` | Every read and write holds a **read** guard until its transaction ends; `grow_map` takes the **write** guard for `Env::resize`. |

**MUST** preserve this gate discipline: no transaction may outlive the guard held in its
`spawn_blocking` closure. **MUST NOT** call `Env::resize` while any `RoTxn` or `RwTxn` is alive.

The environment is opened inside `tokio::task::spawn_blocking`
(`src/infra/storage/environment.rs:78-130`); `unsafe { options.open(...) }` is justified by the
comment there: the path is a validated private real directory, the process lock is held for the
lifetime of the environment, and mapped files are never replaced. Any change to those preconditions
requires re-justifying that safety comment in the same PR.

### 1.4 Map growth (CURRENT)

`Database::write` retries on `StorageError::MapFull` from the operation or commit, calls
`grow_map(attempted_map_size)`, and reruns the closure (`environment.rs:229-269`). `grow_map`:

1. takes the resize **write** guard;
2. coalesces concurrent growth (a waiter whose failed attempt used an older size just retries);
3. doubles the size, capped at `map_size_limit` (`checked_mul(2).min(limit)`, fails with `MapFull` when it cannot grow);
4. writes a zero-byte marker `.exoroute-map-size-<bytes>` **before** resizing
   (`write_map_size_marker`, `fs_guard.rs:121-147`, `sync_all()`);
5. resizes via `resize_with_marker_cleanup` and removes the marker if resize fails
   (`fs_guard.rs:167-181`);
6. stores the new size with `Ordering::Release`.

On the next open, `read_map_size_marker` (`fs_guard.rs:83-119`) walks the doubling sequence and
selects the largest marker present, so a crash between marker creation and resize never shrinks the
mapped size. **MUST** keep the marker protocol intact; it is the crash-safety mechanism.

### 1.5 Filesystem, process, and directory rules (CURRENT)

- Data root: `config::app_dir()` resolves to `$HOME/.exoroute` (or `%USERPROFILE%\.exoroute` on
  Windows) via `config/paths.rs:3-13,227-229`. The default environment directory is
  `exoroute.lmdb` under that root; `EXOROUTE_DATABASE_PATH` can name a different directory.
- `ensure_private_dir` (`config/paths.rs:41-91`): mode `0o700` on Unix; `restrict_windows_acl` on
  Windows; rejects symlinks and reparse points.
- `ensure_private_file` (`config/paths.rs:93-128`): mode `0o600` on Unix; regular file and no
  reparse points on Windows.
- `ensure_no_reparse_path_components` (`config/paths.rs:130-174`) checks **every** existing path
  component, not just the leaf.
- `ensure_environment_directory`, `preflight_environment_files`, and `secure_environment_files`
  (`src/infra/storage/fs_guard.rs:14-81`) reject symlinked or non-regular `data.mdb` / `lock.mdb`
  and harden permissions.
- `lock_database` (`src/infra/lock.rs:15-52`) takes an exclusive `fs2` lock on the sibling
  `<database-path>.lock` file (mode `0o600`, symlink-checked) held for the entire process lifetime
  (`src/app/bootstrap.rs:131`). LMDB permits multi-process access; ExoRoute does **not**, because
  auth/session/rate-limit state is process-local.

**MUST:** keep the database on a local filesystem, single process, private directory. **MUST NOT:**
place it on a network share, open it from a second process, or relax the symlink/permission guards.

### 1.6 Stored-format versioning (CURRENT)

`STORAGE_FORMAT_VERSION: u32 = 4` and `WITHDRAWN_STORAGE_FORMAT_VERSIONS = &[5]` live in
`src/infra/storage/mod.rs:49-76`. `is_readable_storage_version` accepts `1..=4` plus withdrawn
`5` (additive pre-release). The marker is stored in `Table::Meta` under
`storage_format_version` and stamped by `bootstrap_storage_version`
(`environment.rs:317-385`), which also runs registered v1→v2→v3→v4 upgrade steps. Unknown newer
versions and unregistered older versions fail closed.

**MUST** follow the existing rule: an additive change (new named table, optional field older readers
ignore) does **not** bump the version; an incompatible change bumps it **and** registers an upgrade
path in `bootstrap_storage_version`, plus backup validation in `src/admin/database/validate.rs`.

### 1.7 Named database / keyspace inventory (CURRENT)

Authoritative definition: `Table` enum and `Table::ALL` in
`src/infra/storage/records.rs:91-236`. Order in `ALL` is part of the on-disk format because backup
archives reference tables by index. All databases are `HeedDatabase<Str, Bytes>` (string keys,
raw byte values). Keys are validated by `validate_key` / `validate_prefix`
(`src/infra/storage/keys.rs`): 1–500 bytes, no NUL.

"Value" below is the bincode-serialized `Record` shape unless noted. **Encrypted** means the field
carries ChaCha20-Poly1305 ciphertext from `security::encrypt_secret` (Section 5).

| # | `Table::*` / name | Key format | Value / fields (abridged) | Secret? | Backup? |
|---|---|---|---|---|---|
| 1 | `Meta` / `meta` | Fixed strings: `storage_format_version`, `request_log_count`, `statistics_generation`, `provider_count`, `route_count`, `request_log_index_format`, `request_log_dictionary` | Raw bincode scalars (`u32`/`i64`), `Vec<u8>` dictionary | No | Yes (dictionary included; version marker excluded from snapshot) |
| 2 | `Providers` / `providers` | Provider id (UUID string) | `Record`: `id`, `adapter_id`, `base_url`, `auth_type`, counters; **`secret` and `custom_headers` are encrypted blobs** (`admin/providers/lifecycle.rs:53-111`, `gateway/execution/provider.rs:199-202`) | **Yes (fields)** | Yes |
| 3 | `ProviderApiKeys` / `provider_api_keys` | Credential id (UUID string) | `Record`: `provider_id`, `name`, `credential_type`, `enabled`, `invalid`, counters, timestamps; **`secret` = encrypted API key or encrypted OAuth JSON** (`admin/providers/provider_keys.rs:31-56`) | **Yes (field)** | Yes |
| 4 | `ProviderModels` / `provider_models` | `provider/{sha256(pid)}/model/{model}/{sha256(pid + NUL + model)}` (`infra/db/keys.rs:24-52`) | `Record` model metadata | No | Yes |
| 5 | `ProviderUsageMeters` / `provider_usage_meters` | `{credential_id}/{minute}` (`telemetry/store.rs:231`) | `Record` per-minute usage counts | No | No (unless `include_request_logs`) |
| 6 | `Routes` / `routes` | Combo/route id | `Record` route metadata (`admin/combos/storage.rs`) | No | Yes |
| 7 | `RouteTargets` / `route_targets` | `{route_id}/{ordinal}` composite (`admin/combos/storage.rs`) | `Record` target (`provider_id` or `combo_id`, model, order) | No | Yes |
| 8 | `ApiKeys` / `api_keys` | Gateway API key id (UUID string) | `Record`: `token_hash` = SHA-256 digest, `enabled`, scope fields, `request_count`, `request_count_generation` (`admin/api_keys.rs:338-347`) | Hash only | Yes |
| 9 | `AdminAuth` / `admin_auth` | `singleton` | `Record`: `password_hash` (Argon2id PHC string), `must_change_password`, `updated_at` (`admin/auth.rs:103-109`) | Hash only | Yes |
| 10 | `AdminSessionFamilies` / `admin_session_families` | Family id (UUID string) | `Record`: expiry timestamps, `must_change_password`, `revoked_at` (`admin/sessions/records.rs:14-31`) | No | **No** |
| 11 | `AdminRefreshTokens` / `admin_refresh_tokens` | `digest_key(token_digest)` (SHA-256 hex of refresh token, `admin/sessions/records.rs:10,60`) | `Record`: `family_id`, `generation`, `consumed_at` | Hash only | **No** |
| 12 | `SessionIndex` / `session_index` | `families/by-created/<ts>`, `tokens/<family>/<gen>` (`admin/sessions/records.rs:9-10`) | `String` family id / digest key | No | **No** |
| 13 | `StreamRuns` / `stream_runs` | Stream run id | `Record`: `status`, `latest_sequence`, `retained_bytes`, `replay_complete` (`gateway/continuity/records.rs`) | No | **No** |
| 14 | `StreamEvents` / `stream_events` | `{run_id}/{sequence}` (`stream_event_key`, `gateway/continuity/records.rs`) | `Record`: `sequence`, `payload` (SSE bytes) | No | **No** |
| 15 | `StreamRunApiKeyIndex` / `stream_run_api_key_index` | `{api_key_id}/{run_id}` | `String` run id | No | **No** |
| 16 | `RequestLogs` / `request_logs` | Request id (UUID string) | `Record` log fields; values may be zstd frames tagged `0x5A` (`infra/storage/codec.rs`) | No | Optional |
| 17 | `RequestLogIndex` / `request_log_index` | `request_log_by_time/<ts>/<id>` and `request_log_by_desc/<reversed ts>/<reversed id>/<id>` (`infra/db/keys.rs:4-22`) | `String` request id | No | Optional |
| 18 | `UsageMinutes` / `usage_minutes` | `{minute:020}/{kind:02}/{dimension_id}` (`telemetry/windows.rs:43-51`) | `Record` counts | No | Optional |
| 19 | `UsageWindows` / `usage_windows` | `window_key(window_minutes, kind, dimension)` (`telemetry/windows.rs:135`) | `Record` rolling counts | No | Optional |
| 20 | `UsageExpiryCursor` / `usage_expiry_cursor` | Window minutes as decimal string | `Record` `cursor_*` fields | No | Optional |
| 21 | `StatisticsState` / `statistics_state` | `singleton` | `Record` drop counters / timestamps | No | Optional |
| 22 | `StatisticsIndex` / `statistics_index` | `top/<window>/<kind>/<count>/<dimension>` (`telemetry/windows.rs:218`) | `String` dimension id | No | Optional |
| 23 | `OutputStyles` / `output_styles` | `singleton` | `Record`: validated style selections, `revision`, `overridden` (`infra/db/settings_records/output_styles.rs`) | No | Yes (revision bumped on import) |
| 24 | `OperationalSettings` / `operational_settings` | `singleton` | `Record` settings + `revision` + `overridden` | No | Yes |
| 25 | `GatewayResourceLimits` / `gateway_resource_limits` | `singleton` | `Record` bounded limits | No | Yes |
| 26 | `Indexes` / `indexes` | `provider/...` legacy prefix (`admin/providers/deletion.rs:51`) | `String` provider id | No | Yes |
| 27 | `ApiKeyIndex` / `api_key_index` | `created/<created_at>/<id>` (`admin/api_keys.rs`) | `String` key id | No | Yes (rebuilt) |
| 28 | `ApiKeyTokenIndex` / `api_key_token_index` | `token/<hex(sha256(token))>` (`admin/api_keys.rs`) | `String` key id | Hash only | Yes (rebuilt) |
| 29 | `ProviderNameIndex` / `provider_name_index` | hashed name prefix + name | `String` provider id | No | Yes (rebuilt) |
| 30 | `ProviderApiKeyIndex` / `provider_api_key_index` | `provider/{sha256(pid)}/{available or all}/`, `.../oauth-identity/{sha256(identity)}/` (`infra/db/keys.rs:88-138`) | `String` credential id | No | Yes (rebuilt) |
| 31 | `ProviderApiKeyAvailabilityIndex` / `provider_api_key_availability_index` | `provider/{sha256(pid)}/available/<key_id>` | `String` credential id | No | Yes (rebuilt) |
| 32 | `ProviderApiKeyCreatedIndex` / `provider_api_key_created_index` | `provider/{sha256(pid)}/created/<position>` (`infra/db/keys.rs:140-163`) | `String` credential id | No | Yes (rebuilt) |
| 33 | `ProviderModelIndex` / `provider_model_index` | `provider/{sha256(pid)}/model-order/{lowercase model}+{model}` (`infra/db/keys.rs:63-76`) | `String` model name | No | Yes (rebuilt) |
| 34 | `RouteNameIndex` / `route_name_index` | `name/<name>/<id>` (`admin/combos/storage.rs`) | `String` route id | No | Yes (rebuilt) |
| 35 | `RouteTargetProviderIndex` / `route_target_provider_index` | `provider/{pid}/{target key}` | `String` target key / route id | No | Yes (rebuilt) |
| 36 | `RouteTargetComboIndex` / `route_target_combo_index` | `combo/{combo_id}/{target key}` | `String` route id | No | Yes (rebuilt) |
| 37 | `Integrations` / `integrations` | Integration key | `Record` | No | Yes |
| 38 | `Proxies` / `proxies` | Proxy id | `Record` proxy config | No | Yes |
| 39 | `ProxyEndpoints` / `proxy_endpoints` | `{proxy_id}/{endpoint_id}` | `Record`: `url`, `username`, **`secret` = encrypted credential**, `label` (`admin/proxies/storage.rs:66-88`) | **Yes (field)** | Yes |
| 40 | `ProxyNameIndex` / `proxy_name_index` | name + id composite (`admin/proxies/mod.rs`) | `String` proxy id | No | Yes (rebuilt) |
| 41 | `ProxyEndpointRefIndex` / `proxy_endpoint_ref_index` | `ref_proxy_id` + endpoint key | `String` proxy id | No | Yes (rebuilt) |
| 42 | `ProxyProviderIndex` / `proxy_provider_index` | provider id + endpoint key | `String` | No | Yes (rebuilt) |

Snapshot exclusion rules: `excluded_from_snapshot` (`src/infra/storage/snapshot.rs:12-33`) always
excludes session/stream tables (rows 10–15); high-frequency telemetry rows (5, 16–22) are excluded
unless the operator requests `include_request_logs`.

---

## 2. Key Design & Value Serialization Standards

### 2.1 Key validation (MUST, already implemented)

Every key and prefix passes through `validate_key` / `validate_prefix`
(`src/infra/storage/keys.rs`) inside `ReadTxn` and `WriteTxn` methods. Keys MUST be non-empty for
point operations, at most `MAX_KEY_BYTES = 500` bytes, and contain no NUL. Prefixes MAY be empty
(full-table scan) but MUST be deliberate and bounded (Section 3.4). **MUST NOT** build keys by raw
`format!` with unvalidated user input; use the domain key builders in `src/infra/db/keys.rs`,
`src/admin/combos/storage.rs`, `src/admin/api_keys.rs`, `src/admin/sessions/records.rs`, and
`src/infra/telemetry/windows.rs`.

### 2.2 Numeric ordering: fixed-width decimal (CURRENT), big-endian (REQUIRED for new binary keys)

LMDB orders keys as raw bytes, so numeric components must be encoded so byte order equals numeric
order. ExoRoute's existing convention is **fixed-width zero-padded decimal**, e.g. `minute_key` uses
`format!("{minute:020}/{dimension_kind:02}/{dimension_id}")`
(`src/infra/telemetry/windows.rs:43-51`). `reverse_sort_component`
(`src/infra/db/keys.rs:177-185`) maps each byte `b -> 255 - b` in hex and appends `"ff"` for a
descending sort key.

- **MUST** keep fixed-width zero-padded decimal for all existing `UsageMinutes`, `RequestLogIndex`,
  and API-key/combo ordering keys. Changing an existing encoding silently breaks range scans and
  requires a format bump plus migration.
- **REQUIRED (not yet implemented):** if a new key needs a raw binary integer component, encode it
  as **big-endian** (`u64::to_be_bytes`; bias signed values with `value as u64 ^ (1 << 63)` first)
  so `memcmp` order equals numeric order. Never use native-endian bytes or variable-width decimal
  for new sortable keys.
- **MUST NOT** store a signed integer's native bytes directly as a sortable key.

### 2.3 Composite key and prefix conventions (MUST)

- A prefix ends with `/`, followed by ordered components: `namespace/{component}/{component}`.
- Hash long/untrusted identifiers before embedding: `hash_component` uses `sha2::Sha256` hex
  (`src/infra/db/keys.rs:78-86`). This bounds key length and avoids delimiter injection.
- Keep a trailing unique component (usually the record id) so keys are unique even when sort
  components collide (two keys sharing `created_at`: `provider_api_key_order_key` adds `position`).
- Secondary indexes store the **primary key or id**, never a copy of the record. Examples:
  `ApiKeyTokenIndex -> key id`, `ProviderApiKeyAvailabilityIndex -> credential id`.
- Every secondary-index write MUST occur in the **same `RwTxn`** as the primary record write; see
  `store_provider_api_key` (`src/admin/providers/provider_keys.rs`) and `create_api_key`
  (`src/admin/api_keys.rs:350-358`). Import rebuild is centralized in
  `rebuild_backup_secondary_indexes` (`src/admin/database.rs:658-730`).

### 2.4 Value serialization and safe decoding (MUST)

- All values are bincode via `encode_record` / `decode_record`
  (`src/infra/storage/codec.rs:50-73`). `decode_stored_record` decompresses **only**
  `Table::RequestLogs` values beginning with `COMPRESSED_RECORD_PREFIX = 0x5A`, using the runtime
  dictionary loaded by `load_request_log_dictionary`. Backup decoding uses
  `decode_request_log_bytes` with the snapshot's dictionary (`src/infra/storage/codec.rs`,
  `src/infra/storage/snapshot.rs:105-125`).
- **MUST** use typed `Record` accessors (`text`, `integer`, `boolean`, `bytes`, and `optional_*`,
  `src/infra/storage/records.rs:265-348`). They return `StorageError::Invalid` on missing/wrong-typed
  fields. **MUST NOT** `unwrap`, `expect`, or `unwrap_or_default` a stored field to hide corruption —
  fail safely and retain a non-secret diagnostic.
- **MUST** validate semantic bounds after decoding: non-negative counters, bounded enum ids, finite
  sizes (see `api_key_request_count` `src/admin/api_keys.rs:95-103`,
  `output_styles_from_record` `src/infra/db/settings_records/output_styles.rs`). Auth and settings
  records MUST fail closed.
- Compression is an optimization: decompression failures are `StorageError::Codec`, never silent
  empty records. `MAX_COMPRESSED_EXPANSION = 17 * 1024 * 1024` bounds allocation
  (`src/infra/storage/codec.rs:44`).
- Legacy raw-bincode request-log rows remain readable; do not rewrite them in place without a
  deliberate migration.

### 2.5 Schema/version envelope rules

- Environment version: `Meta/storage_format_version` (Section 1.6).
- Archive version: `BACKUP_FORMAT_VERSION = 1`, magic `EXOROUTE-LMDB-BACKUP\0`, SHA-256 checksum
  (`src/admin/database.rs:39-42`, `src/admin/database/backup_format.rs`).
- Record evolution: add optional `Field`s older readers ignore; do not reinterpret an existing
  field's type. Example: v1→v2 provider records omit `local_rpm_target`, readers supply a default
  (`environment.rs:335-341`, `admin/database.rs:normalize_provider_defaults`).
- **REQUIRED (not yet implemented):** give every new secret blob a version/algorithm tag (Section 5.2).

---

## 3. Safe Read Operations (`RoTxn`)

### 3.1 Lifecycle and borrow rules (MUST)

`Database::read` (`src/infra/storage/environment.rs:161-190`) acquires an operation slot, takes a
read guard on `resize_gate`, moves the closure into `spawn_blocking`, opens `env.read_txn()`,
constructs `ReadTxn { txn: &txn, tables: &inner.tables }`, and calls the closure. Transaction and
guard are dropped when the closure returns. The HRTB signature
`F: for<'view, 'env> FnOnce(&ReadTxn<'view, 'env>) -> Result<R, StorageError> + Send + 'static`
with `R: Send + 'static` is intentional.

- **MUST** return owned data (`String`, `Record`, `Vec<_>`) from the closure. `R: 'static` makes it
  impossible to return a `&[u8]` borrowed from the transaction.
- **MUST NOT** stash a `ReadTxn`, `RoTxn`, or borrowed `&[u8]` beyond the closure. Copy with
  `to_owned()` as `read_txn.rs:35-42` does.
- **MUST NOT** perform blocking I/O, network calls, or `.await` in a read closure; it runs on a
  `spawn_blocking` thread and the signature forbids async. Decode inside; HTTP/JSON work outside.
- Reads use `read_txn_without_tls`, so the transaction is not thread-bound; still, never share it.

Canonical pattern (verified in `infra/db/output_styles_store.rs:12-18`):

```rust
let record = database
    .read(|txn| txn.get::<Record>(Table::OutputStyles, "singleton"))
    .await?
    .ok_or_else(|| StorageError::Invalid("output styles record is missing".to_owned()))?;
let snapshot = output_styles_from_record(&record)?; // decode AFTER the txn closes
```

### 3.2 Point lookups vs cursors (MUST)

- Point reads: `get::<T>(table, key)` validates the key, reads bytes, and decodes with
  `decode_stored_record` (`read_txn.rs:23-35`).
- Prefix scans: `scan_prefix`, `scan_prefix_after` (pagination), `scan_prefix_reverse_after`
  (descending time feeds) — `read_txn.rs:37-131`. All enforce nonzero limit and validate prefix/cursor.
- `scan_prefix_after` opens `(Bound::Excluded(cursor), Bound::Unbounded)` and **breaks as soon as the
  key no longer starts with the prefix** (`read_txn.rs:57-70`); this is the correct bounded pagination
  pattern. `scan_prefix_reverse_after` uses `rev_prefix_iter` and skips keys `>= cursor`.
- **MUST NOT** add a raw cursor API that can iterate without a limit; add bounded helpers to
  `ReadTxn` instead.
- **MUST** pass an explicit, justified limit. Reuse established constants: `MAX_HISTORY_SCAN_ROWS =
  100_000` (`telemetry/history.rs`), `MAX_API_KEY_SEARCH_SCAN = 100_000` (`admin/api_keys.rs:8`),
  `MAX_PUBLIC_MODEL_RECORDS` (`gateway/mod.rs:56`),
  `REQUEST_LOG_INDEX_REPAIR_LIMIT = 100_000` (`infra/db/request_logs.rs:19`).

### 3.3 Snapshot consistency (CURRENT / MUST preserve)

A `RoTxn` is a stable MVCC snapshot: every read inside one `read` closure sees the same committed
state. Multi-read workflows that must be consistent (index pointer + primary row) MUST use one
`Database::read` closure — `gateway/auth.rs:38-55` is the reference: token index lookup, primary
`ApiKeys` read, enabled/hash check, and scope decode all in one transaction. **MUST NOT** split such
checks into two `read` calls (TOCTOU).

Snapshot export is one `RoTxn` over all `Table::ALL` entries (`snapshot_entries`,
`read_txn.rs:133-181`), so a backup is internally consistent.

### 3.4 Bounded pagination and full-scan discipline (MUST)

- Request-path reads of growable tables MUST be prefix-bounded or bounded by a constant and MUST
  return `StorageError::Busy` on overflow rather than truncating silently. Compliant examples:
  `admin/api_keys.rs:181-246` (100k scan cap, 256-row pages), `admin/request_logs.rs:76-99`
  (reverse cursor, 100k cap), `admin/overview.rs:17-47` (10k combos),
  `infra/db/bootstrap.rs:105-132` (1000 stream runs).
- `limit == 0` returns an empty `Vec` without opening a cursor (`read_txn.rs:44-46`).
- **MUST NOT** call `scan_prefix` with `usize::MAX` or an empty prefix on a request path without a
  hard downstream cap. Section 7 lists violations found in this snapshot.
- Telemetry/history pages MUST report covered rows when scans are truncated; `telemetry/history.rs`
  already surfaces this next to rollup totals.

---

## 4. Safe Write Operations (`RwTxn`) & Concurrency

### 4.1 Single writer and transaction scope (CURRENT / MUST preserve)

`Database::write` (`environment.rs:229-269`) serializes writers with `writer: Arc<Mutex<()>>`,
takes an operation slot, holds the resize read guard, opens one `RwTxn`, runs the closure, and calls
`txn.commit()` **only if the closure returns `Ok`**. An `Err` drops the transaction, rolling back
every prior write in that closure. **MUST** rely on this for all-or-nothing semantics: put all related
mutations and their validation inside one closure.

`WriteTxn` exposes `get`, `scan_prefix*`, `put`, `put_if_absent`, `put_raw`, `delete`,
`delete_prefix`, `clear` (`write_txn.rs`). `put_raw` is for restore/tests; domain code MUST use
`put` so values are encoded consistently.

### 4.2 Mandatory atomic read-modify-write (MUST)

Every counter increment, state transition, and credential/index update MUST read and write the record
inside the **same `RwTxn`**. Verified correct patterns:

- `save_output_styles` (`infra/db/output_styles_store.rs:37-63`): read current record, compare
  `expected_revision` **inside** the transaction, `checked_add(1)`, put, return `Some(snapshot)` or
  `None` on conflict; handler maps `None` to HTTP 409 (`admin/settings/output_styles.rs:31-41`).
- `save_operational_settings` (`infra/db/operational_settings.rs:38-57`) and
  `rotate_refresh_token_at` (`admin/sessions/lifecycle.rs:147-215`): same in-transaction CAS /
  replay detection.
- `persist_batch` (`infra/telemetry/store.rs:61-163`): all API-key counts, request-log index rows,
  aggregate windows, and provider meters in **one** write transaction; it also checks that a
  credential still exists before creating a meter.
- `mark_provider_key_used` / `mark_provider_key_invalid`
  (`gateway/execution/provider.rs:48-170`), `delete_provider_key`
  (`admin/providers/keys.rs:440-489`): primary record + secondary indexes + provider counters in one
  transaction.
- Stream continuity `append_event_with_settings` / `finish_run`
  (`gateway/continuity/records.rs:141-224`): sequence number and run record updated atomically.

**MUST NOT** read in `Database::read`, mutate in Rust, then `put` in a later `Database::write` when
another writer could change the record. If a value is needed for a network call (OAuth refresh), use a
per-credential lock, then **re-read inside the write transaction** and apply only intended field
deltas — never write back the whole stale record. Section 7 lists sites needing this treatment.

Counter rules (MUST): `checked_add` with `StorageError::Invalid` on overflow for authoritative
counters (`admin/providers/keys.rs:410-416`); `saturating_add` only for best-effort telemetry.
Generation counters (`Meta/statistics_generation`) MUST be read and compared in the same transaction
as the value they guard (`telemetry/store.rs:81-120`) and bumped atomically with history clearing
(`telemetry/maintenance.rs:11-34`).

### 4.3 Async boundary isolation (MUST)

- LMDB I/O MUST stay inside `spawn_blocking`; the facade already does this for every `read`/`write`.
  Handlers MUST `.await` the facade, never call a blocking helper on a Tokio worker.
- Archive encoding/decoding and backup file I/O are wrapped in `spawn_blocking`
  (`admin/database/export.rs:83-97`, `admin/database/restore.rs:99-121`) — keep it that way.
- `MAX_DATABASE_OPERATIONS = 64` bounds concurrent `spawn_blocking` jobs; `try_acquire_owned`
  returns `StorageError::Busy` rather than unbounded queuing. Handlers MUST map `Busy` to a retryable
  response (503) and MUST NOT retry a write after ambiguous cancellation — use GET to verify
  (`admin/settings/output_styles.rs:24-41` documents the client contract).
- **MUST NOT** hold a `std::sync::Mutex`, `tokio::sync::Mutex`, or semaphore permit across
  `.await` unless serialization is required for an invariant and its contention/cancellation
  behavior is documented. Existing compliant uses: `admin_auth_lock`, `output_styles_update_lock`,
  `admin_database_{import,export}_in_flight`.

### 4.4 Batching high-frequency telemetry (MUST)

Telemetry uses a bounded channel (`TELEMETRY_QUEUE_CAPACITY = 8_192`), 128-message batches, and a
100 ms flush interval (`infra/telemetry/mod.rs:41-43`). `persist_batch` coalesces per-key deltas in
memory and commits in one `RwTxn`. **MUST** keep request-path telemetry enqueue-only; **MUST NOT**
open an `RwTxn` per log event. The writer reports drops via `DropCounters` rather than an unbounded
queue. Retention/pruning runs in background workers (`app/workers.rs`) in bounded batches
(`REQUEST_LOG_PRUNE_BATCH_SIZE = 1_000`, `MAX_EXPIRY_ROWS_PER_RANGE = 512`,
`MAX_PROVIDER_METER_EXPIRY_ROWS = 2_048`).

### 4.5 Lock-order deadlock prevention (MUST)

Effective lock order:

1. operation semaphore permit (`try_acquire_owned`);
2. application guard (`admin_auth_lock`, `*_update_lock`, import/export permits);
3. `writer` mutex (writes) or `resize_gate` read guard (all transactions);
4. LMDB transaction;
5. `resize_gate` write guard (growth, after all transactions ended).

**MUST** acquire in this order; **MUST NOT** call `Database::read`, `Database::write`, or
`grow_map` while already holding `resize_gate` or `writer`. `write` releases `writer` before
`grow_map` (the guard is returned from `spawn_blocking` and dropped at `environment.rs:265-269`) —
moving growth inside the mutex would deadlock against a queued reader's resize read guard.

**MUST NOT** hold an async mutex across a `Database::write` await when an inverse-order path exists.
Restore takes `gateway_resource_limits_update_lock`, then `operational_settings_update_lock`, then
`output_styles_update_lock` (`admin/database/restore.rs:78-84`); new code taking these locks MUST
follow the same order.

### 4.6 Publication after commit (MUST)

Runtime snapshots (settings, output styles, provider caches) MUST be published only after the LMDB
write commits. `restore_application_data_inner` (`admin/database/restore.rs:171-190`) commits with
`replace_snapshot_entries` then calls `state.apply_*`. A lost HTTP response MUST NOT trigger an
automatic PUT retry; the client verifies with GET.

---

## 5. Security, Encryption & Export/Import Invariants

### 5.1 Current encryption boundary (CURRENT)

`security::encrypt_secret` / `decrypt_secret` (`src/security/credential_secrets.rs:8-45`) are the
**only** AEAD wrappers:

```rust
// CURRENT shape (src/security/credential_secrets.rs)
let cipher = ChaCha20Poly1305::new(key.into());          // key: Option<&[u8; 32]>
let mut nonce = [0u8; 12];
OsRng.fill_bytes(&mut nonce);
let ciphertext = cipher.encrypt(Nonce::from_slice(&nonce), value.as_bytes())?;
let mut stored = nonce.to_vec();
stored.extend_from_slice(&ciphertext);                   // blob = nonce || ct+tag
```

- Master key: stable 32-byte `EXOROUTE_MASTER_KEY`, generated with `OsRng` and stored base64 in
  `~/.exoroute/.env` by `config::ensure_master_key` (`src/config/env_secrets.rs:74-87`). It is
  **never** written to LMDB or backups.
- Nonces are fresh 96-bit `OsRng` values per encrypt; decrypt reads the embedded nonce. **No
  nonce-reuse issue was found.**
- Verified encrypted fields: `ProviderApiKeys.secret`, `Providers.secret`,
  `Providers.custom_headers`, `ProxyEndpoints.secret` (Section 1.7). Gateway tokens, refresh tokens,
  and the admin password are stored only as SHA-256 digests / Argon2id hashes, never plaintext. No
  plaintext secret storage was found.
- **MUST** keep secrets in `Field::Bytes` under dedicated fields. **MUST NOT** add a secret to a
  `Text` field, log record, tracing event, error message, or HTTP response. Current serializers
  already omit secrets (`admin/api_keys.rs:69-93`, `admin/providers/keys.rs:29-45`,
  `admin/providers/provider_view.rs`, `admin/proxies/check.rs`).

### 5.2 AAD binding and blob versioning (REQUIRED — not yet implemented)

The wrapper passes no AAD: `cipher.encrypt(nonce, value.as_bytes())` and
`cipher.decrypt(nonce, &stored[12..])` (`credential_secrets.rs:19-21,39-41`). A writer with LMDB
access — or a crafted backup — can copy a blob from one record/field to another and it still
decrypts (**ciphertext relocation / context swap**). No algorithm/version tag exists, so import
cannot distinguish a valid blob from a truncated one until first use.

**MUST** implement before adding any new secret field:

```rust
// REQUIRED pattern — aead::Payload binds ciphertext to its exact location.
// use chacha20poly1305::aead::{Aead, Payload};
let aad = format!("exoroute/v2/{table_name}/{key}/{field}");
let payload = Payload { msg: plaintext.as_bytes(), aad: aad.as_bytes() };
let ciphertext = cipher.encrypt(Nonce::from_slice(&nonce), payload)?;

// Blob layout: [MAGIC(2) | VERSION(1) | ALG_ID(1) | NONCE(12) | CIPHERTEXT+TAG]
```

- AAD MUST include table name, full record key, field name, and format version; it MUST match at
  encrypt and decrypt. A blob moved to another key/field MUST fail authentication.
- Decrypt MUST branch on version/algorithm tag and reject unknown tags. Keep legacy
  `nonce` concatenated with ciphertext readers behind an explicit "v1" tag; never a silent fallback.
- Use a distinct key or AAD domain per secret class (provider key vs OAuth account vs proxy
  credential). Do not reuse the raw master key across contexts without domain separation.
- Import MUST authenticate every encrypted field with master key + record-derived AAD and reject
  tampered/relocated blobs before replacing live data (Section 5.4).

### 5.3 Key handling and memory hygiene (MUST / REQUIRED)

- **MUST** keep the master key stable per installation and across backups; exports never contain it
  (no master-key reference in `admin/database.rs`; errors direct operators to `EXOROUTE_MASTER_KEY`).
- **MUST NOT** log, return in an error, or commit plaintext keys, tokens, passwords, nonces, or
  `.env` contents. Existing decrypt errors are sanitized.
- **REQUIRED (not yet implemented):** zeroize plaintext. `decrypt_secret` returns an owned
  `String` (`credential_secrets.rs:42-44`); no `zeroize` dependency exists in `Cargo.toml` and no
  buffer is scrubbed. Wrap plaintext in a zeroizing type at decrypt and at long-lived holders
  (`gateway/execution/credential_rotation.rs:70-88`, `gateway/execution/provider.rs:232`,
  `security/egress/client.rs:227-238`, `admin/model_probes.rs`, `admin/workspace/target.rs:272-298`).
  At minimum, overwrite input buffers on drop paths you control.

### 5.4 Export / import invariants (CURRENT + MUST)

The current workflow is strong on authorization and atomicity:

1. Both endpoints call `require_step_up` with `AdminStepUpScope::DatabaseExport` / `DatabaseImport`
   (`admin/database/export.rs:28-37`, `admin/database/import.rs:18-29`). The proof is issued by
   `reauthenticate` only after Argon2id password verification
   (`admin/sessions/handlers.rs:138-213`), is scope-bound, 300-second-lived, single-use, and kept in
   memory (`state`) — never in LMDB or logs.
2. Export reads one bounded snapshot (`MAX_DATABASE_IMPORT_BYTES = 512 MiB`), writes a magic +
   version + SHA-256 checksum archive in `spawn_blocking`, and streams with
   `Cache-Control: no-store` (`admin/database/export.rs:59-151`).
3. Import enforces 512 MiB upload cap, 15-minute overall / 60-second idle timeouts, private
   `0o600` temp file, checksum verification, fixed-width bincode **preflight** that rejects forged
   lengths before deserialization (`admin/database/preflight.rs`), version checks, full record/shape
   validation (`admin/database/validate.rs`), secondary-index rebuild
   (`admin/database.rs:658-730`), and **atomic** `replace_snapshot_entries` in one `RwTxn`
   (`write_txn.rs:186-202`). Runtime state is published after commit and admin sessions cleared
   (`admin/database/restore.rs:171-194`).

**MUST** preserve all of the above. **MUST NOT** send the raw password with backup data, store/log the
proof, include secrets in URLs, or return a partially restored database. **REQUIRED:** add
cryptographic authentication of encrypted fields on import (5.2) — current validation checks only
structural shape and byte-length ranges (`admin/database/validate.rs:189-196`,
`admin/proxies/backup.rs:172-178`).

### 5.5 Secret vs non-secret separation (SHOULD)

Secrets are co-located as `Field::Bytes` in `ProviderApiKeys`, `Providers`, and
`ProxyEndpoints`; session/stream tables are already strictly separated and excluded from backups.
**SHOULD** evolve toward dedicated secret tables with a single AEAD boundary, but any such change
MUST keep transaction atomicity with the owning primary record and preserve backup compatibility.
Until then, every path reading a secret MUST use the AEAD wrapper and every path writing one MUST
call `encrypt_secret` first (`admin/providers/keys.rs:359`, `admin/providers/custom_headers.rs:58-84`,
`admin/proxies/mod.rs:368-522`, OAuth stores under `src/provider_adapters/auth/`).

---

## 6. Error Handling, Recovery & Retention

### 6.1 Error taxonomy (CURRENT + REQUIRED)

`StorageError` (`src/infra/storage/records.rs:11-88`) has variants `Busy`, `MapFull`,
`Conflict`, `NotFound`, `Invalid(String)`, `Codec(String)`, `Task(String)`, `Backend(String)`,
`Io(std::io::Error)`. `From<heed::Error>` maps `MdbError::MapFull -> MapFull` and
`MdbError::NotFound -> NotFound`; everything else becomes `Backend(String)`.

| Condition | Required handling | Status |
|---|---|---|
| `MDB_MAP_FULL` | `StorageError::MapFull`; `Database::write` grows the map and retries the whole closure once the resize gate is free; if the hard limit is reached, return a sanitized error, leave data unchanged. | **CURRENT** |
| Operation budget / writer contention | `StorageError::Busy`; handlers return 503 and do not auto-retry writes. | **CURRENT** |
| `MDB_READERS_FULL` | **REQUIRED:** map to a dedicated retryable error (e.g. `StorageError::ReadersFull`), emit a metric, return 503. Today it collapses into `Backend(String)` via the catch-all at `records.rs:83`; add a test forcing the path. | **REQUIRED** |
| `MDB_CORRUPTED` / `MDB_INVALID` | **REQUIRED:** map to a distinct fatal error, refuse to serve corrupt tables, require operator restore. Do not fall back to defaults for auth/settings. Today they are `Backend(String)`. | **REQUIRED** |
| Deserialization failure | `StorageError::Codec` / `Invalid`; fail closed, keep stored bytes intact, redacted diagnostic. | **CURRENT** |
| Cancelled / panicked `spawn_blocking` task | `StorageError::Task`; log with context. | **CURRENT** |

**MUST** preserve useful, non-secret diagnostics; log key ids and error categories only.

### 6.2 Recovery and retention (MUST)

- Startup repair is bounded and idempotent: `repair_request_log_indexes`
  (`infra/db/request_logs.rs:30-130`) validates `request_log_count` against scanned rows and
  rebuilds both request-log indexes in one `RwTxn` when marker/counts are stale; fails `Invalid`
  above `REQUEST_LOG_INDEX_REPAIR_LIMIT`.
- `recover_abandoned_stream_runs` (`infra/db/bootstrap.rs:105-132`) fails runs left `running` after
  a restart and deletes runs older than 24 hours with their `StreamEvents` prefix, in one bounded
  transaction (1000 runs).
- Retention workers run in bounded batches and yield between batches (`app/workers.rs`); statistics
  expiry uses cursors in `UsageExpiryCursor` and deletes at most `MAX_EXPIRY_ROWS_PER_RANGE = 512`
  rows per range (`infra/telemetry/retention.rs`). **MUST NOT** replace with unbounded `clear()` on a
  request path. `clear_history_in_database` (`infra/telemetry/maintenance.rs:11-34`) is the one
  explicit full-clear path; it bumps `statistics_generation` in the same transaction so stale
  per-key counters are ignored.
- **MUST** keep prune/write transactions short: batch, yield to the runtime, never hold a writer
  transaction across a network call.

### 6.3 Backup and compaction (MUST)

- Use the admin export/import endpoints only (Section 5.4). Backups are logical snapshots of selected
  tables, not copies of `data.mdb`; copying mapped files while the process is running is unsupported.
- `EXOROUTE_MASTER_KEY` is not in exports; a backup is unreadable for secrets without the matching
  key. Preserve the key separately; treat a key change as a credential-reconnect event.
- Compaction/retention is logical: prune via bounded delete batches, then let LMDB reuse freed pages.
  No offline compaction utility exists in this codebase; do not introduce raw file-copy "compaction"
  scripts without a new reviewed procedure.
- After import, `load_request_log_dictionary` realigns the runtime dictionary with the imported
  snapshot (`admin/database/restore.rs:178-181`) — keep this synchronous before the next log read.

---

## 7. Audit Findings in Current Codebase (Anti-Patterns to Eliminate)

Snapshot `25b0f16`. Severity: **P1** = fix before production exposure; **P2** = significant
security/reliability debt; **P3** = bounded debt / needs measurement. No real secrets are included.

### 7.1 Confirmed findings

| ID | Severity | File / function | Category | Evidence & impact | Required fix pattern |
|---|---|---|---|---|---|
| A-01 | P2 | `src/security/credential_secrets.rs:19-21,39-41` (`encrypt_secret`, `decrypt_secret`) | Security — no AAD | AEAD calls pass no `aead::Payload`. A writer can swap ciphertext between records/fields and it still decrypts; no context separation between provider keys, OAuth blobs, and proxy credentials. | Section 5.2: bind table + key + field + version as AAD; version the blob; migrate v1 rows. Add a test that swapping two blobs fails decrypt. |
| A-02 | P2 | `src/security/credential_secrets.rs:22-23,34,40` | Security — unversioned blob | Blob is only `nonce` concatenated with ciphertext, no algorithm/version tag; `len() < 13` accepts lengths that cannot hold a 16-byte Poly1305 tag. Corruption is detected late at first decrypt; no algorithm agility. | Prefix magic + version + algorithm id; reject unknown tags; tighten minimum length to include tag; validate on import. |
| A-03 | P2 | `src/security/credential_secrets.rs:42-44`; holders `gateway/execution/credential_rotation.rs:70-88`, `gateway/execution/provider.rs:232`, `security/egress/client.rs:227-238`, `admin/workspace/target.rs:272-298` | Security — no zeroization | Plaintext secrets are ordinary `String` values retained in long-lived request objects; no `zeroize` dependency or scrubbing. Heap remnants can surface in core dumps/memory disclosure. | Wrap plaintext in zeroizing types at decrypt and holders; add `zeroize`; test clearing on controlled drop paths. |
| A-04 | P2 | `src/admin/database/validate.rs:189-196`, `src/admin/proxies/backup.rs:172-178` | Import security — structural-only secret validation | Backup validation checks provider secret length `<= 8192` and proxy secret length `28..=1024` only; `decrypt_secret` is never called during import. Tampered/truncated/relocated ciphertext imports cleanly and fails at first upstream call. | Authenticate every encrypted field with master key + record-derived AAD during `validate_backup_payload`; reject on auth failure before `replace_snapshot_entries`. |
| A-05 | P2 | `src/infra/storage/records.rs:80-84` (`From<heed::Error>`) | Error handling — exhausted taxonomy | `MDB_READERS_FULL`, `MDB_CORRUPTED`, `MDB_INVALID`, I/O errors collapse into `StorageError::Backend(String)`. Callers cannot distinguish transient reader exhaustion from permanent corruption; no metric/retry path. | Add typed `ReadersFull` / `Corrupted` variants; map in `From<heed::Error>`; 503 for transient, fail closed for corruption; tests forcing paths. |
| A-06 | P2 | `src/provider_adapters/auth/antigravity/account_store/save.rs:58-62,69-73,92-96` | Memory/scan bound — unbounded scans in a write txn | Three `scan_prefix::<String>(..., usize::MAX)` calls inside the account-save `RwTxn` materialize the full credential list for a provider (unlimited keys are a product requirement). Many keys => long write txn and large allocations. | Use `scan_prefix_after` in bounded batches (`CLINE_ACCOUNT_MATCH_BATCH_SIZE` pattern in `provider_adapters/cline/account.rs:79-106`) or a dedicated counted identity index. Add a many-keys test/benchmark. |
| A-07 | P2 | `src/admin/auth.rs:47-61` → `:110-141` (`admin_login`) | Concurrency — cross-transaction RMW | Bootstrap hash read in one `RoTxn`, hashed with Argon2, written in a later `RwTxn`, then re-read. Write re-checks absence in-txn and `admin_auth_lock` serializes in-process, so **lock-mitigated**, but decision uses a stale snapshot; removing the lock reintroduces a race. | Move read→decide→put→re-read into one `RwTxn` or use `put_if_absent` with one canonical in-txn read; keep lock as defense in depth; add concurrency test. |
| A-08 | P3 | `src/admin/database/restore.rs:122-132` → `:171-177` | Concurrency — cross-transaction RMW | Output-styles revision read before validation and written via whole-snapshot replace without in-txn CAS. Serialized today by `output_styles_update_lock` (`restore.rs:83-84`), but bypasses normal CAS (`infra/db/output_styles_store.rs:37-63`). | Compute/verify revision inside the same `RwTxn` performing replacement; keep runtime lock; test racing restore against save. |
| A-09 | P3 | `src/infra/storage/dictionary.rs:146-151,213-217` (`train_and_store`) | Concurrency — check-then-write | Dictionary absence and row count checked in separate reads; later `db.write` uses plain `put` with no re-check, so concurrent training runs could overwrite each other's dictionary while runtime copy lags. | Re-check absence inside `RwTxn` and use `put_if_absent`; publish runtime dictionary only after commit. |
| A-10 | P3 | `src/gateway/continuity/records.rs:156-167` (`append_event_with_settings`) | Performance — full scan per event | Every stream chunk scans up to 1000 `StreamRuns` and folds `retained_bytes` across all retained runs for the global cap. Bounded and correct but O(retained runs) per chunk on the stream hot path. | Maintain authoritative global retained-bytes counter in `Meta`, updated atomically with append/delete; keep scan as startup repair only. Benchmark under concurrent streams first. |
| A-11 | P3 | `src/gateway/routes.rs:193-224` (`models`) | Performance — large bounded scan on public path | `GET /v1/models` materializes up to `MAX_PUBLIC_MODEL_RECORDS` rows from `Routes`, `Providers`, `ProviderModels`, conditionally `RouteTargets`, in one read txn. Fails closed with `Busy` above cap but can allocate ~400k records per request. | Add precomputed public-model index updated atomically on route/model writes, or paginate/cache. Benchmark with realistic catalog before optimizing. |
| A-12 | P3 | `src/config/env_secrets.rs:82-85` + `src/security/credential_secrets.rs:16` | Security — no KDF / domain separation | Raw 32-byte master key used directly for every ChaCha20-Poly1305 context, no KDF or per-purpose subkey. Combined with A-01, all ciphertext shares one key. | Derive per-purpose subkeys (e.g. HMAC-SHA256 with domain labels) or distinct AAD domains; keep stored key unchanged; add migration tests. |

### 7.2 Areas inspected with no material finding

Per the audit standard, these were checked and found compliant in this snapshot:

- **Async/blocking isolation:** all LMDB and archive I/O goes through `spawn_blocking`; no blocking
  database call on a Tokio worker found (`environment.rs`, `admin/database/export.rs:83`,
  `admin/database/restore.rs:99`).
- **Atomic counters and state machines:** API-key `request_count`, provider usage meters, key
  recovery/invalidation, refresh-token rotation, stream appends, and output-style/settings CAS are
  single-transaction read-modify-writes. No stale write-back found in those paths.
- **Map growth / marker protocol:** doubling growth, pre-resize marker, crash recovery on open, and
  resize gate are correctly implemented (`environment.rs:229-313`, `fs_guard.rs:83-181`).
- **Filesystem/process security:** private `0700` dir, files `0600`, recursive symlink/reparse
  checks, and `fs2` exclusive process lock present and correctly placed.
- **Backup format hardening:** magic, version, SHA-256 checksum, fixed-width preflight before
  deserialization, 512 MiB cap, bounded snapshot, single-use step-up proof, atomic restore with
  post-commit publication — all implemented.
- **Panic safety on stored data:** domain decode paths use `Record` accessors and typed errors; no
  `unwrap`/`expect` on persisted values found in production database callers.
- **Secret-at-rest coverage:** provider keys, OAuth blobs, custom headers, and proxy credentials are
  encrypted; gateway tokens, refresh tokens, and admin password are hashed; no plaintext secret
  field, log statement, or API response found. Nonces are unique per write.

---

## 8. Developer & Reviewer Checklist

Every answer MUST be **Yes** (or **N/A with justification**) before merging a PR touching LMDB,
migrations, or export/import.

### Design & transaction boundaries

- [ ] All LMDB access goes through `Database::read` / `Database::write` and `ReadTxn` / `WriteTxn`; no new `heed` handle or direct transaction.
- [ ] Every read-modify-write (counter, rotation, state transition, credential + index update) is in **one** `RwTxn`.
- [ ] All secondary-index writes are in the same transaction as their primary record; import rebuilds indexes from primaries.
- [ ] If a value is needed for a network call, the write transaction re-reads the current record and applies deltas; never writes a stale cached record.
- [ ] Revision/CAS checks happen **inside** the write transaction, with overflow handled via `checked_add`.
- [ ] No lock acquired in a conflicting order (Section 4.5); no database call while holding `resize_gate` or `writer`.
- [ ] Runtime snapshots published only after commit; no automatic write retry after ambiguous response.

### Keys, values, and decoding

- [ ] All keys/prefixes use existing builders or pass `validate_key`/`validate_prefix`; no NUL, length <= 500.
- [ ] New sortable numeric components use fixed-width zero-padded decimal (existing style) or big-endian binary for new binary keys — never native-endian or variable-width decimal.
- [ ] Composite prefixes end with `/`, include hashed identifiers where needed, and carry a unique trailing id.
- [ ] Values are bincode via `encode_record`/`decode_record`; only `RequestLogs` uses the `0x5A` zstd frame path.
- [ ] All stored fields use typed `Record` accessors and semantic validation; malformed/legacy data fails closed with a redacted error, no `unwrap`/`expect`.
- [ ] Format changes are additive (no bump) or bump `STORAGE_FORMAT_VERSION`, add an upgrade path in `bootstrap_storage_version`, and update backup validation.

### Reads & bounds

- [ ] Point reads use `get`; scans use bounded `scan_prefix`/`scan_prefix_after`/`scan_prefix_reverse_after` with explicit, justified limits.
- [ ] No `usize::MAX` or unbounded full-table scan; any empty-prefix scan has a hard cap returning `StorageError::Busy` on overflow.
- [ ] Multi-read consistency (index pointer + primary row, auth hash + scope) uses a single `read` closure.
- [ ] Read closures return owned data; no borrowed `&[u8]`/`RoTxn` escapes the transaction.
- [ ] Pagination is stable (cursor), bounded, and reports truncated coverage where applicable.

### Async, concurrency, and durability

- [ ] No blocking LMDB or archive I/O on a Tokio worker; new blocking helpers wrapped in `spawn_blocking`.
- [ ] New high-frequency writes are batched; no per-event `RwTxn` on a request path.
- [ ] `Busy`, `MapFull`, `Conflict` paths handled explicitly with non-secret retryable errors.
- [ ] Any new `MDB_READERS_FULL` / corruption handling maps to a typed `StorageError` (A-05) with a test.
- [ ] Retention/pruning uses bounded batches, yields between batches, keeps write transactions short.
- [ ] Map-size marker protocol and resize gate untouched; resize never attempted with a live transaction.
- [ ] Startup repair/recover paths remain bounded and idempotent.

### Security & export/import

- [ ] Every new secret field is encrypted with `encrypt_secret` before `put` and stored in `Field::Bytes` under a dedicated field.
- [ ] New AEAD usage supplies record-derived AAD (table + key + field + version) and a version/algorithm tag; legacy blobs readable only via explicit v1 path (A-01/A-02).
- [ ] Decrypted plaintext wrapped in a zeroizing type, never logged, returned in an error, or serialized to an API response.
- [ ] Secrets excluded from logs/telemetry/metrics; error messages sanitized.
- [ ] Export/import still require fresh, scope-bound, single-use step-up proof from password reauthentication; no raw password travels with backup data.
- [ ] Imports enforce 512 MiB cap, checksum, version/preflight validation, **cryptographic validation of encrypted fields**, secondary-index rebuild, atomic replace, and post-commit runtime publication + session clearing.
- [ ] Backups contain no master key, session families, refresh tokens, or stream recovery tables.
- [ ] File permissions (`0700` dir, `0600` files) and symlink/reparse checks, including process lock, remain intact.

### Verification

- [ ] Tests cover success, invalid input, corrupt/legacy record, `Busy`/`MapFull`/`Conflict`, cancellation, and concurrent writers for the changed path.
- [ ] Relevant checks run and reported: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test --workspace --all-features`, `node scripts/check_source_boundaries.mjs`.
- [ ] No live user database, `.env`, or real credential used as fixture or included in output.
- [ ] Performance claims (if any) backed by a representative benchmark with configuration and measured limits; inspection-only hypotheses labeled as such.

---

### Appendix A — Quick reference: safe skeleton

```rust
// READ: consistent snapshot, owned result, bounded scan
let rows = db.read(move |txn| {
    txn.scan_prefix_after::<Record>(
        Table::RequestLogs,
        REQUEST_LOG_INDEX_PREFIX,
        cursor.as_deref(),
        PAGE_LIMIT,
    )
})
.await?;

// WRITE: validate + mutate + indexes in ONE transaction, CAS inside
let result = db.write(move |txn| {
    let mut current = txn.get::<Record>(Table::ApiKeys, &id)?
        .ok_or(StorageError::NotFound)?;
    if current.integer("revision")? != expected_revision { return Ok(None); }
    let next = expected_revision.checked_add(1)
        .ok_or_else(|| StorageError::Invalid("revision exhausted".into()))?;
    current.insert("revision", Field::I64(next));
    txn.put(Table::ApiKeys, &id, &current)?;
    txn.put(Table::ApiKeyIndex, &index_key, &id)?; // atomic with primary
    Ok(Some(next))
})
.await?; // commit only on Ok; publish runtime state afterwards
```
