# Contributing to semantiCore

Thanks for your interest in contributing. semantiCore is a Rust-core project with Python and TypeScript bindings, distributed under Apache 2.0 in its open tier. This guide covers what you need to get a working local environment and what we expect from patches.

## Development setup

You need a stable Rust toolchain with `rustfmt` and `clippy` components.

```sh
rustup toolchain install stable
rustup component add rustfmt clippy
```

Clone, build, and test:

```sh
git clone https://github.com/elliott_risch/semanticore.git
cd semanticore
cargo build
cargo test
```

We strongly suggest a pre-commit hook that runs the same checks CI runs:

```sh
cargo fmt --check && cargo clippy -- -D warnings && cargo test
```

Drop that into `.git/hooks/pre-commit` and `chmod +x` it; CI will reject any patch that doesn't pass these three gates, so catching it locally saves a round trip.

## Commit conventions

semantiCore uses [Conventional Commits](https://www.conventionalcommits.org/). Every commit subject must start with one of:

- `feat:` a new user-facing capability
- `fix:` a bug fix
- `docs:` documentation only
- `chore:` tooling, build, or repo metadata
- `refactor:` behavior-preserving code change
- `test:` test-only change
- `ci:` CI configuration change

Subject lines stay at or below 72 characters. Bodies wrap at 72 columns. When a commit corresponds to a task in the rebuild plan, reference the task ID in the body (for example, `T1.2`).

## DCO

semantiCore uses the [Developer Certificate of Origin](https://developercertificate.org). Every commit must be signed off:

```sh
git commit -s -m "feat: add foo"
```

Signing off attests that you wrote the change yourself, or have the right to submit it under the project's license. The `-s` flag adds a `Signed-off-by:` trailer with your name and email; commits without that trailer will be rejected.

## RFC process

Non-trivial design changes go through a written RFC under `docs/rfcs/NNNN-title.md` before substantial implementation lands. The full RFC template and process arrive in the architecture phase; this section is a forward reference so contributors know to expect it. For now: if you're proposing a cross-crate API, a wire-format change, or a new public subsystem, open a discussion before writing the code.

## Code of conduct

Be kind, be precise, be useful. Disagreements get resolved on technical merits. Personal attacks, harassment, and bad-faith engagement get you removed without further discussion.
