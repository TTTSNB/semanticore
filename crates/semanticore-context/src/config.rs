//! `SessionConfig` and its parts.

use std::path::Path;
use std::time::Duration;

use semanticore_core::storage::Storage;
use semanticore_store_oxigraph::OxigraphStore;

use crate::error::{storage, SessionError};
use crate::item::{ContextScope, ItemId};

/// The storage behind each scope.
#[derive(Debug)]
#[non_exhaustive]
pub enum StorageConfig<S: Storage> {
    /// One store per scope. The enduring store must advertise
    /// `persistent = true`; [`crate::Session::open`] refuses it otherwise.
    PerScope {
        /// Backs [`ContextScope::Enduring`].
        enduring: S,
        /// Backs [`ContextScope::Major`].
        major: S,
        /// Backs [`ContextScope::Minor`].
        minor: S,
    },
}

/// How reasoners are chosen for each scope.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReasonerSelection {
    /// Pick the best linked reasoner per scope. No reasoner ships at v0.1, so
    /// reasoning operations return
    /// [`SessionError::NotYetImplemented`].
    #[default]
    Auto,
}

/// Per-scope item bounds. `None` is unbounded. Exceeding a bound is not an
/// error: it triggers eviction under [`EvictionPolicy`] and
/// [`DemotionTarget`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScopeBounds {
    /// Maximum items in the enduring tier.
    pub enduring_max_items: Option<u64>,
    /// Maximum items in the major tier.
    pub major_max_items: Option<u64>,
    /// Maximum items in the minor tier.
    pub minor_max_items: Option<u64>,
}

impl ScopeBounds {
    pub(crate) fn get(&self, scope: ContextScope) -> Option<u64> {
        match scope {
            ContextScope::Enduring => self.enduring_max_items,
            ContextScope::Major => self.major_max_items,
            ContextScope::Minor => self.minor_max_items,
        }
    }
}

/// How eviction picks victims within a scope. Ties go to the older item.
///
/// An item is *touched* when it is added and whenever a query returns it;
/// promotion and demotion do not touch it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum EvictionPolicy {
    /// Least recently touched first.
    #[default]
    Lru,
    /// Fewest query accesses first.
    Lfu,
    /// Lowest decayed touch weight first: each touch adds 1, and weight
    /// halves every `half_life`.
    RecencyWeighted {
        /// The weight half-life.
        half_life: Duration,
    },
}

/// What happens to an item that policy eviction selects.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DemotionTarget {
    /// Drop it.
    Drop,
    /// Move it to the next colder scope; drop it if it is already in minor.
    NextColder,
    /// Move it one scope colder if it arrived in its scope by promotion,
    /// otherwise drop it. An item demoted once is therefore dropped the next
    /// time it is evicted, which prevents thrashing.
    #[default]
    OneStepThenDrop,
}

/// Where the enduring tier persists.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum PersistenceTarget {
    /// The enduring scope's own storage, as configured in [`StorageConfig`].
    #[default]
    EnduringStorage,
}

/// How aggressively enduring writes are flushed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum FlushStrategy {
    /// Every Session operation commits its own storage transaction, so
    /// nothing is pending at [`crate::Session::close`].
    #[default]
    OnCommit,
}

/// Persistence settings for the enduring tier.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PersistenceConfig {
    /// Where to persist.
    pub target: PersistenceTarget,
    /// When to flush.
    pub flush_strategy: FlushStrategy,
}

/// When auto-promotion runs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PromotionCadence {
    /// At the end of every successful [`crate::Session::query`].
    #[default]
    AfterEachQuery,
    /// Never; only explicit [`crate::Session::promote`] moves items up.
    Never,
}

/// Auto-promotion thresholds (RFC 0004 "Promotion / demotion semantics").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PromotionConfig {
    /// When auto-promotion runs.
    pub cadence: PromotionCadence,
    /// A minor item returned by more than this many queries within
    /// `minor_window` moves to major.
    pub minor_to_major_accesses: u32,
    /// The access window for `minor_to_major_accesses`.
    pub minor_window: Duration,
    /// A major item cited (via [`crate::Provenance::derived_from`]) by more
    /// than this many other live items moves to enduring.
    pub major_to_enduring_references: u32,
}

impl Default for PromotionConfig {
    fn default() -> Self {
        Self {
            cadence: PromotionCadence::AfterEachQuery,
            minor_to_major_accesses: 3,
            minor_window: Duration::from_secs(60),
            major_to_enduring_references: 5,
        }
    }
}

/// How to open a [`crate::Session`].
#[derive(Debug)]
pub struct SessionConfig<S: Storage = OxigraphStore> {
    /// The storage behind each scope.
    pub storage: StorageConfig<S>,
    /// How reasoners are selected.
    pub reasoners: ReasonerSelection,
    /// Per-scope item bounds.
    pub bounds: ScopeBounds,
    /// How eviction picks victims.
    pub eviction_policy: EvictionPolicy,
    /// What happens to evicted items.
    pub demotion_target: DemotionTarget,
    /// Enduring-tier persistence.
    pub persistence: PersistenceConfig,
    /// Auto-promotion thresholds.
    pub promotion: PromotionConfig,
}

impl<S: Storage> SessionConfig<S> {
    /// A config over `storage` with every other setting at its default.
    pub fn new(storage: StorageConfig<S>) -> Self {
        Self {
            storage,
            reasoners: ReasonerSelection::default(),
            bounds: ScopeBounds::default(),
            eviction_policy: EvictionPolicy::default(),
            demotion_target: DemotionTarget::default(),
            persistence: PersistenceConfig::default(),
            promotion: PromotionConfig::default(),
        }
    }
}

impl SessionConfig<OxigraphStore> {
    /// The OSS default from RFC 0004's capability table: a file-backed
    /// Oxigraph store at `enduring_path` for the enduring tier, in-memory
    /// Oxigraph stores for major and minor.
    pub fn oxigraph(enduring_path: impl AsRef<Path>) -> Result<Self, SessionError> {
        Ok(Self::new(StorageConfig::PerScope {
            enduring: OxigraphStore::open(enduring_path).map_err(storage)?,
            major: OxigraphStore::in_memory().map_err(storage)?,
            minor: OxigraphStore::in_memory().map_err(storage)?,
        }))
    }
}

/// Which items [`crate::Session::evict`] removes from a scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvictionCriteria {
    /// Drop these items.
    Items(Vec<ItemId>),
    /// Move these items to the strictly colder scope `to`.
    Demote {
        /// The items.
        items: Vec<ItemId>,
        /// The target scope.
        to: ContextScope,
    },
    /// Let the [`EvictionPolicy`] pick `count` items and apply the
    /// [`DemotionTarget`] to each.
    Policy {
        /// How many items to evict.
        count: u64,
    },
}
