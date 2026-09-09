# rusty_rtos_core — hardening audit

**Standard**: Kairos (Remade With Rust) recursive hardening process — see the skill's `STANDARD.md`
**Registry**: 41 gates / 12 phases (`use-protection-please` v1)
**Unit**: `rusty_rtos_core` — package (facade + `no_std` core)
**Tier**: critical-path — an RTOS component: the kernel is the trusted computing base of every firmware above it, and every library here parses bytes from a wire, a store or a bus
**Mirrors**: none — the crate README is the only face until the first public flip; the crates.io page becomes a mirror then
**Compliance**: none — no compliance framework in scope for an embedded kernel component; revisit at 1.0.0
**Architect**: [Tim Almond](https://github.com/Ttimmahlax) — accountable for this unit's security design; rendered
at the foot of the block in every README and mirror
**Audit depth**: survey (deny, audit, clippy, Miri probes run)
**Audited**: 2026-09-09 by kairos (K0 pass) · **Next review**: K1, the first oracle diff

> Source of truth for this unit's hardening status. The README's status table is
> **generated from this file** — edit here, then run:
> `kairos harden --plan docs/plans/use-protection-please.md --readme README.md`
> (the Rust renderer in the umbrella's `tools/kairos`; `kairos check --harden`
> refuses a stale table).

**Status tokens**: `Completed` (evidenced pass) · `Scheduled` (owner + date in Target) ·
`Incomplete` (not done, or not evidenced) · `N/A` (out of tier — reason required in
Evidence; excluded from the totals).

---

## Threat sketch

*Assets* — scheduling integrity (the right task runs, priorities and timeouts hold as the C kernel's do); memory safety of every firmware above the kernel; availability (no panic reachable from an API call or a parsed byte); the integrity of the published crates.
*Adversaries* — a buggy or hostile task in the same firmware (a wrong handle, a stale handle, an ISR variant called from a task); crafted bytes on a wire, a store or a bus reaching a parser; a supply-chain actor substituting a dependency; a debugger-less field device that cannot report a fault.
*Highest-value attack path* — a parser or an API path that panics on untrusted input, taking the whole firmware down (the C kernel's `configASSERT` class), or a handle that outlives its object.
*Full model* — `docs/threat-model.md` (to be written with the first milestone; the family model is the mission plan's §2.10)

---

## Checklist

`★` = v1.0.0-blocking. Full probe and pass criteria per gate: the skill's `CHECKLIST.md`.

### Phase 0 — Threat modeling

| ID | Gate | Status | Evidence | Target |
|---|---|---|---|---|
| H-01 | ★ Threat model documented and linked from README | Incomplete | the sketch above; `docs/threat-model.md` is the K1 deliverable, written with the kernel's | |
| H-02 | Threat model revisited after last major change | Incomplete | no major change yet | |

### Phase 1 — Toolchain

| ID | Gate | Status | Evidence | Target |
|---|---|---|---|---|
| H-03 | Toolchain pinned (`rust-toolchain.toml`) | Completed | `rust-toolchain.toml`: channel 1.98.0, clippy + rustfmt, the four bare-metal targets | |
| H-04 | Committed `.cargo/config.toml` hardening defaults | Incomplete | `.cargo/config.toml` is the gitignored sibling-patch seam (`kairos patches`); linker hardening belongs to each firmware's own config | |
| H-05 | ★ Release profile hardened (overflow-checks, LTO, panic policy) | Completed | `Cargo.toml` `[profile.release]`: `overflow-checks = true`, `lto = "thin"`, `codegen-units = 1`; libraries stay unwind-safe, firmware binaries choose `panic = "abort"` | |
| H-06 | Security toolchain available to CI and developers | Incomplete | CI installs cargo-deny (`taiki-e/install-action`); audit / vet / geiger / miri / fuzz are on the developer box, not yet in CI | |

### Phase 2 — Supply chain

| ID | Gate | Status | Evidence | Target |
|---|---|---|---|---|
| H-07 | ★ `Cargo.lock` committed | Completed | `Cargo.lock` tracked in the first commit (`git ls-files Cargo.lock`) | |
| H-08 | ★ `deny.toml` policy present and enforced | Completed | `cargo deny check` 2026-09-09: advisories ok, bans ok, licenses ok, sources ok (fleet gate `kairos check --deny`; ledger) | |
| H-09 | ★ Vulnerability scan clean (`cargo audit`) | Completed | `cargo audit` 2026-09-09: 0 advisories over 17 locked crates, advisory-db of 1243 entries (ledger) | |
| H-10 | ★ `cargo vet` coverage complete | Incomplete | no `supply-chain/` yet | |
| H-11 | Unsafe inventory measured and trending down (geiger) | Incomplete | `forbid(unsafe_code)` in both crates makes the count zero by construction and `UNSAFE.md` says so; a `cargo geiger` report is not archived yet | |
| H-12 | ★ SBOM generated and published with releases | Incomplete | no release yet | |
| H-13 | Git deps pinned; no unknown registries or sources | Completed | every sibling git dep carries a `version`; `deny.toml` `[sources]` denies unknown registries and git, `allow-git` names the siblings | |
| H-14 | Dependency freshness reviewed, human-in-the-loop updates | Incomplete | no update bot yet | |

### Phase 3 — Code level

| ID | Gate | Status | Evidence | Target |
|---|---|---|---|---|
| H-15 | ★ Workspace lint policy set and clean | Completed | `[workspace.lints]`: `unsafe_code = deny`, `undocumented_unsafe_blocks`, `unwrap_used`, `expect_used`, `panic`, `todo`, `unimplemented` = deny, `indexing_slicing` + `arithmetic_side_effects` = warn; `cargo clippy --workspace --all-targets -- -D warnings` clean on the K0 types (2026-09-09) | |
| H-16 | ★ `unsafe` isolated, SAFETY-commented, inventoried | Completed | `forbid(unsafe_code)` in every crate; `UNSAFE.md` lists none | |
| H-17 | Arithmetic safety explicit | Completed | `arithmetic_side_effects = warn` under `-D warnings` is clean: every operation in `tick`, `list`, `arena`, `config`, `time` is `checked_*`, `wrapping_*` or `saturating_*` by name (`ticks_to_ms` via `checked_div`); `tests/no_panic.rs` sweeps 50k random inputs | |
| H-18 | ★ No `unwrap`/`expect`/panic on untrusted paths; typed errors | Completed | `unwrap_used`, `expect_used`, `panic` = deny at the workspace; tests opt out per file | |
| H-19 | Input validation — external bytes treated as hostile | Completed | no byte parser in this crate; every externally supplied value (`Handle::from_raw`, `Priority::new`, `Config::validate`, list and arena indices) returns `Error` on a bad input instead of panicking (`bad_arguments_are_errors_not_panics`; `tests/no_panic.rs`) | |
| H-20 | ★ Secrets zeroized; never logged | Incomplete | no secret enters this crate by design and the `Trace` seam carries handles and counts only; the statement lands in `docs/threat-model.md` (K1) | |
| H-21 | Concurrency discipline | Completed | no `static mut`, no interior mutability, no hand-written `Send`/`Sync`: every seam takes `&mut self`; `Isr` is `!Send` so an ISR token cannot cross to a task; the kernel's locking discipline is `rusty_rtos_kernel`'s own H-21 | |

### Phase 4 — Static analysis

| ID | Gate | Status | Evidence | Target |
|---|---|---|---|---|
| H-22 | Static analysis beyond the default linter runs on every PR | Incomplete | | |

### Phase 5 — Dynamic analysis

| ID | Gate | Status | Evidence | Target |
|---|---|---|---|---|
| H-23 | ★ Tests pass under Miri | Completed | `cargo +nightly miri test -p rusty_rtos_core --lib` 2026-09-09 (miri 0.1.0 of 2026-09-08): 31 unit tests pass, 77 s; the two random sweeps in `tests/no_panic.rs` are `#[cfg_attr(miri, ignore)]` (hours under the interpreter) and run natively in the same gate | |
| H-24 | Critical paths pass the sanitizers (ASan/MSan/TSan) | Incomplete | | |
| H-25 | `cargo careful test` green | Incomplete | | |

### Phase 6 — Fuzzing and properties

| ID | Gate | Status | Evidence | Target |
|---|---|---|---|---|
| H-26 | ★ Fuzz target per public parser, decoder, or message handler | Incomplete | no parser yet | |
| H-27 | ★ Continuous fuzzing with no open crashes | Incomplete | | |
| H-28 | Property tests cover the documented invariants | Completed | `tests/no_panic.rs`: 200 rounds × 500 random arena/list operations assert the documented invariants after every step (occupancy, generation monotonicity, list ordering, cursor validity) under a fixed LCG seed; unit tests pin the `list.c` tie rule and cursor semantics | |
| H-29 | Mutation and/or differential testing on critical modules | Incomplete | the C oracle differential arrives with K1 | |

### Phase 7 — Formal verification

| ID | Gate | Status | Evidence | Target |
|---|---|---|---|---|
| H-30 | Proof of panic-freedom / UB-freedom per `unsafe` module | Incomplete | no unsafe module; Kani harnesses for the CBMC proof list arrive with K2 | |

### Phase 8 — Build and binary

| ID | Gate | Status | Evidence | Target |
|---|---|---|---|---|
| H-31 | ★ Binary hardening applied and verified | N/A | a library; the firmware binaries carry this gate | |
| H-32 | Build is reproducible or fully auditable | Incomplete | | |

### Phase 9 — Runtime privilege

| ID | Gate | Status | Evidence | Target |
|---|---|---|---|---|
| H-33 | Least privilege documented and tested | N/A | a library on bare metal; the MPU package (K8) is the privilege story | |

### Phase 10 — Cryptography

| ID | Gate | Status | Evidence | Target |
|---|---|---|---|---|
| H-34 | Vetted crypto only; no bespoke primitives | N/A | no cryptography in this crate | |
| H-35 | Side-channel discipline (constant-time, no secret branches) | N/A | no secret in this crate | |
| H-36 | Post-quantum migration plan for long-lived keys | N/A | no key in this crate | |

### Phase 11 — CI/CD, release, and operations

| ID | Gate | Status | Evidence | Target |
|---|---|---|---|---|
| H-37 | CI runs the hardening gate on every PR | Incomplete | fmt + clippy + test + deny per push; audit / vet / Miri / fuzz not yet; the hardening-table check runs in the fleet gate (`kairos check --harden`), not in CI | |
| H-38 | Releases signed, attested, and changelogged for security | Incomplete | no release yet | |
| H-39 | ★ `SECURITY.md` with a coordinated disclosure process | Completed | `SECURITY.md`: contact, 5-day acknowledgement, 14-day updates, 90-day disclosure | |
| H-40 | Advisory monitoring and scheduled re-audit | Incomplete | | |
| H-41 | ★ Residual risks listed and accepted; waivers time-bounded | Incomplete | the register below carries the K0 risks (R-001..R-004, from the plan's §6); acceptance and review dates await the architect | |

### Phase 12 — Compliance controls

Only in play when a framework is declared in scope above. With none in scope, every row is
`N/A` — reason: "no compliance framework in scope". Mapping: the skill's `COMPLIANCE.md`.

| ID | Gate | Status | Evidence | Target |
|---|---|---|---|---|
| C-01 | Data inventory — personal/health/card data touched | N/A | no compliance framework in scope | |
| C-02 | Data-flow map including third-party egress | N/A | no compliance framework in scope | |
| C-03 | Encryption in transit for all egress | N/A | no compliance framework in scope | |
| C-04 | Encryption at rest for stored sensitive data | N/A | no compliance framework in scope | |
| C-05 | Key management — generation, storage, rotation, destruction | N/A | no compliance framework in scope | |
| C-06 | Retention limits and honoured deletion | N/A | no compliance framework in scope | |
| C-07 | Audit logging of security-relevant events | N/A | no compliance framework in scope | |
| C-08 | Log hygiene — no PII, secrets, or card data in logs | N/A | no compliance framework in scope | |
| C-09 | Least-privilege access to sensitive data | N/A | no compliance framework in scope | |
| C-10 | Subprocessor and third-party inventory | N/A | no compliance framework in scope | |
| C-11 | Incident response and breach notification path | N/A | no compliance framework in scope | |
| C-12 | Change management — reviewed, approved, traceable | N/A | no compliance framework in scope | |
| C-13 | Availability commitments and their evidence | N/A | no compliance framework in scope | |
| C-14 | Machine-readable SBOM + provenance for regulators | N/A | no compliance framework in scope | |

---

## Scheduled work

In execution order. Cheapest-first is usually correct: configuration gates clear in
minutes and unblock the outcome gates behind them.

| # | Gates | Work | Owner | Target | Notes |
|---|---|---|---|---|---|
| 1 | H-01, H-20 | write `docs/threat-model.md` from the sketch above, alongside the kernel's | Tim Almond | K1 | the family model is the mission plan's §2.10 |
| 2 | H-10 | `cargo vet init` and audit the 17 dev-closure crates (the `no_std` core has none) | | K1 | |
| 3 | H-06, H-22, H-37 | `cargo audit` and Miri into CI; a second linter pass | | K1 | audit and Miri already run on the developer box |
| 4 | H-11 | archive a `cargo geiger` report (expected: zero) | | K1 | minutes |

---

## Residual risk register

Every open risk carries an owner, an acceptance, and a review date (H-41).

| ID | Risk | Likelihood | Impact | Mitigation status | Accepted by | Review date |
|---|---|---|---|---|---|---|
| R-001 | `Lists` diverges from `list.c` in a corner (the `MAX_VALUE` tie rule, the cursor on removal) and the kernel trace never matches the oracle | Medium | High | unit tests pin both semantics; K1's line-for-line diff is the detector | pending (architect) | K1 |
| R-002 | `Config` consts drift from `FreeRTOS.h` defaults across kernel versions | Low | Medium | `ORACLES.md` pins the kernel; a re-pin re-runs `validate()` and the const assertions | pending (architect) | on re-pin |
| R-003 | the `u16` index/generation halves of `Handle` are too small for a large system | Low | Low | per-`Config` `MAX_*` consts; widening is a `FORMAT_VERSION` bump | pending (architect) | 1.0.0 |
| R-004 | `PosixDemoConfig` (64-bit ticks) and `DefaultConfig` (32-bit) make the kernel's tests target-shaped | Medium | Medium | the kernel is generic over `Config`; both profiles are compiled by CI | pending (architect) | K1 |

---

## Waivers

Time-bounded only. An expired waiver is an `Incomplete` gate, not a `Completed` one.

| Gate | Reason | Granted by | Expires |
|---|---|---|---|
| | | | |

---

## Audit log

Append one line per pass; never rewrite history. The trend is the point.

| Date | Depth | Auditor | Completed / Scheduled / Incomplete | ★ met | Note |
|---|---|---|---|---|---|
| 2026-09-09 | survey | kairos (scaffold pass) | 7 / 0 / 28 | 5 | first pass, at stamp time; every Completed row names a file that exists |
| 2026-09-09 | survey + tool probes | kairos (K0 pass) | 15 / 0 / 21 | 9 | deny, audit, clippy and Miri run on the developer box; evidence rows carry the K0 verdicts; risk register R-001..R-004 filled from the plan's §6, acceptance pending the architect |
