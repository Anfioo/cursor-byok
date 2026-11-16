@echo off
rem Entry point for the cursor-proxy plugin.
rem
rem Scoop replaces the install folder (apps\cursor-proxy\current) on every
rem update, so everything the server writes -- .env and the SQLite file --
rem lives in the persist folder instead. The script moves there before
rem launching, which is also the folder cursor-server takes its .env from.
rem
rem The app sets PLUGIN_ID, PLUGIN_DIR and PLUGIN_PERSIST. The fallbacks below
rem keep a double-click from the install folder working.
rem
rem The server stays in the foreground on purpose: the app streams a script's
rem output until the script exits, so a detached daemon would be unkillable from
rem the settings page. stop.cmd covers a copy that outlived the app.
setlocal
if not defined PLUGIN_ID set "PLUGIN_ID=cursor-proxy"
if not defined PLUGIN_DIR set "PLUGIN_DIR=%~dp0"
if "%PLUGIN_DIR:~-1%"=="\" set "PLUGIN_DIR=%PLUGIN_DIR:~0,-1%"
if not defined PLUGIN_PERSIST set "PLUGIN_PERSIST=%~dp0"
if "%PLUGIN_PERSIST:~-1%"=="\" set "PLUGIN_PERSIST=%PLUGIN_PERSIST:~0,-1%"

set "EXE=%PLUGIN_DIR%\cursor-server.exe"
if not exist "%EXE%" (
  echo [cursor-proxy] cursor-server.exe is not in %PLUGIN_DIR% 1>&2
  exit /b 1
)

if not exist "%PLUGIN_PERSIST%" mkdir "%PLUGIN_PERSIST%"
rem The server would fail to open its SQLite file before it could create the
rem folder, so it is made here.
if not exist "%PLUGIN_PERSIST%\data" mkdir "%PLUGIN_PERSIST%\data"

rem Read what the config window may already have written. A value of "" means
rem the file is missing, or was created by a save that never set the key.
set "CURSOR_KEY="
set "CONSOLE_DIR="
if exist "%PLUGIN_PERSIST%\.env" for /f "usebackq eol=# tokens=1,* delims==" %%a in ("%PLUGIN_PERSIST%\.env") do (
  if /i "%%a"=="CURSOR_EXTERNAL_API_KEY" set "CURSOR_KEY=%%b"
  if /i "%%a"=="CURSOR_CONSOLE_DIR" set "CONSOLE_DIR=%%b"
)

if defined CURSOR_KEY (
  echo [cursor-proxy] using the access key from .env
) else (
  echo [cursor-proxy] .env has no CURSOR_EXTERNAL_API_KEY yet - generating one.
  powershell -NoProfile -ExecutionPolicy Bypass -File "%PLUGIN_DIR%\init-env.ps1"
  if errorlevel 1 (
    echo [cursor-proxy] could not generate .env; PowerShell said no. 1>&2
    exit /b 1
  )
)

rem The server resolves CURSOR_CONSOLE_DIR against its working directory, which
rem is the persist folder, while the console ships with the app. A relative
rem value therefore becomes relative to the install folder -- the settings
rem window describes the field that way.
if not defined CONSOLE_DIR set "CONSOLE_DIR=dist"
if "%CONSOLE_DIR:~1,1%"==":" (
  set "CURSOR_CONSOLE_DIR=%CONSOLE_DIR%"
) else (
  set "CURSOR_CONSOLE_DIR=%PLUGIN_DIR%\%CONSOLE_DIR%"
)

cd /d "%PLUGIN_PERSIST%"
echo [cursor-proxy] starting cursor-server from %CD%
"%EXE%"
exit /b %errorlevel%
