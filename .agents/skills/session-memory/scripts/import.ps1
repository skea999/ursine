# Import Memories - PowerShell Version
param([Parameter(Position=0,Mandatory=$true)][string]$InputFile, [switch]$DryRun)

$MemoryDir = if ($env:AGENT_MEMORY_DIR) { $env:AGENT_MEMORY_DIR } else { ".agent-memory" }
if (-not (Test-Path $InputFile)) { Write-Host "File not found: $InputFile"; exit 1 }

$Existing = @()
if (Test-Path $MemoryDir) {
    Get-ChildItem -Path $MemoryDir -Recurse -Filter "*.jsonl" -ErrorAction SilentlyContinue | ForEach-Object {
        Get-Content $_.FullName | ForEach-Object { try { $mem = $_ | ConvertFrom-Json; if($mem){$Existing+=$mem} } catch {} }
    }
}
$ExistingTimestamps = $Existing.ts
$ImportData = Get-Content $InputFile -Encoding UTF8 | ConvertFrom-Json
$ImportCount = 0; $SkipCount = 0

foreach ($Entry in $ImportData) {
    if ($ExistingTimestamps -contains $Entry.ts) { $SkipCount++; continue }
    if ($DryRun) { $ImportCount++; continue }
    
    $Year = [DateTimeOffset]::FromUnixTimeMilliseconds([int64]$Entry.ts).DateTime.ToString('yyyy')
    $Month = [DateTimeOffset]::FromUnixTimeMilliseconds([int64]$Entry.ts).DateTime.ToString('MM')
    $Day = [DateTimeOffset]::FromUnixTimeMilliseconds([int64]$Entry.ts).DateTime.ToString('dd')
    $DirPath = Join-Path $MemoryDir "$Year\$Month"; New-Item -ItemType Directory -Force -Path $DirPath | Out-Null
    $FilePath = Join-Path $DirPath "$Day.jsonl"
    $EntryJson = $Entry | ConvertTo-Json -Compress
    Add-Content -Path $FilePath -Value $EntryJson -Encoding UTF8 -NoNewline; Add-Content -Path $FilePath -Value "`n" -Encoding UTF8
    $ImportCount++
}

if ($DryRun) { Write-Host "Dry run: Would import $ImportCount memories, skip $SkipCount duplicates" }
else { Write-Host "Imported $ImportCount memories, skipped $SkipCount duplicates" }
