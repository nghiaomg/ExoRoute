#!/bin/sh
set -eu

repo="nghiaomg/ExoRoute"
version=${EXOROUTE_VERSION:-latest}
install_dir=${EXOROUTE_INSTALL_DIR:-"$HOME/.local/bin"}

case "$(uname -s)" in
  Linux) os=linux; legacy_os= ;;
  Darwin) os=darwin; legacy_os=macos ;;
  *)
    printf 'ExoRoute installer does not support this operating system.\n' >&2
    exit 1
    ;;
esac

case "$(uname -m)" in
  x86_64 | amd64) arch=x86_64 ;;
  aarch64 | arm64) arch=aarch64 ;;
  *)
    printf 'ExoRoute installer does not support architecture: %s\n' "$(uname -m)" >&2
    exit 1
    ;;
esac

case "$install_dir" in
  /*) ;;
  *)
    printf 'EXOROUTE_INSTALL_DIR must be an absolute path.\n' >&2
    exit 1
    ;;
esac
if [ "$(printf '%s' "$install_dir" | wc -l | tr -d '[:space:]')" -ne 0 ]; then
  printf 'EXOROUTE_INSTALL_DIR cannot contain newline characters.\n' >&2
  exit 1
fi
case "$install_dir" in
  *:*)
    printf 'EXOROUTE_INSTALL_DIR cannot contain a colon because PATH uses it as a separator.\n' >&2
    exit 1
    ;;
esac
if printf '%s' "$install_dir" | LC_ALL=C grep -q '[[:cntrl:]]'; then
  printf 'EXOROUTE_INSTALL_DIR cannot contain control characters.\n' >&2
  exit 1
fi

if [ "$version" = latest ]; then
  release_path=latest/download
else
  case "$version" in
    v[0-9]*)
      case "$version" in
        *[!a-zA-Z0-9.+-]*)
          printf 'EXOROUTE_VERSION must be a version tag such as v0.1.0.\n' >&2
          exit 1
          ;;
      esac
      ;;
    *)
      printf 'EXOROUTE_VERSION must be latest or a version tag such as v0.1.0.\n' >&2
      exit 1
      ;;
  esac
  release_path="download/$version"
fi

asset="exoroute-$os-$arch"
archive_name="$asset.tar.gz"
release_url="https://github.com/$repo/releases/$release_path"
url="$release_url/$archive_name"
if ! command -v cosign >/dev/null 2>&1; then
  printf 'Sigstore Cosign v3.1.3 or newer is required to verify ExoRoute release signatures. Install Cosign, then run this installer again.\n' >&2
  exit 1
fi
cosign_version=$(cosign version 2>/dev/null | awk '$1 == "GitVersion:" { print $2; exit }')
if ! printf '%s\n' "$cosign_version" | awk '
  /^v[0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*$/ {
    version = substr($0, 2)
    split(version, parts, ".")
    if ((parts[1] + 0) > 3 || ((parts[1] + 0) == 3 && ((parts[2] + 0) > 1 || ((parts[2] + 0) == 1 && (parts[3] + 0) >= 3)))) exit 0
  }
  { exit 1 }
'; then
  printf 'Cosign v3.1.3 or newer is required; found %s.\n' "${cosign_version:-unknown}" >&2
  exit 1
fi
tmp_dir=$(mktemp -d)
staged_binary=
cleanup() {
  if [ -n "$staged_binary" ]; then
    rm -f "$staged_binary"
  fi
  rm -rf "$tmp_dir"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM

archive="$tmp_dir/$archive_name"
checksum_manifest="$tmp_dir/SHA256SUMS"
signature_bundle="$tmp_dir/SHA256SUMS.sigstore.json"
download_file() {
  source_url=$1
  destination=$2
  max_bytes=$3
  if command -v curl >/dev/null 2>&1; then
    curl -q --fail --location --silent --show-error --proto '=https' --proto-redir '=https' --tlsv1.2 --connect-timeout 20 --max-time 300 --max-filesize "$max_bytes" "$source_url" --output "$destination"
  else
    printf 'Install curl with HTTPS support, then run this installer again.\n' >&2
    exit 1
  fi
}

if ! download_file "$url" "$archive" 100000000; then
  if [ "${legacy_os:-}" = macos ]; then
    # Keep older tagged releases using the previous macOS asset names installable.
    archive_name="exoroute-$legacy_os-$arch.tar.gz"
    url="$release_url/$archive_name"
    archive="$tmp_dir/$archive_name"
    download_file "$url" "$archive" 100000000
  else
    exit 1
  fi
fi
download_file "$release_url/SHA256SUMS" "$checksum_manifest" 65536
download_file "$release_url/SHA256SUMS.sigstore.json" "$signature_bundle" 1048576
if [ "$version" = latest ]; then
  cosign verify-blob "$checksum_manifest" \
    --bundle "$signature_bundle" \
    --certificate-identity-regexp '^https://github[.]com/nghiaomg/ExoRoute/[.]github/workflows/release[.]yml@refs/tags/v[0-9][0-9A-Za-z.+-]*$' \
    --certificate-oidc-issuer 'https://token.actions.githubusercontent.com'
else
  cosign verify-blob "$checksum_manifest" \
    --bundle "$signature_bundle" \
    --certificate-identity "https://github.com/$repo/.github/workflows/release.yml@refs/tags/$version" \
    --certificate-oidc-issuer 'https://token.actions.githubusercontent.com'
fi
expected_hash=$(awk -v archive="$archive_name" '
  $2 == archive {
    if (found || NF != 2) exit 2
    found = 1
    hash = $1
  }
  END {
    if (found != 1) exit 1
    print hash
  }
' "$checksum_manifest") || {
  printf 'The signed release checksum manifest is invalid or does not contain this archive.\n' >&2
  exit 1
}
archive_size=$(wc -c < "$archive" | tr -d '[:space:]')
if [ "$archive_size" -gt 100000000 ]; then
  printf 'The release archive exceeds the 100 MB download limit.\n' >&2
  exit 1
fi

expected_hash=$(printf '%s' "$expected_hash" | tr '[:upper:]' '[:lower:]')
case "$expected_hash" in
  *[!a-f0-9]* | '')
    printf 'The release checksum is invalid.\n' >&2
    exit 1
    ;;
esac
if [ "${#expected_hash}" -ne 64 ]; then
  printf 'The release checksum is invalid.\n' >&2
  exit 1
fi
if command -v sha256sum >/dev/null 2>&1; then
  actual_hash=$(sha256sum "$archive" | awk '{print $1}')
elif command -v shasum >/dev/null 2>&1; then
  actual_hash=$(shasum -a 256 "$archive" | awk '{print $1}')
else
  printf 'Install sha256sum or shasum to verify the release archive.\n' >&2
  exit 1
fi
if [ "$actual_hash" != "$expected_hash" ]; then
  printf 'The release checksum does not match; the archive was not installed.\n' >&2
  exit 1
fi

package_tar="$tmp_dir/package.tar"
if ! gzip -dc "$archive" | head -c 100000001 > "$package_tar"; then
  printf 'The release archive could not be decompressed safely.\n' >&2
  exit 1
fi
expanded_size=$(wc -c < "$package_tar" | tr -d '[:space:]')
if [ "$expanded_size" -gt 100000000 ]; then
  printf 'The release archive expands beyond the 100 MB extraction limit.\n' >&2
  exit 1
fi
members=$(tar -tf "$package_tar") || {
  printf 'The release archive is invalid.\n' >&2
  exit 1
}
member_count=$(printf '%s\n' "$members" | wc -l | tr -d '[:space:]')
if [ "$member_count" -ne 3 ]; then
  printf 'The release archive does not contain exactly the expected files.\n' >&2
  exit 1
fi
printf '%s\n' "$members" | while IFS= read -r member; do
  case "$member" in
    exoroute | LICENSE | README.md) ;;
    *) printf 'The release archive contains an unexpected path.\n' >&2; exit 1 ;;
  esac
done
if ! printf '%s\n' "$members" | grep -Fxq exoroute; then
  printf 'The release archive does not contain the ExoRoute binary.\n' >&2
  exit 1
fi
if tar -tvf "$package_tar" | awk 'substr($0, 1, 1) != "-" { exit 1 }'; then
  :
else
  printf 'The release archive contains a non-regular file.\n' >&2
  exit 1
fi
tar -xf "$package_tar" -C "$tmp_dir" exoroute
if [ ! -f "$tmp_dir/exoroute" ] || [ -L "$tmp_dir/exoroute" ]; then
  printf 'The release archive did not contain a regular ExoRoute binary.\n' >&2
  exit 1
fi
mkdir -p "$install_dir"
staged_binary=$(mktemp "$install_dir/.exoroute.XXXXXX")
cp "$tmp_dir/exoroute" "$staged_binary"
chmod 0755 "$staged_binary"
if ! mv -f "$staged_binary" "$install_dir/exoroute"; then
  rm -f "$staged_binary"
  printf 'Could not replace the installed ExoRoute binary. The existing binary was left in place.\n' >&2
  exit 1
fi
staged_binary=

shell_name=${SHELL:-}
shell_name=${shell_name##*/}

add_path_profile() {
  profile=$1
  mkdir -p "$(dirname "$profile")"
  if ! grep -Fq '# ExoRoute installer: add binary directory to PATH' "$profile" 2>/dev/null; then
    {
      printf '\n# ExoRoute installer: add binary directory to PATH\n'
      if [ "$shell_name" = fish ]; then
        escaped_install_dir=$(printf '%s' "$install_dir" | sed 's/\\/\\\\/g; s/"/\\"/g; s/\$/\\$/g')
        printf 'fish_add_path --path "%s"\n' "$escaped_install_dir"
      else
        escaped_install_dir=$(printf '%s' "$install_dir" | sed "s/'/'\\\\''/g")
        printf 'case ":$PATH:" in\n  *:\047%s\047:*) ;;\n  *) export PATH=\047%s\047:"$PATH" ;;\nesac\n' "$escaped_install_dir" "$escaped_install_dir"
      fi
    } >>"$profile"
  fi
}

case "$shell_name" in
  bash)
    add_path_profile "$HOME/.bashrc"
    if [ -f "$HOME/.bash_profile" ]; then
      add_path_profile "$HOME/.bash_profile"
    else
      add_path_profile "$HOME/.profile"
    fi
    ;;
  zsh)
    add_path_profile "$HOME/.zshrc"
    if [ -f "$HOME/.zprofile" ]; then
      add_path_profile "$HOME/.zprofile"
    fi
    ;;
  fish)
    add_path_profile "$HOME/.config/fish/config.fish"
    ;;
  *)
    add_path_profile "$HOME/.profile"
    ;;
esac

printf 'ExoRoute was installed to %s/exoroute\n' "$install_dir"
case ":$PATH:" in
  *":$install_dir:"*) printf 'Run it with: exoroute\n' ;;
  *) printf 'Open a new terminal, then run: exoroute\n' ;;
esac
