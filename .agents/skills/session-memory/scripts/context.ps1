# Load Session Context - PowerShell Version
param([int]$Days = 3, [int]$Limit = 20)

$MemoryDir = if ($env:AGENT_MEMORY_DIR) { $env:AGENT_MEMORY_DIR } else { ".agent-memory" }
if (-not (Test-Path $MemoryDir)) { Write-Host "No memories found."; exit 0 }

$Cutoff = (Get-Date).AddDays(-$Days)
$AllMemories = @()
Get-ChildItem -Path $MemoryDir -Recurse -Filter "*.jsonl" -ErrorAction SilentlyContinue | ForEach-Object {
    Get-Content $_.FullName | ForEach-Object { try { $AllMemories += $_ | ConvertFrom-Json } catch {} }
}

if ($AllMemories.Count -eq 0) { Write-Host "No memories found."; exit 0 }

$Recent = $AllMemories | Where-Object { [DateTimeOffset]::FromUnixTimeMilliseconds([int64]$_.ts).DateTime -ge $Cutoff }
$Important = $AllMemories | Where-Object { $_.importance -eq 'high' -or $_.importance -eq 'critical' }

$Combined = @(); $Seen = @{}
$Recent | ForEach-Object { if (-not $Seen.ContainsKey($_.ts)) { $Combined += $_; $Seen[$_.ts] = $true } }
$Important | ForEach-Object { if (-not $Seen.ContainsKey($_.ts)) { $Combined += $_; $Seen[$_.ts] = $true } }

$ImportanceOrder = @{ critical = 4; high = 3; normal = 2; low = 1 }
$Combined = $Combined | Sort-Object { -($ImportanceOrder[$_.importance] -as [int]), -([int64]$_.ts) } | Select-Object -First $Limit

if ($Combined.Count -eq 0) { Write-Host "No recent memories (last $Days days)."; exit 0 }

Write-Host "Session Context (last $Days days + important items)`n"
$ByDate = $Combined | Group-Object { [DateTimeOffset]::FromUnixTimeMilliseconds([int64]$_.ts).DateTime.ToString('yyyy-MM-dd') } | Sort-Object Name -Descending

foreach ($Group in $ByDate) {
    Write-Host "-- $($Group.Name) --"
    foreach ($Memory in $Group.Group) {
        $Time = [DateTimeOffset]::FromUnixTimeMilliseconds([int64]$Memory.ts).DateTime.ToString('HH:mm')
        $Imp = if ($Memory.importance -and $Memory.importance -ne 'normal') { " [$($Memory.importance.ToUpper())]" } else { "" }
        Write-Host "  [$Time] [$($Memory.topic)]$Imp $($Memory.content)"
    }
    Write-Host ""
}
Write-Host "Total: $($Combined.Count) entries"
