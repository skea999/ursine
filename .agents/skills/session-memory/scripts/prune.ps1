# Prune Old Memories - PowerShell Version
param([int]$Days = 30)

$MemoryDir = if ($env:AGENT_MEMORY_DIR) { $env:AGENT_MEMORY_DIR } else { ".agent-memory" }; $ArchiveDir = Join-Path $MemoryDir "archive"
if (-not (Test-Path $MemoryDir)) { Write-Host "No memories found."; exit 0 }

$Cutoff = (Get-Date).AddDays(-$Days)
$AllMemories = @()
Get-ChildItem -Path $MemoryDir -Recurse -Filter "*.jsonl" -ErrorAction SilentlyContinue | ForEach-Object {
    Get-Content $_.FullName | ForEach-Object { try { $mem = $_ | ConvertFrom-Json; if($mem){$mem._file=$_.FullName;$AllMemories+=$mem} } catch {} }
}
if ($AllMemories.Count -eq 0) { Write-Host "No memories found."; exit 0 }

$OldMemories = $AllMemories | Where-Object { [DateTimeOffset]::FromUnixTimeMilliseconds([int64]$_.ts).DateTime -lt $Cutoff }
if ($OldMemories.Count -eq 0) { Write-Host "No memories older than $Days days to prune."; exit 0 }

Write-Host "Found $($OldMemories.Count) memories older than $Days days"
Write-Host "Archive directory: $ArchiveDir`n"
Write-Host "Prune command not fully implemented yet."
Write-Host "Manual: Move old .jsonl files to $ArchiveDir"
