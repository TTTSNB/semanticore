# semanticore-context-py

Python binding for [`semanticore-context`](../semanticore-context), the
semantiCore Context OS (RFC 0004 "Bindings story"). Built with
[PyO3](https://pyo3.rs) and [maturin](https://www.maturin.rs) as an abi3
wheel for CPython 3.10 and later.

```python
from semanticore_context import ContextItem, ContextQuery, ContextScope, Session, SessionConfig

with Session.open(SessionConfig.oxigraph("data/enduring")) as session:
    item = ContextItem.from_triples([("urn:alice", "urn:knows", "urn:bob")], "chat")
    item_id = session.add(ContextScope.MAJOR, item)

    result = session.query(ContextQuery.pattern("urn:alice"))
    for ranked in result.items:
        print(ranked.score, ranked.item)

    # The user confirmed it: keep it beyond this Session.
    session.promote([item_id], ContextScope.MAJOR, ContextScope.ENDURING)
```

The API is synchronous; calls release the GIL. Type hints ship in
`semanticore_context.pyi`.

## Mapping from the Rust crate

| Rust | Python |
|---|---|
| `Session::open` / `close` | `Session.open(config)` / `session.close()`, or `with` |
| `SessionConfig::oxigraph(path)` | `SessionConfig.oxigraph(path, *, bounds=..., ...)` |
| `ContextScope::Major` | `ContextScope.MAJOR` |
| `ContextItem::triples(quads, prov)` | `ContextItem.from_triples([(s, p, o)], prov)` |
| `Provenance::new(src)` | `Provenance(src)` or a bare `str` |
| `ContextQuery::pattern(scopes, pattern)` | `ContextQuery.pattern(s, p, o, *, scopes=None)` |
| `ScopeSelection::{All, Single, Set}` | `scopes=None`, `scopes=scope`, `scopes=[...]` |
| `EvictionCriteria::Demote { .. }` | `EvictionCriteria.demote(ids, to)` |
| `SessionError::PremiumOnly(..)` | `PremiumOnlyError` (`ShaclUnavailableError`, `DatalogUnavailableError`) |
| `SessionError::NotYetImplemented(..)` | `NotYetImplementedError` (`.feature`) |

RDF terms: IRIs are `str`; blank nodes and literals are `BlankNode` and
`Literal`. Every `SessionError` variant has its own exception class,
subclassing `SessionError`; `SessionClosedError` is raised by calls on a
closed Session.

## Development

```sh
python -m venv .venv && . .venv/bin/activate
pip install "maturin>=1.16,<2" pytest
maturin develop -m crates/semanticore-context-py/Cargo.toml
pytest crates/semanticore-context-py/tests
```

`cargo test -p semanticore-context-py` runs the Rust-side tests of the
conversion layer against an embedded interpreter; it needs a CPython
>= 3.10 shared library (`libpython3.x.so`).

Nothing is published to PyPI or crates.io from this crate yet.

Licensed under Apache-2.0.
