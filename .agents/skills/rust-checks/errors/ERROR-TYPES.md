# Concrete error types

Use with [SKILL.md](SKILL.md). Examples preserve concrete errors and typed context;
there are no production-panic or binary-only error-erasure exceptions.

## Meaningful variants and typed context

Represent the failure and its context with types callers can inspect. Distinguish
missing values, duplicates, malformed values, and unsupported constructs when
those differences affect recovery or diagnostics. Avoid replacing a catch-all
string with a catch-all enum that still discards the useful details.

```rust
use std::num::{NonZeroU32, ParseIntError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CountField {
    Interval,
    Occurrences,
}

#[derive(Debug, thiserror::Error)]
pub enum CountError {
    #[error("missing count field {field:?}")]
    Missing { field: CountField },
    #[error("invalid count field {field:?}: {source}")]
    Invalid {
        field: CountField,
        #[source]
        source: ParseIntError,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Count(NonZeroU32);

impl Count {
    pub fn parse(field: CountField, text: Option<&str>) -> Result<Self, CountError> {
        let text = text.ok_or(CountError::Missing { field })?;
        let value = text.parse().map_err(|source| CountError::Invalid {
            field,
            source,
        })?;
        Ok(Self(value))
    }
}
```

`Count` owns parsing and its nonzero invariant. `CountField` identifies the field
without string matching. The parser's concrete error survives as the source.
The error adds context without making callers reconstruct it.

## Variant shapes and source chains

1. Use a unit variant when the failure needs no additional context.
2. Use a tuple variant when its concrete inner value or error is sufficient.
3. Use a struct variant for typed context alongside a concrete source. Paths use
   `PathBuf` or borrowed `Path` as ownership requires; identifiers and quantities
   use their domain types.
4. Use `#[from]` when automatic conversion adds all necessary context. Use
   `#[source]` and an explicit conversion when the caller must supply context.
5. Use `#[error(transparent)]` only for genuine forwarding. Check its source-chain
   behavior; it forwards the wrapped error's source rather than adding a new
   source layer. Use an explicit source variant when that layer must be visible.

`Display` is presentation, not a machine-readable category. It may format typed
values into text; it must not become their stored representation. Avoid redundant
context across layers. Standard alternate formatting (`{:#}`) does not
universally traverse `std::error::Error` chains; inspect `source()` explicitly or
use a reporting facility that documents this behavior.

## Public surface and compatibility

A public enum is appropriate when callers need to match meaningful variants.
A concrete struct with a private typed kind can hide representation when callers
have sufficient typed accessors. Neither shape permits erased causes or
string-coded categories. Do not add a wrapper solely because the crate is public.

Adding an enum variant can break exhaustive matches unless the enum is already
non-exhaustive. Exposing a dependency error in a public signature also exposes
that dependency's type as part of the contract. Assess these tradeoffs against
actual callers; preserve typed context and source chains in either design.

Keep definitions and implementations adjacent. When changing representation,
review the complete error type and affected callers, including derives,
constructors, conversions, source behavior, ownership, and obsolete allocations.

## Application boundaries

Libraries and binaries both propagate concrete errors with `Result` and `?`.
Do not convert to `anyhow::Error`, `Box<dyn Error>`, or a message string at the
entry point. A binary may handle a concrete failure, emit a `tracing` diagnostic,
and return an appropriate exit status. Keep protocol output separate from
logging; use stderr for diagnostics when stdout carries MCP traffic.

Do not panic to report startup failures or internal invariant violations. Model
invariants in owning types and return concrete errors for remaining failure paths.
Assertions, `unwrap`, and `expect` are allowed only in tests. Tests should verify
failure variants and context, and check that preserved causes remain accessible.
