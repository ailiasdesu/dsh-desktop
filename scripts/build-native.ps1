$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/native-hash.ps1"
$nativeRepo = Split-Path -Parent $PSScriptRoot
Push-Location $nativeRepo
try {
    cargo build --locked --release --manifest-path native-helper/Cargo.toml
    if ($LASTEXITCODE -ne 0) { throw 'Native helper build failed' }
    cargo build --locked --release --manifest-path native-addon/Cargo.toml
    if ($LASTEXITCODE -ne 0) { throw 'Native reader build failed' }
    $nativeOutput = Join-Path $nativeRepo 'native'
    New-Item -ItemType Directory -Force -Path $nativeOutput | Out-Null
    Copy-Item -LiteralPath 'native-helper/target/release/dsh-native-helper.exe' -Destination (Join-Path $nativeOutput 'dsh-native-helper.exe') -Force
    Copy-Item -LiteralPath 'native-addon/target/release/dsh_native_reader.dll' -Destination (Join-Path $nativeOutput 'dsh_native_reader.node') -Force
    $nativeFiles = @(Get-ChildItem -LiteralPath $nativeOutput -File | Where-Object { $_.Extension -in @('.exe','.node') } | ForEach-Object {
        @{name=$_.Name;bytes=$_.Length;sha256=(Get-NativeSHA256 $_.FullName)}
    })
    $nativeManifest = @{protocol=1;verifiedKernels=@('0.1.2-rc.1');files=$nativeFiles} | ConvertTo-Json -Depth 5
    [System.IO.File]::WriteAllText((Join-Path $nativeOutput 'manifest.json'), $nativeManifest, [System.Text.UTF8Encoding]::new($false))
} finally { Pop-Location }
