# Review passes

The protocol behind the two axes in `SKILL.md`: what the review needs before
it starts, the prompt for each pass, and how the two lists merge into one
report. Run the passes as parallel sub-agents when the harness supports it;
otherwise run them in order, one at a time. The prompts below are written to
hand to a sub-agent verbatim, with the diff attached — and, for the spec
pass, the request attached as well.

## Inputs the review needs before it starts

Three things:

- **The review scope and changes** — select from the user's request and inspect
  `git status --short` first. For a branch or PR, use `git diff <base>...HEAD`
  against the requested base. For local changes, inspect `git diff --cached`,
  `git diff`, and untracked files listed by `git ls-files --others --exclude-standard`.
  Read relevant untracked files directly; Git diffs omit them. For a combined
  review, include both committed and local changes without duplicating findings.
  State the selected scope; do not silently exclude local work or include it in
  a branch-only review.
- **The request the change answers** — an issue, a PR body, or the user's own
  words.
- **The repo lint configuration** — `[lints.clippy]` in `Cargo.toml`, a
  `clippy.toml`, a `rustfmt.toml`, `#![deny(...)]` in the crate root.

Missing the request means the spec pass cannot run: ask for it rather than
inventing an intent from the diff. Missing the lint configuration means
clippy runs at its default level and `-- -D warnings` is the fallback; a
present configuration always runs at the level it set, never stricter.

## The standards pass

Reads for craft: does the diff meet the standards the repo already lives by.
It starts with the changes, follows affected callers and dependencies, applies
`SMELLS.md`, and reports findings in the one-line format. It does not comment on whether the
change was the right thing to build.

The prompt, in full:

```
You are the standards pass of a two-axis Rust code review.

Start with the changes and touched files. Read untouched files as needed to
trace callers, contracts, invariants, shared types, configuration, and tests.
Review complete changed types and affected usage under repository instructions.
Keep investigation relevant to the change; report unrelated findings separately.

Apply the smell baseline in SMELLS.md line by line. Where a row names an
owning skill, consult its relevant guidance under repository instructions.
Keep this a review: companion guidance does not authorize edits or unrelated
workflows. If a companion is unavailable, use repository rules and concrete
evidence, and disclose any resulting review limitation. Read the
highest-risk lines first: unsafe blocks and FFI, then destructive operations
and error paths, then the public surface, then concurrency, then
expression-level shape.

Reproduce a suspected defect, or trace the exact failing path, before
reporting it as fact. If the evidence is incomplete, say what remains
uncertain and which check would resolve it.

Report findings only, one line each, most severe first:

path:line — severity: what is wrong. What to do instead.

Severity is blocking, should-fix, or note. A finding with no concrete fix is
a question, and is phrased as one. Drop anything below note. Do not comment
on whether the change was the right thing to build; that is the spec pass.
```

## The spec pass

Reads for intent: does the diff do what was asked, nothing more and nothing
less. It reads the request first and the diff second, and it answers three
questions. It does not apply the smell baseline; that is the standards pass.
Unrequested scope is a finding, not a bonus.

The prompt, in full:

```
You are the spec pass of a two-axis Rust code review.

Read the request first — the issue, the PR body, or the words the user gave —
and the diff second, against it. Do not apply the smell baseline and do not
comment on craft; that is the standards pass.

Answer three questions, and report the answer to each:

1. Is everything the request asks for implemented? Name each missing part.
2. Is anything implemented that the request did not ask for? Unrequested
   scope is a finding, not a bonus — name each extra and say whether it
   belongs in this change or in a follow-up.
3. Does any part of the diff contradict a stated requirement? Quote the
   requirement and the line that contradicts it.

Report findings one line each, most severe first:

path:line — severity: what is wrong. What to do instead.
```

## Merging

- Deduplicate where both passes found the same line, keeping the more severe
  framing.
- Group repeated instances of one root cause under a single finding when one
  correction addresses all of them.
- Sort by severity — blocking, should-fix, note — then by file.
- Cap the output: if the merged list runs past roughly fifteen findings,
  report the blocking and should-fix ones in full and summarize the notes as
  a count with a theme. A list nobody finishes reading is a list nobody acts
  on.

## Reporting

Lead with the verdict — blocking issues present, or none. Then the findings,
most severe first. Then the verification output, quoted rather than
summarized. Close with the risks that could not be tested locally, if any.
Never lead with a summary of what the change does; the author knows.
