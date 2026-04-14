# Cosmic Applet Spotify

Tiny COSMIC panel applet that shows the currently playing Spotify track.

## Install

Download the `.flatpak` asset from the latest GitHub release and install it:

```bash
flatpak install --user --bundle io.github.snowjademusic.CosmicAppletSpotify.flatpak
```

## Releases

This repository uses Conventional Commits together with release-plz.

Use commit messages like `feat: add shuffle indicator`, `feat(ui): add shuffle indicator`, or `fix: handle missing player`.
On every push to `master`, release-plz opens or updates a release pull request that:

- bumps the crate version in `Cargo.toml`
- updates `CHANGELOG.md`

Merge that release PR to land the new version and changelog in `master`.

If you later want GitHub Releases or crates.io publishing, add a release job on top of this workflow.

## Screenshots


![Panel with Settings](docs/screenshots/panel-with-settings.png)

![Panel](docs/screenshots/panel.png)

## License

GNU GPL v3.0 or later. See `LICENSE`.
