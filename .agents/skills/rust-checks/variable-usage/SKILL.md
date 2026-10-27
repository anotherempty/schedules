---
description: Choose Rust variable ownership deliberately — consume values when transferring ownership, borrow references for access, and clone only when independent ownership is required. Use when reviewing variable usage, function parameters, moves, borrowing conflicts, unnecessary clones, or shared ownership.
metadata:
    source-repository: https://github.com/rewrite-rs/skills
name: variable-usage
---
# Variable Usage

Decide who owns each value, whether the current owner needs it again, and how
long access must last. Prefer consuming and referencing over cloning.

## The rule

Consuming, borrowing, and cloning are distinct operations:

- Consuming takes ownership by value. For a non-`Copy` value, passing `value`
  moves it; it does not clone it, and the previous binding cannot be used afterward.
- Borrowing passes `&value` or `&mut value`; ownership stays with the owner.
- Cloning explicitly calls `Clone::clone`, usually as `value.clone()`, to create
  another owned value. Shared-handle clones can still refer to the same data.
- Passing a `Copy` value by value copies it implicitly; it does not move ownership
  away from the previous binding or call `Clone::clone`.

1. Consume a value when transferring ownership or expressing a state transition.
   Move it directly when the caller no longer needs it; use consuming methods,
   `into_iter`, and owned fields rather than cloning immediately before a move.
2. Borrow with `&T` for temporary read access and `&mut T` for exclusive mutation.
   Prefer `&str` and slices for borrowed text and collections. Shorten borrow
   scopes or split fields when access conflicts.
3. Clone only when independently owned values must coexist and neither moving
   nor borrowing satisfies the contract. Explain the need from actual usage;
   borrow-checker convenience alone is insufficient. Cloning a shared handle is
   valid when multiple owners are required, but does not create independent data.

Do not replace a legitimate move with a reference merely because the callee
only reads: ownership may be needed for storage, return values, or task lifetime.
Do not add shared ownership solely to avoid choosing an owner.

## Read the error, not the workaround

`E0502` (a mutable borrow while an immutable one is still live) and `E0499` (two
mutable borrows of the same place) name a lifetime conflict; the fix is in the
structure the error describes, not in the `clone` that silences it:

```rust,ignore
// No clone: `first` is Copy, so the borrow ends at the let.
let first = items[0];
for _ in 0..n {
    items.push(first);
}
```

## Borrow splitting

The borrow checker tracks a whole value through method calls, but fields
individually through direct field access. A `&mut self` method that also needs
`self.other_field` is the classic false conflict — destructure once:

```rust,ignore
// The two fields are separate lets and borrow independently.
impl Server {
    fn handle(&mut self) {
        let Self { connections, log, .. } = self;
        for conn in connections.iter_mut() {
            log.record(conn.id());
        }
    }
}
```

`split_at_mut` hands back two disjoint `&mut` halves of one slice; when
destructuring is not enough, extract a free function that takes the two fields.

## Take the cheapest thing that works, in argument position

`&str` over `&String`, `&[T]` over `&Vec<T>`, `impl AsRef<Path>` over a `PathBuf`
for a path a function only reads. A signature that takes `String` when it only
reads forces every caller into a clone — the bug is in the signature.

```rust,ignore
// Forces a clone at every call site that holds a &str.
fn log_message(message: String) { ... }

// Reads only — callers pass a reference and copy nothing.
fn log_message(message: &str) { ... }
```

## `Cow` for genuine borrow-or-own

`Cow<'_, str>` earns its keep when the common path returns the input untouched
and the rare path allocates.

```rust
use std::borrow::Cow;
// The common path borrows; only input containing a bracket allocates.
fn sanitize(input: &str) -> Cow<'_, str> {
    if input.contains(['<', '>']) {
        Cow::Owned(input.replace('<', "&lt;").replace('>', "&gt;"))
    } else {
        Cow::Borrowed(input)
    }
}
```

## When `Rc`/`Arc`/`RefCell`/`Mutex` express a real requirement

Shared ownership is justified when the *runtime* structure genuinely has multiple
owners with no single one outliving the rest, and not when a single owner is
merely inconvenient to name. Run the four-question test in `SHARED-STATE.md`
before adding a `RefCell`; the decision table is in `CLONE-DECISIONS.md`; the
handle clone that passes the one-sentence test is in `SHARED-STATE.md`.

## `mem::take` and `mem::replace`

`mem::take` moves a value out through `&mut` and leaves its default;
`mem::replace` leaves an explicitly supplied replacement. Use them only when
the replacement preserves the owning type's invariants:

```rust,ignore
let old = mem::take(&mut self.buffer);
```

## Avoid statics

A `static` holding mutable or lazily-initialised state is a lifetime nobody wrote
down and an ownership story nobody can follow. Pass the value, or hold it in the
type that owns the work.

## Deferrals

Reshaping how correct code is expressed is `/idiomatic`; modelling invariants
into types is `/type-driven-design`; error types are `/errors`.

## Verification

Review affected callers, lifetimes, mutation visibility, drop timing, and custom
`Clone` behavior when replacing clones with moves or references. Verify affected
behavior at the lint level the repo configures:

```bash
cargo clippy --all-targets   # add -- -D warnings only if the repo has no lint config
cargo test
```

`clippy::redundant_clone` is `nursery` and off by default — run it one-off with
`cargo clippy -- -W clippy::redundant_clone` as a proposal generator, never write
it into the repo lint config; its findings are candidates, not edits.
