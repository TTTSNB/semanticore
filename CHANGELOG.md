# Changelog

All notable changes to the open-tier crates are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/). Before 1.0, minor versions may break APIs.

## [0.1.0] - Unreleased

First release of `semanticore-core`: the backend-neutral storage and reasoner
trait surface specified in RFCs 0002 and 0003. It contains traits and result
types only; reasoner implementations live outside this crate.

### Added
- `storage`: the storage trait (RFC 0002).
- `reasoner`: the SHACL and Datalog reasoner traits, plus the OWL reasoner traits
  `classify`, `realize` and `check_consistency` (RFC 0003).
- `shacl`, `datalog`, `owl`: the result and error types for each reasoner family,
  including `OwlError` and a `License` error variant on `ShaclError` and
  `DatalogError`.
- `ClassHierarchy::unsatisfiable`, which lists the unsatisfiable classes found by
  `classify`.

### Changed
- SHACL validation results take `focus_node` as a `Term`, not an IRI only, so blank
  node and literal focus nodes can be reported.
- SHACL `conforms` is false when a result of any severity is present, not just a
  `Violation`.
- MSRV is 1.88, to match the workspace dependency graph.
