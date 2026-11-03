"""Shared fixtures, mirroring crates/semanticore-context/tests/common/mod.rs."""

from __future__ import annotations

from collections.abc import Callable, Iterator

import pytest

from semanticore_context import (
    ContextItem,
    ContextQuery,
    ContextScope,
    ItemId,
    Provenance,
    Session,
    SessionConfig,
)

EX = "https://example.org/"


def ex(s: str) -> str:
    return EX + s


def triple(s: str, p: str, o: object) -> tuple:
    return (ex(s), ex(p), o)


def prov() -> Provenance:
    return Provenance("test")


def fact(s: str, p: str, o: str) -> ContextItem:
    """A one-triple item: ``<s> <p> <o>``."""
    return ContextItem.from_triples([triple(s, p, ex(o))], prov())


def everything_in(scope: ContextScope) -> ContextQuery:
    return ContextQuery.pattern(scopes=scope)


def subject_in(scope: ContextScope, s: str) -> ContextQuery:
    return ContextQuery.pattern(ex(s), scopes=scope)


def ids(session: Session, q: ContextQuery) -> list[ItemId]:
    return [r.id for r in session.query(q).items]


@pytest.fixture
def config(tmp_path) -> SessionConfig:
    """The default OSS config with the enduring tier under ``tmp_path``."""
    return SessionConfig.oxigraph(tmp_path / "enduring")


@pytest.fixture
def session_with(tmp_path) -> Iterator[Callable[..., Session]]:
    """Open a Session over a fresh config after applying ``tweak`` to it."""
    opened: list[Session] = []

    def make(tweak: Callable[[SessionConfig], None] = lambda c: None) -> Session:
        cfg = SessionConfig.oxigraph(tmp_path / f"enduring-{len(opened)}")
        tweak(cfg)
        s = Session.open(cfg)
        opened.append(s)
        return s

    yield make
    for s in opened:
        s.close()


@pytest.fixture
def session(session_with) -> Session:
    return session_with()
