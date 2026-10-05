# Marketplace (roadmap)

**Not built.** There is no backend, no account, no download. This document keeps only ideas for hosting, listing and signing what LaunchDeck can share; it defines no file format. What would be listed is defined elsewhere:

* plugins: a folder with `plugin.json` and one WebAssembly module, discovered, validated, sandboxed and loaded by the [Plugin SDK](../plugin-sdk/README.md) host;
* Surface files (`.ldsurface`) and Packs (`.ldpack`): portable JSON files of surfaces, their pads and feedback, and, in a Pack, optional context rules and typed inputs. Their format is specified in [the Surface file and Pack format](../artifacts/FORMAT.md).

## Goals

* Share a surface (layout, looks, actions) without sharing code.
* Share a plugin (new actions and state) with explicit permissions.
* Share a Pack: a curated bundle of surfaces for one job, such as "Video editing" or "AI coding".
* Everything is inspectable before install. Installing never runs code silently.

## Hosting

* The marketplace is optional. Anything it lists is a file that can also be installed or imported from disk, and a hosted copy is the same file.
* No install scripts, no post-install hooks.
* Open question: who hosts it, and how listings are moderated.

## Listing

A listing shows what a package asks for before anything is downloaded. The client fetches only the metadata.

* For a plugin: the actions, and the manifest's `capabilities` summarized as permissions. Since manifest schema 4 one capability exists, `network`, with per-address grants the person gives on the Plugins screen (see [plugin permissions](../plugins/permissions.md)); a plugin that asks for any other is refused by a host that cannot provide it. A listing would add around the manifest something like:

  ```json
  {
    "type": "plugin",
    "permissions": [
      { "name": "network", "hosts": ["homeassistant.local:8123"], "reason": "Talk to your Home Assistant" }
    ],
    "checksums": { "homeassistant.wasm": "sha256:..." },
    "signature": "ed25519:..."
  }
  ```

* For a Surface file or a Pack: its manifest (name, version, author, license, device requirements, plugin dependencies) and the review the import shows: every pad that runs a command, sends a request or launches a program.
* Installing still grants nothing: plugins start only after the person grants their permissions, and an import asks about every pad that runs something.
* An update that adds permissions or such pads needs a new review.

## Signing

* Format 1 of Surface files and Packs carries a content digest (`integrity`). It proves the file is intact, not who made it, and the app says "Publisher: not verified". A later format version reserves a `signatures` member over the manifest, which pins the content digest; format 1 refuses a file that has one.
* A plugin listing would carry checksums of its files and a signature, verified before the package is stored.
* Signed packages and a revocation list.
* Plugin code never receives raw MIDI or pad positions, so a malicious plugin cannot read your keystrokes from the surface or fingerprint your layout. A signature does not change what a package may do.
