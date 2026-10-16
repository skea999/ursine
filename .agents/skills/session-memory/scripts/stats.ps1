# Memory Statistics - PowerShell Version
$MemoryDir = if ($env:AGENT_MEMORY_DIR) { $env:AGENT_MEMORY_DIR } else { ".agent-memory" }
if (-not (Test-Path $MemoryDir)) { Write-Host "No memories found."; exit 0 }

$AllMemories = @()
Get-ChildItem -Path $MemoryDir -Recurse -Filter "*.jsonl" -ErrorAction SilentlyContinue | ForEach-Object {
    Get-Content $_.FullName | ForEach-Object { try { $mem = $_ | ConvertFrom-Json; if($mem){$AllMemories+=$mem} } catch {} }
}
if ($AllMemories.Count -eq 0) { Write-Host "No memories found."; exit 0 }

$Total = $AllMemories.Count
$Timestamps = $AllMemories.ts | Sort-Object { [int64]$_ }
$First = [DateTimeOffset]::FromUnixTimeMilliseconds([int64]$Timestamps[0]).DateTime
$Last = [DateTimeOffset]::FromUnixTimeMilliseconds([int64]$Timestamps[-1]).DateTime
$Days = ([TimeSpan]($Last - $First)).Days + 1
$PerDay = if ($Days -gt 0) { [math]::Round($Total / $Days, 2) } else { $Total }

$ByTopic = $AllMemories | Group-Object topic | Sort-Object Count -Descending
$ByImportance = $AllMemories | Group-Object importance | Sort-Object { @{ critical = 4; high = 3; normal = 2; low = 1 }[$_.Name] } -Descending

Write-Host "Memory Statistics`n"
Write-Host "Total memories: $Total"
Write-Host "Date range: $($First.ToString('yyyy-MM-dd')) to $($Last.ToString('yyyy-MM-dd')) ($Days days)"
Write-Host "Average per day: $PerDay`n"
Write-Host "By Importance:"
foreach ($Group in $ByImportance) { Write-Host "  $($Group.Name): $($Group.Count)" }
Write-Host "`nBy Topic:"
foreach ($Group in $ByTopic | Select-Object -First 10) { Write-Host "  $($Group.Name): $($Group.Count)" }
if ($ByTopic.Count -gt 10) { Write-Host "  ... and $($ByTopic.Count - 10) more" }
