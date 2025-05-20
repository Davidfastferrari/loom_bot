@echo off
echo Applying fixes...

REM Find the actual environment.rs file
for /r c:\loom_bot %%f in (*environment.rs) do (
    echo Found environment.rs at: %%f
    copy /y c:\loom_bot\fixed_environment.rs %%f
)

echo Fixes applied successfully!@echo off
echo Applying fixes...

REM Find the actual environment.rs file
for /r c:\loom_bot %%f in (*environment.rs) do (
    echo Found environment.rs at: %%f
    copy /y c:\loom_bot\fixed_environment.rs %%f
)

echo Fixes applied successfully!