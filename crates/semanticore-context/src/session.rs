//! The `Session`: lifecycle, items, queries, promotion and demotion.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use semanticore_core::datalog::{DatalogProgram, DatalogResult};
use semanticore_core::model::{NamedOrBlankNode, NamedOrDefault, Quad, Term};
use semanticore_core::shacl::ShaclValidationReport;
use semanticore_core::storage::{Storage, StorageCapability, StorageTransaction};
use semanticore_store_oxigraph::OxigraphStore;

use crate::config::{
    DemotionTarget, EvictionCriteria, EvictionPolicy, PromotionCadence, PromotionConfig,
    ScopeBounds, SessionConfig, StorageConfig,
};
use crate::error::{storage, ContextError, SessionError, Unimplemented};
use crate::item::{ContextItem, ContextPayload, ContextScope, ItemId};
use crate::query::{ContextQuery, ContextQueryKind, ContextQueryResult, QueryStats, RankedItem};
use crate::record;

/// Counters reported by [`Session::close`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionStats {
    /// Items added via `add` / `add_batch`.
    pub items_added: u64,
    /// Queries that returned a result.
    pub queries_served: u64,
    /// Reasoning passes run. Always 0 at v0.1: no reasoner ships yet.
    pub reasoning_passes: u64,
    /// Items moved to a more durable scope, explicitly or automatically.
    pub items_promoted: u64,
    /// Items moved to a less durable scope, explicitly or by eviction.
    pub items_demoted: u64,
    /// Items dropped by eviction (explicit or bound-driven). `remove` is not
    /// counted.
    pub items_evicted: u64,
}

/// Returned by [`Session::promote`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PromotionStats {
    /// Items moved.
    pub promoted: u64,
}

/// A point-in-time copy of one scope.
#[derive(Debug, Clone, PartialEq)]
pub struct ContextSnapshot {
    /// The scope.
    pub scope: ContextScope,
    /// Its items, oldest first.
    pub items: Vec<(ItemId, ContextItem)>,
    /// Quads in the scope's storage: payload triples plus one metadata quad
    /// per item.
    pub quad_count: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Arrival {
    Added,
    Promoted,
    Demoted,
}

#[derive(Debug)]
struct Entry {
    item: ContextItem,
    scope: ContextScope,
    arrival: Arrival,
    /// Query accesses (LFU).
    accesses: u64,
    /// Last add or query access (LRU).
    last_touch: Instant,
    /// Recent query accesses, pruned to the minor promotion window.
    recent: VecDeque<Instant>,
    /// Recency-weighted touch weight as of `weight_at`.
    weight: f64,
    weight_at: Instant,
}

impl Entry {
    fn new(item: ContextItem, scope: ContextScope, arrival: Arrival, now: Instant) -> Self {
        Self {
            item,
            scope,
            arrival,
            accesses: 0,
            last_touch: now,
            recent: VecDeque::new(),
            weight: 1.0,
            weight_at: now,
        }
    }

    fn weight_at(&self, now: Instant, half_life: Duration) -> f64 {
        let hl = half_life.as_secs_f64().max(f64::MIN_POSITIVE);
        let age = now.saturating_duration_since(self.weight_at).as_secs_f64();
        self.weight * 0.5f64.powf(age / hl)
    }
}

struct State {
    items: BTreeMap<ItemId, Entry>,
    next_id: u64,
    stats: SessionStats,
}

struct Settings {
    bounds: ScopeBounds,
    eviction_policy: EvictionPolicy,
    demotion_target: DemotionTarget,
    promotion: PromotionConfig,
}

struct Tiers<S> {
    enduring: S,
    major: S,
    minor: S,
}

impl<S> Tiers<S> {
    fn get(&self, scope: ContextScope) -> &S {
        match scope {
            ContextScope::Enduring => &self.enduring,
            ContextScope::Major => &self.major,
            ContextScope::Minor => &self.minor,
        }
    }
}

/// A Context OS session over the enduring, major and minor tiers (RFC 0004).
///
/// Each scope is backed by its own [`Storage`]. Every operation commits its
/// own storage transactions before returning, so nothing is pending at
/// [`Session::close`]. Moving an item between scopes writes it to the target
/// store, then deletes it from the source store: a crash in between leaves a
/// copy in both, never in neither, and on reopen the more durable copy wins.
///
/// RFC 0004 also makes `Session` generic over a `ContextReasoner`. That trait
/// is not yet defined in `semanticore-core`, so the parameter is left out
/// until the default reasoners land.
pub struct Session<S: Storage = OxigraphStore> {
    stores: Tiers<S>,
    settings: Settings,
    state: Mutex<State>,
}

impl<S: Storage> std::fmt::Debug for Session<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session").finish_non_exhaustive()
    }
}

impl<S: Storage> Session<S> {
    /// Open a Session.
    ///
    /// Fails with [`SessionError::CapabilityShortfall`] if the enduring store
    /// is not persistent. Items already in the stores (an enduring store from
    /// an earlier Session) are loaded back with fresh access statistics.
    pub fn open(config: SessionConfig<S>) -> Result<Self, SessionError> {
        let StorageConfig::PerScope {
            enduring,
            major,
            minor,
        } = config.storage;
        if !enduring.capabilities().persistent {
            return Err(SessionError::CapabilityShortfall {
                scope: ContextScope::Enduring,
                required: StorageCapability::Persistent,
            });
        }
        let stores = Tiers {
            enduring,
            major,
            minor,
        };
        let now = Instant::now();
        let mut items = BTreeMap::new();
        // Most durable first, so a copy left in two stores by an interrupted
        // move resolves to the more durable one.
        for scope in ContextScope::ALL {
            for (id, item) in load(stores.get(scope))? {
                items
                    .entry(id)
                    .or_insert_with(|| Entry::new(item, scope, Arrival::Added, now));
            }
        }
        let next_id = items.keys().next_back().map_or(0, |id: &ItemId| id.0 + 1);
        Ok(Self {
            stores,
            settings: Settings {
                bounds: config.bounds,
                eviction_policy: config.eviction_policy,
                demotion_target: config.demotion_target,
                promotion: config.promotion,
            },
            state: Mutex::new(State {
                items,
                next_id,
                stats: SessionStats::default(),
            }),
        })
    }

    /// Close the Session and return its counters. Enduring writes are
    /// already committed; the major and minor tiers are dropped with their
    /// stores.
    pub fn close(self) -> Result<SessionStats, SessionError> {
        let state = self.state.into_inner().unwrap_or_else(|e| e.into_inner());
        Ok(state.stats)
    }

    /// Add one item to `scope`. The scope argument wins over the item's
    /// `scope_hint`.
    pub fn add(&mut self, scope: ContextScope, item: ContextItem) -> Result<ItemId, SessionError> {
        let ids = self.add_batch(scope, std::slice::from_ref(&item))?;
        Ok(ids[0])
    }

    /// Add items to `scope` in one storage transaction. Either every item is
    /// added or none is. Bounds are enforced after the whole batch.
    pub fn add_batch(
        &mut self,
        scope: ContextScope,
        items: &[ContextItem],
    ) -> Result<Vec<ItemId>, SessionError> {
        let mut state = self.lock();
        for item in items {
            validate(&state, item)?;
        }
        let first = state.next_id;
        let ids: Vec<ItemId> = (first..first + items.len() as u64).map(ItemId).collect();
        with_write(self.stores.get(scope), |tx| {
            for (id, item) in ids.iter().zip(items) {
                insert_all(tx, &record::encode(*id, item))?;
            }
            Ok(())
        })?;
        let now = Instant::now();
        state.next_id = first + items.len() as u64;
        for (id, item) in ids.iter().zip(items) {
            state
                .items
                .insert(*id, Entry::new(item.clone(), scope, Arrival::Added, now));
        }
        state.stats.items_added += items.len() as u64;
        self.enforce_bounds(&mut state, scope)?;
        Ok(ids)
    }

    /// Remove an item. Returns `false` if it is not in `scope` (including if
    /// it does not exist).
    pub fn remove(&mut self, scope: ContextScope, id: &ItemId) -> Result<bool, SessionError> {
        let mut state = self.lock();
        if state.items.get(id).map(|e| e.scope) != Some(scope) {
            return Ok(false);
        }
        delete(self.stores.get(scope), *id)?;
        state.items.remove(id);
        Ok(true)
    }

    /// Run a query. Pattern queries are evaluated against each consulted
    /// scope's storage; returned items count as accessed, and auto-promotion
    /// runs afterwards (per [`PromotionCadence`]).
    pub fn query(&self, query: ContextQuery) -> Result<ContextQueryResult, SessionError> {
        let pattern = match &query.kind {
            ContextQueryKind::Pattern(p) => p,
            ContextQueryKind::Sparql(_) => return Err(nyi(Unimplemented::Sparql)),
            ContextQueryKind::Similarity { .. } => return Err(nyi(Unimplemented::AnnIndex)),
            ContextQueryKind::Inference(_) => return Err(nyi(Unimplemented::OwlElReasoner)),
        };
        if query.reasoner_hint.is_some() {
            return Err(nyi(Unimplemented::OwlElReasoner));
        }
        let mut state = self.lock();
        let scopes = query.scopes.scopes();
        let mut stats = QueryStats {
            scopes_consulted: scopes.len(),
            ..QueryStats::default()
        };
        let mut hits = Vec::new();
        for scope in scopes {
            let tx = self.stores.get(scope).begin_read().map_err(storage)?;
            let mut found = BTreeSet::new();
            {
                let iter = tx.quads_for_pattern(
                    pattern.subject.as_ref(),
                    pattern.predicate.as_ref(),
                    pattern.object.as_ref(),
                    None,
                );
                for quad in iter {
                    let quad = quad.map_err(storage)?;
                    let NamedOrDefault::Named(g) = &quad.graph else {
                        continue;
                    };
                    let Some(id) = record::parse_item_graph(g) else {
                        continue;
                    };
                    if state.items.get(&id).is_some_and(|e| e.scope == scope) {
                        stats.quads_matched += 1;
                        found.insert(id);
                    }
                }
            }
            tx.rollback().map_err(storage)?;
            hits.extend(found.into_iter().map(|id| (id, scope)));
        }
        if let Some(limit) = query.limit {
            hits.truncate(limit);
        }

        let now = Instant::now();
        let window = self.settings.promotion.minor_window;
        let half_life = match self.settings.eviction_policy {
            EvictionPolicy::RecencyWeighted { half_life } => Some(half_life),
            _ => None,
        };
        let mut items = Vec::with_capacity(hits.len());
        for (id, scope) in hits {
            let e = state.items.get_mut(&id).expect("hit is indexed");
            e.accesses += 1;
            e.last_touch = now;
            e.recent.push_back(now);
            while e
                .recent
                .front()
                .is_some_and(|t| now.saturating_duration_since(*t) > window)
            {
                e.recent.pop_front();
            }
            if let Some(hl) = half_life {
                e.weight = e.weight_at(now, hl) + 1.0;
                e.weight_at = now;
            }
            items.push(RankedItem {
                id,
                item: e.item.clone(),
                score: 1.0,
                source_scope: scope,
            });
        }
        state.stats.queries_served += 1;
        if self.settings.promotion.cadence == PromotionCadence::AfterEachQuery {
            stats.auto_promoted = self.auto_promote(&mut state, now)?;
        }
        Ok(ContextQueryResult {
            items,
            query_stats: stats,
        })
    }

    /// Materialize entailments for `scope` under its reasoner.
    ///
    /// Not yet implemented: the default OWL 2 EL reasoner has not landed.
    pub fn reason(&mut self, scope: ContextScope) -> Result<ReasoningStats, SessionError> {
        let _ = scope;
        Err(nyi(Unimplemented::OwlElReasoner))
    }

    /// Validate the `data` scope against SHACL shapes held in the `shapes`
    /// scope.
    ///
    /// Reserved: premium-only at v0.1, always refused with
    /// [`ContextError::ShaclUnavailable`].
    pub fn validate(
        &self,
        data: ContextScope,
        shapes: ContextScope,
    ) -> Result<ShaclValidationReport, SessionError> {
        let _ = (data, shapes);
        Err(ContextError::ShaclUnavailable.into())
    }

    /// Evaluate a Datalog program over `scope`.
    ///
    /// Reserved: premium-only at v0.1, always refused with
    /// [`ContextError::DatalogUnavailable`].
    pub fn evaluate_datalog(
        &self,
        program: &DatalogProgram,
        scope: ContextScope,
    ) -> Result<DatalogResult, SessionError> {
        let _ = (program, scope);
        Err(ContextError::DatalogUnavailable.into())
    }

    /// Move items from `from` to the equal-or-more-durable scope `to`,
    /// bypassing the auto-promotion thresholds. Every item must be in `from`;
    /// otherwise nothing moves. `from == to` is a no-op.
    pub fn promote(
        &mut self,
        items: &[ItemId],
        from: ContextScope,
        to: ContextScope,
    ) -> Result<PromotionStats, SessionError> {
        if to < from {
            return Err(SessionError::DurabilityViolation { from, to });
        }
        let mut state = self.lock();
        let items = check_in_scope(&state, items, from)?;
        if to == from {
            return Ok(PromotionStats::default());
        }
        for id in &items {
            self.move_item(&mut state, *id, to, Arrival::Promoted)?;
        }
        state.stats.items_promoted += items.len() as u64;
        self.enforce_bounds(&mut state, to)?;
        Ok(PromotionStats {
            promoted: items.len() as u64,
        })
    }

    /// Evict items from `scope`. Returns how many left the scope, whether
    /// dropped or demoted.
    pub fn evict(
        &mut self,
        scope: ContextScope,
        criteria: EvictionCriteria,
    ) -> Result<u64, SessionError> {
        let mut state = self.lock();
        match criteria {
            EvictionCriteria::Items(ids) => {
                let ids = check_in_scope(&state, &ids, scope)?;
                for id in &ids {
                    self.drop_item(&mut state, *id)?;
                }
                Ok(ids.len() as u64)
            }
            EvictionCriteria::Demote { items, to } => {
                if to >= scope {
                    return Err(SessionError::DurabilityViolation { from: scope, to });
                }
                let items = check_in_scope(&state, &items, scope)?;
                for id in &items {
                    self.move_item(&mut state, *id, to, Arrival::Demoted)?;
                }
                state.stats.items_demoted += items.len() as u64;
                self.enforce_bounds(&mut state, to)?;
                Ok(items.len() as u64)
            }
            EvictionCriteria::Policy { count } => {
                let victims = self.victims(&state, scope, count);
                self.evict_victims(&mut state, scope, &victims)?;
                if let Some(colder) = scope.colder() {
                    self.enforce_bounds(&mut state, colder)?;
                }
                Ok(victims.len() as u64)
            }
        }
    }

    /// A copy of `scope`'s items and its storage's quad count.
    pub fn snapshot(&self, scope: ContextScope) -> Result<ContextSnapshot, SessionError> {
        let state = self.lock();
        let items = state
            .items
            .iter()
            .filter(|(_, e)| e.scope == scope)
            .map(|(id, e)| (*id, e.item.clone()))
            .collect();
        let quad_count = self.stores.get(scope).quad_count().map_err(storage)?;
        Ok(ContextSnapshot {
            scope,
            items,
            quad_count,
        })
    }

    /// The scope `id` is in, or `None` if no live item has that id.
    pub fn scope_of(&self, id: ItemId) -> Option<ContextScope> {
        self.lock().items.get(&id).map(|e| e.scope)
    }

    /// How many items are in `scope`.
    pub fn item_count(&self, scope: ContextScope) -> usize {
        self.lock()
            .items
            .values()
            .filter(|e| e.scope == scope)
            .count()
    }

    // -- internals ---------------------------------------------------------

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Write the item to `to`'s store, then delete it from its current one.
    fn move_item(
        &self,
        state: &mut State,
        id: ItemId,
        to: ContextScope,
        arrival: Arrival,
    ) -> Result<(), SessionError> {
        let entry = state.items.get_mut(&id).expect("caller checked the id");
        let from = entry.scope;
        with_write(self.stores.get(to), |tx| {
            insert_all(tx, &record::encode(id, &entry.item))
        })?;
        delete(self.stores.get(from), id)?;
        entry.scope = to;
        entry.arrival = arrival;
        if arrival == Arrival::Demoted {
            entry.recent.clear();
        }
        Ok(())
    }

    fn drop_item(&self, state: &mut State, id: ItemId) -> Result<(), SessionError> {
        let scope = state.items[&id].scope;
        delete(self.stores.get(scope), id)?;
        state.items.remove(&id);
        state.stats.items_evicted += 1;
        Ok(())
    }

    /// Up to `count` items of `scope`, in eviction order.
    fn victims(&self, state: &State, scope: ContextScope, count: u64) -> Vec<ItemId> {
        let now = Instant::now();
        let mut candidates: Vec<(&ItemId, &Entry)> = state
            .items
            .iter()
            .filter(|(_, e)| e.scope == scope)
            .collect();
        // `candidates` is in id order, and the sort is stable: ties go to the
        // older item.
        match self.settings.eviction_policy {
            EvictionPolicy::Lru => candidates.sort_by_key(|(_, e)| e.last_touch),
            EvictionPolicy::Lfu => candidates.sort_by_key(|(_, e)| e.accesses),
            EvictionPolicy::RecencyWeighted { half_life } => candidates.sort_by(|a, b| {
                a.1.weight_at(now, half_life)
                    .total_cmp(&b.1.weight_at(now, half_life))
            }),
        }
        candidates
            .into_iter()
            .take(usize::try_from(count).unwrap_or(usize::MAX))
            .map(|(id, _)| *id)
            .collect()
    }

    /// Apply the demotion target to each victim.
    fn evict_victims(
        &self,
        state: &mut State,
        scope: ContextScope,
        victims: &[ItemId],
    ) -> Result<(), SessionError> {
        for id in victims {
            let target = match self.settings.demotion_target {
                DemotionTarget::Drop => None,
                DemotionTarget::NextColder => scope.colder(),
                DemotionTarget::OneStepThenDrop => match state.items[id].arrival {
                    Arrival::Promoted => scope.colder(),
                    Arrival::Added | Arrival::Demoted => None,
                },
            };
            match target {
                Some(colder) => {
                    self.move_item(state, *id, colder, Arrival::Demoted)?;
                    state.stats.items_demoted += 1;
                }
                None => self.drop_item(state, *id)?,
            }
        }
        Ok(())
    }

    /// Evict from `scope` down to its bound, then from each colder scope in
    /// turn (demoted items may have pushed it over).
    fn enforce_bounds(&self, state: &mut State, scope: ContextScope) -> Result<(), SessionError> {
        let mut next = Some(scope);
        while let Some(scope) = next {
            if let Some(bound) = self.settings.bounds.get(scope) {
                let count = state.items.values().filter(|e| e.scope == scope).count() as u64;
                if count > bound {
                    let victims = self.victims(state, scope, count - bound);
                    self.evict_victims(state, scope, &victims)?;
                }
            }
            next = scope.colder();
        }
        Ok(())
    }

    /// Apply both auto-promotion rules against the state as it stands, so an
    /// item moves at most one tier per call. Returns the number promoted.
    fn auto_promote(&self, state: &mut State, now: Instant) -> Result<u64, SessionError> {
        let cfg = &self.settings.promotion;
        let mut references: HashMap<ItemId, u64> = HashMap::new();
        for (id, e) in &state.items {
            let cited: BTreeSet<&ItemId> = e.item.provenance.derived_from.iter().collect();
            for c in cited.into_iter().filter(|c| *c != id) {
                *references.entry(*c).or_default() += 1;
            }
        }
        let mut to_enduring = Vec::new();
        let mut to_major = Vec::new();
        for (id, e) in &state.items {
            match e.scope {
                ContextScope::Major => {
                    if references.get(id).copied().unwrap_or(0)
                        > u64::from(cfg.major_to_enduring_references)
                    {
                        to_enduring.push(*id);
                    }
                }
                ContextScope::Minor => {
                    let in_window = e
                        .recent
                        .iter()
                        .filter(|t| now.saturating_duration_since(**t) <= cfg.minor_window)
                        .count();
                    if in_window as u64 > u64::from(cfg.minor_to_major_accesses) {
                        to_major.push(*id);
                    }
                }
                ContextScope::Enduring => {}
            }
        }
        for id in &to_enduring {
            self.move_item(state, *id, ContextScope::Enduring, Arrival::Promoted)?;
        }
        for id in &to_major {
            self.move_item(state, *id, ContextScope::Major, Arrival::Promoted)?;
        }
        let promoted = (to_enduring.len() + to_major.len()) as u64;
        state.stats.items_promoted += promoted;
        if !to_enduring.is_empty() {
            self.enforce_bounds(state, ContextScope::Enduring)?;
        } else if !to_major.is_empty() {
            self.enforce_bounds(state, ContextScope::Major)?;
        }
        Ok(promoted)
    }
}

/// Statistics from [`Session::reason`]. No reasoner ships at v0.1, so this
/// is never returned yet.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct ReasoningStats {
    /// Entailed quads added.
    pub quads_added: u64,
}

fn nyi(what: Unimplemented) -> SessionError {
    SessionError::NotYetImplemented(what)
}

fn validate(state: &State, item: &ContextItem) -> Result<(), SessionError> {
    if item.provenance.source.trim().is_empty() {
        return Err(SessionError::MissingProvenance);
    }
    if let ContextPayload::Triples(quads) = &item.payload {
        if quads.iter().any(|q| q.graph != NamedOrDefault::Default) {
            return Err(SessionError::InvalidItem {
                reason: "triples must be in the default graph".into(),
            });
        }
    }
    if let Some(v) = &item.embedding {
        if v.iter().any(|x| !x.is_finite()) {
            return Err(SessionError::InvalidItem {
                reason: "embedding values must be finite".into(),
            });
        }
    }
    if let Some(id) = item
        .provenance
        .derived_from
        .iter()
        .find(|id| !state.items.contains_key(id))
    {
        return Err(SessionError::UnknownItem(*id));
    }
    Ok(())
}

/// Check every id is a live item in `scope`; return them de-duplicated, in
/// first-seen order.
fn check_in_scope(
    state: &State,
    ids: &[ItemId],
    scope: ContextScope,
) -> Result<Vec<ItemId>, SessionError> {
    let mut seen = BTreeSet::new();
    let unique: Vec<ItemId> = ids.iter().copied().filter(|id| seen.insert(*id)).collect();
    for id in &unique {
        match state.items.get(id) {
            None => return Err(SessionError::UnknownItem(*id)),
            Some(e) if e.scope != scope => {
                return Err(SessionError::ScopeMismatch {
                    item: *id,
                    expected: scope,
                    found: e.scope,
                })
            }
            Some(_) => {}
        }
    }
    Ok(unique)
}

/// Run `f` in a write transaction: commit on `Ok`, roll back on `Err`.
fn with_write<'s, S: Storage, T>(
    store: &'s S,
    f: impl FnOnce(&mut S::Transaction<'s>) -> Result<T, SessionError>,
) -> Result<T, SessionError> {
    let mut tx = store.begin_write().map_err(storage)?;
    match f(&mut tx) {
        Ok(v) => {
            tx.commit().map_err(storage)?;
            Ok(v)
        }
        Err(e) => {
            // The original error is the one worth reporting.
            let _ = tx.rollback();
            Err(e)
        }
    }
}

fn insert_all<T: StorageTransaction>(tx: &mut T, quads: &[Quad]) -> Result<(), SessionError> {
    for q in quads {
        tx.insert(q).map_err(storage)?;
    }
    Ok(())
}

/// Delete every quad of item `id` (its graph and its metadata quad).
fn delete<S: Storage>(store: &S, id: ItemId) -> Result<(), SessionError> {
    let graph = record::item_graph(id);
    with_write(store, |tx| {
        let mut quads = collect(tx.quads_for_pattern(
            None,
            None,
            None,
            Some(&NamedOrDefault::Named(graph.clone())),
        ))?;
        quads.extend(collect(tx.quads_for_pattern(
            Some(&NamedOrBlankNode::Named(graph.clone())),
            Some(&record::record_predicate()),
            None,
            Some(&record::meta_graph()),
        ))?);
        for q in &quads {
            tx.remove(q).map_err(storage)?;
        }
        Ok(())
    })
}

/// Every item stored in `store`.
fn load<S: Storage>(store: &S) -> Result<Vec<(ItemId, ContextItem)>, SessionError> {
    let tx = store.begin_read().map_err(storage)?;
    let mut out = Vec::new();
    let records = collect(tx.quads_for_pattern(
        None,
        Some(&record::record_predicate()),
        None,
        Some(&record::meta_graph()),
    ))?;
    for q in records {
        let id = match &q.subject {
            NamedOrBlankNode::Named(n) => record::parse_item_graph(n),
            NamedOrBlankNode::Blank(_) => None,
        };
        let (Some(id), Term::Literal(lit)) = (id, &q.object) else {
            return Err(SessionError::Corrupt {
                reason: format!("unexpected metadata quad {} {}", q.subject, q.object),
            });
        };
        let graph = NamedOrDefault::Named(record::item_graph(id));
        let triples = collect(tx.quads_for_pattern(None, None, None, Some(&graph)))?;
        out.push((id, record::decode(id, lit.value(), triples)?));
    }
    tx.rollback().map_err(storage)?;
    Ok(out)
}

fn collect<E: std::error::Error + Send + Sync + 'static>(
    iter: Box<dyn Iterator<Item = Result<Quad, E>> + '_>,
) -> Result<Vec<Quad>, SessionError> {
    iter.map(|q| q.map_err(storage)).collect()
}
