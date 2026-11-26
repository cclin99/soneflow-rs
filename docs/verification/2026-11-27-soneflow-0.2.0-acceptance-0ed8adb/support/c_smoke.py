from verify import *
vc=r'C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat'
bat=ROOT/'c-smoke.cmd';bat.write_text('@echo off\ncall "'+vc+'" >nul\ncl /nologo /W3 /I"'+str(C/'soneflow-ffi/include')+'" "'+str(C/'soneflow-ffi/tests/smoke.c')+'" /Fe:"'+str(ROOT/'smoke.exe')+'" /Fo:"'+str(ROOT/'smoke.obj')+'" /link /LIBPATH:"'+str(C/'target/release')+'" soneflow_ffi.dll.lib\nif errorlevel 1 exit /b %errorlevel%\nset "PATH='+str(C/'target/release')+';%PATH%"\n"'+str(ROOT/'smoke.exe')+'"\n')
run('c-smoke',['cmd','/c',str(bat)],ROOT)
