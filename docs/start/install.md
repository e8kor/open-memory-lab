<!--
id: start/install
tier: engineering
status: draft for review · the installers since v0.4.0
answers: what install.sh and install.ps1 check and their two outcomes word for word; the flags and variables; where they write; the PATH line; the inspect-first form; what a piped install trusts; uninstalling; Windows PowerShell 5.1 and 7; the manual cosign steps; and the package managers not offered
-->

# Install the `oml` command

**Status:** draft for review · the installers since v0.4.0

**Release:** the per-platform archives since v0.3.1; the installers since v0.4.0, the first
tag to carry them ([releases](../product/releases.md))

Two scripts install the `oml` CLI for the current user, write one file and print the next command
([ADR-0041](../adr/ADR-0041-a-quick-start-in-one-command-per-role.md) d1–d5). The lines
below fetch them at the tag `v0.5.2`. Before that release the scripts
were run against v0.3.1 on Linux x86_64, with cosign, without it and with `latest`
(ADR-0041, "Where each item stands"), and `install.ps1` runs only in CI's `install-smoke` job.

## The line

**macOS is paused.** Releases after v0.5.2 carry no macOS build while GitHub's macOS runners are unreliable ([paused targets](../engineering/ci-and-toolchain.md#paused-targets)); on a Mac the installer stops with that message. Build the CLI from source instead: `cargo install --git https://github.com/e8kor/open-memory-lab --tag vX.Y.Z oml-cli`.

Linux:

```sh
curl -fsSL --proto '=https' --tlsv1.2 https://raw.githubusercontent.com/e8kor/open-memory-lab/v0.5.6/scripts/install/install.sh | sh
```

Windows, in Windows PowerShell 5.1 or PowerShell 7:

```powershell
irm https://raw.githubusercontent.com/e8kor/open-memory-lab/v0.5.6/scripts/install/install.ps1 | iex
```

**What a piped install trusts** (ADR-0041 d5):

- The pipe trusts GitHub's TLS, the repository and the tag. A tag can be moved by whoever can push
  to the repository, which is why the cosign check below pins the tag in the signer's identity.
- A server can tell a pipe from a download and serve each different bytes. To read the script
  before it runs, download it, read it, then run it:

    ```sh
    curl -fsSLO --proto '=https' --tlsv1.2 https://raw.githubusercontent.com/e8kor/open-memory-lab/v0.5.6/scripts/install/install.sh
    less install.sh
    sh install.sh
    ```

- The script never asks for sudo, writes one file and edits no shell profile.
- What it checked is stated as one of the two outcomes below, never as "verified".

## What it checks, and its two outcomes

The script picks the archive for this OS and architecture (Apple silicon is detected under
Rosetta; Windows on ARM64 gets the x86_64 build, which it runs under emulation), downloads it and
the release's `SHA256SUMS` over HTTPS, and compares the archive's SHA-256 with its line. Then:

- **With `cosign` on `PATH`** it also fetches `SHA256SUMS.sigstore.json` and runs `cosign
  verify-blob` under the release workflow's identity, pinned to the exact tag. It prints:

    ```text
    oml install: checked: cosign verified SHA256SUMS's signature by the release workflow at v0.4.0, and the archive matches it
    ```

- **Without cosign** it compares the checksum only, and prints:

    ```text
    oml install: checked: the archive matches SHA256SUMS; its signature was NOT checked, because cosign is not on PATH. That catches a damaged download, not a tampered release: install cosign and rerun with --require-signature for that
    ```

    The PowerShell script names `-RequireSignature` instead. The roadmap tracks the gap as
    [FR-69](../product/roadmap.md).

Every refusal names its reason and installs nothing: a cosign failure, a checksum mismatch, a
missing line in `SHA256SUMS`, or `--require-signature` without cosign. After the copy the script
requires that `oml --version` prints the requested version. The shell script's body is one
function called on its last line, so a download cut short runs nothing, and the PowerShell script
throws rather than exits, so `| iex` never closes your shell.

## Flags and variables

| `install.sh` | `install.ps1` | Variable | Default |
|---|---|---|---|
| `--version X.Y.Z` or `latest` | `-Version` | `OML_VERSION` | the release the script was cut with: 0.5.2 at `v0.5.2` (ADR-0041 d3) |
| `--dir PATH` | `-Dir` | `OML_INSTALL_DIR` | `$HOME/.local/bin`; `%LOCALAPPDATA%\Programs\oml\bin` |
| `--require-signature` | `-RequireSignature` | `OML_REQUIRE_SIGNATURE=1` | off |

`latest` reads `tag_name` from GitHub's latest-release API, which skips drafts and prereleases. A
piped install takes its flags after `sh -s --`, as in `… | sh -s -- --version latest`. `irm … |
iex` takes none, so set `$env:OML_VERSION`, `$env:OML_INSTALL_DIR` or
`$env:OML_REQUIRE_SIGNATURE = '1'` first.

## Where it writes, and `PATH`

The one file is `oml` (`oml.exe`) in the install directory, copied beside its destination and
renamed into place, so a running `oml` is never left half written. When the directory is not on
`PATH`, the script prints the one line that would add it and changes nothing. Then it prints the
next command:

```text
Next, connect to your server with your agent key:
    oml login https://<your OML server>
    oml doctor
```

**Uninstalling** is deleting that one file. `oml login` writes one more, the owner-only
credentials file, `$HOME/.config/oml/credentials` ([CLI
reference](../cli/reference.md#start-settings)); delete it too to forget the server and the key.

## By hand, with cosign

Each release carries the CLI as `oml-cli-X.Y.Z-<target>.tar.gz`, `.zip` on Windows, for the
targets under [release artifacts](../engineering/ci-and-toolchain.md#release-artifacts-sbom-signing-provenance),
each with a cosign signature bundle.

Linux, on x86_64 (on arm64, `t=aarch64-unknown-linux-musl`); the binary is static:

```sh
v=X.Y.Z t=x86_64-unknown-linux-musl          # the release and your target
base=https://github.com/e8kor/open-memory-lab/releases/download/v$v
curl -fsSLO "$base/oml-cli-$v-$t.tar.gz" -O "$base/oml-cli-$v-$t.tar.gz.sigstore.json"
cosign verify-blob \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  --certificate-identity-regexp '^https://github.com/e8kor/open-memory-lab/\.github/workflows/release\.yml@refs/tags/.*$' \
  --bundle "oml-cli-$v-$t.tar.gz.sigstore.json" "oml-cli-$v-$t.tar.gz" \
  && tar -xzf "oml-cli-$v-$t.tar.gz" \
  && install -D -m 0755 "oml-cli-$v-$t/oml" ~/.local/bin/oml
```

macOS: no archive while macOS targets are paused; v0.5.2 and earlier ship `t=aarch64-apple-darwin` (Apple
silicon) and `t=x86_64-apple-darwin` (Intel), installed with `install -m 0755 "oml-cli-$v-$t/oml" /usr/local/bin/oml`.

Windows, in PowerShell:

```powershell
$v = "X.Y.Z"; $t = "x86_64-pc-windows-msvc"; $n = "oml-cli-$v-$t"
$base = "https://github.com/e8kor/open-memory-lab/releases/download/v$v"
Invoke-WebRequest "$base/$n.zip" -OutFile "$n.zip"
Invoke-WebRequest "$base/$n.zip.sigstore.json" -OutFile "$n.zip.sigstore.json"
cosign verify-blob `
  --certificate-oidc-issuer https://token.actions.githubusercontent.com `
  --certificate-identity-regexp '^https://github.com/e8kor/open-memory-lab/\.github/workflows/release\.yml@refs/tags/.*$' `
  --bundle "$n.zip.sigstore.json" "$n.zip"
if ($LASTEXITCODE -eq 0) { Expand-Archive "$n.zip" -DestinationPath . }
```

Then put `$n\oml.exe` in a directory on your `PATH`. Extract only after `cosign verify-blob`
exits 0: an archive that does not verify is not a release. What else differs on Windows, the
owner-only files and the plugin's hook under Git Bash, is in the [CLI
reference](../cli/reference.md#installing-oml-and-what-differs-on-windows).

## Not offered

Homebrew, Scoop and winget are not offered. Since Homebrew 6.0.0 a third-party tap needs `brew
trust` before it installs, and winget publishes through a reviewed pull request per release; each
can follow the script, and none is one command on every OS today (ADR-0041, rejected
alternatives). An npm or `npx` wrapper would make Node a dependency of a static binary.
