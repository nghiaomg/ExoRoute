$ErrorActionPreference = 'Stop'

$repo = 'nghiaomg/ExoRoute'
$version = $env:EXOROUTE_VERSION
$installDir = $env:EXOROUTE_INSTALL_DIR

if ([string]::IsNullOrWhiteSpace($version)) {
    $version = 'latest'
}
if ([string]::IsNullOrWhiteSpace($installDir)) {
    $installDir = Join-Path $env:LOCALAPPDATA 'ExoRoute\bin'
}
if (-not [System.IO.Path]::IsPathRooted($installDir)) {
    throw 'EXOROUTE_INSTALL_DIR must be an absolute path.'
}
$installDir = [System.IO.Path]::GetFullPath($installDir)
if ($installDir.Contains(';')) {
    throw 'EXOROUTE_INSTALL_DIR cannot contain a semicolon because PATH uses it as a separator.'
}
if ($version -ne 'latest' -and $version -notmatch '^v[0-9][0-9A-Za-z.+-]*$') {
    throw 'EXOROUTE_VERSION must be latest or a version tag such as v0.1.0.'
}
$cosign = Get-Command -Name 'cosign' -CommandType Application -ErrorAction SilentlyContinue
if ($null -eq $cosign) {
    throw 'Sigstore Cosign v3.1.3 or newer is required to verify ExoRoute release signatures. Install Cosign, then run this installer again.'
}
$cosignVersionOutput = & $cosign.Source version 2>$null
if ($LASTEXITCODE -ne 0) {
    throw 'Could not read the installed Cosign version.'
}
$cosignVersionLine = $cosignVersionOutput | Where-Object { $_ -match '^GitVersion:\s*v[0-9]+\.[0-9]+\.[0-9]+$' } | Select-Object -First 1
if ($null -eq $cosignVersionLine -or $cosignVersionLine -notmatch '^GitVersion:\s*v([0-9]+\.[0-9]+\.[0-9]+)$') {
    throw 'Could not parse the installed Cosign version.'
}
$cosignVersion = [version]$Matches[1]
if ($cosignVersion -lt [version]'3.1.3') {
    throw 'Cosign v3.1.3 or newer is required to verify ExoRoute release signatures.'
}

$architecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString().ToLowerInvariant()
if ($architecture -ne 'x64') {
    throw "ExoRoute does not have a Windows release for architecture: $architecture."
}

$releasePath = if ($version -eq 'latest') { 'latest/download' } else { "download/$version" }
$asset = 'exoroute-windows-x86_64.zip'
$releaseUrl = "https://github.com/$repo/releases/$releasePath"
$url = "$releaseUrl/$asset"
$tempDir = Join-Path ([System.IO.Path]::GetTempPath()) ([Guid]::NewGuid().ToString('N'))
$stagedBinary = $null
$maxDownloadBytes = 100MB
$ProgressPreference = 'SilentlyContinue'

function Invoke-LimitedHttpsDownload {
    param(
        [Parameter(Mandatory = $true)][uri]$Uri,
        [Parameter(Mandatory = $true)][string]$Destination,
        [Parameter(Mandatory = $true)][long]$MaximumBytes,
        [Parameter(Mandatory = $true)][int]$TimeoutSeconds
    )

    $clock = [System.Diagnostics.Stopwatch]::StartNew()
    $current = $Uri
    for ($redirect = 0; $redirect -le 5; $redirect++) {
        $remainingMilliseconds = [long]($TimeoutSeconds * 1000 - $clock.Elapsed.TotalMilliseconds)
        if ($remainingMilliseconds -le 0) {
            throw 'The release download exceeded its time limit.'
        }
        if ($current.Scheme -ne 'https') {
            throw 'Release downloads must use HTTPS.'
        }
        $request = [System.Net.HttpWebRequest]::Create($current)
        $request.Method = 'GET'
        $request.AllowAutoRedirect = $false
        $request.Timeout = [int][Math]::Min(30000, $remainingMilliseconds)
        $request.ReadWriteTimeout = [int][Math]::Min(30000, $remainingMilliseconds)
        $response = $null
        try {
            $response = $request.GetResponse()
        }
        catch [System.Net.WebException] {
            if ($_.Exception.Response -eq $null) {
                throw
            }
            $response = $_.Exception.Response
        }

        try {
            $statusCode = [int]$response.StatusCode
            if ($statusCode -in @(301, 302, 303, 307, 308)) {
                if ($redirect -eq 5) {
                    throw 'The release server redirected too many times.'
                }
                $location = $response.Headers['Location']
                if ([string]::IsNullOrWhiteSpace($location)) {
                    throw 'The release server returned an invalid redirect.'
                }
                $current = [uri]::new($current, $location)
                continue
            }
            if ($statusCode -ne 200) {
                throw "The release server returned HTTP $statusCode."
            }
            if ($response.ContentLength -gt $MaximumBytes) {
                throw 'The release download exceeds its configured size limit.'
            }

            $source = $response.GetResponseStream()
            if ($source -eq $null) {
                throw 'The release server returned an empty response stream.'
            }
            $output = [System.IO.File]::Open(
                $Destination,
                [System.IO.FileMode]::CreateNew,
                [System.IO.FileAccess]::Write,
                [System.IO.FileShare]::None
            )
            try {
                [byte[]]$buffer = New-Object byte[] 65536
                [long]$total = 0
                while (($read = $source.Read($buffer, 0, $buffer.Length)) -gt 0) {
                    if ($clock.Elapsed.TotalSeconds -gt $TimeoutSeconds) {
                        throw 'The release download exceeded its time limit.'
                    }
                    if ($total -gt ($MaximumBytes - $read)) {
                        throw 'The release download exceeds its configured size limit.'
                    }
                    $output.Write($buffer, 0, $read)
                    $total += $read
                }
                $output.Flush($true)
            }
            finally {
                $output.Dispose()
                $source.Dispose()
            }
            return
        }
        finally {
            if ($response -ne $null) {
                $response.Dispose()
            }
        }
    }
    throw 'The release server redirected too many times.'
}

try {
    New-Item -ItemType Directory -Path $tempDir -Force | Out-Null
    $archive = Join-Path $tempDir $asset
    $checksumManifest = Join-Path $tempDir 'SHA256SUMS'
    $signatureBundle = Join-Path $tempDir 'SHA256SUMS.sigstore.json'
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
    Invoke-LimitedHttpsDownload -Uri ([uri]$url) -Destination $archive -MaximumBytes $maxDownloadBytes -TimeoutSeconds 300
    Invoke-LimitedHttpsDownload -Uri ([uri]"$releaseUrl/SHA256SUMS") -Destination $checksumManifest -MaximumBytes 65536 -TimeoutSeconds 60
    Invoke-LimitedHttpsDownload -Uri ([uri]"$releaseUrl/SHA256SUMS.sigstore.json") -Destination $signatureBundle -MaximumBytes 1MB -TimeoutSeconds 60
    $identityPattern = '^https://github[.]com/nghiaomg/ExoRoute/[.]github/workflows/release[.]yml@refs/tags/v[0-9][0-9A-Za-z.+-]*$'
    if ($version -ne 'latest') {
        $escapedVersion = [regex]::Escape($version)
        $identityPattern = "^https://github[.]com/nghiaomg/ExoRoute/[.]github/workflows/release[.]yml@refs/tags/$escapedVersion$"
    }
    & $cosign.Source verify-blob $checksumManifest --bundle $signatureBundle --certificate-identity-regexp $identityPattern --certificate-oidc-issuer 'https://token.actions.githubusercontent.com'
    if ($LASTEXITCODE -ne 0) {
        throw 'The release checksum signature is invalid or was issued by an untrusted workflow.'
    }

    $expectedHashes = @(
        foreach ($line in Get-Content -LiteralPath $checksumManifest) {
            if ($line -match '^([a-fA-F0-9]{64})  ([^\s]+)$' -and $Matches[2] -ceq $asset) {
                $Matches[1].ToLowerInvariant()
            }
        }
    )
    if ($expectedHashes.Count -ne 1) {
        throw 'The signed release checksum manifest is invalid or does not contain this archive.'
    }
    $expectedHash = $expectedHashes[0]
    if ((Get-Item -LiteralPath $archive).Length -gt $maxDownloadBytes) {
        throw 'The release archive exceeds the 100 MB download limit.'
    }

    if ($expectedHash -notmatch '^[a-f0-9]{64}$') {
        throw 'The release checksum is invalid.'
    }
    $actualHash = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actualHash -ne $expectedHash) {
        throw 'The release checksum does not match; the archive was not installed.'
    }

    Add-Type -AssemblyName System.IO.Compression
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [System.IO.Compression.ZipFile]::OpenRead($archive)
    try {
        $allowedEntries = @('exoroute.exe', 'LICENSE', 'README.md')
        if ($zip.Entries.Count -ne $allowedEntries.Count) {
            throw 'The release archive does not contain exactly the expected files.'
        }
        $entryNames = @()
        [long]$expandedBytes = 0
        foreach ($entry in $zip.Entries) {
            if ($entry.FullName -cnotin $allowedEntries) {
                throw 'The release archive contains an unexpected path.'
            }
            if ($entryNames -ccontains $entry.FullName) {
                throw 'The release archive contains a duplicate path.'
            }
            $unixType = ([int64]$entry.ExternalAttributes -shr 16) -band 0xF000
            if (($entry.ExternalAttributes -band 0x400) -ne 0 -or ($unixType -ne 0 -and $unixType -ne 0x8000)) {
                throw 'The release archive contains an unsafe or oversized entry.'
            }
            if ($entry.Length -gt ($maxDownloadBytes - $expandedBytes)) {
                throw 'The release archive expands beyond the 100 MB limit.'
            }
            $expandedBytes += $entry.Length
            $entryNames += $entry.FullName
        }
        if ($entryNames -cnotcontains 'exoroute.exe' -or $entryNames -cnotcontains 'LICENSE' -or $entryNames -cnotcontains 'README.md') {
            throw 'The release archive does not contain exoroute.exe.'
        }

        $binaryEntry = $zip.GetEntry('exoroute.exe')
        $source = $binaryEntry.Open()
        $destination = [System.IO.File]::Open(
            (Join-Path $tempDir 'exoroute.exe'),
            [System.IO.FileMode]::CreateNew,
            [System.IO.FileAccess]::Write,
            [System.IO.FileShare]::None
        )
        try {
            [byte[]]$buffer = New-Object byte[] 65536
            [long]$copiedBytes = 0
            while (($read = $source.Read($buffer, 0, $buffer.Length)) -gt 0) {
                if ($copiedBytes -gt ($maxDownloadBytes - $read)) {
                    throw 'The release binary expands beyond the 100 MB limit.'
                }
                $destination.Write($buffer, 0, $read)
                $copiedBytes += $read
            }
            if ($copiedBytes -ne $binaryEntry.Length) {
                throw 'The release binary length does not match its archive metadata.'
            }
            $destination.Flush($true)
        }
        finally {
            $destination.Dispose()
            $source.Dispose()
        }
    }
    finally {
        $zip.Dispose()
    }

    $destinationPath = Join-Path $installDir 'exoroute.exe'
    New-Item -ItemType Directory -Path $installDir -Force | Out-Null
    $stagedBinary = Join-Path $installDir ".exoroute.exe.$([Guid]::NewGuid().ToString('N')).tmp"
    Copy-Item -LiteralPath (Join-Path $tempDir 'exoroute.exe') -Destination $stagedBinary
    if (Test-Path -LiteralPath $destinationPath) {
        if (-not (Test-Path -LiteralPath $destinationPath -PathType Leaf) -or ([System.IO.File]::GetAttributes($destinationPath) -band [System.IO.FileAttributes]::ReparsePoint)) {
            throw 'The existing ExoRoute install path is not a regular file.'
        }
        [System.IO.File]::Replace($stagedBinary, $destinationPath, $null)
    }
    else {
        [System.IO.File]::Move($stagedBinary, $destinationPath)
    }
    $stagedBinary = $null

    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    $pathEntries = @($userPath -split ';' | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
    $alreadyInUserPath = $pathEntries | Where-Object { $_.TrimEnd('\') -ieq $installDir.TrimEnd('\') }
    if (-not $alreadyInUserPath) {
        $newUserPath = (@($pathEntries) + $installDir) -join ';'
        if ($newUserPath.Length -gt 32767) {
            throw 'The user PATH is too long to add the ExoRoute install directory.'
        }
        [Environment]::SetEnvironmentVariable('Path', $newUserPath, 'User')
    }

    $processEntries = @($env:PATH -split ';' | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
    $alreadyInProcessPath = $processEntries | Where-Object { $_.TrimEnd('\') -ieq $installDir.TrimEnd('\') }
    if (-not $alreadyInProcessPath) {
        $env:PATH = (@($processEntries) + $installDir) -join ';'
    }

    Write-Host "ExoRoute was installed to $(Join-Path $installDir 'exoroute.exe')."
    Write-Host 'Run it with: exoroute'
    Write-Host 'Open a new terminal to use the updated PATH in other sessions.'
}
finally {
    if ($stagedBinary -and (Test-Path -LiteralPath $stagedBinary)) {
        Remove-Item -LiteralPath $stagedBinary -Force
    }
    if (Test-Path -LiteralPath $tempDir) {
        Remove-Item -LiteralPath $tempDir -Recurse -Force
    }
}
