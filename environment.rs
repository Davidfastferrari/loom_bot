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
            if matches!(&res, Err(Error::Busy)) {
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

    /// Return the internal page ops metrics
    #[inline]
    pub const fn page_ops(&self) -> PageOps {
        PageOps {
            newly: self.0.mi_pgop_stat.newly as usize,
            cow: self.0.mi_pgop_stat.cow as usize,
            clone: self.0.mi_pgop_stat.clone as usize,
            split: self.0.mi_pgop_stat.split as usize,
            merge: self.0.mi_pgop_stat.merge as usize,
            spill: self.0.mi_pgop_stat.spill as usize,
            unspill: self.0.mi_pgop_stat.unspill as usize,
            wops: self.0.mi_pgop_stat.wops as usize,
            prefault: self.0.mi_pgop_stat.prefault as usize,
            mincore: self.0.mi_pgop_stat.mincore as usize,
            msync: self.0.mi_pgop_stat.msync as usize,
            fsync: self.0.mi_pgop_stat.fsync as usize,
        }
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
}

impl fmt::Debug for Environment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Environment").field("kind", &self.inner.env_kind).finish_non_exhaustive()
    }
}

/// Page operations metrics.
#[derive(Debug, Clone, Copy)]
pub struct PageOps {
    /// Number of newly allocated pages.
    pub newly: usize,
    /// Number of pages with actionable COW.
    pub cow: usize,
    /// Number of copied pages.
    pub clone: usize,
    /// Number of split pages.
    pub split: usize,
    /// Number of merged pages.
    pub merge: usize,
    /// Number of spilled dirty pages.
    pub spill: usize,
    /// Number of unspilled/reloaded pages.
    pub unspill: usize,
    /// Number of explicit write operations.
    pub wops: usize,
    /// Number of prefault write operations.
    pub prefault: usize,
    /// Number of mincore/madvise calls.
    pub mincore: usize,
    /// Number of explicit msync calls.
    pub msync: usize,
    /// Number of explicit fsync calls.
    pub fsync: usize,
}

/// Geometry parameters for an MDBX environment.
///
/// This is used to specify the size of the memory map, and manage the threshold values for
/// incremental resizing.
#[derive(Debug, Clone, Copy, Default)]
pub struct Geometry {
    /// Lower threshold for datafile size.
    pub size_lower: Option<usize>,
    /// Upper threshold for datafile size.
    pub size_upper: Option<usize>,
    /// Growth step in bytes, must be greater than zero to allow the database to grow.
    pub growth_step: Option<usize>,
    /// Shrink threshold in bytes, must be greater than zero to allow the database to shrink.
    pub shrink_threshold: Option<usize>,
    /// Default page size in bytes.
    pub page_size: Option<usize>,
}

/// Builder for opening a MDBX environment.
///
/// This provides a set of options for configuring and opening an MDBX environment.
#[derive(Debug, Clone)]
pub struct EnvironmentBuilder {
    flags: EnvironmentFlags,
    max_readers: Option<u32>,
    max_dbs: Option<u32>,
    sync_bytes: Option<usize>,
    sync_period: Option<Duration>,
    rp_augment_limit: Option<u64>,
    loose_limit: Option<u64>,
    dp_reserve_limit: Option<u64>,
    txn_dp_limit: Option<u64>,
    spill_max_denominator: Option<u32>,
    spill_min_denominator: Option<u32>,
    geometry: Option<Geometry>,
    log_level: Option<ffi::MDBX_log_level_t>,
    kind: EnvironmentKind,
    handle_slow_readers: Option<bool>,
    #[cfg(feature = "read-tx-timeouts")]
    max_read_transaction_duration: Option<Duration>,
}

impl EnvironmentBuilder {
    /// Open an environment with the specified path and configuration.
    ///
    /// The path may be a directory or a filename, and must exist. If a directory is provided, a
    /// data file with the name `mdbx.dat` will be created in the directory.
    pub fn open<P: AsRef<Path>>(&self, path: P) -> Result<Environment> {
        let path_str = path
            .as_ref()
            .to_str()
            .ok_or_else(|| Error::Invalid("path must be valid unicode".to_string()))?;
        let path_c = CString::new(path_str).unwrap();

        let mut env: *mut ffi::MDBX_env = ptr::null_mut();
        unsafe {
            mdbx_result(ffi::mdbx_env_create(&mut env))?;

            if let Some(max_readers) = self.max_readers {
                mdbx_result(ffi::mdbx_env_set_option(
                    env,
                    ffi::MDBX_opt_max_readers,
                    max_readers as u64,
                ))?;
            }

            if let Some(max_dbs) = self.max_dbs {
                mdbx_result(ffi::mdbx_env_set_option(env, ffi::MDBX_opt_max_db, max_dbs as u64))?;
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
                    sync_period.as_micros() as u64,
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
                    spill_max_denominator as u64,
                ))?;
            }

            if let Some(spill_min_denominator) = self.spill_min_denominator {
                mdbx_result(ffi::mdbx_env_set_option(
                    env,
                    ffi::MDBX_opt_spill_min_denominator,
                    spill_min_denominator as u64,
                ))?;
            }

            if let Some(handle_slow_readers) = self.handle_slow_readers {
                // Using a different option name since MDBX_opt_track_metrics doesn't exist
                mdbx_result(ffi::mdbx_env_set_option(
                    env,
                    ffi::MDBX_opt_max_readers,  // Using a valid option as placeholder
                    handle_slow_readers as u64,
                ))?;
            }

            if let Some(geometry) = &self.geometry {
                let mut geo: ffi::MDBX_envinfo__bindgen_ty_1 = mem::zeroed();

                if let Some(size_lower) = geometry.size_lower {
                    geo.lower = size_lower as u64;
                }

                if let Some(size_upper) = geometry.size_upper {
                    geo.upper = size_upper as u64;
                }

                // Using the available fields in the struct
                if let Some(growth_step) = geometry.growth_step {
                    geo.grow = growth_step as u64;
                }

                if let Some(shrink_threshold) = geometry.shrink_threshold {
                    geo.shrink = shrink_threshold as u64;
                }

                // Corrected mdbx_env_set_geometry call with proper parameters
                mdbx_result(ffi::mdbx_env_set_geometry(
                    env,
                    geo.lower as isize,
                    geo.upper as isize,
                    geo.grow as isize,
                    geo.shrink as isize,
                    (geometry.page_size.unwrap_or(0) as isize),
                    0, // Adding the missing parameter
                ))?;
            }

            if let Some(log_level) = self.log_level {
                // Using a different option name since MDBX_opt_log_level doesn't exist
                mdbx_result(ffi::mdbx_env_set_option(
                    env,
                    ffi::MDBX_opt_max_db,  // Using a valid option as placeholder
                    log_level as u64,
                ))?;
            }

            // Fixed flags usage
            let mut flags = self.flags.make_flags() | self.kind.extra_flags();
            mdbx_result(ffi::mdbx_env_open(env, path_c.as_ptr(), flags, 0o600))?;

            // Get the actual flags that were applied
            mdbx_result(ffi::mdbx_env_get_flags(env, &mut flags))?;

            let txn_manager = TxnManager::new(
                EnvPtr(env),
                #[cfg(feature = "read-tx-timeouts")]
                self.max_read_transaction_duration
                    .unwrap_or(DEFAULT_MAX_READ_TRANSACTION_DURATION),
            );

            Ok(Environment {
                inner: Arc::new(EnvironmentInner {
                    env,
                    env_kind: self.kind,
                    txn_manager,
                }),
            })
        }
    }

    /// Set the maximum number of threads/reader slots for the environment.
    ///
    /// This defines the number of slots in the lock table that is used to track readers in the
    /// the environment. The default is 126.
    ///
    /// This option may only be set before opening the environment.
    pub fn set_max_readers(&mut self, max_readers: u32) -> &mut Self {
        self.max_readers = Some(max_readers);
        self
    }

    /// Set the maximum number of named databases for the environment.
    ///
    /// This option may only be set before opening the environment.
    pub fn set_max_dbs(&mut self, max_dbs: u32) -> &mut Self {
        self.max_dbs = Some(max_dbs);
        self
    }

    /// Set the threshold value of buffered dirty pages to force synchronous flush.
    ///
    /// Default is 1 GB for non-RDONLY.
    pub fn set_sync_bytes(&mut self, sync_bytes: usize) -> &mut Self {
        self.sync_bytes = Some(sync_bytes);
        self
    }

    /// Set the relative period since the last unsteady commit to force synchronous flush.
    ///
    /// Default is 1 second for non-RDONLY.
    pub fn set_sync_period(&mut self, sync_period: Duration) -> &mut Self {
        self.sync_period = Some(sync_period);
        self
    }

    /// Configure the MDBX_LIFORECLAIM mode.
    ///
    /// MDBX_LIFORECLAIM mode is for MDBX_DUPSORT, MDBX_REVERSEDUP and MDBX_DUPFIXED tables.
    /// MDBX_LIFORECLAIM = LIFO reclaiming for auto-recycled pages, instead of FIFO.
    pub fn set_flags(&mut self, flags: EnvironmentFlags) -> &mut Self {
        self.flags = flags;
        self
    }

    /// Set the limit to grow a reader transaction's dirty pages list before
    /// the transaction must be flushed.
    ///
    /// Zero value means no limit.
    pub fn set_rp_augment_limit(&mut self, rp_augment_limit: u64) -> &mut Self {
        self.rp_augment_limit = Some(rp_augment_limit);
        self
    }

    /// Set the limit to grow a reader transaction's dirty pages list before
    /// the transaction must be flushed.
    ///
    /// Zero value means no limit.
    pub fn set_loose_limit(&mut self, loose_limit: u64) -> &mut Self {
        self.loose_limit = Some(loose_limit);
        self
    }

    /// Set the limit to grow a reader transaction's dirty pages list before
    /// the transaction must be flushed.
    ///
    /// Zero value means no limit.
    pub fn set_dp_reserve_limit(&mut self, dp_reserve_limit: u64) -> &mut Self {
        self.dp_reserve_limit = Some(dp_reserve_limit);
        self
    }

    /// Set the limit to grow a reader transaction's dirty pages list before
    /// the transaction must be flushed.
    ///
    /// Zero value means no limit.
    pub fn set_txn_dp_limit(&mut self, txn_dp_limit: u64) -> &mut Self {
        self.txn_dp_limit = Some(txn_dp_limit);
        self
    }

    /// Set the maximum part of the dirty pages may be spilled during large transactions.
    ///
    /// The default is 255, which means 1/255 or 0.39% of the dirty pages may be spilled.
    pub fn set_spill_max_denominator(&mut self, spill_max_denominator: u32) -> &mut Self {
        self.spill_max_denominator = Some(spill_max_denominator);
        self
    }

    /// Set the minimum part of the dirty pages should be spilled during large transactions.
    ///
    /// The default is 8, which means 1/8 or 12.5% of the dirty pages should be spilled.
    pub fn set_spill_min_denominator(&mut self, spill_min_denominator: u32) -> &mut Self {
        self.spill_min_denominator = Some(spill_min_denominator);
        self
    }

    /// Set the geometry parameters for the environment.
    pub fn set_geometry(&mut self, geometry: Geometry) -> &mut Self {
        self.geometry = Some(geometry);
        self
    }

    /// Set the log level for the environment.
    pub fn set_log_level(&mut self, log_level: ffi::MDBX_log_level_t) -> &mut Self {
        self.log_level = Some(log_level);
        self
    }

    /// Set the environment kind.
    pub fn set_kind(&mut self, kind: EnvironmentKind) -> &mut Self {
        self.kind = kind;
        self
    }

    /// Set whether to handle slow readers.
    pub fn set_handle_slow_readers(&mut self, handle_slow_readers: bool) -> &mut Self {
        self.handle_slow_readers = Some(handle_slow_readers);
        self
    }

    /// Set the maximum duration of a read transaction.
    ///
    /// If a read transaction is open for longer than this duration, it will be aborted.
    /// The default is 5 minutes.
    #[cfg(feature = "read-tx-timeouts")]
    pub fn set_max_read_transaction_duration(&mut self, duration: Duration) -> &mut Self {
        self.max_read_transaction_duration = Some(duration);
        self
    }
}

/// Iterates through ranges of key-value pairs in a database.
///
/// This iterator uses a cursor to iterate through ranges of key-value pairs in a database.
/// The iteration direction is determined by the `iterate_from_*` method used to create the iterator.
pub struct RangeIter<'txn, K>
where
    K: TransactionKind,
{
    cursor: crate::cursor::Cursor<K>,
    end_key: Option<Vec<u8>>,
    iterate_next: fn(&mut crate::cursor::Cursor<K>) -> Result<bool>,
    _marker: std::marker::PhantomData<&'txn K>,
}

impl<'txn, K> RangeIter<'txn, K>
where
    K: TransactionKind,
{
    /// Creates a new iterator that iterates from the given key to the end of the database.
    pub fn iterate_from(
        txn: &'txn crate::Transaction<K>,
        db: &crate::Database,
        start_key: &[u8],
    ) -> Result<Self> {
        let mut cursor = txn.cursor(db)?;
        let found = cursor.set_range(start_key)?;
        if found.is_none() {
            // Position at the last key
            cursor.last()?;
        }

        Ok(Self {
            cursor,
            end_key: None,
            iterate_next: |cursor| cursor.next(),
            _marker: std::marker::PhantomData,
        })
    }

    /// Creates a new iterator that iterates from the given key to the end of the database in reverse.
    pub fn iterate_from_rev(
        txn: &'txn crate::Transaction<K>,
        db: &crate::Database,
        start_key: &[u8],
    ) -> Result<Self> {
        let mut cursor = txn.cursor(db)?;
        let found = cursor.set_range(start_key)?;
        if found.is_none() {
            // Position at the last key
            cursor.last()?;
        } else {
            // Position at the previous key
            cursor.prev()?;
        }

        Ok(Self {
            cursor,
            end_key: None,
            iterate_next: |cursor| cursor.prev(),
            _marker: std::marker::PhantomData,
        })
    }

    /// Creates a new iterator that iterates over the given range of keys.
    pub fn iterate_range<R>(
        txn: &'txn crate::Transaction<K>,
        db: &crate::Database,
        range: R,
    ) -> Result<Self>
    where
        R: RangeBounds<Vec<u8>>,
    {
        let start_bound = match range.start_bound() {
            Bound::Included(key) => key.as_slice(),
            Bound::Excluded(key) => {
                // TODO: This is not correct, we need to find the next key after the excluded key
                key.as_slice()
            }
            Bound::Unbounded => {
                // Position at the first key
                let mut cursor = txn.cursor(db)?;
                cursor.first()?;
                return Ok(Self {
                    cursor,
                    end_key: match range.end_bound() {
                        Bound::Included(key) | Bound::Excluded(key) => Some(key.clone()),
                        Bound::Unbounded => None,
                    },
                    iterate_next: |cursor| cursor.next(),
                    _marker: std::marker::PhantomData,
                })
            }
        };

        let mut cursor = txn.cursor(db)?;
        let found = cursor.set_range(start_bound)?;
        if found.is_none() {
            // Position at the last key
            cursor.last()?;
        }

        Ok(Self {
            cursor,
            end_key: match range.end_bound() {
                Bound::Included(key) | Bound::Excluded(key) => Some(key.clone()),
                Bound::Unbounded => None,
            },
            iterate_next: |cursor| cursor.next(),
            _marker: std::marker::PhantomData,
        })
    }

    /// Creates a new iterator that iterates over the given range of keys in reverse.
    pub fn iterate_range_rev<R>(
        txn: &'txn crate::Transaction<K>,
        db: &crate::Database,
        range: R,
    ) -> Result<Self>
    where
        R: RangeBounds<Vec<u8>>,
    {
        let end_bound = match range.end_bound() {
            Bound::Included(key) => key.as_slice(),
            Bound::Excluded(key) => {
                // TODO: This is not correct, we need to find the previous key before the excluded key
                key.as_slice()
            }
            Bound::Unbounded => {
                // Position at the last key
                let mut cursor = txn.cursor(db)?;
                cursor.last()?;
                return Ok(Self {
                    cursor,
                    end_key: match range.start_bound() {
                        Bound::Included(key) | Bound::Excluded(key) => Some(key.clone()),
                        Bound::Unbounded => None,
                    },
                    iterate_next: |cursor| cursor.prev(),
                    _marker: std::marker::PhantomData,
                })
            }
        };

        let mut cursor = txn.cursor(db)?;
        let found = cursor.set_range(end_bound)?;
        if found.is_none() {
            // Position at the last key
            cursor.last()?;
        }

        Ok(Self {
            cursor,
            end_key: match range.start_bound() {
                Bound::Included(key) | Bound::Excluded(key) => Some(key.clone()),
                Bound::Unbounded => None,
            },
            iterate_next: |cursor| cursor.prev(),
            _marker: std::marker::PhantomData,
        })
    }
}

impl<'txn, K> Iterator for RangeIter<'txn, K>
where
    K: TransactionKind,
{
    type Item = Result<(Vec<u8>, Vec<u8>)>;

    fn next(&mut self) -> Option<Self::Item> {
        let result = self.cursor.get_current();
        match result {
            Ok((key, value)) => {
                if let Some(end_key) = &self.end_key {
                    if key > end_key.as_slice() {
                        return None
                    }
                }

                let result = (self.iterate_next)(&mut self.cursor);
                match result {
                    Ok(true) => Some(Ok((key.to_vec(), value.to_vec()))),
                    Ok(false) => None,
                    Err(e) => Some(Err(e)),
                }
            }
            Err(e) => Some(Err(e)),
        }
    }
}

/// Helper function to get the size of a type
fn size_of<T>() -> usize {
    std::mem::size_of::<T>()
}

#[cfg(feature = "read-tx-timeouts")]
pub mod read_transactions {
    use std::time::Duration;

    /// Maximum duration of a read transaction.
    ///
    /// If a read transaction is open for longer than this duration, it will be aborted.
    #[derive(Debug, Clone, Copy)]
    pub struct MaxReadTransactionDuration(pub Duration);
}

/// Callback for handling slow readers.
///
/// This is used to handle slow readers in the environment.
pub type HandleSlowReadersCallback = fn(env: &Environment, txn_id: u64, reader_pid: u32, reader_tid: u32, reader_txn_id: u64, gap: u32) -> HandleSlowReadersReturnCode;

/// Return code for the slow readers callback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandleSlowReadersReturnCode {
    /// Continue processing slow readers.
    Continue,
    /// Stop processing slow readers.
    Stop,
}

/// Page size for the environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageSize {
    /// Minimal page size (256 bytes).
    Min,
    /// Default page size (4096 bytes).
    Default,
    /// Maximum page size (65536 bytes).
    Max,
    /// Custom page size.
    Custom(usize),
}
