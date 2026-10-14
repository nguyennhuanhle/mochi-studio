## What does this change?

<!-- A short description. Link the related issue: "Fixes #123". -->

## How was it tested?

<!-- What you recorded / edited / exported to check it, and on what hardware. -->

## Checklist

- [ ] `cargo check --workspace --tests` passes
- [ ] `pnpm --dir apps/desktop typecheck` passes
- [ ] `cargo deny … check bans licenses sources` passes (if dependencies changed)
- [ ] New UI text is added to both `en.json` and `vi.json`
- [ ] Changes to `vendor/cap/` are marked `[MOCHI]` and listed in `vendor/cap/PROVENANCE.md`
- [ ] `CHANGELOG.md` is updated for user-visible changes
