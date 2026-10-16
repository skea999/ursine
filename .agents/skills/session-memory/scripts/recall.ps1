# Recall Memories - PowerShell Version
param([Parameter(Position=0)][string]$Query = "", [int]$Limit = 10, [string]$Topic = "", [string]$Importance = "")

$MemoryDir = if ($env:AGENT_MEMORY_DIR) { $env:AGENT_MEMORY_DIR } else { ".agent-memory" }
if (-not (Test-Path $MemoryDir)) { Write-Host "No memories found."; exit 0 }

$AllMemories = @()
Get-ChildItem -Path $MemoryDir -Recurse -Filter "*.jsonl" -ErrorAction SilentlyContinue | ForEach-Object {
    Get-Content $_.FullName | ForEach-Object { try { $mem = $_ | ConvertFrom-Json; if($mem){$AllMemories+=$mem} } catch {} }
}
if ($AllMemories.Count -eq 0) { Write-Host "No memories found."; exit 0 }

if ($Topic) { $AllMemories = $AllMemories | Where-Object { $_.topic -eq $Topic } }
if ($Importance) { $AllMemories = $AllMemories | Where-Object { $_.importance -eq $Importance } }

if ($Query) {
    $Keywords = $Query.ToLower().Split(' ', [StringSplitOptions]::RemoveEmptyEntries)
    $AllMemories = $AllMemories | Where-Object {
        $Text = "$($_.content.ToLower()) $($_.topic.ToLower()) $(if($_.tags){($_.tags -join ' ').ToLower()})"
        $MatchCount = 0; foreach($k in $Keywords){if($Text -like "*$k*"){$MatchCount++}}; $MatchCount -eq $Keywords.Count
    }
}

$ImportanceOrder = @{ critical = 4; high = 3; normal = 2; low = 1 }
$Results = $AllMemories | Sort-Object { -($ImportanceOrder[$_.importance] -as [int]), -([int64]$_.ts) } | Select-Object -First $Limit

if ($Results.Count -eq 0) { Write-Host "No memories found matching your criteria."; exit 0 }

Write-Host "Recall Results (found $($Results.Count))`n"
foreach ($Memory in $Results) {
    $Date = [DateTimeOffset]::FromUnixTimeMilliseconds([int64]$Memory.ts).DateTime.ToString('yyyy-MM-dd HH:mm')
    $Imp = if ($Memory.importance -and $Memory.importance -ne 'normal') { " [$($Memory.importance.ToUpper())]" } else { "" }
    $Tags = if ($Memory.tags -and $Memory.tags.Count -gt 0) { " #$(($Memory.tags -join ' #'))" } else { "" }
    Write-Host "[$Date] [$($Memory.topic)]$Imp $($Memory.content)$Tags"
}
