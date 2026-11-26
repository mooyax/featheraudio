# Builds the browser demo (WebAssembly) into web\dist.
#
#   powershell -ExecutionPolicy Bypass -File D:\develop\featheraudio\web\build-web.ps1
#   powershell -ExecutionPolicy Bypass -File D:\develop\featheraudio\web\build-web.ps1 -Serve
#
#   -Serve   also start a local web server and open http://localhost:8000/ (needs Python)
#
# Needs rustup / cargo. The first run installs the wasm32 target and wasm-bindgen-cli.
param([switch]$Serve)

$ErrorActionPreference = 'Stop'
$web = Split-Path -Parent $MyInvocation.MyCommand.Path
$dist = Join-Path $web 'dist'
Set-Location $web

rustup target add wasm32-unknown-unknown
if ($LASTEXITCODE -ne 0) { throw 'rustup target add failed' }

cargo build --release --target wasm32-unknown-unknown
if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }

# wasm-bindgen-cli must be exactly the version of the wasm-bindgen crate (Cargo.toml pins it)
$want = '0.2.130'
$have = ''
if (Get-Command wasm-bindgen -ErrorAction SilentlyContinue) { $have = ((wasm-bindgen --version) -split ' ')[1] }
if ($have -ne $want) {
  cargo install wasm-bindgen-cli --version $want --locked
  if ($LASTEXITCODE -ne 0) { throw 'installing wasm-bindgen-cli failed' }
}

wasm-bindgen --target no-modules --no-typescript --out-dir $dist (Join-Path $web 'target\wasm32-unknown-unknown\release\featheraudio_web.wasm')
if ($LASTEXITCODE -ne 0) { throw 'wasm-bindgen failed' }
Copy-Item (Join-Path $web 'index.html') $dist -Force
Write-Host "== built $dist"

if ($Serve) {
  Start-Process 'http://localhost:8000/'
  python -m http.server 8000 --directory $dist
}
