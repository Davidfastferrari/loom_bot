/// Return the mode of the database
#[inline]
pub const fn mode(&self) -> Mode {
    let mode = self.0.mi_mode;
    if (mode & (ffi::MDBX_RDONLY as u32)) != 0 {
        Mode::ReadOnly
    } else if (mode & (ffi::MDBX_UTTERLY_NOSYNC as u32)) != 0 {
        Mode::ReadWrite { sync_mode: SyncMode::UtterlyNoSync }
    } else if (mode & (ffi::MDBX_NOMETASYNC as u32)) != 0 {
        Mode::ReadWrite { sync_mode: SyncMode::NoMetaSync }
    } else if (mode & (ffi::MDBX_SAFE_NOSYNC as u32)) != 0 {
        Mode::ReadWrite { sync_mode: SyncMode::SafeNoSync }
    } else {
        Mode::ReadWrite { sync_mode: SyncMode::Durable }
    }
}/// Return the mode of the database
#[inline]
pub const fn mode(&self) -> Mode {
    let mode = self.0.mi_mode;
    if (mode & (ffi::MDBX_RDONLY as u32)) != 0 {
        Mode::ReadOnly
    } else if (mode & (ffi::MDBX_UTTERLY_NOSYNC as u32)) != 0 {
        Mode::ReadWrite { sync_mode: SyncMode::UtterlyNoSync }
    } else if (mode & (ffi::MDBX_NOMETASYNC as u32)) != 0 {
        Mode::ReadWrite { sync_mode: SyncMode::NoMetaSync }
    } else if (mode & (ffi::MDBX_SAFE_NOSYNC as u32)) != 0 {
        Mode::ReadWrite { sync_mode: SyncMode::SafeNoSync }
    } else {
        Mode::ReadWrite { sync_mode: SyncMode::Durable }
    }
}