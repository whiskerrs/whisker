# Whisker — agent instructions

## Code comments

Authority: [docs/comment-style.md](docs/comment-style.md). Operational
rules for WRITING and EDITING code, in order of how often they are
violated:

- **Comments describe the code as it is now — never the change you are
  making.** No "was / previously / now we …", no "this fixes …", no
  explaining why your edit is correct, no narrating what the diff does.
  That context belongs in the commit message and PR body; in the file
  it is noise the moment the change merges.
- **The default for any edit is zero new comments.** Before writing
  one, check the bar: does the code fail to express a *live*
  constraint — an invariant, an ordering rule, a platform-quirk
  workaround (with its reason), a why-not against an alternative the
  next editor would plausibly reach for? If not, don't write it.
- **One sentence.** Two only when naming the concrete failure mode.
  Anything longer belongs in `docs/` or the commit message.
- When touching existing comments: deleting a stale one beats updating
  it; updating beats adding. Do not grow doc comments as a side effect
  of an unrelated edit.
- `SAFETY:` comments on `unsafe` are mandatory and exempt from the
  brevity pressure.
