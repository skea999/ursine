# Session Memory Wrapper for PowerShell
# Usage: .\memory.ps1 <command> [args...]

$env:AGENT_MEMORY_DIR = if ($env:AGENT_MEMORY_DIR) { $env:AGENT_MEMORY_DIR } else { ".agent-memory" }

$COMMAND = $args[0]
$ARGS = if ($args.Length -gt 1) { $args[1..($args.Length - 1)] } else { @() }

function Convert-Flags {
    param([string[]]$InputArgs)
    $out = @()
    foreach ($a in $InputArgs) {
        if ($a -match '^--') {
            $name = $a.Substring(2)
            $parts = $name -split '-'
            $converted = ($parts | ForEach-Object { $_.Substring(0,1).ToUpper() + $_.Substring(1).ToLower() }) -join ''
            $out += "-$converted"
        } else {
            $out += $a
        }
    }
    return $out
}
$ARGS = Convert-Flags $ARGS

if (-not $COMMAND) {
    Write-Host "Session Memory - Persistent context for AI agents"
    Write-Host ""
    Write-Host "Usage: .\memory.ps1 <command> [args...]"
    Write-Host ""
    Write-Host "Commands:"
    Write-Host "  save <topic> <content> [tags...]   Save a memory"
    Write-Host "  recall <query> [--limit N]         Search memories"
    Write-Host "  context [--days N]                 Load session context"
    Write-Host "  daily [YYYY-MM-DD]                 View day's memories"
    Write-Host "  topics                             List all topics"
    Write-Host "  consolidate [--since DATE]         Group by topic"
    Write-Host "  stats                              Memory statistics"
    Write-Host "  edit <ts> --content <new>          Edit memory"
    Write-Host "  export [-o file.json]              Export memories"
    Write-Host "  import <file.json]                 Import memories"
    Write-Host "  prune [days]                       Archive old memories"
    Write-Host ""
    Write-Host "Examples:"
    Write-Host "  .\memory.ps1 save decision 'Chose Postgres over SQLite' database"
    Write-Host "  `$env:MEMORY_IMPORTANCE='high'; .\memory.ps1 save insight 'Users prefer dark mode'"
    Write-Host "  .\memory.ps1 context --days 3"
    Write-Host "  .\memory.ps1 recall 'database migration' --limit 5"
    exit 0
}

switch ($COMMAND) {
    "save" { & "$PSScriptRoot\save.ps1" @ARGS }
    "recall" { & "$PSScriptRoot\recall.ps1" @ARGS }
    "context" {
        $DaysParam = if ($ARGS -contains '-Days') { $ARGS[$ARGS.IndexOf('-Days') + 1] } else { 3 }
        $LimitParam = if ($ARGS -contains '-Limit') { $ARGS[$ARGS.IndexOf('-Limit') + 1] } else { 20 }
        & "$PSScriptRoot\context.ps1" -Days $DaysParam -Limit $LimitParam
    }
    "daily" { & "$PSScriptRoot\daily.ps1" @ARGS }
    "topics" { & "$PSScriptRoot\topics.ps1" }
    "consolidate" { & "$PSScriptRoot\consolidate.ps1" @ARGS }
    "stats" { & "$PSScriptRoot\stats.ps1" }
    "edit" { & "$PSScriptRoot\edit.ps1" @ARGS }
    "export" { & "$PSScriptRoot\export.ps1" @ARGS }
    "import" { & "$PSScriptRoot\import.ps1" @ARGS }
    "prune" { & "$PSScriptRoot\prune.ps1" @ARGS }
    default { Write-Host "Unknown command: $COMMAND"; exit 1 }
}
