@echo off
setlocal
cd /d "%~dp0"

where npm >nul 2>nul
if errorlevel 1 (
  echo [Sepia] Node.js and npm are required.
  pause
  exit /b 1
)

where cargo >nul 2>nul
if errorlevel 1 (
  echo [Sepia] Rust and Cargo are required.
  pause
  exit /b 1
)

if not exist "frontend\node_modules\" (
  echo [Sepia] Installing frontend dependencies...
  call npm ci --prefix frontend
  if errorlevel 1 (
    echo [Sepia] Frontend dependency installation failed.
    pause
    exit /b 1
  )
)

start "Sepia Gateway" /D "%~dp0" cmd /k cargo run --manifest-path backend/Cargo.toml
start "Sepia Frontend" /D "%~dp0" cmd /k npm run dev --prefix frontend

echo [Sepia] Gateway:  http://127.0.0.1:3425
echo [Sepia] Frontend: http://localhost:5173
echo [Sepia] Close the two service windows to stop Sepia.
endlocal
