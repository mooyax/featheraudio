# Puts the Expo module (built with ..\build-mobile.ps1 first) into modules\feather-audio, as
# in a game. (The app reads its sounds from sounds\.)
$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$mod = Split-Path -Parent $here
$dst = Join-Path $here 'modules\feather-audio'
if (Test-Path $dst) { Remove-Item -Recurse -Force $dst }
New-Item -ItemType Directory -Force -Path $dst | Out-Null
foreach ($f in 'package.json', 'expo-module.config.json', 'FeatherAudio.podspec', 'src', 'ios', 'android') {
  Copy-Item -Recurse (Join-Path $mod $f) $dst
}
Write-Host 'module in place'
