param([Parameter(Mandatory=$true)][string]$Path,[Parameter(Mandatory=$true)][string]$ExpectedVersion)
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/native-hash.ps1"
$version = [System.Diagnostics.FileVersionInfo]::GetVersionInfo($Path).ProductVersion
if ($version -ne $ExpectedVersion) { throw "Desktop binary version mismatch: expected $ExpectedVersion, got $version" }
@{version=$version;sha256=(Get-NativeSHA256 $Path)} | ConvertTo-Json -Compress
