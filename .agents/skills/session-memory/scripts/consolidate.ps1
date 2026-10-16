# Consolidate Memories - PowerShell Version
param([string]$Since = "", [string]$Topic = "")

$MemoryDir = if ($env:AGENT_MEMORY_DIR) { $env:AGENT_MEMORY_DIR } else { ".agent-memory" }
if (-not (Test-Path $MemoryDir)) { Write-Host "No memories found."; exit 0 }

$AllMemories = @()
Get-ChildItem -Path $MemoryDir -Recurse -Filter "*.jsonl" -ErrorAction SilentlyContinue | ForEach-Object {
    Get-Content $_.FullName | ForEach-Object { try { $mem = $_ | ConvertFrom-Json; if($mem){$AllMemories+=$mem} } catch {} }
}
if ($AllMemories.Count -eq 0) { Write-Host "No memories found."; exit 0 }

if ($Since) { $Cutoff = [DateTime]::Parse($Since); $AllMemories = $AllMemories | Where-Object { [DateTimeOffset]::FromUnixTimeMilliseconds([int64]$_.ts).DateTime -ge $Cutoff } }
if ($Topic) { $AllMemories = $AllMemories | Where-Object { $_.topic -eq $Topic } }

$ByTopic = $AllMemories | Group-Object topic | Sort-Object Name
Write-Host "Memory Consolidation`nPeriod: $(if($Since){"since $Since"}else{"all time"})  Filter: $(if($Topic){"topic=$Topic"}else{"all topics"})`n"

foreach ($Group in $ByTopic) {
    $Timestamps = $Group.Group.ts | Sort-Object { [int64]$_ }
    $First = [DateTimeOffset]::FromUnixTimeMilliseconds([int64]$Timestamps[0]).DateTime.ToString('yyyy-MM-dd')
    $Last = [DateTimeOffset]::FromUnixTimeMilliseconds([int64]$Timestamps[-1]).DateTime.ToString('yyyy-MM-dd')
    $Tags = if ($Group.Group.tags) { ($Group.Group.tags | Where-Object {$_} | Sort-Object | Get-Unique | Select-Object -First 5) -join ', ' } else { "none" }
    $Latest = $Group.Group | Sort-Object ts -Descending | Select-Object -First 1
    $Preview = if ($Latest.content.Length -gt 60) { $Latest.content.Substring(0, 60) + "..." } else { $Latest.content }
    
    Write-Host "-- $($Group.Name) --`n  Count: $($Group.Count)`n  Date range: $First to $Last`n  Top tags: $Tags`n  Latest: $Preview`n"
}
Write-Host "Total: $($ByTopic.Count) topics, $($AllMemories.Count) memories"
