# Design records

This directory is version-controlled on purpose. The rest of `docs/` holds
vendored copies of other projects and stays local (see the `docs/*` rule in
`.gitignore`), so nothing under it survives a fresh clone.

A design record is the *reasoning* behind an integration: what was verified, what
was inferred, which reference was trusted where the references disagreed, and how
to close what is still open. Losing that turns a solved investigation into an
open one, and lets a decision that had a documented reason be silently reversed.

## Records

| Record | Covers |
|---|---|
| [`muse-integration-report.md`](muse-integration-report.md) | The Meta Muse (`muse-code`) provider integration: reference evidence with line citations, the design decisions, and the reasons `muse-spark-web` was not built. |

## Reference revisions

The two gateway projects the Muse integration was researched from are vendored
under `docs/` and are **not** version-controlled here. They are checked out
repositories, so record the revision any conclusion came from:

| Reference | Revision | Version |
|---|---|---|
| `docs/9router` | `a99cf57239ff778b61e434c2786009d5ed1c412c` (`master`) | 0.5.95 |
| `docs/OmniRoute` | `2f42a9ac19d1a247ec9ce5473b790843724b3061` (`release/v3.8.51`) | 3.8.51 |

A conclusion drawn from one of these is only as current as the revision above.
When re-checking a claim, confirm the vendored copy is still at that revision
before trusting the surrounding code.

## Outstanding verification: Muse

The Muse adapter is implemented and covered by tests, but Meta publishes no
third-party contract for it, so parts of the request shape had to be inferred
from the two references above. Those are the parts a local test cannot settle.

| Item | State |
|---|---|
| Device authorization and token polling (RFC 8628) | Verified locally; endpoints and states come from the public OIDC host. |
| Mint request: `POST https://api.meta.ai/muse-code/key` | **Verified locally** — method, path, bearer token, `x-api-version: 1.0.0`, and user agent are asserted against a scripted upstream in `src/provider_adapters/muse/tests.rs`. |
| Mint request **body** | **Inferred, not verified against the live service.** The references disagree: 9Router sends `{"onboard": true}`, OmniRoute sends `{"dca_token": …}`. This gateway sends the `onboard` form, pinned by `mint_request_body()` and its test. If minting fails against the real service, this is the first suspect. |
| `x-api-version: 1.0.0` header | **Inferred.** Always sent. 9Router sends it, OmniRoute does not. Harmless if unwanted; if inference returns a 4xx, try removing it. |
| User agent (`muse-code/1.0.2`, `muse-build/1.3.0 …`) | **Inferred.** One reference states the Muse CLI user agent is required by Meta. |
| OAuth `client_id` = `1031625952748946` | **Inferred, and it can rotate.** Override with `EXOROUTE_MUSE_CLIENT_ID`. |
| OAuth scope / PKCE | Neither reference sends either; treated as unsupported by Meta. **Unverified.** |
| `GET /v1/models` under the inference user agent | **Inferred.** Falls back to the preset's static model list when discovery fails. |
| `muse-code` subscription quota | Not modelled by either reference; the preset reports `usage_limits: false`. |
| `muse-spark-web` | Deliberately not built. The reasoning and the ordered prerequisite list are in §14 of the report. |

Closing the "inferred" rows needs one manual run with a real Meta account. A unit
test cannot do it, and a green suite must not be read as proof that the upstream
contract is right.

## Decisions taken against that uncertainty

Two behaviours exist only because the contract above cannot be confirmed here.
Both are deliberate, and changing either should start by reading this section.

**The remint budget is a rolling window, not a lifetime cap.** A rejected
inference key is repaired in place rather than invalidated, because the durable
device token outlives the key. Repair means minting, and minting is upstream
traffic a client can trigger, so it is bounded: one mint per cooldown, and at
most `REMINT_ATTEMPT_LIMIT` mints inside `REMINT_BUDGET_WINDOW_SECONDS`
(`src/provider_adapters/muse/mod.rs`). All mint paths — a request with no key,
catalog discovery, a model probe, and rejected-key repair — funnel through the
single throttle in `remint_and_store`, so none of them can escape it.

The window matters as much as the limit. A lifetime cap would let one bad
upstream day permanently disable an account that works fine later, forcing an
operator to sign in again to clear a counter that has no security value; with a
window, a spent budget simply ages out. `remint_attempts` older than the window
stop counting and the next mint starts a fresh window, which the throttle test
asserts.

**The mint request shape is inferred, so its failure is explicit.** The body is
pinned by `mint_request_body()` and asserted against a scripted upstream, but
that only proves the gateway keeps sending what it decided to send. A mint that
comes back with any status other than 401/403 therefore reports that the shape is
inferred and should be checked against a working Muse client, because a wrong
body is the most likely cause and the one thing no local test can rule out.

Related, for the same reason: a Muse Spark request whose output budget is absent
or below `MUSE_SPARK_MIN_OUTPUT_TOKENS` is raised to that floor, since the model
spends the budget on hidden reasoning and can otherwise return HTTP 200 with an
empty message. Because that rewrites a field the caller sent, the change is
logged at debug rather than applied silently.
