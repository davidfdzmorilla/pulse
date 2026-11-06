# How Pulse is run: open source core, hybrid model

> **Draft for the maintainer to review.** It describes the license and the
> contribution terms as they stand in `LICENSE` and `CONTRIBUTING.md`. It
> announces nothing new. Where a decision hasn't been made, it says so.

## What is open, and stays open

Pulse, the application in this repository, is free software under the
**GNU General Public License v3.0 or later** ([LICENSE](../LICENSE)). That
covers the engine, the interface, the instruments and effects, the analysis
tools and the build and packaging scripts.

Everything published under the GPL stays available under the GPL. A license
already granted for a released version can't be withdrawn. Anyone can use,
study, modify and redistribute it, including commercially, under the GPL's
terms.

## What "hybrid" means here

The maintainer may, in the future, offer things around Pulse that are not in
this repository or not under the GPL. Examples of what that *could* mean (none
of them is decided, planned or promised):

- paid services that run outside the app (for example, hosted processing or
  sound content);
- commercial licenses, for organisations that can't accept the GPL's terms,
  of the code whose rights the author holds (outside contributions are
  GPL-only, see below);
- paid support.

Today there are no paid features, no accounts, and nothing in the app is
locked. If that changes, it will be announced in advance in this document and
in the release notes.

## How contributions are licensed

[CONTRIBUTING.md](../CONTRIBUTING.md#licensing-of-contributions) (original
Spanish wording in [CONTRIBUTING.es.md](../CONTRIBUTING.es.md#licencia-de-las-contribuciones))
says that by contributing you:

1. certify you have the right to contribute it (Developer Certificate of
   Origin 1.1, `git commit -s`);
2. publish it under GPL-3.0-or-later;

That's all: there is no Contributor License Agreement (CLA) and no extra
license to the author. Until 2026-11-06 there was one (a "point 3" allowing
the author to relicense contributions); it was removed before any outside
contribution was merged, so none was ever made under it. A commercial license
covering someone else's contribution would need that person's permission, or
that part rewritten.

## Protecting the project's independence

- The GPL release can't be taken back. If the project ever went in a
  direction the community didn't like, anyone could continue it from the
  last GPL version.
- Third-party code and models in Pulse keep their own licenses (all listed in
  [`packaging/THIRD_PARTY_NOTICES.txt`](../packaging/THIRD_PARTY_NOTICES.txt)).
  Any commercial offer must respect them.
- Pulse runs locally and offline. That's a design principle and not a
  free-tier limit.

## Open questions for the maintainer (not legal advice)

None of these blocks contributions. Most matter only if a paid offer is ever
planned:

1. **Which text is authoritative.** The English translation in
   CONTRIBUTING.md says the Spanish text prevails. Confirm that's the
   intended choice.
2. **Dependencies and relicensing.** A commercial license of Pulse is only
   possible for code whose rights the author holds or that is under a
   permissive license (outside contributions only with their authors'
   permission). Before offering one, check every dependency and model
   in `THIRD_PARTY_NOTICES.txt` (copyleft dependencies would block it).
3. **Training data of the drum model.** `crates/stems/models/DRUMS_NOTICE`
   credits the Lakh MIDI Dataset and the Groove MIDI Dataset (CC BY 4.0).
   The Lakh MIDI files are transcriptions of commercial songs. The dataset's
   license doesn't settle the rights in those songs, so this is worth a
   professional opinion if the model is ever used commercially.
4. **Trademarks.** The README's trademark note covers Ableton, Live, VST,
   Roland. The UI and guide also name *OTT* (an Xfer Records product) for the
   multiband effect and its preset (`docs/guia.md`, "Multibanda (OTT)").
   Consider whether to keep that name or describe it generically.
5. **Legal entity.** There is no company or foundation behind Pulse. The
   copyright holder is David Fernández Morilla as an individual. If paid
   services start, the entity question should be settled first.
