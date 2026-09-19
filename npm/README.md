# ExoRoute

Install the ExoRoute native gateway through npm:

```sh
npm install -g exoroute
exoroute
```

The package downloads the release binary for the current operating system and
CPU, verifies its SHA-256 checksum from the matching GitHub release, and
installs a small launcher. Node.js 18 or newer is required. Supported targets
are Linux x64/arm64, macOS x64/arm64, and Windows x64.

To install a specific release, use the matching npm package version:

```sh
npm install -g exoroute@0.1.0
```

The `EXOROUTE_VERSION` environment variable can override the release tag for
testing, for example `v0.1.0` or `latest`.
