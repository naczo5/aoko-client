@echo off
setlocal
cd /d "%~dp0"

set "GPP=C:\mingw64\mingw64\bin\g++.exe"
if not exist "%GPP%" (
    echo [NativeTests] ERROR: compiler not found at %GPP%
    exit /b 1
)

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

echo [NativeTests] Building fight_status_core_tests.exe...
"%GPP%" -m64 -std=c++11 -O2 -static-libgcc -static-libstdc++ -o tests\fight_status_core_tests.exe tests\fight_status_core_tests.cpp
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] Running fight_status_core_tests.exe...
tests\fight_status_core_tests.exe
if %errorlevel% neq 0 exit /b %errorlevel%

echo [NativeTests] All native harness tests passed.
exit /b 0

