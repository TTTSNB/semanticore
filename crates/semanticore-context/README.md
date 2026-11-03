# semanticore-context

The semantiCore Context OS (RFC 0004): a `Session` over three context tiers,
each backed by its own `Storage` (RFC 0002).

| Scope | Lifetime | OSS default storage |
|---|---|---|
| `Enduring` | survives the Session | file-backed Oxigraph |
| `Major` | the Session | in-memory Oxigraph |
| `Minor` | the Session (meant for one turn) | in-memory Oxigraph |

```rust
use semanticore_context::{ContextItem, ContextScope, Provenance, Session, SessionConfig};

let mut session = Session::open(SessionConfig::oxigraph("data/enduring")?)?;
let id = session.add(ContextScope::Minor, ContextItem::chunk("hi", "text/plain", Provenance::new("chat")))?;
session.promote(&[id], ContextScope::Minor, ContextScope::Enduring)?;
session.close()?;
```

## Status: v0.1 skeleton

Implemented: the Session lifecycle; adding, removing and pattern-querying
items; explicit and automatic promotion; eviction, demotion and per-scope
bounds; reloading the enduring tier on open.

Reserved: SHACL (`Session::validate`) and Datalog
(`Session::evaluate_datalog`) are premium-only at v0.1 and return
`SessionError::PremiumOnly`.

Not yet implemented, and returning `SessionError::NotYetImplemented`: the
default OWL 2 EL reasoner (`Session::reason`, inference queries, reasoner
hints), the default ANN/HNSW index (similarity queries) and SPARQL queries.
The Python binding is `semanticore-context-py`; the TypeScript binding does
not exist yet.

Licensed under Apache-2.0.
