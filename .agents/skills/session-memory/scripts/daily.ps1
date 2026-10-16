# Daily View - PowerShell Version
param([string]$Date = "")

$MemoryDir = if ($env:AGENT_MEMORY_DIR) { $env:AGENT_MEMORY_DIR } else { ".agent-memory" }
if (-not $Date) { $Date = Get-Date -Format 'yyyy-MM-dd' }

try {
    $ParsedDate = [DateTime]::Parse($Date)
    $Year = $ParsedDate.ToString('yyyy'); $Month = $ParsedDate.ToString('MM'); $Day = $ParsedDate.ToString('dd')
} catch { Write-Host "Invalid date format. Use YYYY-MM-DD"; exit 1 }

$FilePath = Join-Path $MemoryDir "$Year\$Month\$Day.jsonl"
if (-not (Test-Path $FilePath)) { Write-Host "No memories found for $Date"; exit 0 }

$Memories = @()
Get-Content $FilePath | ForEach-Object { try { $mem = $_ | ConvertFrom-Json; if($mem){$Memories+=$mem} } catch {} }
if ($Memories.Count -eq 0) { Write-Host "No memories found for $Date"; exit 0 }

Write-Host "Daily View: $Date`n"
$Memories = $Memories | Sort-Object ts
foreach ($Memory in $Memories) {
    $Time = [DateTimeOffset]::FromUnixTimeMilliseconds([int64]$Memory.ts).DateTime.ToString('HH:mm')
    $Imp = if ($Memory.importance -and $Memory.importance -ne 'normal') { " [$($Memory.importance.ToUpper())]" } else { "" }
    $Tags = if ($Memory.tags -and $Memory.tags.Count -gt 0) { " #$(($Memory.tags -join ' #'))" } else { "" }
    Write-Host "[$Time] [$($Memory.topic)]$Imp $($Memory.content)$Tags"
}
Write-Host "`nTotal: $($Memories.Count) memories"
