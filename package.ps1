# 打包脚本:把 cursor-server + 前端 dist + .env + 启动脚本 组装到 output/
# 用法:  pwsh ./package.ps1
# 产物:  output/
#          ├── cursor-server.exe
#          ├── dist/                  (前端静态文件)
#          ├── .env                   (配置,首次运行从 .env.example 复制)
#          ├── start.bat              (双击启动)
#          └── data/                  (SQLite 数据库,自动生成)

$ErrorActionPreference = "Stop"
$root = $PSScriptRoot
Set-Location $root

Write-Host "==> [1/4] 编译 release cursor-server" -ForegroundColor Cyan
cargo build --release --package cursor-server --bin cursor-server

Write-Host "==> [2/4] 编译前端 dist" -ForegroundColor Cyan
Push-Location apps\desktop
npm run build
Pop-Location

Write-Host "==> [3/4] 组装 output/" -ForegroundColor Cyan
$out = Join-Path $root "output"
if (Test-Path $out) { Remove-Item -Recurse -Force $out }
New-Item -ItemType Directory -Path $out | Out-Null
New-Item -ItemType Directory -Path (Join-Path $out "data") | Out-Null

Copy-Item "target\release\cursor-server.exe" $out
Copy-Item -Recurse "apps\desktop\dist" (Join-Path $out "dist")

# .env:已存在则保留,否则从模板复制
$envFile = Join-Path $out ".env"
if (-not (Test-Path $envFile)) {
    Copy-Item ".env.example" $envFile
}

# start.bat
$bat = @'
@echo off
chcp 65001 >nul
cd /d "%~dp0"
echo Starting cursor-byok...
echo Dashboard: http://127.0.0.1:3000/__byok-api__/
start "" http://127.0.0.1:3000/__byok-api__/
cursor-server.exe
pause
'@
Set-Content -Path (Join-Path $out "start.bat") -Value $bat -Encoding ASCII

Write-Host "==> [4/4] 完成" -ForegroundColor Green
Write-Host "产物目录: $out"
Write-Host "把整个 output/ 文件夹拷到别的电脑,双击 start.bat 即可。"
Write-Host "配置在 output\.env,先改 CURSOR_EXTERNAL_API_KEY 和 CURSOR_LISTEN_ADDR。"
