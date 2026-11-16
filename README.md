# Everyday Cool — the whole workshop in one app

The complete [Everyday Cool](https://github.com/caeled/everyday-cool) collection
as a single installable program: one file to download, everything inside.

## What's inside

The full workshop, exactly as it appears on the web: the hub, learning paths,
educator and homeschool pages, and every project — A Library, Decisions
Everywhere, the Library App showcase, Internet Detective, Java Young Robot
Builders, Nerd Heaven (the Tech Support Workshop), Numbers in the Wild, Packet
Playground, Secret Squares, Space Camp, Tonight's Sky, and Weather Everywhere — plus
The Games Room, The Map Room, and (new in v1.0.5) Find It, the traveller's
category finder with its Help menu and documentation — plus the
project ZIP downloads, so the "take a copy" links work offline too.

Most of the workshop runs fully offline inside the app. A few rooms use live
data when you are online (the weather, the library catalogues, Spectrum
Search), and links that leave the workshop — documentation, source repos,
live copies of the projects — open in your default browser.

## Install

Grab the installer for your system from the
[releases page](https://github.com/caeled/everyday-cool-app/releases):

- **Windows:** `Everyday.Cool_1.0.5_x64-setup.exe` (or the `.msi`)
- **macOS:** `Everyday.Cool_1.0.5_universal.dmg` (Intel and Apple silicon)

The builds are unsigned, so Windows SmartScreen or macOS Gatekeeper may ask
you to confirm before the first run.

## The live sites

- The collection: https://caeled.github.io/everyday-cool/
- Source of the site itself: https://github.com/caeled/everyday-cool

## Building

Tauri v2 shell around the collection's static site in `dist/`. Push a tag like
`v1.0.0` and GitHub Actions builds the Windows and macOS bundles and attaches
them to the release.
