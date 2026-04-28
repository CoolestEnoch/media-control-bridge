$ErrorActionPreference = "Stop"
Set-Location "$PSScriptRoot\..\rust\media-control-bridge"
rustup target add x86_64-pc-windows-msvc | Out-Null
cargo build --release --target x86_64-pc-windows-msvc
New-Item -ItemType Directory -Force -Path "..\..\dist" | Out-Null
Copy-Item "target\x86_64-pc-windows-msvc\release\media-control-bridge.exe" "..\..\dist\media-control-bridge-x86_64-windows.exe" -Force
