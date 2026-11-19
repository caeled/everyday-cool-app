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
category finder with its Help menu and documentation — plus (new in v1.0.11)
The Radio Room, a retro wooden radio that plays live SomaFM streams, with
its listening shelf, radio workshop, and Help menu — plus (new in v1.0.12)
Everyday Chemistry, a visual atom bench and numbers bench with eight
kitchen experiments and its Help menu — plus (new in v1.0.13)
Everyday Electronics, a live circuit bench, Ohm's law and LED resistor
benches, a crystal radio explained and built, a fruit battery, and its
Help menu, cross-linked with The Radio Room both ways — plus (new in
v1.0.14) Everyday Physics, a playable pool table with the reflection
angles and momentum drawn on, a ballistics launcher, pendulum and
seesaw benches, and its Help menu; the hub now leads with a STEM block
(Nerd Heaven, then Physics, Chemistry, Electronics, Tonight's Sky, The
Radio Room); Everyday Electronics gains its Wikipedia "read deeper"
shelf; and The Radio Room gains two shelves of its own — "Ham radio &
CB — the two-way side," with a cool-stations-to-try list, and "Clubs —
the people side," with the ARRL club search and the clubs around
Defiance, Ohio — plus (new in v1.0.15) Everyday Body, the anatomical
explorer ChatGPT built for the collection: a working body atlas with
twelve organs, a brain explorer with twelve regions and the networks
caveat stated on the page, connection scenarios that follow everyday
actions across cooperating systems, a quiz, and its Help menu — it
takes the fifth seat in the hub's STEM block, right after Everyday
Electronics — plus (new in v1.0.16) Everyday Language, the Secret
Squares sibling built on Steven's line that language is a code too: an
explorable Indo-European family tree with English's Germanic ancestry
drawn in gold, a cognates bench of eleven word families with
false-friend warnings, a ten-phrase phrasebook in six languages with a
"Take it traveling" card pointing at the separate Travel Kit app, a
brain language-network bench cross-linked to Everyday Body, and its
Help menu; it sits in the hub right after Secret Squares, which gains
the reciprocal cross-link card — plus (new in v1.0.17) Everyday
Measure, the metric system for Americans: an interactive prefix ladder
with each prefix's meaning in plain words, four converter benches
(length, weight, volume, temperature) that show their working from the
exact legal definitions, seven "feel it" anchors, a comparison shelf
reading familiar things in both systems at once, trap cards for the
tonne/ton and US/Imperial gallon collisions, the Mars Climate Orbiter
and Gimli Glider breakage stories, and its Help menu; it takes the
sixth seat in the hub's Science run, right after Everyday Body — plus
(new in v1.0.18) a refresh of the whole workshop to the current
collection: Everyday Physics and The Radio Room pick up the review
fixes from the collection's PR #7 — reworked Physics benches, scripts
and tests, Radio Room fixes, refreshed room ZIPs and provenance for
both rooms, and the hub's Radio Room card rewritten — with a line each
refreshed in Everyday Chemistry and Everyday Electronics and all
23 rooms current — plus the project ZIP downloads, so
the "take a copy" links work offline too.

Most of the workshop runs fully offline inside the app. A few rooms use live
data when you are online (the weather, the library catalogues, Spectrum
Search), and links that leave the workshop — documentation, source repos,
live copies of the projects — open in your default browser.

## Install

Grab the installer for your system from the
[releases page](https://github.com/caeled/everyday-cool-app/releases):

- **Windows:** `Everyday.Cool_1.0.13_x64-setup.exe` (or the `.msi`)
- **macOS:** `Everyday.Cool_1.0.13_universal.dmg` (Intel and Apple silicon)

The builds are unsigned, so Windows SmartScreen or macOS Gatekeeper may ask
you to confirm before the first run.

## The live sites

- The collection: https://caeled.github.io/everyday-cool/
- Source of the site itself: https://github.com/caeled/everyday-cool

## Building

Tauri v2 shell around the collection's static site in `dist/`. Push a tag like
`v1.0.0` and GitHub Actions builds the Windows and macOS bundles and attaches
them to the release.
