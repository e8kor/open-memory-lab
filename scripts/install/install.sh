#!/bin/sh
# Install the oml CLI on Linux or macOS, for this user, without sudo
# (docs/adr/ADR-0041-a-quick-start-in-one-command-per-role.md d1-d3).
#
#   curl -fsSL --proto '=https' --tlsv1.2 \
#     https://raw.githubusercontent.com/e8kor/open-memory-lab/vX.Y.Z/scripts/install/install.sh | sh
#
# Arguments follow `sh -s --`:
#   --version X.Y.Z | latest   the release to install (default: the one this
#                              script was released with, DEFAULT_VERSION below)
#   --dir PATH                 where to put oml (default: $HOME/.local/bin)
#   --require-signature        fail unless cosign verifies SHA256SUMS
# The same three as environment variables: OML_VERSION, OML_INSTALL_DIR,
# OML_REQUIRE_SIGNATURE=1.
#
# What it checks: the archive's SHA-256 against the release's SHA256SUMS, and,
# when cosign is on PATH, SHA256SUMS's keyless signature against the release
# workflow's identity at that tag. Without cosign only the checksum is checked,
# and the script says so. It writes one file, the binary, and edits no shell
# profile. The whole body is one function called on the last line, so a
# download cut short runs nothing.

set -eu

REPO="e8kor/open-memory-lab"
# The release installed when no version is named: the workspace version of
# the commit this script is served from (scripts/check_release.py holds the
# two equal).
DEFAULT_VERSION="0.5.2"
ISSUER="https://token.actions.githubusercontent.com"
DOCS="https://e8kor.github.io/open-memory-lab"

die() {
    echo "oml install: $*" >&2
    exit 1
}

say() {
    echo "oml install: $*"
}

usage() {
    cat <<EOF
usage: install.sh [--version X.Y.Z|latest] [--dir PATH] [--require-signature]

Installs the oml CLI for this user, without sudo.
  --version            the release to install (default $DEFAULT_VERSION); latest asks GitHub
  --dir                where to put oml (default \$HOME/.local/bin)
  --require-signature  fail unless cosign verifies the release's SHA256SUMS
Environment: OML_VERSION, OML_INSTALL_DIR, OML_REQUIRE_SIGNATURE=1.
EOF
}

need() {
    command -v "$1" >/dev/null 2>&1 || die "$1 is required and was not found on PATH"
}

fetch() { # url file
    # -S keeps curl's own reason, such as "returned error: 404", under -s.
    curl --proto '=https' --proto-redir '=https' --tlsv1.2 -fsSL -S --retry 3 -o "$2" "$1" \
        || die "could not download $1"
}

sha256_of() { # file
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d ' ' -f 1
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | cut -d ' ' -f 1
    else
        die "neither sha256sum nor shasum was found, so the download cannot be checked"
    fi
}

detect_target() {
    os="$(uname -s)"
    arch="$(uname -m)"
    case "$os" in
        Linux)
            # The Linux CLI is static (musl), so one archive serves every distribution.
            case "$arch" in
                x86_64 | amd64) echo "x86_64-unknown-linux-musl" ;;
                aarch64 | arm64) echo "aarch64-unknown-linux-musl" ;;
                *) die "no oml release for Linux on $arch; the targets are x86_64 and aarch64" ;;
            esac
            ;;
        Darwin)
            # macOS release targets are paused (docs/engineering/ci-and-toolchain.md "Paused targets").
            die "no oml release for macOS in this version: macOS builds are paused; build it from source with cargo install --git https://github.com/e8kor/open-memory-lab --tag v$version oml-cli (see docs/start/install.md)"
            ;;
        MINGW* | MSYS* | CYGWIN*)
            die "on Windows, run install.ps1 in PowerShell instead: $DOCS/cli/reference/"
            ;;
        *) die "no oml release for $os; build from source: $DOCS/cli/reference/" ;;
    esac
}

resolve_latest() { # file
    fetch "https://api.github.com/repos/$REPO/releases/latest" "$1"
    tag="$(sed -n 's/^[[:space:]]*"tag_name":[[:space:]]*"\([^"]*\)".*/\1/p' "$1" | head -n 1)"
    [ -n "$tag" ] || die "the GitHub release API named no latest release of $REPO"
    echo "${tag#v}"
}

main() {
    version="${OML_VERSION:-$DEFAULT_VERSION}"
    dir="${OML_INSTALL_DIR:-}"
    require_signature="${OML_REQUIRE_SIGNATURE:-0}"
    while [ $# -gt 0 ]; do
        case "$1" in
            --version)
                [ $# -ge 2 ] || die "--version needs a value"
                version="$2"
                shift 2
                ;;
            --version=*)
                version="${1#*=}"
                shift
                ;;
            --dir)
                [ $# -ge 2 ] || die "--dir needs a value"
                dir="$2"
                shift 2
                ;;
            --dir=*)
                dir="${1#*=}"
                shift
                ;;
            --require-signature)
                require_signature=1
                shift
                ;;
            -h | --help)
                usage
                return 0
                ;;
            *) die "unknown argument: $1 (try --help)" ;;
        esac
    done
    if [ -z "$dir" ]; then
        [ -n "${HOME:-}" ] || die "HOME is not set; name a directory with --dir"
        dir="$HOME/.local/bin"
    fi
    need curl
    need tar
    need uname
    need mktemp

    target="$(detect_target)"
    tmp="$(mktemp -d 2>/dev/null || mktemp -d -t oml-install)"
    trap 'rm -rf "$tmp"' EXIT
    trap 'exit 130' INT TERM

    if [ "$version" = "latest" ]; then
        version="$(resolve_latest "$tmp/latest.json")"
    fi
    version="${version#v}"
    case "$version" in
        "" | *[!0-9A-Za-z.-]*) die "not a release version: '$version'" ;;
    esac

    base="https://github.com/$REPO/releases/download/v$version"
    archive="oml-cli-${version}-${target}.tar.gz"
    say "oml $version for $target"
    fetch "$base/$archive" "$tmp/$archive"
    fetch "$base/SHA256SUMS" "$tmp/SHA256SUMS"

    # The signature covers SHA256SUMS, and SHA256SUMS covers the archive.
    if command -v cosign >/dev/null 2>&1; then
        fetch "$base/SHA256SUMS.sigstore.json" "$tmp/SHA256SUMS.sigstore.json"
        tag_re="$(printf '%s' "v$version" | sed 's/\./\\./g')"
        # cosign's last line is the reason: a bad signature, a wrong identity, or no
        # network to the transparency log's root of trust.
        if ! said="$(cosign verify-blob \
            --certificate-oidc-issuer "$ISSUER" \
            --certificate-identity-regexp "^https://github.com/e8kor/open-memory-lab/\\.github/workflows/release\\.yml@refs/tags/${tag_re}\$" \
            --bundle "$tmp/SHA256SUMS.sigstore.json" "$tmp/SHA256SUMS" 2>&1)"; then
            die "cosign did not verify SHA256SUMS of v$version under the release workflow's identity; nothing was installed. cosign: $(printf '%s\n' "$said" | tail -n 1)"
        fi
        checked="cosign verified SHA256SUMS's signature by the release workflow at v$version, and the archive matches it"
    elif [ "$require_signature" = "1" ]; then
        die "--require-signature was given and cosign is not on PATH; nothing was installed"
    else
        checked="the archive matches SHA256SUMS; its signature was NOT checked, because cosign is not on PATH. That catches a damaged download, not a tampered release: install cosign and rerun with --require-signature for that"
    fi

    want="$(awk -v a="$archive" '$2 == a || $2 == "./" a || $2 == "*" a || $2 == "*./" a { print $1; exit }' "$tmp/SHA256SUMS")"
    [ -n "$want" ] || die "SHA256SUMS of v$version lists no $archive"
    got="$(sha256_of "$tmp/$archive")"
    [ "$got" = "$want" ] || die "$archive has SHA-256 $got, SHA256SUMS says $want; nothing was installed"

    tar -xzf "$tmp/$archive" -C "$tmp"
    bin="$tmp/oml-cli-${version}-${target}/oml"
    [ -f "$bin" ] || die "$archive holds no oml-cli-${version}-${target}/oml"

    mkdir -p "$dir" || die "cannot create $dir"
    # Copy beside the destination, then rename over it: a running oml is never
    # left half-written.
    cp "$bin" "$dir/.oml.install.$$" || die "cannot write to $dir"
    chmod 0755 "$dir/.oml.install.$$"
    mv -f "$dir/.oml.install.$$" "$dir/oml" || die "cannot replace $dir/oml"
    printed="$("$dir/oml" --version 2>/dev/null || true)"
    [ "$printed" = "oml $version" ] || die "$dir/oml printed '$printed', expected 'oml $version'"

    say "installed $dir/oml ($printed)"
    say "checked: $checked"
    case ":${PATH:-}:" in
        *":$dir:"*) cmd="oml" ;;
        *)
            cmd="$dir/oml"
            say "$dir is not on your PATH; this script edits no shell profile. To add it, put this line in yours:"
            echo "    export PATH=\"$dir:\$PATH\""
            ;;
    esac
    echo
    if "$dir/oml" login --help >/dev/null 2>&1; then
        echo "Next, connect to your server with your agent key:"
        echo "    $cmd login https://<your OML server>"
        echo "    $cmd doctor"
    else
        echo "Next: oml $version reads its server and key from flags or the environment."
        echo "    $cmd --help"
        echo "    $DOCS/cli/reference/"
    fi
}

main "$@"
