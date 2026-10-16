# Save a Memory - PowerShell Version
param(
    [Parameter(Mandatory=$true, Position=0)][string]$Topic,
    [Parameter(Mandatory=$true, Position=1)][string]$Content,
    [Parameter(Position=2)][string[]]$Tags = @()
)

$Importance = if ($env:MEMORY_IMPORTANCE) { $env:MEMORY_IMPORTANCE } else { "normal" }
$MemoryDir = if ($env:AGENT_MEMORY_DIR) { $env:AGENT_MEMORY_DIR } else { ".agent-memory" }

$Year = Get-Date -Format "yyyy"; $Month = Get-Date -Format "MM"; $Day = Get-Date -Format "dd"
$DirPath = Join-Path $MemoryDir "$Year\$Month"
New-Item -ItemType Directory -Force -Path $DirPath | Out-Null

$Entry = [PSCustomObject]@{
    ts = [int64]([DateTimeOffset]::Now.ToUnixTimeMilliseconds())
    topic = $Topic; content = $Content; tags = $Tags; importance = $Importance
}

$FilePath = Join-Path $DirPath "$Day.jsonl"
$EntryJson = $Entry | ConvertTo-Json -Compress
Add-Content -Path $FilePath -Value $EntryJson -Encoding UTF8 -NoNewline
Add-Content -Path $FilePath -Value "`n" -Encoding UTF8

$Preview = if ($Content.Length -gt 80) { $Content.Substring(0, 80) + "..." } else { $Content }
Write-Host "Memory saved: [$Topic] $Preview"
