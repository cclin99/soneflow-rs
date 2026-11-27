@echo off
call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\Tools\VsDevCmd.bat" -arch=x64
if errorlevel 1 exit /b %errorlevel%
cl /nologo /W3 /I"<AUDIT_ROOT>\repo\soneflow-ffi\include" "<AUDIT_ROOT>\repo\soneflow-ffi\tests\smoke.c" /link /LIBPATH:"<AUDIT_ROOT>\target-gates\release" soneflow_ffi.dll.lib
if errorlevel 1 exit /b %errorlevel%
copy /Y "<AUDIT_ROOT>\target-gates\release\soneflow_ffi.dll" . >NUL
smoke.exe
