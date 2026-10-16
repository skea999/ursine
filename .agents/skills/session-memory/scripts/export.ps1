# Export Memories - PowerShell Version
param([string]$o = "", [string]$Since = "")

$OutputFile = if ($o) { $o } else { "memory-backup-$(Get-Date -Format 'yyyyMMdd').json" }
$MemoryDir = if ($env:AGENT_MEMORY_DIR) { $env:AGENT_MEMORY_DIR } else { ".agent-memory" }
if (-not (Test-Path $MemoryDir)) { Write-Host "No memories found."; exit 0 }

$AllMemories = @()
Get-ChildItem -Path $MemoryDir -Recurse -Filter "*.jsonl" -ErrorAction SilentlyContinue | ForEach-Object {
    Get-Content $_.FullName | ForEach-Object { try { $mem = $_ | ConvertFrom-Json; if($mem){$AllMemories+=$mem} } catch {} }
}
if ($AllMemories.Count -eq 0) { Write-Host "No memories found."; exit 0 }

if ($Since) { $Cutoff = [DateTime]::Parse($Since); $AllMemories = $AllMemories | Where-Object { [DateTimeOffset]::FromUnixTimeMilliseconds([int64]$_.ts).DateTime -ge $Cutoff } }

$AllMemories | ConvertTo-Json -Depth 10 | Set-Content -Path $OutputFile -Encoding UTF8
Write-Host "Exported $($AllMemories.Count) memories to $OutputFile"
