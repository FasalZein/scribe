# Install the scribe binary and check its runtime tools (Windows).
#
# - Installs scribe when it is missing or older than this skill's version:
#   downloads the prebuilt binary from the latest GitHub Release, verifies its
#   SHA-256 and copies it to $env:SCRIBE_INSTALL_DIR (default ~/.local/bin).
#   Without a matching binary, builds from source with cargo when cargo, cmake
#   and a C++ compiler (cl.exe or clang++) exist.
# - Checks ffmpeg, ffprobe, and uvx or yt-dlp. Prints the winget command for
#   each missing tool. It never runs a package manager.
# Safe to run again. Exits 0 when everything is ready, 1 otherwise.
$ErrorActionPreference = 'Stop'
$Repo = 'FasalZein/scribe'
$SkillDir = Split-Path -Parent $PSScriptRoot
$InstallDir = if ($env:SCRIBE_INSTALL_DIR) { $env:SCRIBE_INSTALL_DIR } else { Join-Path $HOME '.local\bin' }
$Exe = Join-Path $InstallDir 'scribe.exe'
$Missing = New-Object System.Collections.Generic.List[string]

function Have($name) { [bool](Get-Command $name -ErrorAction SilentlyContinue) }
function ScribeVersion($path) {
    try { ((& $path --version 2>$null) -split ' ')[1] } catch { $null }
}
function FindScribe {
    if (Test-Path $Exe) { return $Exe }
    $cmd = Get-Command scribe -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }
    return $null
}

$SkillVersion = (Select-String -Path (Join-Path $SkillDir 'Cargo.toml') -Pattern '^version = "(.*)"' |
    Select-Object -First 1).Matches[0].Groups[1].Value

# Returns 0 on success, 2 when no release asset matches, 1 on other failures.
function InstallRelease {
    if ($env:PROCESSOR_ARCHITECTURE -ne 'AMD64') {
        Write-Host "no prebuilt scribe for Windows $($env:PROCESSOR_ARCHITECTURE)"; return 2
    }
    try {
        $tag = (Invoke-RestMethod "https://api.github.com/repos/$Repo/releases/latest").tag_name
    } catch { Write-Host "cannot find the latest release of ${Repo}: $_"; return 2 }
    $asset = "scribe-$($tag.TrimStart('v'))-x86_64-pc-windows-msvc.zip"
    $base = "https://github.com/$Repo/releases/download/$tag"
    $tmp = Join-Path ([IO.Path]::GetTempPath()) ("scribe-" + [guid]::NewGuid())
    New-Item -ItemType Directory $tmp | Out-Null
    try {
        Write-Host "downloading $base/$asset"
        try {
            Invoke-WebRequest "$base/$asset" -OutFile (Join-Path $tmp $asset) -UseBasicParsing
            Invoke-WebRequest "$base/$asset.sha256" -OutFile (Join-Path $tmp "$asset.sha256") -UseBasicParsing
        } catch { Write-Host "release $tag has no asset $asset"; return 2 }
        $expected = ((Get-Content (Join-Path $tmp "$asset.sha256") -Raw).Trim() -split '\s+')[0].ToLower()
        $actual = (Get-FileHash (Join-Path $tmp $asset) -Algorithm SHA256).Hash.ToLower()
        if ($expected -ne $actual) { Write-Host "SHA-256 mismatch for ${asset}: expected $expected, got $actual"; return 1 }
        Write-Host "SHA-256 verified: $actual"
        Expand-Archive (Join-Path $tmp $asset) -DestinationPath $tmp -Force
        New-Item -ItemType Directory -Force $InstallDir | Out-Null
        Copy-Item (Join-Path $tmp 'scribe.exe') $Exe -Force
        Write-Host "installed $Exe ($tag)"
        return 0
    } catch { Write-Host "install failed: $_"; return 1 }
    finally { Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue }
}

function InstallFromSource {
    $cxx = (Have 'cl') -or (Have 'clang++')
    if ((Have 'cargo') -and (Have 'cmake') -and $cxx) {
        Write-Host 'building scribe from source with cargo (a few minutes)'
        & cargo install --locked --path $SkillDir
        if ($LASTEXITCODE -ne 0) { $Missing.Add("cargo install --locked --path $SkillDir failed; see the output above") }
        return
    }
    $Missing.Add('scribe: no prebuilt binary and cannot build from source. Install: winget install Rustlang.Rustup Kitware.CMake Microsoft.VisualStudio.2022.BuildTools (with the C++ workload), then run this script again.')
}

$current = FindScribe
$version = if ($current) { ScribeVersion $current } else { $null }
if ($version -and ([version]$version -ge [version]$SkillVersion)) {
    Write-Host "ok: scribe $version ($current)"
} else {
    if ($version) { Write-Host "scribe $version is older than $SkillVersion; updating" }
    else { Write-Host 'scribe is not installed; installing' }
    switch (InstallRelease) {
        0 { }
        2 { InstallFromSource }
        default { $Missing.Add('scribe: the release download failed; see the output above') }
    }
    $current = FindScribe
    $version = if ($current) { ScribeVersion $current } else { $null }
    if ($version) { Write-Host "ok: scribe $version ($current)" }
    elseif ($current) { $Missing.Add("scribe: $current --version fails on this machine") }
}

if ((Test-Path $Exe) -and -not (($env:PATH -split ';') -contains $InstallDir)) {
    $Missing.Add("PATH: $InstallDir is not on PATH. Run: `$env:PATH = `"$InstallDir;`$env:PATH`" (add it to your profile), or call $Exe directly.")
}

foreach ($tool in 'ffmpeg', 'ffprobe') {
    if (Have $tool) { Write-Host "ok: $tool" }
    else { $Missing.Add("${tool}: not found. Install: winget install Gyan.FFmpeg") }
}
if (Have 'uvx') { Write-Host 'ok: uvx (runs yt-dlp@latest)' }
elseif (Have 'yt-dlp') { Write-Host 'ok: yt-dlp (keep it current; old versions get HTTP 403 from YouTube)' }
else { $Missing.Add('uvx or yt-dlp: not found. Install uv: winget install astral-sh.uv') }

if ($Missing.Count -gt 0) {
    Write-Host ''
    Write-Host 'NOT READY:'
    $Missing | ForEach-Object { Write-Host "- $_" }
    exit 1
}
Write-Host "ready: scribe $version"
