# Releasing SC LangPatch

## Prerequisites (one-time setup)

- `cargo install cargo-release` (if not already installed)
- GitHub Secrets configured:
  - `TAURI_SIGNING_PRIVATE_KEY` — your Tauri signing private key
  - `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` — the password for the key
- Your local private key backed up somewhere safe

## Release checklist

### 1. Update the changelog

Edit `CHANGELOG.md` and add entries under `## [Unreleased]`:

```markdown
## [Unreleased]

### Added
- New weapon stats module

### Fixed
- Crash when Data.p4k is locked by the game
```

Use these categories: **Added**, **Changed**, **Fixed**, **Removed**.

### 2. Preview the release (dry run)

In VS Code: `Ctrl+Shift+P` > `Tasks: Run Task` > **Release: Dry Run (patch)**

Or from terminal:

```bash
cd src-tauri
cargo release patch --no-publish
```

This shows what will happen without changing anything. Verify:
- Version bump looks correct (patch/minor/major)
- Changelog section will be stamped with the right version
- tauri.conf.json and package.json versions will be synced

### 3. Execute the release

In VS Code: `Ctrl+Shift+P` > `Tasks: Run Task` > pick one:
- **Release: Patch (0.0.x)** — bug fixes
- **Release: Minor (0.x.0)** — new features
- **Release: Major (x.0.0)** — breaking changes

This will:
1. Bump the version in Cargo.toml
2. Sync the version to tauri.conf.json and package.json
3. Stamp the changelog `[Unreleased]` section with the version and date
4. Commit everything as `release: v0.x.y`
5. Create a git tag `v0.x.y`
6. Push the commit and tag to GitHub

### 4. Wait for CI

The `v*` tag push triggers GitHub Actions, which:
1. Builds the NSIS installer on Windows
2. Signs the update artifacts with your private key
3. Creates a GitHub Release with the changelog as the body
4. Uploads the installer and `latest.json` for auto-updates

Monitor progress at: https://github.com/VeeLume/sc-langpatch/actions

### 5. Verify the release

- Check the [Releases page](https://github.com/VeeLume/sc-langpatch/releases)
- Verify the installer `.exe` is attached
- Verify `latest.json` is attached (needed for auto-updates)
- Download and test the installer if this is a major release

## How auto-updates work

Existing installations check `latest.json` on startup. If a newer version is found, users get a dialog prompt. The update is verified against the public key in `tauri.conf.json` before installing.

## Version is stored in three places

`cargo-release` keeps these in sync automatically:
- `src-tauri/Cargo.toml` — source of truth
- `src-tauri/tauri.conf.json` — used by Tauri bundler
- `package.json` — used by npm/pnpm

Never edit versions manually — always use `cargo release`.

## Troubleshooting

**CI fails to build:** Check that the svarog repo is public and the `VeeLume/svarog` checkout in the workflow is correct.

**Auto-update not working:** Verify `latest.json` exists in the latest release. Check that the public key in `tauri.conf.json` matches your signing key pair.

**Lost your signing key:** You cannot sign updates for existing installations. Users will need to manually download the new installer. Generate a new key pair and update `tauri.conf.json` + GitHub Secrets.
