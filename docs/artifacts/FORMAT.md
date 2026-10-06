# Surface file and Pack format, version 1

This is the specification of the two files LaunchDeck shares: a **Surface file** (one surface) and a **Pack** (several surfaces, with optional context rules and inputs). It describes what the code in `src-tauri/src/artifacts/` reads and writes. What these files are for is in [README.md](README.md), how they are defended in [security.md](security.md), and how a person exports and imports one in [sharing](../user/sharing.md).

**Format version 1 is an alpha format.** It is public so that files can be read, written and checked by others, but it is not a stability promise: a later version may change anything, under a new `formatVersion`. A version is never changed in place.

Status: the reader, the writer and the import are implemented, and their tests ran on macOS only, in this milestone. CI has not run them: NOT VERIFIED (GitHub Actions blocked by account billing). Linux and Windows: NOT VERIFIED (no such system here). The example files in `examples/artifacts/` are made from the Linux starter surfaces with X11 identities, so that they do not depend on the system they are made on, and were checked on macOS only. Using export and import in the app's window, the macOS Downloads prompt included: NOT VERIFIED (not done by a person yet).

## The two kinds

| | Surface file | Pack |
| --- | --- | --- |
| Extension | `.ldsurface` | `.ldpack` |
| Media type | `application/vnd.launchdeck.surface+json` | `application/vnd.launchdeck.pack+json` |
| `format` | `launchdeck.surface` | `launchdeck.pack` |
| `formatVersion` | `1` | `1` |
| Surfaces | exactly 1 | 1 to 16 |
| Context rules | none | 0 to 64 |
| Inputs | none | 0 to 32 |

`format` decides the kind. The extension is only a hint: a Pack named `x.ldsurface`, or a file with no extension, is read by its `format`, with the warning `extensionMismatch`.

A file is one UTF-8 JSON document (RFC 8259) of at most 2 MiB, without a byte order mark. There is no archive, no compression and no embedded file.

## The envelope

The top level is an object with exactly four members, written in this order:

```json
{
  "format": "launchdeck.pack",
  "formatVersion": 1,
  "manifest": { },
  "content": { }
}
```

A reader does not check the order. Any other member refuses the file: `signatures` with `unsupportedSignature`, `assets` with `unsupportedAsset`, anything else with `unknownField`. A missing `manifest` is `invalidManifest`, a missing `content` `invalidContent`.

## How a file is read

A file is read as hostile input, in a fixed order (`artifacts::inspect`). Each step refuses the file with a code, a sentence and, when the problem has a place, a JSON pointer; nothing after a refusing step runs.

1. **Size**, counted in bytes before anything is parsed: an empty file is `empty`, one over 2 MiB `tooLarge`.
2. **Text**: a byte order mark, or bytes that are not UTF-8, are `notUtf8`.
3. **Pre-pass** over the raw text, as a stream of tokens, before any value is typed: arrays and objects nested deeper than 32 are `tooDeep`; the same member name twice in one object, compared after unescaping (`"a"` and `"\u0061"` are the same name), is `duplicateKey`; a string over 64 KiB or a member name over 128 bytes, both counted after unescaping, is `stringTooLong`; anything that is not exactly one JSON value (a syntax error, text after the value, a number too large such as `1e400`) is `notJson`.
4. **Envelope**: not an object, no `format`, or a `format` that is not one of the two kinds is `notArtifact`. Then the members: `signatures` is `unsupportedSignature`, `assets` `unsupportedAsset`, any other unknown member `unknownField`. Then the version: a whole number greater than 1 is `tooNew`; any other value (0, a negative number, `1.0`, `"1"`, a missing version) is `unsupportedFormatVersion`. The version is checked before the manifest is read, because a newer file may look different.
5. **Manifest**, typed (unknown members refused) and checked: `invalidManifest`, or `tooMany` for a count over a limit.
6. **Integrity**: the digest of `content` as it is in the file must equal `manifest.integrity.content`, else `integrity`.
7. **Content**, typed (unknown members refused) and checked: `invalidContent`, or `tooMany` for a count over a limit. This step also refuses an action that runs things whose arguments hide a character, and a request whose address the runtime would read differently from what a person is shown (see [security.md](security.md#actions-that-run-things)).
8. **Cross-checks** across manifest and content, in this order: what each kind may hold (`kindMismatch`); device requirements no weaker than the content (`invalidManifest`); every plugin the content uses declared as a dependency (`invalidManifest`); every reference resolved (`invalidContent`).

Reading writes nothing, runs nothing and reads nothing but the bytes and the file's name. The name is only shown (without control characters or any other character a person cannot see, at most 128 characters) and compared with the kind.

### Rejections

A refused file gives `{ code, message, at? }`. `message` is a sentence for a person. `at` is a JSON pointer (RFC 6901) into the file, such as `/content/surfaces/0/pages/1/controls/3/press`, absent when the problem is the whole file. A value from the file repeated in a message is cut to 40 characters and loses its control and direction characters.

| Code | Meaning |
| --- | --- |
| `empty` | The file has no bytes (or no file was received). |
| `tooLarge` | The file has more than 2 MiB. |
| `notUtf8` | The file is not UTF-8 text, or starts with a byte order mark. |
| `tooDeep` | Arrays and objects are nested more than 32 deep. |
| `duplicateKey` | One object names the same member twice, compared after unescaping. |
| `stringTooLong` | A string is over 64 KiB, or a member name over 128 bytes. |
| `notJson` | The file is not one valid JSON value (a ZIP archive renamed `.ldpack` ends here). |
| `notArtifact` | Not a LaunchDeck file: not an object, no `format`, or another `format` (`launchdeck.workspace`, a raw `workspace.json`). |
| `tooNew` | A `formatVersion` greater than this version of LaunchDeck reads. |
| `unsupportedFormatVersion` | Any other `formatVersion` that is not 1, or none. |
| `unknownField` | A top-level member that is not part of the envelope. |
| `unsupportedSignature` | A top-level `signatures` member: reserved for a later version. |
| `unsupportedAsset` | A top-level `assets` member: format 1 carries no embedded file. |
| `invalidManifest` | The manifest breaks a rule: a member, a text, an id, a version, a requirement, an input, the digest's form; also requirements weaker than the content, and a plugin used but not declared. |
| `integrity` | The content does not have the digest the manifest names. |
| `invalidContent` | The content breaks a rule: a member, a text, an id, a duplicate id, geometry, arguments, a feedback rule, a reference, a hidden character in what an action runs. |
| `tooMany` | More of something than a file may hold (surfaces, pages, controls, looks, rules, identities, dependencies, inputs, options, tested devices, input bindings, arguments, surface references). |
| `kindMismatch` | A Surface file holding what only a Pack may: more than one surface, context rules, inputs. |

## Text rules

The texts of the manifest and the names, labels, descriptions, icons, ids and look names of the content refuse Unicode control characters (category Cc) and the explicit direction controls U+202A to U+202E and U+2066 to U+2069. A description may hold line breaks (`\n`); no other text may. A name, a label, an icon, an option and a look name are 1 to their maximum characters and have no space at either end. Lengths below are in characters (Unicode scalar values) unless they say bytes. The arguments of an action are its own (see [Assignment](#assignment)); those of an action that runs things are held to a stricter rule (see [security.md](security.md#actions-that-run-things)).

## Manifest

`manifest` refuses unknown members.

| Member | Rule |
| --- | --- |
| `id` (required) | Lowercase and dotted, at least two parts, each `[a-z][a-z0-9-]*`, 3 to 64 characters in all, not starting with `launchdeck.` (the plugin id grammar, a separate namespace). Export proposes `<author or local>.<name>`. |
| `name` (required) | 1 to 60. |
| `version` (required) | SemVer `MAJOR.MINOR.PATCH[-PRERELEASE]`, at most 32; build metadata (`+…`) refused; `1.0` and `01.0.0` refused. |
| `description` | At most 1000; line breaks allowed. |
| `author` | `{ name, url? }`: `name` 1 to 64; `url` at most 256, an `https` address with a host and no user name or password. Shown as text, never fetched. |
| `license` | At most 64, one line. Not checked against SPDX. |
| `createdWith` (required) | `{ app: "launchdeck", version }`, `version` a SemVer version as above. Informational. |
| `requirements` (required) | What a device needs to show the content (below). |
| `testedWith` | At most 8 `{ model, kind }`: `model` `[a-z0-9-]{1,32}`, `kind` `hardware` or `virtual`. Informational. |
| `dependencies` | At most 32, one per `pluginId`, sorted by `pluginId` (below). |
| `inputs` | Pack only. At most 32, unique `id` (below). |
| `integrity` (required) | `{ content: "sha256:" + 64 lowercase hexadecimal digits }` (below). |

A member equal to its default is left out when a file is written: an empty `description` or `license`, no `author`, empty lists.

### Device requirements

```json
"requirements": { "grid": { "minWidth": 4, "minHeight": 8 }, "aux": { "top": 5, "side": 0 }, "rgb": true }
```

`grid` (absent when no control is on the grid) is the smallest grid that holds every grid control: the largest `col + span.w` and `row + span.h`, each 1 to 16. `aux.top` and `aux.side` (0 to 16) are the largest index above and beside the grid plus one. `rgb` is true when any colour of any control, its idle look or another look, is not a grey. The exporter computes them; the importer computes them again and refuses a file that declares less than its content needs (`invalidManifest` at `/manifest/requirements/grid`, `/aux` or `/rgb`). A file may declare more.

`testedWith` lists, on export, the models of the devices connected at that moment, each model once per kind. It is informational. On import, how the file fits is shown for each device the app lists (the virtual Launchpad X among them), with the number of controls outside it and whether colour is available. Fit never blocks an import.

### Dependencies

```json
{ "pluginId": "example.counter", "name": "Counter Plugin", "version": "^0.2.0", "required": true }
```

* `pluginId`: a plugin id, as for `id` above. A provider of one part (`system`, `navigation`, `shell`, `http`, `agent-control`, `demo`) is part of LaunchDeck, never a plugin, and cannot be a dependency.
* `name` (optional): 1 to 64, for display.
* `version`: a version requirement in [Cargo's syntax](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html#version-requirement-syntax), as the `semver` crate reads it: a bare `1.2.3` means `^1.2.3`, and below 1.0 the minor version is the breaking one (`^0.2.0` accepts `0.2.5`, not `0.3.0`). At most 64 characters and 4 comparators; a comparator naming a pre-release refused; `*` accepts any version.
* `required`: true when a press or long press uses the plugin, false when only feedback reads it.

Every provider of two or more parts that an assignment or a state rule uses must be declared; otherwise the file is `invalidManifest` at the place that uses it. A declared plugin nothing uses is the warning `unusedDependency`. An event rule may listen to a source that is no plugin (an outside program sending events); such a source is not a dependency.

On import, each dependency gets a status against what is installed: `compatible` (installed in a version the requirement accepts, and running), `disabled` (installed in an accepted version, but turned off or failed to load), `incompatible` (installed in a version the requirement does not accept) or `missing`. None of them blocks an import. There is no solver and nothing is downloaded or installed.

### Inputs (Pack only)

An input is a value the person importing the Pack gives: a folder on their computer, their server's address, the application a rule is for.

```json
{ "id": "project_dir", "type": "path", "label": "Project folder", "description": "Where your code is." }
```

| Member | Rule |
| --- | --- |
| `id` | `[a-z][a-z0-9_]{0,31}`, unique in the file. |
| `type` | `path`, `url`, `enum`, `string` or `application`. Any other, `secret` included, is refused: a secret stays in the credential store and is never an input. |
| `label` | 1 to 40. |
| `description` | At most 280; line breaks allowed. |
| `required` | Default true (left out when true). |
| `default` | An input value (below) the input accepts: an application for `application`, a text for every other type, at most the type's length, not empty (except for `string`), one of the options for `enum`, an `http` or `https` address with a host and no user name or password for `url`. Whether a path is a full path depends on the system it is imported on, so that is checked on import. |
| `options` | `enum` only, and required for it: 1 to 32 `{ value, label }`, `value` 1 to 64 and unique, `label` 1 to 40. |
| `maxLength` | `string` only: 1 to 2048; 256 when absent. |

An input value is `{ "kind": "text", "value": "…" }` or `{ "kind": "application", "namespace": "macos", "id": "com.apple.Safari", "displayName": "Safari" }`. On import, an answer is checked in Rust, on the system it is imported on:

| Type | A valid answer |
| --- | --- |
| `path` | 1 to 1024 characters, a full path on this system: from `/` on macOS and Linux; from a drive (`C:\`, `C:/`) or a share (`\\server\share`) on Windows. |
| `url` | At most 2048, `http` or `https`, a host, no user name or password. A request's address is also read as the runtime will send it (see [security.md](security.md#actions-that-run-things)). |
| `enum` | One of the option values. |
| `string` | At most `maxLength`. |
| `application` | An identity in this system's namespace (`macos`, `windows` or `x11`), valid as an application identity, its name at most 80. |

No text answer may hold a control character or any other character a person cannot see. After an answer is put in place, an installed provider checks the pad as it checks a pad saved in the editor, and what it says about the bound argument becomes a problem of the input.

## Content

```json
"content": { "surfaces": [ ], "contextRules": [ ] }
```

`surfaces` is required (1 in a Surface file, 1 to 16 in a Pack); `contextRules` is Pack only. Every type below refuses unknown members, and a member equal to its default is left out when written. These are the format's own types, converted to and from the workspace model; the workspace is never written out as it is.

### Surface

| Member | Rule |
| --- | --- |
| `id` | `[a-z0-9][a-z0-9-]{0,63}`, unique in the file. |
| `name` | 1 to 40. |
| `description` | At most 400; line breaks allowed. |
| `icon` | 1 to 8. |
| `category` | `[a-z][a-z0-9-]{0,31}`. LaunchDeck knows `agents`, `development`, `productivity`, `smart-home`, `accessibility` and `custom`; any other is imported as `custom`, with the warning `unknownCategory`. |
| `pages` | 1 to 32. |

### Page

| Member | Rule |
| --- | --- |
| `id` | `[A-Za-z0-9._:-]{1,64}`, unique in its surface. |
| `name` | 1 to 40. |
| `controls` | At most 128 (and at most 2048 in the whole file). |

### Control

| Member | Rule |
| --- | --- |
| `id` | `[A-Za-z0-9._:-]{1,64}`, unique on its page. |
| `pos` | `{ "kind": "grid", "col", "row" }`, `{ "kind": "top", "index" }` or `{ "kind": "side", "index" }`; each number 0 to 15. There is no `logo` position: the logo LED shows, it holds no control. |
| `span` | Grid only, absent for one cell: `{ "w", "h" }`, each 1 to 8. A grid control may not reach past column or row 16. |
| `label` | 1 to 40. |
| `description` | At most 400; line breaks allowed. |
| `icon` | 1 to 8. |
| `visual` | The idle look: `{ "color": "#rrggbb" (lowercase), "animation"?: "none" \| "pulse" \| "flash" }`. |
| `looks` | At most 32 named looks, each a `visual`; names 1 to 40. The workspace calls them `states`. |
| `press`, `longPress` | An assignment (below), each optional. |
| `feedback` | At most 32 feedback rules (below). |

Each page is also checked as the editor checks it, on the page as it would be imported: no two controls on the same cell or the same top or side pad.

### Assignment

What a press or a long press does.

```json
{ "provider": "navigation", "action": "openSurface", "args": { "surfaceId": "development" }, "surfaceRefs": ["surfaceId"] }
```

| Member | Rule |
| --- | --- |
| `provider` | 1 to 64. One part is part of LaunchDeck; two or more a plugin, which must be declared. |
| `action` | 1 to 64. |
| `args` | An object: at most 16 members, each name `[A-Za-z_][A-Za-z0-9_]{0,31}`; nested at most 4 deep (`args` itself counts); at most 32 KiB written compactly; no text over 16384 characters. |
| `inputs` | Pack only: `{ argument name: input id }`, the arguments an answer replaces (below). |
| `surfaceRefs` | Argument names, each once and at most 16 (`tooMany` beyond), whose value names a surface of this file (below). |

### Feedback rule

A rule is exactly the workspace's feedback rule of schema 4 (`state::feedback::FeedbackRule`), held to its shape by a golden test: a state rule (`when.state` with a test `when.is`) or an event rule (`when.event` with up to 8 `where` conditions), showing `show` (a look of the control, or `idle`), or `showFromValue` (state rules) or `showFromField` (event rules), and for event rules a `hold` of `{ "ms": 200 }` to `{ "ms": 60000 }` or `"latch"`. Unknown members are refused, also inside `is` and `hold`. Each rule passes the checks that need no provider (`check_rule_shape`): one look to show, the hold, the payload conditions, the state address's grammar. The rules themselves are described in [state and feedback](../state-and-feedback.md#feedback-rules).

### Context rule (Pack only)

```json
{ "application": { "name": "Visual Studio Code", "identities": [ { "namespace": "macos", "id": "com.microsoft.VSCode" } ] }, "surface": "development", "page": "home" }
```

* `application` is exactly one of:
  * an application as each system names it: `name` 1 to 80 (for people, never matched) and 1 to 8 `identities` `{ namespace, id }`, one per namespace, each a valid application identity (`namespace` `[a-z][a-z0-9-]*` of at most 16 bytes; `id` 1 to 255, no space at either end, no control character). Any valid namespace is kept, also one this version does not observe;
  * `{ "input": "<id>" }`, an `application` input the person answers on import.
* `surface`: the id of a surface of this file.
* `page` (optional): a page of that surface.

On import a rule takes only the identity in the namespace of the system it is imported on (see [Context rules](#context-rules) below).

## Inputs, bindings and references

**Binding an argument.** `inputs` on an assignment maps an argument to an input: the answer replaces the whole argument. There is no template language. Only a text argument can be bound, and only to an input of type `path`, `url`, `enum` or `string`; an `application` input binds only to a context rule's `application`. A binding to an undeclared input, of the wrong type, or to an argument that is not a text is `invalidContent`. At most 256 bindings (arguments and rules together) per file.

In the file, a bound argument holds the input's default, or an empty text: never the value it had on the exporter's computer. On import, a valid answer is put in its place; an optional input left without an answer removes the argument; a required one without an answer blocks the import while a kept pad or a selected rule needs it.

**Surface references.** `surfaceRefs` lists the arguments whose value is the id of a surface of the same file. On import each is renamed to the id that surface gets here. Each must be a text argument naming a surface of the file, else `invalidContent`. The exporter marks every argument whose definition is a reference to a surface (navigation's `surfaceId`, and plugin parameters of that kind) and whose value is an exported surface. An argument that names a surface not in the file keeps its value, so it opens a surface of that id on the importing computer if there is one (warning `externalSurfaceReference`). The importer never consults the catalog to find references: only `surfaceRefs` is renamed. Page and control ids, all other arguments, and the importer's own rules and pads are never rewritten.

## Integrity

```
integrity.content = "sha256:" + lowercase hex SHA-256 of the bytes of the `content` member
                    as they appear in the file, with every space (0x20), tab (0x09),
                    line feed (0x0A) and carriage return (0x0D) outside strings removed
```

Re-indenting the file or changing its line ends does not change the digest; any change of a value, a member name, an order or an escape does (`"x\u0020y"` and `"x y"` differ). A file whose content does not match is refused with `integrity`. The form is checked before: exactly `sha256:` and 64 lowercase hexadecimal digits (`invalidManifest` otherwise).

The digest proves the content is the content the manifest was written with. It proves nothing about who wrote either: anyone can change a file and write a new digest. There is no publisher identity in version 1: the publisher of a file is never verified.

**Reserved for signing.** A later format version may add a top-level `signatures` member, a list of `{ keyId, algorithm: "ed25519" | "sigstore-bundle", signature }` over the manifest's bytes with the same whitespace removed; the manifest pins the content through its digest. Version 1 has no signatures and refuses a file that carries the member (`unsupportedSignature`).

## Export

Export (`artifacts::export`) is a pure function of the workspace, what the app knows about its providers and devices, the app's version and the request. It writes no file and reads no secret, grant, plugin setting, agent state or runtime value.

**What is written:** the chosen surfaces, their pages and controls; in a Pack, if asked, the workspace's context rules that show an exported surface; the inputs and bindings the person asked for; the dependencies, requirements and devices derived from them; the metadata.

**What is never written:** the workspace's `schemaVersion`, preferences, last active surface, Auto Context's switch and fallback, rules for surfaces left out, a surface's context hint and `importedFrom`, a control's legacy bindings, actions of the `legacy` provider (their slot is left empty), any runtime state (values, held or temporary looks, pending actions, provider status, availability), agent sessions, settings, the audit trail, the hook token, plugin settings, grants, secrets, logs, device ids, times and anything that identifies the machine or the person. The preview counts what was stripped: legacy bindings, legacy actions, context hints.

**Bound arguments and credentials.** A bound argument is written as its input's default or empty. A value shaped like a credential in an argument that is not bound, at any depth, in a feedback rule (a state's arguments, an event's conditions), or in an input's default, blocks the file (`credential`): the person binds the argument to an input, or removes the value (a feedback rule has no input to stand in). The shapes: an address with a user name or password; an address parameter, in the query or the fragment, named `key`, `auth`, `sig`, `pass` or `pwd`, or ending in `token`, `secret`, `password`, `passwd`, `apikey`, `signature` or `credential` (compared without case, `-` and `_`), or ending in a separate word `key` (`api_key`, `accessKey`); the same names, except `key`, `auth` and `sig`, for an argument or a member of a JSON argument; an `Authorization`, `Proxy-Authorization`, `Cookie` or `X-API-Key` header; a Home Assistant webhook id (also written with escapes such as `%77ebhook`); and the secret part of a Slack, Discord, Zapier or IFTTT webhook address or of a Telegram bot address. Inside a text (a request body, a command, an instruction) it also finds a text that is JSON, read as JSON; an address of any scheme with a user name or password (`postgresql://user:pw@…`); a `name=value` word with such a name (`password=…`, `GITHUB_TOKEN=…`, `--api-key=…`); a header written `Name: value` with one of the header names above or a hyphenated secret name (`X-Auth-Token`, `Private-Token`), also right after curl's `-H`; and a header given as `{ "name": …, "value": … }`. Every address is read twice, as the HTTP client that sends `http.request` reads it and as a browser does, and each reading is checked for these shapes; an address only one of them reads, or that they read with different hosts (`https://ex%61mple.com`, `https://e.com\x`, a host outside ASCII, `http://0x7f.1`), blocks the file too, with its own sentence, since what it carries cannot be told; and a `@` between `://` and the host's end counts as a user name whatever either reader makes of it. The check knows these shapes only: `curl -u user:pw`, a user name and password without a scheme, or a password in prose are not found.

**Dependencies.** A plugin used by a press or long press is `required: true` with `^` and its installed version; one only feedback reads is `required: false`. A plugin of two or more parts that is not installed here, or whose version cannot be read, is declared for `*` with a warning (`notInstalled`). An event source that is not an installed plugin is not a dependency. Host providers never are.

**Metadata.** An empty name becomes the first surface's name, an empty version `1.0.0`, an empty id `<author>.<name>` from the author's name (at most 24 characters) and the name, reduced to lowercase letters, digits and dashes, or `local.<name>` without an author.

**Errors and warnings.** The preview lists errors, which block the file (no bytes are produced), and warnings:

| Code | Kind | Meaning |
| --- | --- | --- |
| `noSurfaces` | error | No surface chosen. |
| `unknownSurface` | error | A chosen id names no surface. |
| `kindMismatch` | error | A Surface file asked for more than one surface, context rules or inputs. |
| `invalidMetadata` | error | The id, name, version, description, author or license breaks a manifest rule. |
| `invalidInput` | error | An input breaks a rule. |
| `invalidBinding` | error | A binding, rule input or extra identity names nothing exported, or the wrong kind of input or argument. |
| `invalidIdentity` | error | An identity added for another system is not valid. |
| `credential` | error | A value shaped like a credential, or an address read two ways, as above. |
| `tooMany` | error | A count over a limit. |
| `tooLarge` | error | The file would be over 2 MiB. |
| `invalidContent` | error | Anything else the importer would refuse, such as a hidden character in what an action runs. |
| `reviewBeforeSharing` | warning | A pad that runs a command, sends a request, starts a program or instructs an agent: check it holds no password or token. |
| `externalSurfaceReference` | warning | A pad opens a surface that is not in the file. |
| `notInstalled` | warning | A plugin the content uses is not installed here (or its version cannot be read). |
| `localPath` | warning | A text argument looks like a path on this computer (`/…`, `~/…`, `C:\…`); the preview suggests asking the importer for it. It never does so on its own. Also, without a suggestion, a longer text (a command, arguments, an instruction) that names a folder in a home folder (`/Users/<name>/`, `/home/<name>/`, `C:\Users\<name>\`), which shows a user name. |
| `nonTextArgument` | warning | A pad's argument is an object or a list, which the preview's parameters (text arguments with their values) do not show; the warning names the pad and the argument, so nothing is exported unseen. |
| `ruleTargetMissing` | warning | A context rule shows a page that no longer exists, and is left out. |

Before any bytes are produced, the file goes through the importer's own checks of the manifest, the content and both together; what they refuse is an error here.

**Determinism.** The same workspace, environment and request give the same bytes. Surfaces are in workspace order whatever the order chosen, pages and controls in their stored order, rules in workspace order, dependencies by `pluginId`, inputs in the request's order. A surface's id in the file is the id its own file gave it when it was imported and no other chosen surface came from the same id, else its local id; a second surface wanting an id gets `-2`, `-3`…, the id cut short first where needed so the result has at most 64 characters. The writer is canonical: object members sorted by their names as UTF-16 code units, two-space indentation, LF line ends and one LF at the end, strings and numbers as `serde_json` writes them (non-ASCII text as UTF-8), the four envelope members in their fixed order. Tests export twice and compare bytes, and export, reset, import and export again and compare bytes.

**File name and folder.** `<name>-<version>.ldsurface` or `.ldpack`, the name reduced to `[a-z0-9-]`, the version to `[a-z0-9.-]`, at most 64 characters (`development-1.0.0.ldpack`). The file is saved in the system's Downloads folder (where a system has none, in an `exports` folder next to the workspace), written atomically. A name already taken gets `-2`, `-3`… up to `-999` before the extension; an existing file is never replaced. The webview never sends a path. There is no save dialog in version 1. On macOS the system may ask once whether LaunchDeck may use Downloads; a refusal is shown as a sentence naming System Settings, Privacy & Security, Files and Folders. That prompt and its refusal have not been seen in the app: NOT VERIFIED.

## Provenance

When a surface is imported, the workspace records where it came from on the surface itself, as `importedFrom`:

| Member | Holds |
| --- | --- |
| `format` | `launchdeck.surface` or `launchdeck.pack`. |
| `artifactId`, `artifactName`, `artifactVersion` | From the file's manifest. |
| `instance` | Which import of that file, from 1: importing it again as new makes another instance; replacing one keeps its number. |
| `sourceSurfaceId` | The surface's id in the file. |
| `contentDigest` | The file's `integrity.content`. |
| `surfaceDigest` | `sha256:` and the digest of the surface as it was written here, in its canonical file form. A surface whose digest is another one now has been changed since it was imported. |

It is local: never exported, with no time, path or anything about the computer, and never sent anywhere. The workspace schema stays 4: the member is optional and a workspace without imports is written byte for byte as before. A version of LaunchDeck from before this format reads such a workspace, because its surface type accepts unknown members, and drops the member when it saves (from the code; not run with an older build). Import finds earlier imports of a file by it.

## Import

### Flow

Import is two pure functions and a transaction:

1. `inspect(bytes, file name)` reads the file as above: a parsed file, or a rejection.
2. `plan(parsed file, workspace, environment, decisions)` says exactly what an import would do, changing nothing: the **ImportPlan**. The environment is built from the app's own getters (the snapshot of the workspace, the action catalog, the observable catalog, the Context Engine's namespace and recent applications, the devices) and the runtime's own checks of a pad and of a state rule.
3. Applying writes exactly the plan's effects, in one transaction (below).

The plan is computed in Rust. Whatever shows it computes no id, conflict, dependency or validity of its own, and asks for the plan again (`artifacts_preview`) when a decision changes.

| Command | Does |
| --- | --- |
| `artifacts_limits()` | The limits, as `ArtifactLimits`. |
| `artifacts_inspect` | The file's bytes as the raw body and its name in the header `x-launchdeck-file-name` (as `encodeURIComponent` writes it). Returns `{ status: "plan", plan }`, planned with the decisions the screen proposes, or `{ status: "rejected", rejection }`. |
| `artifacts_preview(planId, decisions)` | The plan again with other decisions: `{ status: "plan", plan }` or `{ status: "expired" }`. |
| `artifacts_apply(planId, decisions, planDigest)` | `applied`, `stale`, `blocked`, `failed` or `expired` (below). |
| `artifacts_discard(planId)` | Forget the pending import. |
| `artifacts_export_preview(request)` | The export preview, without writing. |
| `artifacts_export_save(request)` | Save the file: `{ fileName, folder, sizeBytes, contentDigest }`. |
| `artifacts_show_export()` | Show the folder of the last saved file in the system's file manager. |

One import is pending at a time. Inspecting another file replaces it, also when that file is refused; it is dropped when applied or discarded.

### The ImportPlan

| Part | Says |
| --- | --- |
| `planId` | 32 hexadecimal digits from the system's random source: which pending import this is. |
| `planDigest` | `sha256:` and the digest of the canonical JSON of `effects`. |
| `source` | The file name as shown, its size, its content digest, its format version. |
| `artifact` | Kind, id, name, version, description, author, license, `createdWith`, `testedWith`. |
| `surfaces` | Each surface of the file: its id in the file and here, name, category as imported, pages and controls, `add` or `replace` (with the version it replaces and whether that surface was edited here), whether its id changed, the local surface with the same name if any, and the providers its pads and state rules use that are not here. |
| `contextRules` | Each rule of the file: the application's name and identity here, the surface and page it shows, and its outcome (below). |
| `dependencies` | Each declared plugin: requirement, `required`, status, and the controls that use it. |
| `inputs` | Each input: its definition, the answer given, the problems with it, the pads it fills. |
| `review` | Each pad that runs things, with exactly what it runs. |
| `existing` | Earlier imports of the same file (below). |
| `compatibility` | This system's namespace, the declared requirements, and the fit on each device. |
| `warnings` | What a person should know that does not stop the import, by place in the file, then by code. |
| `decisions`, `suggested` | The decisions this plan was made with, and those the screen proposes. |
| `blockers`, `ready` | What stops the import; `ready` when nothing does. |
| `effects` | Exactly what applying writes, and the only part the digest covers: the instance, the surfaces added (`sourceId`, `localId`, `name`), the surfaces replaced, the rules added, the slots emptied, the valid answers. |

The texts a person reads in the plan are display-safe: the file's name, description, author and license, each surface's name (and the names of earlier imports), the labels of the controls that use a plugin, each rule's application name, each dependency's name, and each input's label, description and option labels have every character a person cannot see written as an escape, as review text has (right-to-left letters stay as they are). `effects` and what applying writes keep the file's own text.

### Decisions

```json
{ "resolution": { "kind": "importAsNew" }, "runs": "remove", "inputs": { }, "contextRules": [0], "confirmOverwriteEdits": false }
```

* `resolution`: needed only when the file was imported before: `{ "kind": "importAsNew" }` or `{ "kind": "replace", "instance": 1 }`.
* `runs`: needed only when a pad runs a command, sends a request or starts a program: `keep` or `remove`.
* `inputs`: answers by input id. Answers to inputs the file does not declare are ignored.
* `contextRules`: the positions of the file's rules to add. None by default.
* `confirmOverwriteEdits`: replace surfaces that were changed here since they were imported.

What the screen proposes (`suggested`): import as new when there is a choice to make, remove the pads that run things when there are any, the inputs' defaults that are valid here, and **no context rule**.

### Blockers

In this order:

| Code | Until |
| --- | --- |
| `resolutionNeeded` | The file was imported before and no valid resolution is chosen. |
| `overwriteEditsNotConfirmed` | A surface to replace was edited here and the replacement is not confirmed. |
| `runsDecisionNeeded` | Pads run things and `runs` is not chosen. |
| `inputRequired` | A required input has no answer while a kept pad or a selected rule takes it; or an optional input is left unanswered and a pad cannot do without the argument it would fill. |
| `inputInvalid` | An answer is given and wrong. |
| `workspaceFull` | The workspace would hold more than 64 surfaces. |

### Warnings

| Code | Meaning |
| --- | --- |
| `extensionMismatch` | The file's name does not end in its kind's extension. |
| `unknownCategory` | A category this version does not know: imported as `custom`. |
| `keptFromPreviousVersion` | Replacing an import, a surface the new version no longer has is kept as it is. |
| `externalSurfaceReference` | A pad opens a surface by an id the file does not rename. |
| `invalidForInstalledVersion` | What an installed provider says of a pad or a state rule; the pad or rule is kept as the file has it. |
| `unknownHostProvider` | A provider of one part that this version of LaunchDeck does not have: the pad will not work here. |
| `externalEventSource` | An event rule listens to a source nothing in LaunchDeck sends: another program must send the events. |
| `unusedDependency` | A declared plugin nothing uses. |
| `missingDependency` | "Some pads will not work until you install X <requirement>." |
| `disabledDependency` | The plugin is installed but off or failing. |
| `incompatibleDependency` | The plugin is installed in a version the file does not accept. |

### Conflicts and importing again

* **Same surface id.** A new surface gets the file's id when it is free here, else `<id>-2`, `-3`… (the id cut short first where needed, so the result has at most 64 characters and can be exported again), never an id taken here or by an earlier surface of the same import. The plan shows `idChanged`. Not a choice.
* **Same name.** The name is kept; the plan names the local surface that has it (`sameNameAs`). The imported one is told apart by its `importedFrom`.
* **The same file again.** Surfaces whose `importedFrom` has the same `format` and `artifactId` are earlier imports, grouped by instance. Each is listed with its version (the newest of its surfaces), how the file's version compares by SemVer precedence (`newer`, `same`, `older`), whether its content is identical, and which of its surfaces were edited here. A resolution must be chosen. **Import as new** makes a new instance with surfaces of its own. **Replace** puts each surface of the file in the place of the surface of that instance it came from (same local id and position, a new `importedFrom`), adds the file's other surfaces to that instance, keeps the instance's surfaces the file no longer has (`keptFromPreviousVersion`), and waits for `confirmOverwriteEdits` when any replaced surface was edited. A downgrade or an identical file is shown and remains the person's choice. There is no merge, no automatic update and no update check.
* **Duplicate ids inside the file** (surfaces, pages of a surface, controls of a page) refuse it.

### Context rules

Rules are opt-in: none is added unless the person selects it. For each selected rule, the application is the identity in this system's namespace, or the answer to its application input. A rule is added after every local rule, in the file's order, when it passes the Context Engine's own check against the workspace as it would be after the import. Otherwise its outcome says why not:

| Outcome | Meaning |
| --- | --- |
| `add` | Added, at this position. |
| `notSelected` | The person did not select it. |
| `skipConflict` | A local rule names the same application; the local rule wins. |
| `skipShadowed` | An earlier rule of the file, added, names the same application. |
| `skipNoIdentity` | The file names no application of this system's kind. |
| `skipCapacity` | The workspace already has 64 rules. |
| `needsInput` | It waits for the answer to its application input. |

An import never turns Auto Context on or off and never changes its fallback.

### Actions that run things

Planning classifies every press and long press as the runtime would run it, after answers and renaming:

| Kind | Which | Text shown |
| --- | --- | --- |
| `runsCommand` | `shell.execute` | The command, in full. |
| `sendsRequest` | `http.request`, and any other action of LaunchDeck itself (not of a plugin) marked privileged | The method, the address, where it goes (host and port; "on this computer" or "on your local network" when it is), and the body; for another privileged action, its id and all its arguments. |
| `launchesProgram` | `system.launchApplication`, every one, on every system: a plain name without arguments too, since on Linux and Windows it runs any program of that name on `PATH` | The program and each argument as the runtime passes them. |
| `instructsAgent` | `agent-control.newTask`, `agent-control.continue`, `agent-control.approve`, `agent-control.reject`, `agent-control.retry` | The instruction (and the folder of a new task); for an answer or a retry, the command in words: "approves the agent's pending request", "rejects the agent's pending request", "retries the agent's failed or cancelled session". |

Every hidden character in a review text is shown as an escape. The first three kinds need the decision `runs`: `remove` (proposed) empties those slots and keeps each control's label, looks and feedback; `keep` imports them as the file has them. An instruction to an agent is shown, needs no decision and is always kept. Plugin actions are never review items: a plugin cannot be privileged, and its network access stays behind the grants the person gives, which an import never changes.

Nothing is executed during an import: no action, no test run, no install, no rescan, no download, no grant. See [security.md](security.md#nothing-runs-during-an-import).

### Missing or incompatible plugins

A dependency that is missing, off or in another version never blocks an import, and the content is stored as the file has it, never adjusted to what is installed. Until the plugin is there, its pads show as unavailable, as any pad of a missing plugin does, and its state rules see the value as unavailable. Recovery needs no import step: install the plugin (the bundled examples install from the Plugins screen), and once it is running the pads work. For an installed provider, each pad is checked as the editor checks it and each state rule against the states the provider declares; what does not fit is the warning `invalidForInstalledVersion` with its place, and is kept as it is.

### Transactional apply

`artifacts_apply(planId, decisions, planDigest)`:

1. A plan id that is not the pending import is `expired`.
2. Under the runtime's workspace lock, the plan is made again against the workspace as it is now, with what is installed now. If its digest is not `planDigest`, the outcome is `stale` with the new plan to check again; if anything blocks it, `blocked` with the plan.
3. The new workspace is built from the current one and exactly the plan's effects, and checked as the runtime checks an edit: surface ids unique, every control of every added or replaced page through the editor's control check, the context rules through the Context Engine's check.
4. It is saved to `workspace.json` (an atomic write), and only after the save succeeds is it shown.

| Outcome | Meaning |
| --- | --- |
| `{ status: "applied", report }` | Done. `report`: the local ids of the surfaces added and replaced, the rules added and skipped, the slots emptied, the missing dependencies, and whether Auto Context is on. |
| `{ status: "stale", plan }` | Something changed since the preview: the workspace, the decisions or what is installed. |
| `{ status: "blocked", plan }` | Still waiting for a decision or an answer. |
| `{ status: "failed", message, detail? }` | It could not be built, checked or saved. `detail` is what the system said. A failed save leaves the import pending, so it can be tried again. |
| `{ status: "expired" }` | Another file was opened since, or the import was applied or discarded. |

On every outcome but `applied`, the workspace in memory and `workspace.json` are as they were.

## Limits

The numbers are in `src-tauri/src/artifacts/limits.rs`; the app reads them through `artifacts_limits` (field names in camelCase below). A count over a limit is `tooMany`; a length over a limit is `invalidManifest` or `invalidContent`, unless the table says otherwise.

| Limit | Value | `ArtifactLimits` |
| --- | --- | --- |
| File | 2 MiB (`tooLarge`) | `fileBytes` |
| JSON nesting | 32 (`tooDeep`) | `jsonDepth` |
| String, after unescaping | 64 KiB (`stringTooLong`) | `stringBytes` |
| Member name | 128 bytes (`stringTooLong`) | `keyBytes` |
| Embedded files | 0 | `assets` |
| Surfaces | 1 in a Surface file, 1 to 16 in a Pack | `surfaceFileSurfaces`, `packSurfaces` |
| Pages per surface | 1 to 32 | `pages` |
| Controls | 128 per page, 2048 per file | `controlsPerPage`, `controls` |
| Looks per control | 32, names 1 to 40 | `looks`, `lookName` |
| Feedback rules per control | 32 | `feedbackRules` |
| Conditions of an event rule | 8 | `whereConditions` |
| Hold | 200 to 60000 ms, or latch | `minHoldMs`, `maxHoldMs` |
| Grid | coordinates 0 to 15; nothing past column or row 16 | `grid` |
| Span | 1 to 8 each way | `span` |
| Top and side pads | index 0 to 15 | `aux` |
| Context rules | 64 per file; 64 in the workspace after import (more are skipped) | `contextRules` |
| Identities of an application | 1 to 8; id 1 to 255; namespace 1 to 16 bytes | `identities`, `identityId`, `namespace` |
| Application name | 1 to 80 | `applicationName` |
| Arguments of an assignment | 16 members, 4 deep, 32 KiB, a text 16384; 16 surface references | `argKeys`, `argDepth`, `argBytes`, `argText` |
| Input bindings | 256 per file | `bindings` |
| Manifest name | 1 to 60 | `name` |
| Manifest description | 1000 | `description` |
| Author | name 1 to 64, address 256 | `authorName`, `authorUrl` |
| License | 64 | `license` |
| Version | 32 | `version` |
| Version requirement | 64, 4 comparators | `requirement`, `comparators` |
| Dependencies | 32, name 1 to 64 | `dependencies`, `dependencyName` |
| Tested devices | 8, model 1 to 32 | `testedWith`, `model` |
| Inputs | 32, id 1 to 32, label 1 to 40, description 280 | `inputs`, `inputId`, `inputLabel`, `inputDescription` |
| Enum options | 1 to 32, value 1 to 64 | `enumOptions`, `optionValue` |
| String input | `maxLength` 1 to 2048, default 256 | `stringInput`, `defaultStringInput` |
| Path answer | 1024 | `path` |
| Address answer | 2048 | `url` |
| Ids (surfaces, pages, controls, providers, actions) | 64 | `id` |
| Surface and page name, label, option label | 1 to 40 | `label` |
| Surface and control description | 400 | `surfaceDescription`, `controlDescription` |
| Icon | 1 to 8 | `icon` |
| Category | 32 | `category` |
| Surfaces in the workspace after import | 64 (blocker `workspaceFull`) | `workspaceSurfaces` |

Where the editor or another part of LaunchDeck already has a limit (feedback rules, look names, hold, coordinates, context rules, identities, arguments, controls per page, label, description, span), the format takes that number: by reference where it is public, and otherwise a test proves the editor refuses one more than the format allows.

## Versions

`formatVersion` 1 is shared by both kinds. It is independent of every other version number in LaunchDeck: the workspace schema (4), the plugin manifest schema (4), the plugin ABI (1), the AssemblyScript SDK (0.2.1) and the app's own version. A reader accepts exactly the versions it knows: a greater one is `tooNew`, any other `unsupportedFormatVersion`. `createdWith.version` records the app that wrote a file and decides nothing.

## Not in version 1

* **Embedded files** (`assets`): images, icons as files, anything binary. Refused with `unsupportedAsset`.
* **Signatures** and any publisher identity: reserved as described under [Integrity](#integrity), refused with `unsupportedSignature`.
* **Archives or compression**: a file is one JSON document.
* **Merging** an import into existing surfaces, **automatic updates** and **update checks**. Importing again is an explicit choice: a new copy, or replacing an earlier import.
* **Removing an imported surface** through the import: there is no uninstall step.
* **Installing or downloading plugins** on import, and any dependency solving.
* **Agent pieces.** An earlier draft "Agent Pack" manifest (adapters, state mappings, workflow templates) was validated but never loaded, and has been removed. Adapters, state mappings (a provider's status words to normalized statuses) and workflow templates (named starting tasks) are candidates for a later format version. Two rules of that draft would carry over: a state mapping may not target `approval_required` or `offline`, which an adapter cannot report through a status update; and a workflow template carries text, never a command. See [Agent Control: sharing adapters](../agent-control/adapter-sdk.md#sharing-adapters-not-built).

## Example

A Pack written by LaunchDeck, **trimmed**: it is `examples/artifacts/development.ldpack` (the Development starter surface with a rule for Visual Studio Code, written by a test runtime with app version 9.9.9) with its second page (Git) and 11 of the 15 controls of its first page left out. The digest is the whole file's, so this excerpt, saved as a file, is refused with `integrity`.

The four pads show three cases: **Terminal** launches an application by a plain name and **Docker** runs a command, which are both reviewed and need the `runs` decision; **Agents** opens a surface that is not in the file (`externalSurfaceReference`), while **Dev** opens the file's own surface and lists the argument in `surfaceRefs`, so it is renamed if the surface's id changes on import.

```json
{
  "format": "launchdeck.pack",
  "formatVersion": 1,
  "manifest": {
    "author": {
      "name": "LaunchDeck examples"
    },
    "createdWith": {
      "app": "launchdeck",
      "version": "9.9.9"
    },
    "description": "The Development starter surface, shown for Visual Studio Code.",
    "id": "launchdeck-examples.development",
    "integrity": {
      "content": "sha256:c659b2bb18a2f7d1ace559fb6166ec14534caa0688ad9fc1948104fc6ef8430c"
    },
    "license": "MIT",
    "name": "Development",
    "requirements": {
      "aux": {
        "side": 0,
        "top": 5
      },
      "grid": {
        "minHeight": 8,
        "minWidth": 4
      },
      "rgb": true
    },
    "testedWith": [
      {
        "kind": "virtual",
        "model": "launchpad-x"
      }
    ],
    "version": "1.0.0"
  },
  "content": {
    "contextRules": [
      {
        "application": {
          "identities": [
            {
              "id": "Code",
              "namespace": "x11"
            },
            {
              "id": "com.microsoft.VSCode",
              "namespace": "macos"
            },
            {
              "id": "code.exe",
              "namespace": "windows"
            }
          ],
          "name": "Visual Studio Code"
        },
        "surface": "development"
      }
    ],
    "surfaces": [
      {
        "category": "development",
        "description": "Build, test and ship. Pads pulse while a command runs, then turn green or flash red.",
        "icon": "⌘",
        "id": "development",
        "name": "Development",
        "pages": [
          {
            "controls": [
              {
                "description": "Launch a terminal (placeholder command).",
                "icon": "›_",
                "id": "g.0.0",
                "label": "Terminal",
                "looks": {
                  "error": {
                    "animation": "flash",
                    "color": "#ff2832"
                  },
                  "running": {
                    "animation": "pulse",
                    "color": "#ffc81e"
                  },
                  "success": {
                    "color": "#1edc5a"
                  }
                },
                "pos": {
                  "col": 0,
                  "kind": "grid",
                  "row": 0
                },
                "press": {
                  "action": "launchApplication",
                  "args": {
                    "app": "x-terminal-emulator"
                  },
                  "provider": "system"
                },
                "visual": {
                  "color": "#1ed2e6"
                }
              },
              {
                "description": "Placeholder: harmless echo. Replace with `docker compose up`.",
                "icon": "▣",
                "id": "g.3.0",
                "label": "Docker",
                "looks": {
                  "error": {
                    "animation": "flash",
                    "color": "#ff2832"
                  },
                  "running": {
                    "animation": "pulse",
                    "color": "#ffc81e"
                  },
                  "success": {
                    "color": "#1edc5a"
                  }
                },
                "pos": {
                  "col": 3,
                  "kind": "grid",
                  "row": 0
                },
                "press": {
                  "action": "execute",
                  "args": {
                    "command": "sleep 1 && echo 'docker: demo only'"
                  },
                  "provider": "shell"
                },
                "visual": {
                  "color": "#1ed2e6"
                }
              },
              {
                "description": "Switch to the Agents surface.",
                "icon": "◉",
                "id": "t.0",
                "label": "Agents",
                "pos": {
                  "index": 0,
                  "kind": "top"
                },
                "press": {
                  "action": "openSurface",
                  "args": {
                    "surfaceId": "agent-control"
                  },
                  "provider": "navigation"
                },
                "visual": {
                  "color": "#3c1c59"
                }
              },
              {
                "description": "Switch to the Dev surface.",
                "icon": "⌘",
                "id": "t.1",
                "label": "Dev",
                "pos": {
                  "index": 1,
                  "kind": "top"
                },
                "press": {
                  "action": "openSurface",
                  "args": {
                    "surfaceId": "development"
                  },
                  "provider": "navigation",
                  "surfaceRefs": [
                    "surfaceId"
                  ]
                },
                "visual": {
                  "color": "#ffffff"
                }
              }
            ],
            "id": "home",
            "name": "Home"
          }
        ]
      }
    ]
  }
}
```

The other files in `examples/artifacts/` are what the app's commands return for this file: the export preview, the import plan (on a workspace that already has a `development` surface, so the id becomes `development-2`), the plan of a Pack whose Counter plugin is missing, and a rejection. A test (`tests/artifacts_contract.rs`) keeps them equal to what the code emits.
