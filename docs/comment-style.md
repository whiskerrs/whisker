# Comment Style Guide

How Whisker writes comments — both the user-facing kind (rustdoc)
and the internal kind. Apply this when authoring new code; later
PRs sweep existing code crate-by-crate to match.

> **Audience.** Anyone writing Rust in this repo. Reviewers point at
> the relevant section when comments drift from the standard.

---

## Two audiences, two rules

| Comment kind | Who reads it | Rule |
|---|---|---|
| **Rustdoc** (`///`, `//!`) | End users + IDE hover | Be thorough. Explain concepts, show examples, name the trade-off. |
| **Internal comments** (`//`) | Whisker contributors | Only the *non-obvious why*, plus `TODO` / `FIXME`. Delete anything else. |

Both audiences benefit from clarity. The split is about *quantity*:
rustdoc errs on more, internal comments err on less.

---

## Rustdoc — write enough that the user doesn't have to read the source

Every `pub` item visible from a published crate root or the prelude
needs rustdoc that meets the following bar.

### Minimum content

1. **One-line summary**, present tense, no trailing period.
   Hover-card readers see this first.

2. **A paragraph (2–5 sentences)** of context. Answer: what problem
   does this solve? What's the conceptual model? Name the
   relevant invariants. If the user has to understand a sibling type
   to use this one, say so and link it.

3. **A code example** for anything non-trivial. Always use `````rust`
   fences (not `\`\`\`ignore` unless the snippet won't compile —
   typical reasons: needs a runtime, references a still-private
   item). Inline examples must be cargo-test-able where possible.

4. **A "Why" or "Trade-off" section** when the design isn't obvious.
   Half of the rustdoc-improvement wins come from naming the
   tension the API resolves — see [`whisker::owner`](../crates/whisker-runtime/src/reactive/owner.rs)
   or [`whisker::Player`](../packages/whisker-audio/src/lib.rs) for
   the pattern.

5. **Cross-references** via `[`name`]` / `[`name`](path)`. Don't
   repeat content that already lives on the linked type — point.

### Example: good rustdoc

```rust
/// Reactive observable — the canonical reading-end of a [`signal`].
///
/// A `ReadSignal<T>` is a cheap (`Copy`) handle backed by an arena
/// slot in the runtime. `.get()` reads the latest value and
/// registers the calling effect / computed as a subscriber so the
/// reactive graph re-runs when the source updates. `.with(|v| …)`
/// borrows without cloning, useful for `T: !Clone` or to avoid an
/// allocation on hot paths.
///
/// # Example
///
/// ```rust
/// use whisker::prelude::*;
///
/// let (count, set_count) = signal(0);
/// let doubled = computed(move || count.get() * 2);
/// set_count.set(3);
/// assert_eq!(doubled.get(), 6);
/// ```
///
/// # Lifetime
///
/// The handle stays alive as long as the [`Owner`] that allocated
/// it. Once the owner disposes, subsequent `.get()` calls return
/// `T::default()`. See [`Owner`](crate::owner::Owner) for the
/// scope model.
pub struct ReadSignal<T: 'static> { /* … */ }
```

What this does right:
- One-line summary front-loaded.
- Concept paragraph names the invariants (`Copy`, arena-backed,
  subscription).
- Code example uses `rust` fence and would actually run.
- "Lifetime" section answers a question users predictably ask.
- Cross-references `Owner`.

### Example: rustdoc that needs more

```rust
/// Reactive signal handle.
pub struct ReadSignal<T: 'static> { /* … */ }
```

What's missing:
- No concept (subscription, lifetime, copy-ness all hidden).
- No example.
- No links to siblings.

### Module-level rustdoc (`//!`)

Each `lib.rs` and major submodule gets a `//!` block at the top
that:

- Names the **concept** the module is about (one line).
- Names the **boundary** — what's in scope vs. what lives
  elsewhere (link the elsewhere).
- For umbrella crates, lists the **prelude shape** so users know
  what `use whisker::prelude::*;` actually drags in.
- For internal modules used by other crate-internal modules,
  gives a 2-line orientation — enough that a contributor opening
  the file cold can join the thread.

### Doc-test policy

- Public examples MUST use `````rust` unless they need a runtime
  (then `````ignore` is fine).
- Do not silently disable doc-tests with `````text` — that hides
  rot from CI.

### Things rustdoc should NOT include

- Implementation details that the user can't act on.
- Internal TODOs (those go in `//` comments or as GitHub issues).
- Author names / dates / change-log lines (git carries those).
- Sentences that just restate the function signature.

---

## Internal comments — only the non-obvious *why*

Internal `//` comments inside function bodies and private items
exist to **answer the question a future reader will actually ask**,
not to narrate the code.

### Keep

- **Why-not** decisions. "We avoid `Arc<Mutex<_>>` here because the
  reactive runtime is single-threaded; a regular `Rc<RefCell<_>>`
  is sufficient." A reader will ask why.

- **Invariants** the type system can't express. "This Vec is
  always sorted by `id` ascending — `binary_search_by` callers
  depend on it."

- **Bug references** when the code shape exists because of a
  specific upstream bug. "Without the `.clone()` here, Lynx's
  Android Animator latches the transform forward — see memory note
  `lynx_android_transform_latched_after_fill_forwards` or issue
  #99."

- **TODOs / FIXMEs** with concrete intent. "`TODO(#NNN): replace
  this stub with the real keyboard-height observer once we ship
  Phase 7.5`."

- **Subtle ordering / lifetime** notes. "Drop the `RefCell` borrow
  *before* invoking the callback — otherwise re-entering the
  runtime panics."

### Delete

- **Sentences that paraphrase the next line.** `// increment the
  counter` above `counter += 1` is noise.

- **Section headers** in short functions. `// ---- Setup ----`
  inside a 15-line fn just adds vertical space.

- **Diary entries** from when the code was being figured out.
  "First we tried X, but it didn't work; then we tried Y."
  Interesting in git history; clutter in the file.

- **Comment-out blocks** ("// debug only" code blocks that were
  never deleted). Either delete or wrap in `#[cfg(test)]` /
  feature-flagged code.

- **Outdated references.** "Phase 4 will replace this" when we
  finished Phase 5 already.

- **`println!`-style debugging tags** ("// XXX", "// debug",
  "// remove me before merge") left from local debugging.

### Edge cases

- **`TODO` policy:** acceptable in committed code only when
  paired with either an issue number or a concrete trigger
  ("when Lynx ships X", "when we drop iOS 14 support"). Bare
  `// TODO` is delete-on-sight.

- **Memory-note references** (e.g. `lynx_version_pinning`,
  `whisker_signal_update_i32_only`) — keep them. They're the
  bridge between code and the auto-memory system.

- **License / copyright headers** are out of scope for this guide.

---

## Rule of thumb

> If you remove the comment, would a future reader (rustdoc user
> or contributor) be measurably worse off?
>
> - Rustdoc: usually yes — keep and expand.
> - Internal `//`: often no — delete.

The asymmetry is the point. Whisker's public API is the contract
with users; internal comments are the contract with future
contributors. Different bars.

---

## Per-crate sweep

Existing crates get retroactively brought up to this standard via
PR-per-crate. Order tracked in the [pre-1.0 polish
project](../README.md). When sweeping:

1. Open the crate's `lib.rs`. Audit the `//!` module doc against
   the "Module-level rustdoc" checklist.
2. For each `pub` item, expand rustdoc per "Minimum content".
3. For each `//` comment in private code, decide keep / delete /
   rewrite per "Internal comments". Lean toward delete.
4. Don't refactor code in a sweep PR. If the comment cluster
   reveals a real refactor target, file an issue and link it.

This guide is the standard the sweep PRs answer to. Cite it in
PR review when a comment doesn't fit.
