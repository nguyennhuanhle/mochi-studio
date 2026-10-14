# Contributing to Mochi Studio

Thanks for your interest in Mochi Studio! Bug reports, feature ideas, documentation fixes and code are all welcome.

## Before you start

- **Bugs:** search [existing issues](../../issues) first, then open a bug report using the template. Include your Windows version, GPU, and the steps to reproduce.
- **Features:** open a feature request before writing code, so we can agree on the behaviour first. Mochi Studio has a deliberately defined scope (see "Out of scope" below), and a short discussion saves wasted work.
- **Security issues:** please do **not** open a public issue. Follow [SECURITY.md](SECURITY.md).

## Development setup

Follow [Building from source](README.md#building-from-source) in the README. In short:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/setup.ps1
pnpm install
pnpm dev
```

## Making a change

1. Fork the repository and create a branch from `main`.
2. Keep each pull request focused on one topic. Don't mix bug fixes, new features and refactoring.
3. Make sure these pass locally:
   ```powershell
   cargo check --workspace --tests
   cargo test -p <crate you changed>
   pnpm --dir apps/desktop typecheck
   cargo deny --manifest-path apps/desktop/src-tauri/Cargo.toml check bans licenses sources
   ```
4. Test the change for real: record, edit and export a short clip that exercises your change. Describe what you tried in the pull request.
5. Open a pull request using the template.

## Project conventions

- **Same renderer for preview and export.** Any visual effect must be implemented in the shared renderer (`crates/mochi-render`, or the vendored Cap renderer). Never implement it only in the preview.
- **Non-destructive editing.** Code must never modify a recording or an imported media file. All edits live in `project.json`.
- **No network access.** Do not add dependencies that can talk to the network or send telemetry. `deny.toml` enforces this, so don't weaken it.
- **Vendored Cap code** (`vendor/cap/`): keep changes minimal. Mark every change with a `// [MOCHI]` comment and add a row to [`vendor/cap/PROVENANCE.md`](vendor/cap/PROVENANCE.md). This keeps upstream updates manageable.
- **User-facing text** goes through i18n. Add every new key to both `apps/desktop/src/i18n/en.json` and `apps/desktop/src/i18n/vi.json`.
- **Tauri commands** are exported to TypeScript with `tauri-specta`. Running the app in dev mode regenerates `apps/desktop/src/bindings.ts`; commit the regenerated file.
- **Changed dependencies:** run `python scripts/third_party_crates.py` to refresh `legal/third-party-crates.txt`.
- **Language:** many existing code comments are in Vietnamese. New comments may be in English or Vietnamese. Identifiers, commit messages and pull request descriptions should be in English.

## Out of scope

To keep the project focused, these are intentionally **not** planned:
- cloud hosting, share links, accounts or real-time collaboration;
- quizzes, SCORM or other e-learning interactivity;
- AI avatars, text-to-speech or AI dubbing;
- livestreaming, game capture hooks, or recording phones over USB;
- bypassing DRM.

## License of contributions

Mochi Studio is licensed under the [GNU AGPL v3.0 only](LICENSE). By submitting a contribution, you agree that it is licensed under the same terms.
