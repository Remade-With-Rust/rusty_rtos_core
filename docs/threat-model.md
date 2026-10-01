# Threat model — `rusty_rtos_core`

**Unit tier:** critical-path. **Model version:** 1, 2026-10-01.
**Scope:** `rusty_rtos_core` — the family's shared vocabulary: generational
handles, the arena, the intrusive lists, tick and time arithmetic, the
`Config` trait and the `Port` / `Trace` / `TickHook` seams — and
`rusty_rtos_alloc`, the allocator seam over `rusty_alloc`. The kernel, the
ports and the heap are separate units with their own models; where a risk
belongs to one of them this document says so and stops.

Satisfies `use-protection-please` **H-01**. The secrets position is §5
(**H-20**); the residual-risk register is §7 (**H-41**).

---

## 1. What this unit is, in one paragraph

Every Kairos crate is built from these types. The scheduler's ready, delayed
and event lists ARE this crate's lists; every kernel object is a slot in this
crate's arena, named by this crate's handles. So this unit has no API of its
own that an end user calls, and yet a defect here is a defect in every
scheduling decision and every object lookup above it. The C it replaces is
`list.c` and the pointer-as-handle convention, whose known failure is silent
corruption: a double insert or a remove of an unlinked item breaks a list and
nothing notices until a task never runs again.

---

## 2. Assets

| asset | why it matters | what failure looks like |
|---|---|---|
| **List integrity** | the scheduler's every decision walks these lists | an item in two lists, a length that disagrees with the walk, a task that is never selected again |
| **Handle integrity** | a handle is the only way any caller reaches a kernel object | a forged or stale handle resolving to a live object it does not name |
| **Arithmetic correctness** | ticks wrap; time conversions multiply | a timeout that fires early after a wrap, a delay that overflows into "never" |
| **Availability** | a panic in `no_std` is a halt | the whole firmware down from a value a caller chose |
| **Integrity of the published crates** | downstream builds resolve them by name | a substituted dependency executing at build time or on the device |

---

## 3. Adversaries and what they can do

1. **A buggy or hostile task in the same firmware**, reaching this crate
   through the kernel's API: it chooses handle values (including forged and
   stale ones), list and item ids through any API that takes them, tick counts
   and durations at their extremes.
2. **A C caller through the C ABI**, which can construct any 32-bit handle,
   including ones Rust code could never hold.
3. **A supply-chain actor** substituting a dependency or a build script.

**Out of scope:** physical attacks, side channels, a compromised toolchain,
and memory corruption by code that is itself `unsafe` (the ports and the
allocator own theirs).

---

## 4. The attack paths, and the evidence against each

### 4.1 A forged or stale handle

*Mitigation:* a handle is an index and a **generation**, never a pointer.
Removing a value bumps its slot's generation, so every copy of the old handle
names a generation that no longer exists and is refused. A free slot is
marked with a bit ABOVE the sixteen generation bits `from_raw` can produce,
so no handle a C caller can construct resolves to a free slot.

*Evidence:* `no_forgeable_handle_resolves_to_a_free_slot` checks it
EXHAUSTIVELY over the whole forgeable generation space, with every slot in
turn the live one. `generations_never_mint_a_null_handle` (70,000 cycles of
insert/remove on one slot) proves the generation never wraps into the null
handle; until 2026-10-01 that test had silently never run (its `#[test]` sat
on the test above it), which is how it came to be in this list. And
`fuzz/lists_arena` checks, against a reference model after every operation,
that live handles resolve to exactly their values and removed or invented
handles never do.

### 4.2 A corrupted list

*Mitigation:* every list operation validates its ids and answers an `Error`
where the C corrupts: a double insert is `Busy`, a remove of an unlinked
item is `NotActive`, an id naming no item is `InvalidArgument`. The end
markers live in the same node array behind ids no caller is handed.

*Evidence:* `tests/no_panic.rs` drives 100,000 random operations over lists
and the arena, checking after every one that each list's length equals its
walk and every walked item names its container. `fuzz/lists_arena` goes
further, coverage-guided and differential: it mirrors every operation that
answered `Ok` in a model and fails on ANY disagreement — membership, length,
`remove`'s remaining count, stored values.

### 4.3 Arithmetic at the edges

*Mitigation:* `Tick` is width-parametric (16, 32, 64 bits) with explicit
`wrapping_` / `checked_` / `since` / `overflows_by` arithmetic; unchecked
arithmetic is lint-denied in library code (`clippy::arithmetic_side_effects`),
and the two places it is allowed say why at the site.

*Evidence:* `tests/no_panic.rs` sweeps 50,000 edge values (0, 1, MAX, MAX-1,
`u32::MAX`, random) through every tick, duration and timeout conversion at
every width.

### 4.4 A panic reachable from a caller's value

*Mitigation:* both crates are `#![forbid(unsafe_code)]`; `unwrap`, `expect`,
`panic`, indexing and unchecked arithmetic are lint-denied in library code,
and every fallible path returns `Result`. CI runs clippy with `-D warnings`.

*Evidence:* the two suites above run under `catch_unwind` and name the input
that panicked; `fuzz/lists_arena` runs coverage-guided.

### 4.5 A substituted dependency

*Mitigation:* `Cargo.lock` is committed and resolves in a fresh clone;
`cargo deny` denies `*-sys`, `ring`, `aws-lc-sys` and `libc`-linked crates.
`cargo vet` (`supply-chain/`) covers 14 of the 15 dependencies: the house's
own by publisher, the rest by the imported Mozilla / Google / ISRG / zcash /
Bytecode Alliance / Embark / ariel-os audit sets. CI runs `cargo deny check`
and `cargo vet --locked` on every push.

*Residual:* `portable-atomic` is exempted — §7.

---

## 5. Secrets — H-20

**No secret enters this unit, by design.** It defines data structures and
arithmetic; it holds no key material, no credentials and no entropy source,
and it logs nothing (the `Trace` seam is implemented by the kernel and its
users, not here). There is nothing to zeroize.

**This is a constraint, not an observation.** The lists and the arena hold
whatever values their users store; a user that stores a secret in an arena
slot owns zeroizing it, and if this crate ever gains a type meant to hold
key material, H-20 reopens here.

---

## 6. Assumptions this model depends on

1. **`rusty_alloc` is sound.** `rusty_rtos_alloc` forbids `unsafe` itself but
   hands out `rusty_alloc`'s allocator, whose `unsafe` is audited in that
   unit, at an EXACT pin (`=2.2.1`) because earlier releases carried
   use-after-frees.
2. **The kernel calls the list API correctly where it relies on invariants
   it established itself** (e.g. `unlink` of an item it knows is linked).
   Those call sites are the kernel's to evidence, and are, by its
   conformance corpus.

---

## 7. Residual risks — H-41

Every row has an owner, the reason it is accepted, and the condition that
closes it. **Owner:** the Architect named in the README's hardening block.
**Review:** at every release, and no later than 2027-01-01.

| # | residual risk | severity | why accepted for now | closes when |
|---|---|---|---|---|
| R-1 | **`portable-atomic` is exempted in `cargo vet`** (H-10). It is a dependency of `rusty_alloc`, published by a GitHub trusted publisher that none of the imported audit sets trusts. | low | 30k lines; it is the allocator's fallback for targets without native atomics, and `cargo deny` still checks it for advisories | the owner trusts the publisher (`cargo vet trust portable-atomic github:taiki-e/portable-atomic`) or an audit is certified |
| R-2 | **Continuous fuzzing has not yet run 30 days** (H-27). The target exists and ran clean locally; the nightly job is in `scheduled.yml`. | medium | the gate is calendar time, not engineering | 30 nights of `scheduled.yml` with no open crash |
| R-3 | **No sanitizer or `cargo careful` run** (H-24, H-25). | low | both crates forbid `unsafe`, so the classes these tools catch are confined to `rusty_alloc`, a separate unit | a sanitizer CI job runs the test suite |
| R-4 | **Releases are not signed** (H-38). | medium | needs the owner's signing key | tags are signed and the release workflow attests artifacts |

---

## 8. How to attack this document

Ask of every mitigation: **which command proves it?** Each one above names a
test, a fuzz target or a CI job. A claim with no command belongs in §7, and
§7 is where the uncomfortable ones are.
