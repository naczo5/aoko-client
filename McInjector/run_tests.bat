@echo off
setlocal
cd /d "%~dp0"

set "GPP=C:\mingw64\mingw64\bin\g++.exe"
if defined AOKO_GXX set "GPP=%AOKO_GXX%"
if not exist "%GPP%" (
    echo [NativeTests] ERROR: compiler not found at %GPP%
    exit /b 1
)

REM jni.h is needed by the accountCommand/serverCommand wire test.
set "JAVA_INC=C:/Program Files/Java/jdk-17/include"
if defined JAVA_HOME set "JAVA_INC=%JAVA_HOME:\=/%/include"

if not exist "tests" mkdir "tests"

echo [NativeTests] Building auto_rod_core_tests.exe...
"%GPP%" -m64 -std=c++11 -O2 -static-libgcc -static-libstdc++ -o tests\auto_rod_core_tests.exe tests\auto_rod_core_tests.cpp
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Running auto_rod_core_tests.exe...
tests\auto_rod_core_tests.exe
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Building auto_tool_core_tests.exe...
"%GPP%" -m64 -std=c++11 -O2 -static-libgcc -static-libstdc++ -o tests\auto_tool_core_tests.exe tests\auto_tool_core_tests.cpp
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Running auto_tool_core_tests.exe...
tests\auto_tool_core_tests.exe
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Building json_config_reader_tests.exe...
"%GPP%" -m64 -std=c++11 -O2 -static-libgcc -static-libstdc++ -o tests\json_config_reader_tests.exe tests\json_config_reader_tests.cpp
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Running json_config_reader_tests.exe...
tests\json_config_reader_tests.exe
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Building bounded_newline_buffer_tests.exe...
"%GPP%" -m64 -std=c++11 -O2 -static-libgcc -static-libstdc++ -o tests\bounded_newline_buffer_tests.exe tests\bounded_newline_buffer_tests.cpp
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Running bounded_newline_buffer_tests.exe...
tests\bounded_newline_buffer_tests.exe
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Building telemetry_schedule_tests.exe...
"%GPP%" -m64 -std=c++11 -O2 -static-libgcc -static-libstdc++ -o tests\telemetry_schedule_tests.exe tests\telemetry_schedule_tests.cpp
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Running telemetry_schedule_tests.exe...
tests\telemetry_schedule_tests.exe
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Building native_perf_diagnostics_tests.exe...
"%GPP%" -m64 -std=c++11 -O2 -static-libgcc -static-libstdc++ -o tests\native_perf_diagnostics_tests.exe tests\native_perf_diagnostics_tests.cpp
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Running native_perf_diagnostics_tests.exe...
tests\native_perf_diagnostics_tests.exe
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Building mapping_probe_gate_tests.exe...
"%GPP%" -m64 -std=c++11 -O2 -static-libgcc -static-libstdc++ -o tests\mapping_probe_gate_tests.exe tests\mapping_probe_gate_tests.cpp
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Running mapping_probe_gate_tests.exe...
tests\mapping_probe_gate_tests.exe
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Building exclusive_resource_arbiter_tests.exe...
"%GPP%" -m64 -std=c++11 -O2 -static-libgcc -static-libstdc++ -o tests\exclusive_resource_arbiter_tests.exe tests\exclusive_resource_arbiter_tests.cpp
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Running exclusive_resource_arbiter_tests.exe...
tests\exclusive_resource_arbiter_tests.exe
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Building chunk_scan_tests.exe...
"%GPP%" -m64 -std=c++11 -O2 -static-libgcc -static-libstdc++ -o tests\chunk_scan_tests.exe tests\chunk_scan_tests.cpp
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Running chunk_scan_tests.exe...
tests\chunk_scan_tests.exe
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Building block_esp_tests.exe...
"%GPP%" -m64 -std=c++11 -O2 -static-libgcc -static-libstdc++ -o tests\block_esp_tests.exe tests\block_esp_tests.cpp
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Running block_esp_tests.exe...
tests\block_esp_tests.exe
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Building chest_esp_tests.exe...
"%GPP%" -m64 -std=c++11 -O2 -static-libgcc -static-libstdc++ -o tests\chest_esp_tests.exe tests\chest_esp_tests.cpp
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Running chest_esp_tests.exe...
tests\chest_esp_tests.exe
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Building bedplates_tests.exe...
"%GPP%" -m64 -std=c++11 -O2 -static-libgcc -static-libstdc++ -o tests\bedplates_tests.exe tests\bedplates_tests.cpp
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Running bedplates_tests.exe...
tests\bedplates_tests.exe
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Building nick_hider_tests.exe...
"%GPP%" -m64 -std=c++11 -O2 -static-libgcc -static-libstdc++ -o tests\nick_hider_tests.exe tests\nick_hider_tests.cpp
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Running nick_hider_tests.exe...
tests\nick_hider_tests.exe
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Building vk_overlay_tests.exe...
"%GPP%" -m64 -std=c++11 -O2 -static-libgcc -static-libstdc++ -o tests\vk_overlay_tests.exe tests\vk_overlay_tests.cpp
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Running vk_overlay_tests.exe...
tests\vk_overlay_tests.exe
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Building silent_aura_aim_tests.exe...
"%GPP%" -m64 -std=c++11 -O2 -static-libgcc -static-libstdc++ -o tests\silent_aura_aim_tests.exe tests\silent_aura_aim_tests.cpp
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Running silent_aura_aim_tests.exe...
tests\silent_aura_aim_tests.exe
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Building kill_aura_core_tests.exe...
"%GPP%" -m64 -std=c++11 -O2 -static-libgcc -static-libstdc++ -o tests\kill_aura_core_tests.exe tests\kill_aura_core_tests.cpp
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Running kill_aura_core_tests.exe...
tests\kill_aura_core_tests.exe
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Building classfile_entry_injector_tests.exe...
"%GPP%" -m64 -std=c++11 -O2 -static-libgcc -static-libstdc++ -o tests\classfile_entry_injector_tests.exe tests\classfile_entry_injector_tests.cpp
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Running classfile_entry_injector_tests.exe...
tests\classfile_entry_injector_tests.exe
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Building effect_check_rewriter_tests.exe...
"%GPP%" -m64 -std=c++11 -O2 -static-libgcc -static-libstdc++ -o tests\effect_check_rewriter_tests.exe tests\effect_check_rewriter_tests.cpp
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Running effect_check_rewriter_tests.exe...
tests\effect_check_rewriter_tests.exe
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Building aim_assist_projection_tests.exe...
"%GPP%" -m64 -std=c++11 -O2 -static-libgcc -static-libstdc++ -o tests\aim_assist_projection_tests.exe tests\aim_assist_projection_tests.cpp
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Running aim_assist_projection_tests.exe...
tests\aim_assist_projection_tests.exe
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Building throwpot_core_tests.exe...
"%GPP%" -m64 -std=c++11 -O2 -static-libgcc -static-libstdc++ -o tests\throwpot_core_tests.exe tests\throwpot_core_tests.cpp
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Running throwpot_core_tests.exe...
tests\throwpot_core_tests.exe
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Building fight_status_core_tests.exe...
"%GPP%" -m64 -std=c++11 -O2 -static-libgcc -static-libstdc++ -o tests\fight_status_core_tests.exe tests\fight_status_core_tests.cpp
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Running fight_status_core_tests.exe...
tests\fight_status_core_tests.exe
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Building reach_core_tests.exe...
"%GPP%" -m64 -std=c++11 -O2 -static-libgcc -static-libstdc++ -o tests\reach_core_tests.exe tests\reach_core_tests.cpp
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Running reach_core_tests.exe...
tests\reach_core_tests.exe
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Building fireball_warning_tests.exe...
"%GPP%" -m64 -std=c++11 -O2 -static-libgcc -static-libstdc++ -o tests\fireball_warning_tests.exe tests\fireball_warning_tests.cpp
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Running fireball_warning_tests.exe...
tests\fireball_warning_tests.exe
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Building reach_bytecode_injector_tests.exe...
"%GPP%" -m64 -std=c++11 -O2 -static-libgcc -static-libstdc++ -o tests\reach_bytecode_injector_tests.exe tests\reach_bytecode_injector_tests.cpp
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Running reach_bytecode_injector_tests.exe...
tests\reach_bytecode_injector_tests.exe
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Building beta_modules_tests.exe...
"%GPP%" -m64 -std=c++11 -O2 -static-libgcc -static-libstdc++ -o tests\beta_modules_tests.exe tests\beta_modules_tests.cpp
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Running beta_modules_tests.exe...
tests\beta_modules_tests.exe
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Building account_command_router_tests.exe...
"%GPP%" -m64 -std=c++11 -O2 -static-libgcc -static-libstdc++ -I"%JAVA_INC%" -I"%JAVA_INC%/win32" -Isrc/main/cpp -o tests\account_command_router_tests.exe tests\account_command_router_tests.cpp
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Running account_command_router_tests.exe...
tests\account_command_router_tests.exe
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Building session_helper_format_tests.exe...
"%GPP%" -m64 -std=c++11 -O2 -static-libgcc -static-libstdc++ -I"%JAVA_INC%" -I"%JAVA_INC%/win32" -Isrc/main/cpp -o tests\session_helper_format_tests.exe tests\session_helper_format_tests.cpp src\main\cpp\jni_core\session_helper_bridge.cpp
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Running session_helper_format_tests.exe...
tests\session_helper_format_tests.exe
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Building method_hook_rewriter_tests.exe...
"%GPP%" -m64 -std=c++11 -O2 -static-libgcc -static-libstdc++ -o tests\method_hook_rewriter_tests.exe tests\method_hook_rewriter_tests.cpp
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Running method_hook_rewriter_tests.exe...
tests\method_hook_rewriter_tests.exe
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Building visuals_config_tests.exe...
"%GPP%" -m64 -std=c++11 -O2 -static-libgcc -static-libstdc++ -o tests\visuals_config_tests.exe tests\visuals_config_tests.cpp
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Running visuals_config_tests.exe...
tests\visuals_config_tests.exe
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] All native harness tests passed.
exit /b 0

