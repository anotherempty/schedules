# Third-party notices

Project code is licensed under MIT OR Apache-2.0. Vendored agent skills retain
their upstream licenses:

1. `.agents/skills/rust-checks`: adapted from
   [rewrite-rs/skills](https://github.com/rewrite-rs/skills), copyright
   2026-Present, rewrite.rs. Licensed under
   [BSD-3-Clause](.agents/skills/rust-checks/LICENSE). Local adaptations include
   repository-specific Rust standards, typed errors, and panic-free production paths.
2. `.agents/skills/commit`: adapted from
   [getsentry/skills](https://github.com/getsentry/skills), copyright
   2025 Functional Software, Inc. dba Sentry. Licensed under
   [Apache-2.0](.agents/skills/commit/LICENSE). Local adaptations include branch
   handling and repository commit instructions.

Cargo dependencies retain their own licenses, declared in their package metadata.
