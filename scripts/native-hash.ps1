function Get-NativeSHA256([string]$LiteralPath) {
    $stream = [System.IO.File]::OpenRead($LiteralPath)
    $hash = [System.Security.Cryptography.SHA256]::Create()
    try { return ([System.BitConverter]::ToString($hash.ComputeHash($stream))).Replace('-', '').ToLowerInvariant() }
    finally { $hash.Dispose(); $stream.Dispose() }
}
