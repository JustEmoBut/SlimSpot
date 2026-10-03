# Builds SlimSpot and installs it for the current user (no admin rights needed).
#   .\install.ps1               build, copy, Start menu shortcut; autostart on first install
#   .\install.ps1 -NoAutostart  same, but never turn autostart on
#   .\install.ps1 -Release      full LTO build (~4 min per change instead of ~40 s; same RAM, 0.5 MB smaller exe)
#   .\install.ps1 -Uninstall    remove the copy, shortcut and autostart entry (keeps %APPDATA%\SlimSpot)
# Re-run after pulling changes to update the installed copy.
param([switch]$NoAutostart, [switch]$Uninstall, [switch]$Release)
$ErrorActionPreference = 'Stop'

$InstallDir = Join-Path $env:LOCALAPPDATA 'Programs\SlimSpot'
$Exe = Join-Path $InstallDir 'SlimSpot.exe'
$Shortcut = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\SlimSpot.lnk'
$RunKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
# Must match autostart.rs: "<exe>" --tray
$RunValue = "`"$Exe`" --tray"

function Stop-SlimSpot {
    $running = Get-Process -Name SlimSpot -ErrorAction SilentlyContinue
    if ($running) {
        Write-Host 'Stopping the running SlimSpot (playback will stop)...'
        $running | Stop-Process -Force
        $running | Wait-Process -Timeout 10 -ErrorAction SilentlyContinue
    }
}

if ($Uninstall) {
    Stop-SlimSpot
    Remove-ItemProperty -Path $RunKey -Name SlimSpot -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $Shortcut -ErrorAction SilentlyContinue
    if (Test-Path -LiteralPath $InstallDir) { Remove-Item -LiteralPath $InstallDir -Recurse -Force }
    Write-Host "Uninstalled. Login, settings and logs remain in $env:APPDATA\SlimSpot."
    return
}

Push-Location $PSScriptRoot
try {
    # The `fast` profile (Cargo.toml) skips LTO: measured 2026-10-04, same idle RAM as release.
    $BuildProfile = if ($Release) { 'release' } else { 'fast' }
    Write-Host "Building ($BuildProfile)..."
    cargo build --profile $BuildProfile
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed with exit code $LASTEXITCODE" }
    $Built = Join-Path $PSScriptRoot "target\$BuildProfile\slimspot.exe"
} finally {
    Pop-Location
}

$FirstInstall = -not (Test-Path -LiteralPath $Exe)
Stop-SlimSpot
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
Copy-Item -LiteralPath $Built -Destination $Exe -Force
Write-Host "Installed to $Exe"

$Shell = New-Object -ComObject WScript.Shell
$Link = $Shell.CreateShortcut($Shortcut)
$Link.TargetPath = $Exe
$Link.WorkingDirectory = $InstallDir
$Link.Description = 'SlimSpot'
$Link.Save()
Write-Host "Start menu shortcut: $Shortcut"

# Autostart: on for a first install unless -NoAutostart; afterwards only keep an existing entry
# pointing at this copy, so turning it off in the app sticks across updates.
$Existing = (Get-ItemProperty -Path $RunKey -Name SlimSpot -ErrorAction SilentlyContinue).SlimSpot
if ($NoAutostart) {
    Remove-ItemProperty -Path $RunKey -Name SlimSpot -ErrorAction SilentlyContinue
    Write-Host 'Autostart: off'
} elseif ($Existing -or $FirstInstall) {
    Set-ItemProperty -Path $RunKey -Name SlimSpot -Value $RunValue
    Write-Host 'Autostart: on (starts in the tray; toggle it in the app)'
} else {
    Write-Host 'Autostart: left off (turned off in the app earlier)'
}

Start-Process -FilePath $Exe
Write-Host 'SlimSpot started.'
