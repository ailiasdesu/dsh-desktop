param([switch]$BundleOnly)
$ErrorActionPreference = 'Stop'
$buildRepo = Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $buildRepo
$resultFile = Join-Path $buildRepo 'target/release-build-result.json'
if (Test-Path -LiteralPath $resultFile) { Remove-Item -LiteralPath $resultFile }
$code = 1
try {
    if ($BundleOnly) {
        & (Join-Path $buildRepo 'runtime/node.exe') (Join-Path $buildRepo 'node_modules/@tauri-apps/cli/tauri.js') bundle --bundles nsis
    } else {
        & (Join-Path $buildRepo 'runtime/node.exe') (Join-Path $buildRepo 'node_modules/@tauri-apps/cli/tauri.js') build --bundles nsis
    }
    $code = $LASTEXITCODE
} finally {
    $result = @{exitCode=$code;finishedAt=(Get-Date).ToString('o')} | ConvertTo-Json -Compress
    [System.IO.File]::WriteAllText($resultFile,$result,[System.Text.UTF8Encoding]::new($false))
}
exit $code
