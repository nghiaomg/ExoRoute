# ExoRoute

[![npm version](https://img.shields.io/npm/v/exoroute.svg)](https://www.npmjs.com/package/exoroute)
[![license](https://img.shields.io/npm/l/exoroute.svg)](https://github.com/nghiaomg/ExoRoute/blob/main/LICENSE)
[![node](https://img.shields.io/node/v/exoroute.svg)](https://www.npmjs.com/package/exoroute)

**A local-first AI protocol gateway and model router, installed as a native binary.**

ExoRoute gives your applications a single HTTP endpoint (`http://127.0.0.1:8686/v1` by
default) that translates between OpenAI Chat Completions, OpenAI Responses, and Anthropic
Messages while routing requests across the providers you configure. It is one
self-contained executable — no Docker, no Python runtime, no external database — using an
embedded LMDB environment and a dashboard compiled into the binary.

This npm package is a **launcher**. It does not contain the gateway itself: installing it
downloads the official release binary for your platform, verifies its checksum, and adds an
`exoroute` command to your PATH.

```sh
npm install -g exoroute
exoroute
```

Then open <http://127.0.0.1:8686> and sign in.

## Contents

- [Requirements](#requirements)
- [Supported platforms](#supported-platforms)
- [Install](#install)
- [First run](#first-run)
- [Commands](#commands)
- [Install or pin a specific version](#install-or-pin-a-specific-version)
- [What the installer does](#what-the-installer-does)
- [Verifying a release yourself](#verifying-a-release-yourself)
- [Upgrading](#upgrading)
- [Uninstalling](#uninstalling)
- [Troubleshooting](#troubleshooting)
- [Documentation](#documentation)
- [License](#license)

## Requirements

| Requirement | Detail |
| --- | --- |
| Node.js | 18 or newer. The package declares `"node": ">=18"`, and the installer needs the global `fetch` API. |
| Platform | A 64-bit target listed under [Supported platforms](#supported-platforms). 32-bit systems and Windows on ARM are not published. |
| Network | HTTPS access to `github.com` and its release asset hosts while installing or upgrading. |
| Permissions | Write access to the npm global prefix. No elevation beyond what a normal `npm install -g` already needs. |

The gateway itself does not need Node.js at runtime — the launcher only spawns a native
process. Node.js is still what runs the `exoroute` command, so keep it installed for normal
use; the binary itself is a standalone executable in the package's `vendor/` directory.

Uninstall the npm package rather than deleting Node.js: it is what removes the launcher and
the downloaded binary cleanly.

## Supported platforms

The installer maps your `process.platform` and `process.arch` to a published release asset:

| `process.platform` | `process.arch` | Downloaded asset |
| --- | --- | --- |
| `linux` | `x64` | `exoroute-linux-x86_64.tar.gz` |
| `linux` | `arm64` | `exoroute-linux-aarch64.tar.gz` |
| `darwin` | `x64` | `exoroute-darwin-x86_64.tar.gz` |
| `darwin` | `arm64` | `exoroute-darwin-aarch64.tar.gz` |
| `win32` | `x64` | `exoroute-windows-x86_64.zip` |

Linux binaries are built against glibc (`x86_64-unknown-linux-gnu` and
`aarch64-unknown-linux-gnu`), so musl-based distributions such as Alpine are not supported.

Any other combination fails during installation with
`ExoRoute does not publish a binary for <platform>/<arch>.`, and nothing is written to disk.

## Install

```sh
npm install -g exoroute
```

The package's `postinstall` script downloads and verifies the release, so a failed install
usually points at the network or the platform check rather than at the gateway. On success
the installer reports what it installed:

```
Installed ExoRoute v0.1.1 for win32/x64.
```

`npx exoroute` also works for a one-off run, but the binary is still fetched and cached per
package location rather than being rebuilt on each invocation.

## First run

```sh
exoroute
```

The launcher starts the native binary with your working directory, environment, and
arguments, and forwards its exit code.

On its first start, ExoRoute creates `~/.exoroute/` (`%USERPROFILE%\.exoroute` on Windows),
writes a starter `.env`, and serves the dashboard and API on `http://127.0.0.1:8686`.
Interactive terminals print the generated initial admin password once; otherwise the path
that holds it is printed instead. Change that password when the dashboard asks — gateway
requests stay disabled until the change is complete. The same directory receives
`EXOROUTE_MASTER_KEY`, which encrypts stored provider credentials, so keep the file private
and keep the key for as long as you need those credentials or your backups.

ExoRoute binds to loopback only, because its listener serves plain HTTP. For remote access,
keep the bind address on loopback and put a TLS reverse proxy in front of it.

Interactive documentation — SDKs, Cursor, Continue, Cline, and Roo Code setup — is served at
<http://127.0.0.1:8686/docs> while ExoRoute is running.

## Commands

The launcher forwards every argument to the native binary, which accepts:

```sh
exoroute            # same as: exoroute serve
exoroute serve      # start the gateway and dashboard
exoroute check      # validate configuration and exit
exoroute version    # print the version; --version and -V also work
```

Any other argument fails with an error carrying
`unknown command '<name>'. Use 'serve', 'check', or 'version'.`

## Install or pin a specific version

The launcher downloads the release tag matching its own package version
(`v<package version>`), so the npm version you install decides which binary you get:

```sh
npm install -g exoroute@0.1.1   # the v0.1.1 launcher and the v0.1.1 binary
npm install -g exoroute@latest  # the newest published npm version
```

Set `EXOROUTE_VERSION` to override the tag during installation, for example to install the
newest release independently of the launcher version:

```sh
EXOROUTE_VERSION=latest npm install -g exoroute
EXOROUTE_VERSION=v0.1.1 npm install -g exoroute
```

Accepted values are `latest` or a release tag, with the leading `v` optional (`0.1.1` and
`v0.1.1` are the same tag). Anything else fails with
`EXOROUTE_VERSION must be latest or a valid release tag.` The variable is read at install
time only; it does not change how a running gateway behaves.

In PowerShell, set it for the current session with
`$env:EXOROUTE_VERSION = "latest"` before running `npm install`.

## What the installer does

`postinstall` performs the following and stops at the first failure, without leaving a
partially installed binary behind:

1. Resolves the target and release tag described above.
2. Downloads the platform archive and the `SHA256SUMS` manifest into a private temporary
   directory, following redirects only to GitHub release hosts. The archive is capped at
   100 MiB and the manifest at 64 KiB, and the temporary directory is removed even when the
   install fails.
3. Requires the manifest to hold exactly one SHA-256 entry for the archive, hashes the
   archive, and compares the two. A mismatch is fatal and nothing is installed.
4. Extracts the archive and requires a precisely known file list: the binary plus `LICENSE`
   and `README.md`. Unexpected, missing, duplicate, or symbolic-link entries are rejected,
   and every extracted file must be a regular file within the size limit.
5. Copies the binary into the package's `vendor/` directory through a temporary file and an
   atomic rename, and marks it executable (`0755`) on Linux and macOS. An existing install
   that is not a regular file is never replaced.

The npm package verifies checksums but deliberately **does not require Cosign**. The
checksum manifest is signed by the release workflow with Sigstore; if you want that
signature verified too, use the next section.

## Verifying a release yourself

Each release publishes the five platform archives, a `SHA256SUMS` manifest, per-asset
`.sha256` files, and a `SHA256SUMS.sigstore.json` Sigstore bundle.

Verify that the manifest came from the ExoRoute release workflow, then check your archive
against it:

```sh
# 1. Verify the checksum manifest's Sigstore signature and the workflow identity.
cosign verify-blob SHA256SUMS \
  --bundle SHA256SUMS.sigstore.json \
  --certificate-identity "https://github.com/nghiaomg/ExoRoute/.github/workflows/release.yml@refs/tags/v0.1.1" \
  --certificate-oidc-issuer "https://token.actions.githubusercontent.com"

# 2. Verify the archive you downloaded.
sha256sum --check --ignore-missing SHA256SUMS
```

Replace `v0.1.1` in the certificate identity with the tag you installed — the signature is
issued for the exact tag ref, so a mismatch means the manifest is from another release. On
Windows, use `Get-FileHash -Algorithm SHA256 <file>` or `certutil -hashfile <file> SHA256`
instead of `sha256sum`.

The launcher already performs the archive checksum check by itself; this manual path is for
installs that also want the signature verified, or that do not use npm at all. Archives can
be downloaded directly from the
[releases page](https://github.com/nghiaomg/ExoRoute/releases).

## Upgrading

Upgrading the package upgrades the binary, because the launcher fetches the release matching
its own version:

```sh
npm install -g exoroute@latest
exoroute version
```

To see what is published:

```sh
npm outdated -g exoroute
npm view exoroute version
```

Your data in `~/.exoroute/` is untouched by installs, upgrades, and uninstalls. Read the
release notes before crossing a release that changes the storage format, and take a
dashboard database export first if you want a rollback point.

## Uninstalling

```sh
npm uninstall -g exoroute
```

This removes the launcher and the downloaded binary. It does **not** remove `~/.exoroute/`
(`%USERPROFILE%\.exoroute` on Windows), which holds your configuration, encrypted provider
credentials, and request history. Delete that directory yourself to remove the data too —
and keep `EXOROUTE_MASTER_KEY` from its `.env` if you may restore a backup later.

## Troubleshooting

| Symptom | Cause and fix |
| --- | --- |
| `ExoRoute does not publish a binary for <platform>/<arch>.` | The platform/architecture pair has no release build. Use a supported 64-bit target or build from source. |
| `download failed with HTTP 404` | The requested tag does not exist, or its assets are not published yet. Use a tag from the releases page, or `EXOROUTE_VERSION=latest`. |
| `download failed with HTTP 403`, a DNS error, or a connection error | The install host cannot reach GitHub. Install from a network with access to `github.com`, or install an archive manually from the releases page. |
| `SHA-256 mismatch for <archive>; the archive was not installed.` | The download was corrupted or tampered with. Retry on a trusted network; if it repeats, report it instead of forcing the install. |
| `archive contains unexpected or duplicate paths` / `extracted entry ... is not a safe regular file.` | The archive is not one this launcher produces. Download it again from the official release rather than extracting it by hand into the package. |
| `ExoRoute binary is not installed: ...` when running `exoroute` | The install ran with scripts disabled (for example `--ignore-scripts`), or the vendor binary was removed. Re-run the install scripts with `npm rebuild -g exoroute` (or `npm rebuild -g` for every global package), or reinstall the package. |
| `npm package directory is not a real directory` / `existing ExoRoute install is not a regular file` | A symlink or an unexpected file sits where the launcher expects the package or the binary. Remove it and reinstall. |
| Disk usage grows across upgrades | Every npm version keeps its own copy of the binary in its package directory. Remove leftover versions under your npm global prefix if you have installed many. |

## Documentation

- Full documentation — providers, combos, gateway API keys, output styles, stream
  continuity, storage, and backups:
  [github.com/nghiaomg/ExoRoute](https://github.com/nghiaomg/ExoRoute)
- Release notes and downloadable archives:
  [releases](https://github.com/nghiaomg/ExoRoute/releases)
- Bug reports and questions: [issues](https://github.com/nghiaomg/ExoRoute/issues)
- Interactive SDK and editor integration guides: served by your own instance at
  <http://127.0.0.1:8686/docs>
- Building from source, benchmarks, and the security model: repository
  [README](https://github.com/nghiaomg/ExoRoute/blob/main/README.md)

## License

MIT. See [LICENSE](https://github.com/nghiaomg/ExoRoute/blob/main/LICENSE).
