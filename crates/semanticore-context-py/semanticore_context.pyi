"""Type stubs for semanticore_context, the Python binding for
semanticore-context (RFC 0004).

IRIs are plain ``str``; blank nodes and literals are :class:`BlankNode`
and :class:`Literal`. A triple is a ``(subject, predicate, object)`` tuple.
"""

from collections.abc import Iterable, Sequence
from datetime import timedelta
from os import PathLike
from pathlib import Path
from types import TracebackType
from typing import Any, ClassVar, Final, Literal as _L, NoReturn, final

__version__: Final[str]
__all__ = [
    "__version__",
    "BlankNode",
    "Literal",
    "ContextScope",
    "ItemId",
    "Provenance",
    "ContextItem",
    "ContextQuery",
    "ContextQueryResult",
    "RankedItem",
    "QueryStats",
    "SessionConfig",
    "ScopeBounds",
    "PromotionConfig",
    "PromotionCadence",
    "EvictionPolicy",
    "DemotionTarget",
    "EvictionCriteria",
    "Session",
    "SessionStats",
    "PromotionStats",
    "ReasoningStats",
    "ContextSnapshot",
    "SessionError",
    "CapabilityShortfallError",
    "MissingProvenanceError",
    "InvalidItemError",
    "UnknownItemError",
    "ScopeMismatchError",
    "DurabilityViolationError",
    "PremiumOnlyError",
    "ShaclUnavailableError",
    "DatalogUnavailableError",
    "NotYetImplementedError",
    "CorruptStoreError",
    "StorageError",
    "SessionClosedError",
]

_Subject = str | BlankNode
_Predicate = str
_Object = str | BlankNode | Literal
_Triple = tuple[_Subject, _Predicate, _Object]
#: ``None`` = all scopes (most durable first); one scope; or several, in order.
_Scopes = ContextScope | Iterable[ContextScope] | None
#: A number of seconds or a ``timedelta``.
_DurationLike = float | int | timedelta
#: A JSON-compatible value (an event body).
_Json = None | bool | int | float | str | list[_Json] | tuple[_Json, ...] | dict[str, _Json]

# ---------------------------------------------------------------------------
# RDF terms
# ---------------------------------------------------------------------------

@final
class BlankNode:
    """An RDF blank node."""

    def __new__(cls, id: str) -> BlankNode: ...
    @property
    def id(self) -> str: ...
    def __eq__(self, other: object, /) -> bool: ...
    def __hash__(self) -> int: ...

@final
class Literal:
    """An RDF literal. ``Literal(value)`` is an ``xsd:string``; pass
    ``datatype`` (an IRI) or ``language`` (a tag), not both."""

    def __new__(
        cls, value: str, datatype: str | None = None, language: str | None = None
    ) -> Literal: ...
    @property
    def value(self) -> str: ...
    @property
    def datatype(self) -> str: ...
    @property
    def language(self) -> str | None: ...
    def __eq__(self, other: object, /) -> bool: ...
    def __hash__(self) -> int: ...

# ---------------------------------------------------------------------------
# _Scopes, ids, items
# ---------------------------------------------------------------------------

@final
class ContextScope:
    """The three context tiers, ordered by durability:
    ``MINOR < MAJOR < ENDURING``."""

    MINOR: ClassVar[ContextScope]
    MAJOR: ClassVar[ContextScope]
    ENDURING: ClassVar[ContextScope]
    #: Every scope, most durable first.
    ALL: ClassVar[list[ContextScope]]

    def colder(self) -> ContextScope | None:
        """The next less durable scope, or ``None`` for ``MINOR``."""

    def __eq__(self, other: object, /) -> bool: ...
    def __lt__(self, other: ContextScope, /) -> bool: ...
    def __le__(self, other: ContextScope, /) -> bool: ...
    def __gt__(self, other: ContextScope, /) -> bool: ...
    def __ge__(self, other: ContextScope, /) -> bool: ...
    def __hash__(self) -> int: ...
    def __int__(self) -> int: ...

@final
class ItemId:
    """An opaque item handle, stable for the lifetime of its Session.
    Obtained from ``Session.add``; not constructible."""

    def __eq__(self, other: object, /) -> bool: ...
    def __lt__(self, other: ItemId, /) -> bool: ...
    def __le__(self, other: ItemId, /) -> bool: ...
    def __gt__(self, other: ItemId, /) -> bool: ...
    def __ge__(self, other: ItemId, /) -> bool: ...
    def __hash__(self) -> int: ...

@final
class Provenance:
    """Where an item came from. ``source`` must not be empty: the Session
    refuses items without provenance."""

    def __new__(cls, source: str, derived_from: Sequence[ItemId] = ...) -> Provenance: ...
    @property
    def source(self) -> str: ...
    @property
    def derived_from(self) -> list[ItemId]: ...
    def __eq__(self, other: object, /) -> bool: ...

_ProvenanceLike = Provenance | str

@final
class ContextItem:
    """The unit of content: a payload plus provenance, an optional scope
    hint and an optional embedding. Immutable."""

    @staticmethod
    def from_triples(
        triples: Iterable[_Triple],
        provenance: _ProvenanceLike,
        *,
        scope_hint: ContextScope | None = None,
        embedding: Sequence[float] | None = None,
    ) -> ContextItem: ...
    @staticmethod
    def event(
        kind: str,
        body: _Json,
        provenance: _ProvenanceLike,
        *,
        scope_hint: ContextScope | None = None,
        embedding: Sequence[float] | None = None,
    ) -> ContextItem: ...
    @staticmethod
    def chunk(
        content: str,
        content_type: str,
        provenance: _ProvenanceLike,
        *,
        scope_hint: ContextScope | None = None,
        embedding: Sequence[float] | None = None,
    ) -> ContextItem: ...
    @property
    def payload_type(self) -> _L["triples", "event", "chunk"]: ...
    @property
    def triples(self) -> list[_Triple] | None: ...
    @property
    def event_kind(self) -> str | None: ...
    @property
    def event_body(self) -> Any: ...
    @property
    def content(self) -> str | None: ...
    @property
    def content_type(self) -> str | None: ...
    @property
    def provenance(self) -> Provenance: ...
    @property
    def scope_hint(self) -> ContextScope | None: ...
    @property
    def embedding(self) -> list[float] | None: ...
    def with_provenance(self, provenance: _ProvenanceLike) -> ContextItem:
        """A copy with a different provenance."""

    def __eq__(self, other: object, /) -> bool: ...

# ---------------------------------------------------------------------------
# Queries
# ---------------------------------------------------------------------------

@final
class ContextQuery:
    """A query against a Session. Immutable; ``with_*`` return copies."""

    @staticmethod
    def pattern(
        subject: _Subject | None = None,
        predicate: _Predicate | None = None,
        object: _Object | None = None,
        *,
        scopes: _Scopes = None,
        limit: int | None = None,
        reasoner_hint: ContextScope | None = None,
    ) -> ContextQuery:
        """Match items whose triples match the pattern; ``None`` matches
        anything. Every hit scores 1.0."""

    @staticmethod
    def sparql(
        query: str,
        *,
        scopes: _Scopes = None,
        limit: int | None = None,
        reasoner_hint: ContextScope | None = None,
    ) -> ContextQuery:
        """Not yet implemented: ``Session.query`` raises
        ``NotYetImplementedError``."""

    @staticmethod
    def similarity(
        query_embedding: Sequence[float],
        top_k: int,
        *,
        scopes: _Scopes = None,
        limit: int | None = None,
        reasoner_hint: ContextScope | None = None,
    ) -> ContextQuery:
        """Not yet implemented: ``Session.query`` raises
        ``NotYetImplementedError``."""

    @staticmethod
    def inference(
        subject: _Subject | None = None,
        predicate: _Predicate | None = None,
        object: _Object | None = None,
        *,
        scopes: _Scopes = None,
        limit: int | None = None,
        reasoner_hint: ContextScope | None = None,
    ) -> ContextQuery:
        """Not yet implemented: ``Session.query`` raises
        ``NotYetImplementedError``."""

    def with_limit(self, limit: int) -> ContextQuery: ...
    def with_reasoner_hint(self, scope: ContextScope) -> ContextQuery:
        """Query-time reasoning is not yet implemented: ``Session.query``
        raises ``NotYetImplementedError`` for any hint."""

    @property
    def kind(self) -> _L["pattern", "sparql", "similarity", "inference"]: ...
    @property
    def scopes(self) -> ContextScope | list[ContextScope] | None: ...
    @property
    def limit(self) -> int | None: ...
    @property
    def reasoner_hint(self) -> ContextScope | None: ...
    def __eq__(self, other: object, /) -> bool: ...

@final
class RankedItem:
    @property
    def id(self) -> ItemId: ...
    @property
    def item(self) -> ContextItem: ...
    @property
    def score(self) -> float: ...
    @property
    def source_scope(self) -> ContextScope:
        """The scope that served the item (before any auto-promotion)."""

    def __eq__(self, other: object, /) -> bool: ...

@final
class QueryStats:
    @property
    def scopes_consulted(self) -> int: ...
    @property
    def quads_matched(self) -> int: ...
    @property
    def auto_promoted(self) -> int: ...
    def __eq__(self, other: object, /) -> bool: ...

@final
class ContextQueryResult:
    @property
    def items(self) -> list[RankedItem]:
        """Highest score first; ties in scope-selection order, then oldest
        item first."""

    @property
    def query_stats(self) -> QueryStats: ...
    def __eq__(self, other: object, /) -> bool: ...

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------

@final
class ScopeBounds:
    """Per-scope item bounds; ``None`` is unbounded. Exceeding a bound
    triggers eviction, not an error."""

    enduring_max_items: int | None
    major_max_items: int | None
    minor_max_items: int | None
    def __new__(
        cls,
        *,
        enduring_max_items: int | None = None,
        major_max_items: int | None = None,
        minor_max_items: int | None = None,
    ) -> ScopeBounds: ...
    def __eq__(self, other: object, /) -> bool: ...

@final
class PromotionCadence:
    AFTER_EACH_QUERY: ClassVar[PromotionCadence]
    NEVER: ClassVar[PromotionCadence]
    def __eq__(self, other: object, /) -> bool: ...
    def __hash__(self) -> int: ...
    def __int__(self) -> int: ...

@final
class PromotionConfig:
    """Auto-promotion thresholds. Unset arguments take the Rust defaults
    (more than 3 accesses within 60 s; more than 5 references)."""

    cadence: PromotionCadence
    minor_to_major_accesses: int
    major_to_enduring_references: int
    def __new__(
        cls,
        *,
        cadence: PromotionCadence | None = None,
        minor_to_major_accesses: int | None = None,
        minor_window: _DurationLike | None = None,
        major_to_enduring_references: int | None = None,
    ) -> PromotionConfig: ...
    @property
    def minor_window(self) -> float:
        """The access window in seconds. Set with seconds or a ``timedelta``."""

    @minor_window.setter
    def minor_window(self, value: _DurationLike) -> None: ...
    def __eq__(self, other: object, /) -> bool: ...

@final
class EvictionPolicy:
    @staticmethod
    def lru() -> EvictionPolicy:
        """Least recently touched first (the default)."""

    @staticmethod
    def lfu() -> EvictionPolicy:
        """Fewest query accesses first."""

    @staticmethod
    def recency_weighted(half_life: _DurationLike) -> EvictionPolicy:
        """Lowest decayed touch weight first."""

    @property
    def kind(self) -> _L["lru", "lfu", "recency_weighted"]: ...
    @property
    def half_life(self) -> float | None: ...
    def __eq__(self, other: object, /) -> bool: ...

@final
class DemotionTarget:
    DROP: ClassVar[DemotionTarget]
    NEXT_COLDER: ClassVar[DemotionTarget]
    ONE_STEP_THEN_DROP: ClassVar[DemotionTarget]
    def __eq__(self, other: object, /) -> bool: ...
    def __hash__(self) -> int: ...
    def __int__(self) -> int: ...

@final
class SessionConfig:
    """How to open a Session. ``bounds`` and ``promotion`` are live
    objects: ``cfg.bounds.major_max_items = 2`` changes this config."""

    bounds: ScopeBounds
    eviction_policy: EvictionPolicy
    demotion_target: DemotionTarget
    promotion: PromotionConfig
    @staticmethod
    def oxigraph(
        enduring_path: str | PathLike[str],
        *,
        bounds: ScopeBounds | None = None,
        eviction_policy: EvictionPolicy | None = None,
        demotion_target: DemotionTarget | None = None,
        promotion: PromotionConfig | None = None,
    ) -> SessionConfig:
        """The OSS default: a file-backed Oxigraph enduring store at
        ``enduring_path``, in-memory major and minor stores. The stores are
        opened by ``Session.open``."""

    @property
    def enduring_path(self) -> Path: ...

@final
class EvictionCriteria:
    @staticmethod
    def items(item_ids: Sequence[ItemId]) -> EvictionCriteria:
        """Drop these items."""

    @staticmethod
    def demote(item_ids: Sequence[ItemId], to: ContextScope) -> EvictionCriteria:
        """Move these items to the strictly colder scope ``to``."""

    @staticmethod
    def policy(count: int) -> EvictionCriteria:
        """Let the eviction policy pick ``count`` items."""

    def __eq__(self, other: object, /) -> bool: ...

# ---------------------------------------------------------------------------
# Session
# ---------------------------------------------------------------------------

@final
class SessionStats:
    @property
    def items_added(self) -> int: ...
    @property
    def queries_served(self) -> int: ...
    @property
    def reasoning_passes(self) -> int: ...
    @property
    def items_promoted(self) -> int: ...
    @property
    def items_demoted(self) -> int: ...
    @property
    def items_evicted(self) -> int: ...
    def __eq__(self, other: object, /) -> bool: ...

@final
class PromotionStats:
    @property
    def promoted(self) -> int: ...
    def __eq__(self, other: object, /) -> bool: ...

@final
class ReasoningStats:
    @property
    def quads_added(self) -> int: ...
    def __eq__(self, other: object, /) -> bool: ...

@final
class ContextSnapshot:
    @property
    def scope(self) -> ContextScope: ...
    @property
    def items(self) -> list[tuple[ItemId, ContextItem]]: ...
    @property
    def quad_count(self) -> int: ...
    def __eq__(self, other: object, /) -> bool: ...

@final
class Session:
    """A Context OS session over the enduring, major and minor tiers.

    Synchronous; calls release the GIL. Use it as a context manager so it is
    always closed. Every method raises ``SessionClosedError`` after
    ``close``, except ``close`` itself.
    """

    @classmethod
    def open(cls, config: SessionConfig) -> Session: ...
    def close(self) -> SessionStats:
        """Close and return the counters. Idempotent."""

    @property
    def closed(self) -> bool: ...
    def __enter__(self) -> Session: ...
    def __exit__(
        self,
        exc_type: type[BaseException] | None = None,
        exc: BaseException | None = None,
        tb: TracebackType | None = None,
    ) -> bool: ...
    def add(self, scope: ContextScope, item: ContextItem) -> ItemId: ...
    def add_batch(self, scope: ContextScope, items: Sequence[ContextItem]) -> list[ItemId]: ...
    def remove(self, scope: ContextScope, item_id: ItemId) -> bool: ...
    def query(self, query: ContextQuery) -> ContextQueryResult: ...
    def reason(self, scope: ContextScope) -> ReasoningStats:
        """Not yet implemented: raises ``NotYetImplementedError``."""

    def validate(self, data: ContextScope, shapes: ContextScope) -> NoReturn:
        """Premium-only at v0.1: raises ``ShaclUnavailableError``."""

    def evaluate_datalog(self, scope: ContextScope) -> NoReturn:
        """Premium-only at v0.1: raises ``DatalogUnavailableError``."""

    def promote(
        self, item_ids: Sequence[ItemId], from_scope: ContextScope, to_scope: ContextScope
    ) -> PromotionStats: ...
    def evict(self, scope: ContextScope, criteria: EvictionCriteria) -> int: ...
    def snapshot(self, scope: ContextScope) -> ContextSnapshot: ...
    def scope_of(self, item_id: ItemId) -> ContextScope | None: ...
    def item_count(self, scope: ContextScope) -> int: ...

# ---------------------------------------------------------------------------
# Exceptions
# ---------------------------------------------------------------------------

class SessionError(Exception):
    """Base class for every error raised by a Session."""

class CapabilityShortfallError(SessionError):
    scope: ContextScope
    #: The missing storage capability, e.g. ``"Persistent"``.
    required: str

class MissingProvenanceError(SessionError): ...

class InvalidItemError(SessionError):
    reason: str

class UnknownItemError(SessionError):
    item_id: ItemId

class ScopeMismatchError(SessionError):
    item_id: ItemId
    expected: ContextScope
    found: ContextScope

class DurabilityViolationError(SessionError):
    from_scope: ContextScope
    to_scope: ContextScope

class PremiumOnlyError(SessionError):
    """Reserved for the premium tier at v0.1."""

class ShaclUnavailableError(PremiumOnlyError): ...
class DatalogUnavailableError(PremiumOnlyError): ...

class NotYetImplementedError(SessionError):
    """Specified by RFC 0004 but not implemented in this release."""

    feature: _L["owl_el_reasoner", "ann_index", "sparql", "unknown"]

class CorruptStoreError(SessionError):
    reason: str

class StorageError(SessionError): ...

class SessionClosedError(SessionError):
    """The Session has been closed (binding lifecycle; no Rust counterpart)."""
