# Agent notes

- Never add GitHub Actions workflows (`.github/workflows/**`) or other CI
  configuration to this repository. The owner does not want CI on GitHub; run
  the gate locally instead: `cargo fmt --check`, `cargo clippy --all-targets
  -- -D warnings`, `cargo test`.
- Follow SemVer on every change and bump the version in `Cargo.toml`,
  `Cargo.lock` and `CHANGELOG.md`, then tag `vX.Y.Z`, when the owner asks for a
  release. Bug fixes are patch, features minor, breaking changes major.
- Keep changes minimal and run the local gate before claiming work is done.
