# Store Submission Design

`whisker submit` uploads a built binary to a store; `whisker store`
keeps what the stores hold *about* the app — page text, images,
categories, review contacts — in step with the app's `store.rs`. Both
talk to App Store Connect and Google Play over their REST APIs, with
credentials from the same age-encrypted `credentials/` store builds
use.

This doc is the *design* — the boundaries and the "why". The
implementation lives in `crates/whisker-submit` (API clients and sync
logic), `crates/whisker-config/src/store.rs` (the `store.rs` builder),
and `crates/whisker-cli/src/{submit,store}`.

> Status: **implemented, not yet exercised against live stores
> end to end.** Request shapes come from the stores' API references.

## Commands and what each owns

| Command | Sends |
| --- | --- |
| `whisker submit ios` / `android` | One binary, plus what belongs to that binary: `appstore.beta_build`, `playstore.release` |
| `whisker store push appstore` / `playstore` | Everything else `store.rs` declares |
| `whisker store pull appstore` / `playstore` | Nothing — prints the store's current state as a `store.rs` |

The split follows lifetime, not store. A release note describes one
build and is wrong for the next one; a description outlives every
build. Folding both into one command (fastlane's model) makes "upload
the binary" able to overwrite a store page, and needs a skip flag for
every part to avoid it.

`submit android` only uploads by default. Creating a release on a
track is opt-in (`--track`) so that the bare command reaches nobody,
matching iOS, where upload and distribution are separate steps.

Submit never builds: it takes the artifact the last `whisker build`
left, or `--path`.

## Why `store.rs` is a separate program

`whisker.rs` is "run this and a native project comes out". Store
metadata reaches no generated file, so putting it there would let a
missing release-note file stop `whisker run`, make edits to a
description invalidate generation fingerprints, and make every submit
compile the generator's plugin dependencies.

`store.rs` reuses the mechanism instead of the file: the CLI builds it
as its own executable (`whisker_cng::run_store`) linking only
`whisker`, runs it, and reads a JSON report. It keeps what makes
`whisker.rs` pleasant — types, completion, `mod` for splitting,
`include_str!` for long text — and is executed only by `submit` and
`store`.

The builder lives in `whisker-config` and is re-exported as
`whisker::store`, so an app needs no extra dependency. It is built
against the CLI's own `whisker` rather than the app's registry copy:
the report format is only guaranteed within one release, and a CLI
built from a checkout must not depend on what is already published.

## Why `store.rs` mirrors the store APIs

Each store is declared separately, and each block is one API resource
with that API's attribute names:

```
playstore: details, listing(language), images(language), release
appstore:  app, app_info, asset_library, version, review_detail,
           beta_app, beta_app_review_detail, beta_build
```

A shared, store-neutral shape was tried and rejected. The stores
disagree about what exists (keywords, short description), what a value
is attached to (a description is app-wide on Play but belongs to the
version being prepared on the App Store), when it may change, and how
languages are spelled (`ja-JP` vs `ja`). A neutral shape has to hide
those differences, and they are exactly what an author needs to know
to predict what a push will do. Mirroring keeps the stores' own
definitions, makes their API references the documentation, and means
new attributes are added under the name they already have. The cost is
declaring an app's name twice.

For the same reason App Store images use the App Asset Library
(library images plus placements) rather than `appScreenshotSets`,
which App Store Connect deprecated in API 4.5. Placement groups are
passed through as written: Apple adds device families to its reference
data without an API version change, so a built-in list would go stale.

## Sync semantics

- **Only what is set is sent.** An unset attribute, an undeclared
  locale, or an undeclared image type is left as the store has it.
  Nothing is deleted because it is absent from `store.rs`.
- **A declared image group is authoritative.** The store is made to
  hold exactly the declared files in order. Unchanged groups are
  detected (Play: SHA-256; App Store: reference name, file name and
  size) and skipped.
- **Validate before the first request.** Limits, locale codes and
  image files are checked locally.

Google Play changes go into one edit and appear only on commit, so a
failed push leaves the store untouched.

App Store Connect has no transaction. A push therefore resolves
everything it needs — the app, an editable app info, an editable
version, the asset library — before its first write and refuses to
start if any is missing. Creating the version is opt-in
(`--create-version`). An uploaded image is placed only after App Store
Connect finishes processing it, because a file matching no image
specification fails there and a placement on it would never appear.

## What is deliberately absent

- **Demo account credentials.** `store.rs` is plaintext in the
  repository; these belong in `credentials/`. Not built yet, so
  `demo_account_required(true)` currently has no way to supply the
  account.
- **A locale mapping table.** Several app locales are ambiguous on
  Play (`en` → `en-US` or `en-GB`), so each store's codes are written
  directly.
- **Images in `store pull`.** It prints text only.

Not yet covered: app preview videos and the age rating declaration.
