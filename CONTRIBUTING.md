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
git clone https://github.com/TTTSNB/semanticore.git
cd semanticore
cargo build
cargo test
```

We strongly suggest a pre-commit hook that runs the same checks CI runs:

```sh
cargo fmt --check && cargo clippy -- -D warnings && cargo test
```

Drop that into `.git/hooks/pre-commit` and `chmod +x` it; CI will reject any patch that doesn't pass these three gates, so catching it locally saves a round trip.

## CI checks

`.github/workflows/ci.yml` runs these jobs on every pull request to `main`:

| Job | What it runs | Blocking |
| --- | --- | --- |
| `lint` | `cargo fmt --check`, `cargo clippy -D warnings` | yes (required check) |
| `test` | `cargo test --all --all-features` on stable | yes (required check) |
| `msrv (1.88)` | build and test on exactly Rust 1.88, the declared `rust-version` | yes |
| `docs` | `cargo doc --no-deps` with `RUSTDOCFLAGS="-D warnings"`, `cargo test --doc` | yes |
| `publish dry-run` | `cargo publish --dry-run -p semanticore-core` | yes |
| `cargo-deny` | `cargo deny check` against `deny.toml` (advisories, licenses, bans, sources) | yes |
| `semver-checks` | `cargo semver-checks` against the latest crates.io release | informational |

To run the extra gates locally:

```sh
cargo +1.88 test --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo test --workspace --doc --all-features
cargo publish --dry-run -p semanticore-core
cargo install cargo-deny --locked && cargo deny check
cargo install cargo-semver-checks --locked && cargo semver-checks -p semanticore-core
```

If you raise the MSRV, update `rust-version` in the root `Cargo.toml` and the toolchain and job name of the `msrv` job in the same commit.

All actions in the workflow are pinned to full commit SHAs, with the tag recorded in a trailing comment. Update both when bumping an action.

### Semver checks

`semver-checks` compares the public API of `semanticore-core` against its latest release on crates.io. Until 0.1.0 is published there is nothing to compare against, so the job checks crates.io, emits a "skipped" notice, and passes. It is also marked `continue-on-error: true`, so any violation it reports does not fail the workflow.

To make it blocking once 0.1.0 is on crates.io:

1. In `.github/workflows/ci.yml`, delete `continue-on-error: true` from the `semver` job.
2. In the same job, delete the "Look for a published baseline" step and the `if:` line on the "cargo semver-checks" step, so the check always runs (a missing baseline then fails loudly instead of skipping).
3. In the repository's branch protection rule for `main`, add `semver-checks` to the required status checks.

After that, a breaking API change needs a version bump that semver allows (for 0.x, a minor bump such as 0.1 → 0.2) before it can merge.

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
