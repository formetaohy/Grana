[CmdletBinding()]
param(
    [string]$Version,
    [string]$Binary,
    [string]$Output = 'dist'
)

$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$art = Join-Path $root 'installer\windows\art'
$stage = Join-Path $root 'target\installer\windows'
if (-not $Binary) {
    $Binary = Join-Path $root 'target\release\grana.exe'
}
if (-not [System.IO.Path]::IsPathRooted($Binary)) {
    $Binary = Join-Path $root $Binary
}
if (-not [System.IO.Path]::IsPathRooted($Output)) {
    $Output = Join-Path $root $Output
}

if (-not $Version) {
    $metadata = & cargo metadata --no-deps --format-version 1 --locked --manifest-path (Join-Path $root 'Cargo.toml')
    if ($LASTEXITCODE -ne 0) {
        throw 'cargo metadata failed'
    }
    $Version = (($metadata | ConvertFrom-Json).packages | Where-Object { $_.name -eq 'grana' }).version
}
if ($Version -notmatch '^\d+\.\d+\.\d+$') {
    throw "installer version must be numeric: $Version"
}
if (-not (Test-Path -LiteralPath $Binary)) {
    throw "release binary not found: $Binary"
}
if (-not (Test-Path -LiteralPath (Join-Path $art 'grana.ico'))) {
    throw "installer artwork not found: $art"
}

$env:PATH = "$env:PATH;$env:USERPROFILE\.dotnet\tools"
if (-not (Get-Command wix -ErrorAction SilentlyContinue)) {
    dotnet tool install --global wix --version 5.0.2 | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw 'installing the WiX toolset failed'
    }
}
wix extension add -g WixToolset.BootstrapperApplications.wixext/5.0.2 | Out-Null
if ($LASTEXITCODE -ne 0) {
    throw 'installing the WiX bootstrapper extension failed'
}

New-Item -ItemType Directory -Force -Path $stage, $Output | Out-Null

function Set-Signature([string]$Path) {
    if (-not $env:GRANA_SIGN_CERTIFICATE) {
        Write-Host "unsigned: $Path"
        return
    }
    $signtool = Get-Command signtool -ErrorAction SilentlyContinue
    if (-not $signtool) {
        $kits = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
        $signtool = Get-ChildItem -Path $kits -Filter signtool.exe -Recurse -ErrorAction SilentlyContinue |
            Where-Object { $_.FullName -match '\\x64\\' } |
            Sort-Object -Property FullName -Descending |
            Select-Object -First 1
        if (-not $signtool) {
            throw 'signtool.exe not found'
        }
    }
    & $signtool.Source sign /fd SHA256 /td SHA256 /tr 'http://timestamp.digicert.com' /f $env:GRANA_SIGN_CERTIFICATE /p $env:GRANA_SIGN_PASSWORD $Path
    if ($LASTEXITCODE -ne 0) {
        throw "signing failed: $Path"
    }
}

$msi = Join-Path $stage 'Grana.msi'
wix build -arch x64 -d Version=$Version -d Binary=$Binary -d Art=$art (Join-Path $PSScriptRoot 'Grana.wxs') -o $msi
if ($LASTEXITCODE -ne 0) {
    throw 'building the MSI package failed'
}
Set-Signature $msi

$setup = Join-Path $stage 'GranaSetup.exe'
wix build -arch x64 -d Version=$Version -d Msi=$msi -d Art=$art -bindpath $PSScriptRoot -ext WixToolset.BootstrapperApplications.wixext (Join-Path $PSScriptRoot 'GranaSetup.wxs') -o $setup
if ($LASTEXITCODE -ne 0) {
    throw 'building the installer failed'
}
Set-Signature $setup

$name = "Grana-$Version-windows-x64"
Copy-Item -LiteralPath $msi -Destination (Join-Path $Output "$name.msi") -Force
Copy-Item -LiteralPath $setup -Destination (Join-Path $Output "$name-setup.exe") -Force
Write-Host "installer: $(Join-Path $Output "$name-setup.exe")"

