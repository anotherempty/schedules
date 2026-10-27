---
name: errors
description: Design and review concrete Rust error types, typed failure context, source preservation, and panic-free production paths. Use when adding or changing errors, reviewing unwrap or expect, handling parsing failures, or choosing error propagation and representation.
metadata:
  source-repository: https://github.com/rewrite-rs/skills
---
# Errors

Design concrete errors with typed context, preserve their causes, and keep
production paths free of deliberate panics. These requirements apply to libraries
and binaries, including startup and internal invariant checks.

## Panic policy

1. Do not use `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!`, assertions,
   `debug_assert!`, or other deliberate panic paths outside tests. There are no
   exceptions for `main`, startup preconditions, or allegedly impossible states.
2. Avoid operations that can panic on unchecked input, including unchecked
   indexing. Use checked access, checked arithmetic, and fallible conversions
   where input or computed values can exceed supported bounds.
3. Prefer types that prevent invalid states. Where a failure remains possible,
   return a meaningful concrete error; do not replace a panic with a silent
   default or discard the failure.
4. Assertions and panic-based checks are permitted in tests only. Being in a
   benchmark, example, or binary does not create a test-only exemption.

## Concrete errors and typed context

1. Use concrete error types with meaningful variants and typed context. Callers
   must distinguish failure categories without matching display strings.
2. Do not encode categories, field identities, states, or actions in `String` or
   `&'static str` payloads such as `Invalid("field")`. Use appropriate enums or
   domain types. Strings are for actual text, including diagnostic messages.
3. Keep identifiers, paths, quantities, dates, and other context in their domain
   representations. A string newtype does not fix unsuitable storage.
4. Do not erase errors into strings, `anyhow::Error`, or general-purpose boxed
   errors such as `Box<dyn Error>`, including at binary entry points. A borrowed
   `dyn Error` returned by the standard `Error::source` API is not error erasure.
5. Use `thiserror` for mechanical `Display`, `Error`, and conversion implementations
   when appropriate. Declare dependencies at the workspace root; members inherit
   them with `workspace = true`.
6. Choose enum variants or a concrete struct with a typed kind based on callers'
   needs. Publication alone does not require a wrapper. Preserve useful typed
   distinctions and consider compatibility when changing the public surface.

Read [ERROR-TYPES.md](ERROR-TYPES.md) when designing variant shapes, adding source
context, or choosing between a public enum and a concrete wrapper.

## Propagation and ownership

1. Propagate recoverable errors with `Result` and `?`; never silently discard
   errors. Preserve original concrete causes using `#[source]` or `#[from]`.
2. Add context the current layer knows without replacing the source with its
   display string. Use `map_err` when adding typed context, not to erase causes.
3. Put parsing, validation, normalization, and invariant enforcement on the domain
   type that owns the value. Enforce invariants through constructors, conversions,
   deserialization, and public mutations; callers must not repeat validation.
4. Keep each type local: definition, trait implementations, inherent
   implementations. Borrow for access; consume when transferring ownership.
5. Log handled failures with `tracing` at the responsible boundary. Do not use
   printing macros for diagnostics or log the same propagated failure repeatedly.

## Whole-type review

Whenever an error or related domain type changes:

1. Review its complete representation, variants, construction and mutation
   invariants, parsing and serialization boundaries, trait implementations,
   source chains, and ownership semantics.
2. Update affected callers in the same change. Remove redundant allocations,
   clones, conversions, string comparisons, and duplicated validation.
3. Reevaluate derives. Small value kinds should implement `Copy` when their
   fields and semantics permit it; source-bearing errors need not be copyable.
   Assess `Clone`, `Eq`, `Ord`, and `Hash` by domain semantics.
4. Resolve related omissions without waiting for the user to identify each one.
   Report unrelated findings separately; do not expand into an unsolicited
   repository-wide rewrite.

## Verification

Run relevant tests and the repository's configured lint checks, including
feature-enabled and feature-disabled builds when error types are feature-gated.
Test typed variants, context, and source preservation, not only message strings.
Keep domain and operational numbers named in tests as well as production code.

Inspect affected production code for deliberate panic paths, unchecked operations,
string-coded categories, erased sources, and ignored errors. Inspect inline test
modules separately; a file-path filter cannot reliably distinguish test code.
Passing compilation or Clippy does not replace this review. Fix production
violations rather than justifying them with comments or lint suppressions.
