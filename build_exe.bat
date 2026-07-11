@echo off
setlocal
cd /d "%~dp0"

echo [Aoko.exe] Building C# loader...
if not "%~1"=="" echo [Aoko.exe] Version: %~1
echo.

cd Aoko
set "PUBLISH_DIR=bin\Release\net8.0-windows\win-x64\publish"
set "VERSION_ARG="
if not "%~1"=="" set "VERSION_ARG=-p:VersionPrefix=%~1"
if exist "%PUBLISH_DIR%" rmdir /s /q "%PUBLISH_DIR%"
dotnet publish -c Release %VERSION_ARG% 2>&1
if %errorlevel% neq 0 (
    echo.
    echo [Aoko.exe] BUILD FAILED.
    exit /b %errorlevel%
)
cd ..

echo.
echo [Aoko.exe] Output:
echo   %~dp0Aoko\bin\Release\net8.0-windows\win-x64\publish\Aoko.exe
endlocal
