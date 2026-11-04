# Install the oml CLI on Windows, for this user, without administrator rights
# (docs/adr/ADR-0041-a-quick-start-in-one-command-per-role.md d1-d3).
#
#   irm https://raw.githubusercontent.com/e8kor/open-memory-lab/vX.Y.Z/scripts/install/install.ps1 | iex
#
# With arguments:
#   & ([scriptblock]::Create((irm <same url>))) -Version latest -Dir C:\tools\oml -RequireSignature
#
#   -Version X.Y.Z | latest   the release to install (default: the one this
#                             script was released with, $DefaultVersion below)
#   -Dir PATH                 where to put oml.exe (default: %LOCALAPPDATA%\Programs\oml\bin)
#   -RequireSignature         fail unless cosign verifies SHA256SUMS
# The same three as environment variables: OML_VERSION, OML_INSTALL_DIR,
# OML_REQUIRE_SIGNATURE=1.
#
# What it checks: the archive's SHA-256 against the release's SHA256SUMS, and,
# when cosign is on PATH, SHA256SUMS's keyless signature against the release
# workflow's identity at that tag. Without cosign only the checksum is checked,
# and the script says so. It writes one file, oml.exe, and changes no PATH.
# Every failure throws rather than exits, so `| iex` never closes the shell.

param(
    [string]$Version = $env:OML_VERSION,
    [string]$Dir = $env:OML_INSTALL_DIR,
    [switch]$RequireSignature = ($env:OML_REQUIRE_SIGNATURE -eq '1')
)

function Install-Oml {
    param([string]$Version, [string]$Dir, [bool]$RequireSignature)

    $ErrorActionPreference = 'Stop'
    $ProgressPreference = 'SilentlyContinue'
    $Repo = 'e8kor/open-memory-lab'
    # The release installed when no version is named: the workspace version of
    # the commit this script is served from (scripts/check_release.py holds the
    # two equal).
    $DefaultVersion = '0.4.1'
    $Issuer = 'https://token.actions.githubusercontent.com'
    $Docs = 'https://e8kor.github.io/open-memory-lab'
    $Target = 'x86_64-pc-windows-msvc'

    function Fail([string]$Message) { throw "oml install: $Message" }
    function Say([string]$Message) { Write-Host "oml install: $Message" }
    # A native command's stderr becomes an error record under 'Stop' in Windows
    # PowerShell 5.1, which a usage message on stderr would trip; the exit code decides.
    function Invoke-Quiet([scriptblock]$Command) {
        $saved = $ErrorActionPreference
        $ErrorActionPreference = 'Continue'
        try { & $Command *> $null } finally { $ErrorActionPreference = $saved }
        return $LASTEXITCODE
    }
    function Fetch([string]$Url, [string]$File) {
        try { Invoke-WebRequest -UseBasicParsing -Uri $Url -OutFile $File }
        catch { Fail "could not download ${Url}: $($_.Exception.Message)" }
    }

    # Windows PowerShell 5.1 may not offer TLS 1.2 by default; GitHub requires it.
    [Net.ServicePointManager]::SecurityProtocol =
        [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

    # A 32-bit PowerShell on 64-bit Windows reports x86 and names the machine in
    # PROCESSOR_ARCHITEW6432.
    $arch = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
    switch ($arch) {
        'AMD64' { }
        'ARM64' { Say 'no ARM64 build; installing the x86_64 build, which Windows 11 runs under emulation' }
        default { Fail "no oml release for Windows on $arch" }
    }
    if (-not $Version) { $Version = $DefaultVersion }
    if (-not $Dir) {
        if (-not $env:LOCALAPPDATA) { Fail 'LOCALAPPDATA is not set; name a directory with -Dir' }
        $Dir = Join-Path $env:LOCALAPPDATA 'Programs\oml\bin'
    }
    if ($Version -eq 'latest') {
        try { $release = Invoke-RestMethod -UseBasicParsing -Uri "https://api.github.com/repos/$Repo/releases/latest" }
        catch { Fail "the GitHub release API did not answer: $($_.Exception.Message)" }
        if (-not $release.tag_name) { Fail "the GitHub release API named no latest release of $Repo" }
        $Version = [string]$release.tag_name
    }
    $Version = $Version -replace '^v', ''
    if ($Version -notmatch '^[0-9A-Za-z.-]+$') { Fail "not a release version: '$Version'" }

    $base = "https://github.com/$Repo/releases/download/v$Version"
    $archive = "oml-cli-$Version-$Target.zip"
    $tmp = Join-Path ([IO.Path]::GetTempPath()) ('oml-install-' + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $tmp | Out-Null
    try {
        Say "oml $Version for $Target"
        Fetch "$base/$archive" (Join-Path $tmp $archive)
        Fetch "$base/SHA256SUMS" (Join-Path $tmp 'SHA256SUMS')

        # The signature covers SHA256SUMS, and SHA256SUMS covers the archive.
        if (Get-Command cosign -ErrorAction SilentlyContinue) {
            Fetch "$base/SHA256SUMS.sigstore.json" (Join-Path $tmp 'SHA256SUMS.sigstore.json')
            $tagRe = ('v' + $Version) -replace '\.', '\.'
            $identity = '^https://github.com/e8kor/open-memory-lab/\.github/workflows/release\.yml@refs/tags/' + $tagRe + '$'
            # cosign's last line is the reason: a bad signature, a wrong identity, or no
            # network to the transparency log's root of trust.
            $saved = $ErrorActionPreference
            $ErrorActionPreference = 'Continue'
            try {
                $said = & cosign verify-blob --certificate-oidc-issuer $Issuer --certificate-identity-regexp $identity `
                    --bundle (Join-Path $tmp 'SHA256SUMS.sigstore.json') (Join-Path $tmp 'SHA256SUMS') 2>&1 |
                    ForEach-Object { "$_" }
                $verified = $LASTEXITCODE
            } finally { $ErrorActionPreference = $saved }
            if ($verified -ne 0) {
                Fail "cosign did not verify SHA256SUMS of v$Version under the release workflow's identity; nothing was installed. cosign: $(@($said)[-1])"
            }
            $checked = "cosign verified SHA256SUMS's signature by the release workflow at v$Version, and the archive matches it"
        } elseif ($RequireSignature) {
            Fail '-RequireSignature was given and cosign is not on PATH; nothing was installed'
        } else {
            $checked = 'the archive matches SHA256SUMS; its signature was NOT checked, because cosign is not on PATH. That catches a damaged download, not a tampered release: install cosign and rerun with -RequireSignature for that'
        }

        $want = $null
        foreach ($line in Get-Content (Join-Path $tmp 'SHA256SUMS')) {
            $hash, $name = $line -split '\s+', 2
            if ($name -and (($name -replace '^\*', '') -replace '^\./', '') -eq $archive) { $want = $hash.ToLowerInvariant(); break }
        }
        if (-not $want) { Fail "SHA256SUMS of v$Version lists no $archive" }
        $got = (Get-FileHash -Algorithm SHA256 -Path (Join-Path $tmp $archive)).Hash.ToLowerInvariant()
        if ($got -ne $want) { Fail "$archive has SHA-256 $got, SHA256SUMS says $want; nothing was installed" }

        Expand-Archive -Path (Join-Path $tmp $archive) -DestinationPath $tmp -Force
        $bin = Join-Path $tmp "oml-cli-$Version-$Target\oml.exe"
        if (-not (Test-Path -PathType Leaf $bin)) { Fail "$archive holds no oml-cli-$Version-$Target\oml.exe" }

        New-Item -ItemType Directory -Force -Path $Dir | Out-Null
        $dest = Join-Path $Dir 'oml.exe'
        try { Copy-Item -Force -Path $bin -Destination $dest }
        catch { Fail "cannot write $dest (is oml running?): $($_.Exception.Message)" }
        $saved = $ErrorActionPreference
        $ErrorActionPreference = 'Continue'
        try { $printed = (& $dest --version 2>$null | Out-String).Trim() } finally { $ErrorActionPreference = $saved }
        if ($printed -ne "oml $Version") { Fail "$dest printed '$printed', expected 'oml $Version'" }

        Say "installed $dest ($printed)"
        Say "checked: $checked"
        $onPath = ($env:Path -split ';') -contains $Dir
        $cmd = if ($onPath) { 'oml' } else { "& '$dest'" }
        if (-not $onPath) {
            Say "$Dir is not on your PATH; this script changes no PATH. To add it for your user, run:"
            Write-Host "    [Environment]::SetEnvironmentVariable('Path', '$Dir;' + [Environment]::GetEnvironmentVariable('Path', 'User'), 'User')"
        }
        Write-Host ''
        if ((Invoke-Quiet { & $dest login --help }) -eq 0) {
            Write-Host 'Next, connect to your server with your agent key:'
            Write-Host "    $cmd login https://<your OML server>"
            Write-Host "    $cmd doctor"
        } else {
            Write-Host "Next: oml $Version reads its server and key from flags or the environment."
            Write-Host "    $cmd --help"
            Write-Host "    $Docs/cli/reference/"
        }
    } finally {
        Remove-Item -Recurse -Force -Path $tmp -ErrorAction SilentlyContinue
    }
}

Install-Oml -Version $Version -Dir $Dir -RequireSignature ([bool]$RequireSignature)
