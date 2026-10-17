@echo off
pushd "%~dp0"
call :build_aris
if %errorlevel% neq 0 exit /b %errorlevel%

set "ARIS=third_party\aris-ui"
set "ARIS_BUILD=%ARIS%\build-bridge"
"C:\mingw64\mingw64\bin\g++.exe" -m64 -std=c++20 -c -o overlay_canvas.o src\main\cpp\overlay_canvas.cpp -I"%ARIS%\include" -I"%ARIS%\third_party\yoga" -I"%ARIS%\third_party\vulkan\include" -I"%ARIS%\third_party\glad\include"
if %errorlevel% neq 0 exit /b %errorlevel%

"C:\mingw64\mingw64\bin\g++.exe" -m64 -std=c++11 -shared -DIMGUI_IMPL_VULKAN_NO_PROTOTYPES -o bridge_261.dll overlay_canvas.o src/main/cpp/bridge_261.cpp src/main/cpp/nick_hider_jvmti.cpp src/main/cpp/anti_debuff_jvmti.cpp src/main/cpp/kill_aura_premotion.cpp src/main/cpp/render_backend.cpp src/main/cpp/gl_loader.cpp src/main/cpp/jni_core/resolver.cpp src/main/cpp/jni_core/helper_bridge.cpp src/main/cpp/imgui/minhook_src/buffer.c src/main/cpp/imgui/minhook_src/hook.c src/main/cpp/imgui/minhook_src/trampoline.c src/main/cpp/imgui/minhook_src/hde/hde64.c -I"C:/Program Files/Java/jdk-17/include" -I"C:/Program Files/Java/jdk-17/include/win32" -I"src/main/cpp" -I"src/main/cpp/vulkan" -I"src/main/cpp/imgui/minhook_src" -I"src/main/cpp/imgui/minhook_src/include" "%ARIS_BUILD%\libaris-ui.a" "%ARIS_BUILD%\libglad.a" "%ARIS_BUILD%\third_party\yoga\yoga\libyogacore.a" -lws2_32 -lopengl32 -lgdi32 -ldwmapi -static-libgcc -static-libstdc++ -Wl,--add-stdcall-alias
if %errorlevel% neq 0 exit /b %errorlevel%
echo Compilation successful!
call :stage bridge_261.dll
popd
exit /b 0

:build_aris
if not exist "third_party\aris-ui\CMakeLists.txt" (
    echo Missing third_party\aris-ui. Add the aris-ui submodule first.
    exit /b 1
)
if not exist "third_party\aris-ui\build-bridge\libaris-ui.a" (
    "C:\Program Files\CMake\bin\cmake.exe" -S "third_party\aris-ui" -B "third_party\aris-ui\build-bridge" -G "MinGW Makefiles" -DCMAKE_BUILD_TYPE=Release -DCMAKE_C_COMPILER="C:/mingw64/mingw64/bin/gcc.exe" -DCMAKE_CXX_COMPILER="C:/mingw64/mingw64/bin/g++.exe" -DARIS_UI_BUILD_EXAMPLES=OFF -DARIS_UI_BUILD_TOOLS=OFF -DARIS_UI_BUILD_TESTS=OFF
    if %errorlevel% neq 0 exit /b %errorlevel%
)
"C:\Program Files\CMake\bin\cmake.exe" --build "third_party\aris-ui\build-bridge" --target aris-ui glad yogacore
exit /b %errorlevel%

:stage
if not exist "..\Aoko\bin\Release\net8.0-windows" mkdir "..\Aoko\bin\Release\net8.0-windows"
copy /Y "%~1" "..\Aoko\bin\Release\net8.0-windows\%~1"
echo Copied %~1 to Aoko Release folder.
copy /Y "%~1" "..\Aoko\%~1" >nul
echo Copied %~1 to ..\Aoko\%~1
set "DBG=..\Aoko\bin\Debug\net8.0-windows"
set "REL=..\Aoko\bin\Release\net8.0-windows"
set "PUB=..\Aoko\bin\Release\net8.0-windows\win-x64\publish"
set "DESKTOP_REL=..\Aoko_Release"
call :copyout "%DBG%" %1
call :copyout "%REL%" %1
call :copyout "%PUB%" %1
call :copyout "%DESKTOP_REL%" %1
call :copyfont "..\Aoko"
call :copyfont "%DBG%"
call :copyfont "%REL%"
call :copyfont "%PUB%"
call :copyfont "%DESKTOP_REL%"
exit /b 0

:copyout
if exist "%~1\" (
    copy /Y "%~2" "%~1\%~2" >nul
    echo Copied %~2 to %~1
)
exit /b 0

:copyfont
if exist "%~1\" (
    copy /Y "assets\font\inter.png" "%~1\inter.png" >nul
    copy /Y "assets\font\inter.json" "%~1\inter.json" >nul
)
exit /b 0
