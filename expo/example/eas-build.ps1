# Builds this test app with EAS (preview profile: internal distribution) after
# ..\build-mobile.ps1. Copies the module in, and uploads only this folder (EAS_NO_VCS):
# with git's rules, the module (git-ignored here) would not be uploaded.
#
#   powershell -ExecutionPolicy Bypass -File .\eas-build.ps1            (iOS)
#   powershell -ExecutionPolicy Bypass -File .\eas-build.ps1 android
param([string]$Platform = 'ios')
$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
& (Join-Path $here 'setup.ps1')
Set-Location $here
$env:EAS_NO_VCS = '1'
npx.cmd eas-cli build -p $Platform --profile preview
