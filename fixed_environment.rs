use crate::{
    database::Database,
    error::{mdbx_result, Error, Result},
    flags::EnvironmentFlags,
    transaction::{RO, RW},
    txn_manager::{TxnManager, TxnManagerMessage, TxnPtr},
    Mode, SyncMode, Transaction, TransactionKind,
};
use byteorder::{ByteOrder, NativeEndian};
use std::{
    ffi::CString,
    fmt::{self, Debug},
    mem,
    ops::{Bound, RangeBounds},
    path::Path,
    ptr,
    sync::{mpsc::sync_channel, Arc},
    thread::sleep,
    time::Duration,
};
use tracing::warn;

/// The default maximum duration of a read transaction.
#[cfg(feature = "read-tx-timeouts")]
const DEFAULT_MAX_READ_TRANSACTION_DURATION: Duration = Duration::from_secs(5 * 60);

/// An environment supports multiple databases, all residing in the same shared-memory map.
///
/// Accessing the environment is thread-safe.
/// The environment will be closed when the last instance of this type is dropped.
#[derive(Clone)]
pub struct Environment {
    inner: Arc<EnvironmentInner>,
}

impl Environment {
    /// Creates a new builder for specifying options for opening an MDBX environment.
    pub fn builder() -> EnvironmentBuilder {
        EnvironmentBuilder {
            flags: EnvironmentFlags::default(),
            max_readers: None,
            max_dbs: None,
            sync_bytes: None,
            sync_period: None,
            rp_augment_limit: None,
            loose_limit: None,
            dp_reserve_limit: None,
            txn_dp_limit: None,
            spill_max_denominator: None,
            spill_min_denominator: None,
            geometry: None,
            log_level: None,
            kind: Default::default(),
            handle_slow_readers: None,
            #[cfg(feature = "read-tx-timeouts")]
            max_read_transaction_duration: None,
        }
    }

    /// Returns true if the environment was opened as WRITEMAP.
    #[inline]
    pub fn is_write_map(&self) -> bool {
        self.inner.env_kind.is_write_map()
    }

    /// Returns the kind of the environment.
    #[inline]
    pub fn env_kind(&self) -> EnvironmentKind {
        self.inner.env_kind
    }

    /// Returns true if the environment was opened in [`crate::Mode::ReadWrite`] mode.
    #[inline]
    pub fn is_read_write(&self) -> Result<bool> {
        Ok(!self.is_read_only()?)
    }

    /// Returns true if the environment was opened in [`crate::Mode::ReadOnly`] mode.
    #[inline]
    pub fn is_read_only(&self) -> Result<bool> {
        Ok(matches!(self.info()?.mode(), Mode::ReadOnly))
    }

    /// Returns the transaction manager.
    #[inline]
    pub(crate) fn txn_manager(&self) -> &TxnManager {
        &self.inner.txn_manager
    }

    /// Returns the number of timed out transactions that were not aborted by the user yet.
    #[cfg(feature = "read-tx-timeouts")]
    pub fn timed_out_not_aborted_transactions(&self) -> usize {
        self.inner.txn_manager.timed_out_not_aborted_read_transactions().unwrap_or(0)
    }

    /// Create a read-only transaction for use with the environment.
    #[inline]
    pub fn begin_ro_txn(&self) -> Result<Transaction<RO>> {
        Transaction::new(self.clone())
    }

    /// Create a read-write transaction for use with the environment. This method will block while
    /// there are any other read-write transactions open on the environment.
    pub fn begin_rw_txn(&self) -> Result<Transaction<RW>> {
        let mut warned = false;
        let txn = loop {
            let (tx, rx) = sync_channel(0);
            self.txn_manager().send_message(TxnManagerMessage::Begin {
                parent: TxnPtr(ptr::null_mut()),
                flags: RW::OPEN_FLAGS,
                sender: tx,
            });
            let res = rx.recv().unwrap();
            if let Err(Error::Busy) = res {
                if !warned {
                    warned = true;
                    warn!(target: "libmdbx", "Process stalled, awaiting read-write transaction lock.");
                }
                sleep(Duration::from_millis(250));
                continue
            }

            break res
        }?;
        Ok(Transaction::new_from_ptr(self.clone(), txn.0))
    }

    /// Returns a raw pointer to the underlying MDBX environment.
    ///
    /// The caller **must** ensure that the pointer is never dereferenced after the environment has
    /// been dropped.
    #[inline]
    pub(crate) fn env_ptr(&self) -> *mut ffi::MDBX_env {
        self.inner.env
    }

    /// Executes the given closure once
    ///
    /// This is only intended to be used when accessing mdbx ffi functions directly is required.
    ///
    /// The caller **must** ensure that the pointer is only used within the closure.
    #[inline]
    #[doc(hidden)]
    pub fn with_raw_env_ptr<F, T>(&self, f: F) -> T
    where
        F: FnOnce(*mut ffi::MDBX_env) -> T,
    {
        f(self.env_ptr())
    }

    /// Flush the environment data buffers to disk.
    pub fn sync(&self, force: bool) -> Result<bool> {
        mdbx_result(unsafe { ffi::mdbx_env_sync_ex(self.env_ptr(), force, false) })
    }

    /// Retrieves statistics about this environment.
    pub fn stat(&self) -> Result<Stat> {
        unsafe {
            let mut stat = Stat::new();
            mdbx_result(ffi::mdbx_env_stat_ex(
                self.env_ptr(),
                ptr::null(),
                stat.mdb_stat(),
                size_of::<Stat>(),
            ))?;
            Ok(stat)
        }
    }

    /// Retrieves info about this environment.
    pub fn info(&self) -> Result<Info> {
        unsafe {
            let mut info = Info(mem::zeroed());
            mdbx_result(ffi::mdbx_env_info_ex(
                self.env_ptr(),
                ptr::null(),
                &mut info.0,
                size_of::<Info>(),
            ))?;
            Ok(info)
        }
    }

    /// Retrieves the total number of pages on the freelist.
    ///
    /// Along with [`Environment::info()`], this can be used to calculate the exact number
    /// of used pages as well as free pages in this environment.
    ///
    /// ```
    /// # use reth_libmdbx::Environment;
    /// let dir = tempfile::tempdir().unwrap();
    /// let env = Environment::builder().open(dir.path()).unwrap();
    /// let info = env.info().unwrap();
    /// let stat = env.stat().unwrap();
    /// let freelist = env.freelist().unwrap();
    /// let last_pgno = info.last_pgno() + 1; // pgno is 0 based.
    /// let total_pgs = info.map_size() / stat.page_size() as usize;
    /// let pgs_in_use = last_pgno - freelist;
    /// let pgs_free = total_pgs - pgs_in_use;
    /// ```
    ///
    /// Note:
    ///
    /// * MDBX stores all the freelists in the designated database 0 in each environment, and the
    ///   freelist count is stored at the beginning of the value as `uint32_t` in the native byte
    ///   order.
    ///
    /// * It will create a read transaction to traverse the freelist database.
    pub fn freelist(&self) -> Result<usize> {
        let mut freelist: usize = 0;
        let txn = self.begin_ro_txn()?;
        let db = Database::freelist_db();
        let cursor = txn.cursor(&db)?;

        for result in cursor.iter_slices() {
            let (_key, value) = result?;
            if value.len() < size_of::<usize>() {
                return Err(Error::Corrupted)
            }

            let s = &value[..size_of::<usize>()];
            freelist += NativeEndian::read_u32(s) as usize;
        }

        Ok(freelist)
    }
}

/// Container type for Environment internals.
///
/// This holds the raw pointer to the MDBX environment and the transaction manager.
/// The env is opened via [`mdbx_env_create`](ffi::mdbx_env_create) and closed when this type drops.
struct EnvironmentInner {
    /// The raw pointer to the MDBX environment.
    ///
    /// Accessing the environment is thread-safe as long as long as this type exists.
    env: *mut ffi::MDBX_env,
    /// Whether the environment was opened as WRITEMAP.
    env_kind: EnvironmentKind,
    /// Transaction manager
    txn_manager: TxnManager,
}

impl Drop for EnvironmentInner {
    fn drop(&mut self) {
        // Close open mdbx environment on drop
        unsafe {
            ffi::mdbx_env_close_ex(self.env, false);
        }
    }
}

// SAFETY: internal type, only used inside [Environment]. Accessing the environment pointer is
// thread-safe
unsafe impl Send for EnvironmentInner {}
unsafe impl Sync for EnvironmentInner {}

/// Determines how data is mapped into memory
///
/// It only takes affect when the environment is opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EnvironmentKind {
    /// Open the environment in default mode, without WRITEMAP.
    #[default]
    Default,
    /// Open the environment as mdbx-WRITEMAP.
    /// Use a writeable memory map unless the environment is opened as `MDBX_RDONLY`
    /// ([`crate::Mode::ReadOnly`]).
    ///
    /// All data will be mapped into memory in the read-write mode [`crate::Mode::ReadWrite`]. This
    /// offers a significant performance benefit, since the data will be modified directly in
    /// mapped memory and then flushed to disk by single system call, without any memory
    /// management nor copying.
    ///
    /// This mode is incompatible with nested transactions.
    WriteMap,
}

impl EnvironmentKind {
    /// Returns true if the environment was opened as WRITEMAP.
    #[inline]
    pub const fn is_write_map(&self) -> bool {
        matches!(self, Self::WriteMap)
    }

    /// Additional flags required when opening the environment.
    pub(crate) const fn extra_flags(&self) -> ffi::MDBX_env_flags_t {
        match self {
            Self::Default => ffi::MDBX_ENV_DEFAULTS,
            Self::WriteMap => ffi::MDBX_WRITEMAP,
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct EnvPtr(pub(crate) *mut ffi::MDBX_env);
unsafe impl Send for EnvPtr {}
unsafe impl Sync for EnvPtr {}

/// Helper function to get the size of a type
fn size_of<T>() -> usize {
    std::mem::size_of::<T>()
}

/// Environment statistics.
///
/// Contains information about the size and layout of an MDBX environment or database.
#[derive(Debug)]
#[repr(transparent)]
pub struct Stat(ffi::MDBX_stat);

impl Stat {
    /// Create a new Stat with zero'd inner struct `ffi::MDB_stat`.
    pub(crate) const fn new() -> Self {
        unsafe { Self(mem::zeroed()) }
    }

    /// Returns a mut pointer to `ffi::MDB_stat`.
    pub(crate) fn mdb_stat(&mut self) -> *mut ffi::MDBX_stat {
        &mut self.0
    }
}

impl Stat {
    /// Size of a database page. This is the same for all databases in the environment.
    #[inline]
    pub const fn page_size(&self) -> u32 {
        self.0.ms_psize
    }

    /// Depth (height) of the B-tree.
    #[inline]
    pub const fn depth(&self) -> u32 {
        self.0.ms_depth
    }

    /// Number of internal (non-leaf) pages.
    #[inline]
    pub const fn branch_pages(&self) -> usize {
        self.0.ms_branch_pages as usize
    }

    /// Number of leaf pages.
    #[inline]
    pub const fn leaf_pages(&self) -> usize {
        self.0.ms_leaf_pages as usize
    }

    /// Number of overflow pages.
    #[inline]
    pub const fn overflow_pages(&self) -> usize {
        self.0.ms_overflow_pages as usize
    }

    /// Number of data items.
    #[inline]
    pub const fn entries(&self) -> usize {
        self.0.ms_entries as usize
    }
}

#[derive(Debug)]
#[repr(transparent)]
pub struct GeometryInfo(ffi::MDBX_envinfo__bindgen_ty_1);

impl GeometryInfo {
    pub const fn min(&self) -> u64 {
        self.0.lower
    }
    
    pub const fn max(&self) -> u64 {
        self.0.upper
    }
    
    pub const fn current(&self) -> u64 {
        self.0.current
    }
    
    pub const fn shrink(&self) -> u64 {
        self.0.shrink
    }
    
    pub const fn grow(&self) -> u64 {
        self.0.grow
    }
}

/// Environment information.
///
/// Contains environment information about the map size, readers, last txn id etc.
#[derive(Debug)]
#[repr(transparent)]
pub struct Info(ffi::MDBX_envinfo);

impl Info {
    pub const fn geometry(&self) -> GeometryInfo {
        GeometryInfo(self.0.mi_geo)
    }

    /// Size of memory map.
    #[inline]
    pub const fn map_size(&self) -> usize {
        self.0.mi_mapsize as usize
    }

    /// Last used page number
    #[inline]
    pub const fn last_pgno(&self) -> usize {
        self.0.mi_last_pgno as usize
    }

    /// Last transaction ID
    #[inline]
    pub const fn last_txnid(&self) -> usize {
        self.0.mi_recent_txnid as usize
    }

    /// Max reader slots in the environment
    #[inline]
    pub const fn max_readers(&self) -> usize {
        self.0.mi_maxreaders as usize
    }

    /// Max reader slots used in the environment
    #[inline]
    pub const fn num_readers(&self) -> usize {
        self.0.mi_numreaders as usize
    }

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
    }

    /// Return the internal page ops metrics
    #[inline]
    pub const fn page_ops(&self) -> PageOps {
        PageOps {
            newly: self.0.mi_pgop_stat.newly,
            cow: self.0.mi_pgop_stat.cow,
            clone: self.0.mi_pgop_stat.clone,
            split: self.0.mi_pgop_stat.split,
            merge: self.0.mi_pgop_stat.merge,
            spill: self.0.mi_pgop_stat.spill,
            unspill: self.0.mi_pgop_stat.unspill,
            wops: self.0.mi_pgop_stat.wops,
            prefault: self.0.mi_pgop_stat.prefault,
        }
    }

    /// Return the mode in which the environment was opened
    #[inline]
    pub const fn mode(&self) -> Mode {
        if (self.0.mi_mode & ffi::MDBX_RDONLY) != 0 {
            Mode::ReadOnly
        } else {
            Mode::ReadWrite
        }
    }

    /// Return the sync mode of the environment
    #[inline]
    pub const fn sync_mode(&self) -> SyncMode {
        if (self.0.mi_mode & ffi::MDBX_NOSYNC) != 0 {
            SyncMode::Disabled
        } else if (self.0.mi_mode & ffi::MDBX_NOMETASYNC) != 0 {
            SyncMode::NoMetaSync
        } else {
            SyncMode::Enabled
        }
    }
}

/// Page operations metrics
#[derive(Debug, Clone, Copy)]
pub struct PageOps {
    /// Number of newly allocated pages
    pub newly: u64,
    /// Number of pages copied for update
    pub cow: u64,
    /// Number of parent pages cloned for update with shrinking
    pub clone: u64,
    /// Number of page splits
    pub split: u64,
    /// Number of page merges
    pub merge: u64,
    /// Number of spilled dirty pages
    pub spill: u64,
    /// Number of unspilled/reloaded pages
    pub unspill: u64,
    /// Number of pages write operations (independent from txn_commit)
    pub wops: u64,
    /// Number of explicit pre-fault write operations
    pub prefault: u64,
}

/// Defines the geometry of a database.
#[derive(Debug, Clone, Copy)]
pub struct Geometry {
    /// Lower limit for datafile size
    pub size_lower: Option<usize>,
    /// Upper limit for datafile size
    pub size_upper: Option<usize>,
    /// Growth step in bytes
    pub growth_step: Option<usize>,
    /// Shrink threshold in bytes
    pub shrink_threshold: Option<usize>,
    /// Page size
    pub page_size: Option<usize>,
}

impl Default for Geometry {
    fn default() -> Self {
        Self {
            size_lower: None,
            size_upper: None,
            growth_step: None,
            shrink_threshold: None,
            page_size: None,
        }
    }
}

/// Used to create and configure an environment before opening it.
#[derive(Debug, Clone)]
pub struct EnvironmentBuilder {
    flags: EnvironmentFlags,
    max_readers: Option<u64>,
    max_dbs: Option<u64>,
    sync_bytes: Option<usize>,
    sync_period: Option<Duration>,
    rp_augment_limit: Option<u64>,
    loose_limit: Option<u64>,
    dp_reserve_limit: Option<u64>,
    txn_dp_limit: Option<u64>,
    spill_max_denominator: Option<u64>,
    spill_min_denominator: Option<u64>,
    geometry: Option<Geometry>,
    log_level: Option<u64>,
    kind: EnvironmentKind,
    handle_slow_readers: Option<bool>,
    #[cfg(feature = "read-tx-timeouts")]
    max_read_transaction_duration: Option<Duration>,
}

impl EnvironmentBuilder {
    /// Open an environment with the specified configuration.
    pub fn open<P: AsRef<Path>>(&self, path: P) -> Result<Environment> {
        self.open_with_mode(path, Mode::ReadWrite)
    }

    /// Open an environment with the specified configuration and mode.
    pub fn open_with_mode<P: AsRef<Path>>(&self, path: P, mode: Mode) -> Result<Environment> {
        let mut env: *mut ffi::MDBX_env = ptr::null_mut();
        unsafe {
            mdbx_result(ffi::mdbx_env_create(&mut env))?;

            let res = self.configure_env(env, mode);
            if res.is_err() {
                ffi::mdbx_env_close_ex(env, false);
                return res;
            }

            let path_c = match CString::new(
                path.as_ref()
                    .to_str()
                    .ok_or_else(|| Error::Invalid("path contains invalid characters".to_string()))?,
            ) {
                Ok(path_c) => path_c,
                Err(e) => {
                    ffi::mdbx_env_close_ex(env, false);
                    return Err(Error::Invalid(format!("path contains null character: {e}")));
                }
            };

            let res = mdbx_result(ffi::mdbx_env_open(
                env,
                path_c.as_ptr(),
                mode.as_raw() | self.kind.extra_flags(),
                0o600,
            ));
            if res.is_err() {
                ffi::mdbx_env_close_ex(env, false);
                return res;
            }

            let txn_manager = TxnManager::new(EnvPtr(env));

            #[cfg(feature = "read-tx-timeouts")]
            let txn_manager = {
                let max_read_transaction_duration = self
                    .max_read_transaction_duration
                    .unwrap_or(DEFAULT_MAX_READ_TRANSACTION_DURATION);
                txn_manager.with_max_read_transaction_duration(max_read_transaction_duration)
            };

            Ok(Environment {
                inner: Arc::new(EnvironmentInner {
                    env,
                    env_kind: self.kind,
                    txn_manager,
                }),
            })
        }
    }

    /// Configure the environment with the specified options.
    unsafe fn configure_env(&self, env: *mut ffi::MDBX_env, mode: Mode) -> Result<()> {
        if let Some(max_readers) = self.max_readers {
            mdbx_result(ffi::mdbx_env_set_option(
                env,
                ffi::MDBX_opt_max_readers,
                max_readers,
            ))?;
        }

        if let Some(max_dbs) = self.max_dbs {
            mdbx_result(ffi::mdbx_env_set_option(env, ffi::MDBX_opt_max_db, max_dbs))?;
        }

        if let Some(sync_bytes) = self.sync_bytes {
            mdbx_result(ffi::mdbx_env_set_option(
                env,
                ffi::MDBX_opt_sync_bytes,
                sync_bytes as u64,
            ))?;
        }

        if let Some(sync_period) = self.sync_period {
            mdbx_result(ffi::mdbx_env_set_option(
                env,
                ffi::MDBX_opt_sync_period,
                sync_period.as_millis() as u64,
            ))?;
        }

        if let Some(rp_augment_limit) = self.rp_augment_limit {
            mdbx_result(ffi::mdbx_env_set_option(
                env,
                ffi::MDBX_opt_rp_augment_limit,
                rp_augment_limit,
            ))?;
        }

        if let Some(loose_limit) = self.loose_limit {
            mdbx_result(ffi::mdbx_env_set_option(
                env,
                ffi::MDBX_opt_loose_limit,
                loose_limit,
            ))?;
        }

        if let Some(dp_reserve_limit) = self.dp_reserve_limit {
            mdbx_result(ffi::mdbx_env_set_option(
                env,
                ffi::MDBX_opt_dp_reserve_limit,
                dp_reserve_limit,
            ))?;
        }

        if let Some(txn_dp_limit) = self.txn_dp_limit {
            mdbx_result(ffi::mdbx_env_set_option(
                env,
                ffi::MDBX_opt_txn_dp_limit,
                txn_dp_limit,
            ))?;
        }

        if let Some(spill_max_denominator) = self.spill_max_denominator {
            mdbx_result(ffi::mdbx_env_set_option(
                env,
                ffi::MDBX_opt_spill_max_denominator,
                spill_max_denominator,
            ))?;
        }

        if let Some(spill_min_denominator) = self.spill_min_denominator {
            mdbx_result(ffi::mdbx_env_set_option(
                env,
                ffi::MDBX_opt_spill_min_denominator,
                spill_min_denominator,
            ))?;
        }

        if let Some(handle_slow_readers) = self.handle_slow_readers {
            mdbx_result(ffi::mdbx_env_set_option(
                env,
                ffi::MDBX_opt_cleanup_period,
                if handle_slow_readers { 1 } else { 0 },
            ))?;
        }

        if let Some(log_level) = self.log_level {
            mdbx_result(ffi::mdbx_env_set_option(
                env,
                ffi::MDBX_opt_log_level,
                log_level,
            ))?;
        }

        if let Some(geometry) = self.geometry {
            let mut geo: ffi::MDBX_envinfo__bindgen_ty_1 = mem::zeroed();

            if let Some(size_lower) = geometry.size_lower {
                geo.lower = size_lower as u64;
            }

            if let Some(size_upper) = geometry.size_upper {
                geo.upper = size_upper as u64;
            }

            if let Some(growth_step) = geometry.growth_step {
                geo.grow = growth_step as u64;
            }

            if let Some(shrink_threshold) = geometry.shrink_threshold {
                geo.shrink = shrink_threshold as u64;
            }

            if let Some(page_size) = geometry.page_size {
                geo.current = page_size as u64;
            }

            mdbx_result(ffi::mdbx_env_set_geometry(
                env,
                geo.lower,
                geo.current,
                geo.upper,
                geo.grow,
                geo.shrink,
                geo.current,
            ))?;
        }

        mdbx_result(ffi::mdbx_env_set_flags(
            env,
            self.flags.bits(),
            true,
        ))?;

        Ok(())
    }

    /// Set the maximum number of threads or reader slots for the environment.
    ///
    /// This defines the number of slots in the lock table that is used to track readers in the
    /// the environment. The default is 126.
    ///
    /// This option may only be set before calling [`open`](Self::open).
    pub fn set_max_readers(mut self, max_readers: u64) -> Self {
        self.max_readers = Some(max_readers);
        self
    }

    /// Set the maximum number of named databases for the environment.
    ///
    /// This option may only be set before calling [`open`](Self::open).
    pub fn set_max_dbs(mut self, max_dbs: u64) -> Self {
        self.max_dbs = Some(max_dbs);
        self
    }

    /// Set the size of the memory map.
    ///
    /// This option may only be set before calling [`open`](Self::open).
    pub fn set_geometry(mut self, geometry: Geometry) -> Self {
        self.geometry = Some(geometry);
        self
    }

    /// Set the log level for the environment.
    ///
    /// This option may only be set before calling [`open`](Self::open).
    pub fn set_log_level(mut self, log_level: u64) -> Self {
        self.log_level = Some(log_level);
        self
    }

    /// Set the environment kind.
    ///
    /// This option may only be set before calling [`open`](Self::open).
    pub fn set_kind(mut self, kind: EnvironmentKind) -> Self {
        self.kind = kind;
        self
    }

    /// Set the maximum duration of a read transaction.
    ///
    /// This option may only be set before calling [`open`](Self::open).
    #[cfg(feature = "read-tx-timeouts")]
    pub fn set_max_read_transaction_duration(mut self, duration: Duration) -> Self {
        self.max_read_transaction_duration = Some(duration);
        self
    }

    /// Set the environment flags.
    ///
    /// This option may only be set before calling [`open`](Self::open).
    pub fn set_flags(mut self, flags: EnvironmentFlags) -> Self {
        self.flags = flags;
        self
    }

    /// Set the environment flags.
    ///
    /// This option may only be set before calling [`open`](Self::open).
    pub fn flags(mut self, flags: EnvironmentFlags) -> Self {
        self.flags = flags;
        self
    }

    /// Set the environment to handle slow readers.
    ///
    /// This option may only be set before calling [`open`](Self::open).
    pub fn handle_slow_readers(mut self, handle_slow_readers: bool) -> Self {
        self.handle_slow_readers = Some(handle_slow_readers);
        self
    }
}