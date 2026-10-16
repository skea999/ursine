# Topics Index - PowerShell Version
$MemoryDir = if ($env:AGENT_MEMORY_DIR) { $env:AGENT_MEMORY_DIR } else { ".agent-memory" }
if (-not (Test-Path $MemoryDir)) { Write-Host "No memories found."; exit 0 }

$AllMemories = @()
Get-ChildItem -Path $MemoryDir -Recurse -Filter "*.jsonl" -ErrorAction SilentlyContinue | ForEach-Object {
    Get-Content $_.FullName | ForEach-Object { try { $mem = $_ | ConvertFrom-Json; if($mem){$AllMemories+=$mem} } catch {} }
}
if ($AllMemories.Count -eq 0) { Write-Host "No memories found."; exit 0 }

$ByTopic = $AllMemories | Group-Object topic | Sort-Object Count -Descending
Write-Host "Topics Index`n"
foreach ($Group in $ByTopic) {
    $Latest = ($Group.Group | Sort-Object ts -Descending | Select-Object -First 1)
    $Date = [DateTimeOffset]::FromUnixTimeMilliseconds([int64]$Latest.ts).DateTime.ToString('yyyy-MM-dd')
    Write-Host "  $($Group.Name): $($Group.Count) memories (latest: $Date)"
}
Write-Host "`nTotal: $($ByTopic.Count) topics, $($AllMemories.Count) memories"
