# Contributing to Buitenzorg OS

## Principles

Follow [requirements.md](requirements.md) §1: safety by default (Rust in ring 0),
productivity by default (C# in user-space), a firm ABI boundary, microkernel-
leaning, and performance as policy.

## Coding standards

**Rust (`kernel/`)**
- `cargo fmt` + `cargo clippy` clean before a PR.
- Every `unsafe` block carries a `// Safety:` comment explaining its invariants.
- The kernel is `no_std`; new dependencies must be `no_std`-compatible and
  weighed carefully (each crate adds trust surface in ring 0).
- Build `bzkernel` only for `x86_64-unknown-none` (it is not a default member).

**C# (`runtime/`, `sdk/`)**
- Nullable + ImplicitUsings are on; `Buitenzorg.Runtime` must stay
  NativeAOT-compatible (`IsAotCompatible=true`) — avoid dynamic reflection.
- Interop structs: `[StructLayout(LayoutKind.Sequential)]` + a byte-size test.

**The ABI contract (`kernel/abi` ↔ `runtime/.../Sys`)**
- Any ABI change must touch **both sides + both contract test suites +
  docs/abi.md** in the same PR. Syscall numbers are append-only.

## PR flow

1. Branch from `master`, one topic per PR.
2. Must be green: `cargo test -p bz-abi`, the kernel build, the QEMU boot smoke
   test, and `dotnet test`. CI runs all of it (`.github/workflows/ci.yml`).
3. Update the checklist in requirements.md §17 when you complete an item.
4. Commit messages: a concise English subject line; the body may be EN or ID.

## Tracking progress

Completed items are marked `[x]` in [requirements.md §17](requirements.md) in the
same PR as the implementation — that checklist is the project's status board.

## Contribution license

The project is licensed under the **MIT License** ([LICENSE](LICENSE)). By
submitting a contribution, you agree it is licensed under MIT as well.
