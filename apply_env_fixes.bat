@echo off
echo Applying environment.rs fixes...

REM Create the as_flags method implementation
echo // Fix for the as_flags() method in EnvironmentFlags > c:\loom_bot\env_fixes.txt
echo impl EnvironmentFlags { >> c:\loom_bot\env_fixes.txt
echo     pub fn as_flags(^&self) -^> ffi::MDBX_env_flags_t { >> c:\loom_bot\env_fixes.txt
echo         let mut flags = ffi::MDBX_ENV_DEFAULTS; >> c:\loom_bot\env_fixes.txt
echo         >> c:\loom_bot\env_fixes.txt
echo         if self.no_rdahead { >> c:\loom_bot\env_fixes.txt
echo             flags |= ffi::MDBX_NORDAHEAD; >> c:\loom_bot\env_fixes.txt
echo         } >> c:\loom_bot\env_fixes.txt
echo         if self.no_meminit { >> c:\loom_bot\env_fixes.txt
echo             flags |= ffi::MDBX_NOMEMINIT; >> c:\loom_bot\env_fixes.txt
echo         } >> c:\loom_bot\env_fixes.txt
echo         if self.coalesce { >> c:\loom_bot\env_fixes.txt
echo             flags |= ffi::MDBX_COALESCE; >> c:\loom_bot\env_fixes.txt
echo         } >> c:\loom_bot\env_fixes.txt
echo         if self.liforeclaim { >> c:\loom_bot\env_fixes.txt
echo             flags |= ffi::MDBX_LIFORECLAIM; >> c:\loom_bot\env_fixes.txt
echo         } >> c:\loom_bot\env_fixes.txt
echo         if self.exclusive { >> c:\loom_bot\env_fixes.txt
echo             flags |= ffi::MDBX_EXCLUSIVE; >> c:\loom_bot\env_fixes.txt
echo         } >> c:\loom_bot\env_fixes.txt
echo         if self.accede { >> c:\loom_bot\env_fixes.txt
echo             flags |= ffi::MDBX_ACCEDE; >> c:\loom_bot\env_fixes.txt
echo         } >> c:\loom_bot\env_fixes.txt
echo         >> c:\loom_bot\env_fixes.txt
echo         flags >> c:\loom_bot\env_fixes.txt
echo     } >> c:\loom_bot\env_fixes.txt
echo } >> c:\loom_bot\env_fixes.txt

REM Copy the fixes to the environment.rs file
copy c:\loom_bot\environment.rs c:\loom_bot\environment.rs.bak
type c:\loom_bot\env_fixes.txt >> c:\loom_bot\environment.rs

REM Fix the mode() function
powershell -Command "(Get-Content 'c:\loom_bot\environment.rs') -replace '(?s)pub const fn mode\(\&self\) -> Mode \{.*?\}', (Get-Content 'c:\loom_bot\mode_fix.txt' -Raw) | Set-Content 'c:\loom_bot\environment.rs'"

REM Fix the is_read_only and is_read_write methods
powershell -Command "(Get-Content 'c:\loom_bot\environment.rs') -replace '(?s)pub fn is_read_only\(\&self\) -> Result<bool> \{.*?pub fn is_read_write\(\&self\) -> Result<bool> \{.*?\}', (Get-Content 'c:\loom_bot\read_mode_fix.txt' -Raw) | Set-Content 'c:\loom_bot\environment.rs'"

echo Fixes applied successfully!