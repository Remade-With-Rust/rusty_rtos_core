# Changelog

Security-relevant changes are called out under **Security** (hardening gate
H-38). Versions follow SemVer; in 0.x a minor bump may break the API.

## Unreleased

### Security
- `cargo vet` coverage (`supply-chain/`): 14 of 15 dependencies certified;
  `portable-atomic` exempted pending the owner's decision.
- `fuzz/lists_arena`: a coverage-guided fuzz target checking the arena and
  the lists against a reference model after every operation.
- A threat model (`docs/threat-model.md`) with a residual-risk register.
- CI: every action pinned to a commit SHA, `permissions: contents: read`,
  `cargo vet --locked`, the unsafe census, the hardening-table check and a
  fuzz regression per push; fuzzing, AddressSanitizer and `cargo careful`
  nightly.

### Fixed
- `generations_never_mint_a_null_handle` had no `#[test]` and had never run;
  it runs, and passes.
- CI was red at 0.2.1 (clippy `-D warnings`, `cargo fmt --check`); green.
