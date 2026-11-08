@echo off
where cargo >nul 2>nul
if errorlevel 1 (
  echo Falta Rust. Instalalo desde https://rustup.rs y volve a ejecutar este archivo.
  pause
  exit /b 1
)
cargo build --release
if errorlevel 1 ( pause & exit /b 1 )
if not exist dist mkdir dist
copy /Y target\release\minicord.exe dist\minicord.exe
echo.
echo Listo: dist\minicord.exe
pause
