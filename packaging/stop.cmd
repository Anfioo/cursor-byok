@echo off
rem Stop script for the cursor-proxy plugin.
rem
rem Stopping already kills the process tree the app started, so this script is
rem for what that cannot see: a cursor-server that outlived the app, or one
rem started by hand. It looks for it the way a user would -- by the port the
rem server listens on -- and only kills it when the process really is
rem cursor-server, so a port that happens to be shared never takes an unrelated
rem program down with it.
rem
rem The settings live in the persist folder; a copy next to the script is the
rem fallback for a manual run from the install folder.
setlocal
if not defined PLUGIN_PERSIST set "PLUGIN_PERSIST=%~dp0"
if "%PLUGIN_PERSIST:~-1%"=="\" set "PLUGIN_PERSIST=%PLUGIN_PERSIST:~0,-1%"
cd /d "%~dp0"

set "ENV_FILE=%PLUGIN_PERSIST%\.env"
if not exist "%ENV_FILE%" set "ENV_FILE=%~dp0.env"

rem CURSOR_LISTEN_ADDR holds "host:port". A hand-edited line may carry an inline
rem comment, which is part of the value here, so drop everything from the first
rem space or hash before reading the port off the tail of "host:port".
set "CURSOR_ADDR="
if exist "%ENV_FILE%" for /f "usebackq eol=# tokens=1,* delims==" %%a in ("%ENV_FILE%") do (
  if /i "%%a"=="CURSOR_LISTEN_ADDR" set "CURSOR_ADDR=%%b"
)

set "CURSOR_PORT="
if defined CURSOR_ADDR for /f "tokens=1 delims=# " %%c in ("%CURSOR_ADDR%") do (
  for /f "tokens=2 delims=:" %%d in ("%%c") do set "CURSOR_PORT=%%d"
)
rem No .env (or an address with no port): fall back to the address cursor-server
rem itself defaults to. The process-name check below is what keeps a fallback
rem from touching anything that is not ours.
if not defined CURSOR_PORT set "CURSOR_PORT=3000"

echo [cursor-proxy] looking for a cursor-server listener on port %CURSOR_PORT%
powershell -NoProfile -ExecutionPolicy Bypass -Command "$port = [int]'%CURSOR_PORT%'; $stopped = 0; Get-NetTCPConnection -LocalPort $port -State Listen -ErrorAction SilentlyContinue | Select-Object -ExpandProperty OwningProcess -Unique | ForEach-Object { $proc = Get-Process -Id $_ -ErrorAction SilentlyContinue; if ($proc -and $proc.ProcessName -like 'cursor-server*') { Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue; Write-Host ('[cursor-proxy] stopped ' + $proc.ProcessName + ' (pid ' + $proc.Id + ')'); $stopped++ } }; if ($stopped -eq 0) { Write-Host '[cursor-proxy] nothing to stop' }"
if errorlevel 1 echo [cursor-proxy] could not read port %CURSOR_PORT% from .env; the tree kill still runs. 1>&2

rem Always succeed: the app kills the process tree either way, and a stop script
rem that fails would only add noise to the log.
exit /b 0
