# Puts the Expo module (built with ..\build-mobile.ps1 first) into modules\feather-audio, as
# in a game, and the test sounds into assets\.
$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$mod = Split-Path -Parent $here
$dst = Join-Path $here 'modules\feather-audio'
if (Test-Path $dst) { Remove-Item -Recurse -Force $dst }
New-Item -ItemType Directory -Force -Path $dst, (Join-Path $here 'assets') | Out-Null
foreach ($f in 'package.json', 'expo-module.config.json', 'FeatherAudio.podspec', 'src', 'ios', 'android') {
  Copy-Item -Recurse (Join-Path $mod $f) $dst
}
# Android keeps assets as resources named without the extension: the names must differ
foreach ($e in 'ogg', 'mp3', 'mp2') {
  Copy-Item (Join-Path $mod "..\tests\data\stereo.$e") (Join-Path $here "assets\test_$e.$e")
}
Write-Host 'module and test sounds in place'
