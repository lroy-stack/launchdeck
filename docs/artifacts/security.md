# Security of Surface files and Packs

A Surface file or a Pack comes from someone else. LaunchDeck treats every file to import as hostile: written by anyone, changed by anyone, named anything, and made to crash the app, hide what it does, or change more than the person agreed to. This page says what a file can and cannot do, how each threat is closed, and where the tests that prove it are. The format itself is in [FORMAT.md](FORMAT.md).

## What is protected

* **The person's configuration.** Nothing changes until the person confirms an import, and then exactly what the preview showed: surfaces added or replaced, the context rules they selected, the slots they chose to empty. `plugins.json` (plugin settings), `permissions.json` (network grants), secrets, Auto Context's switch and fallback, and every surface the import does not replace stay as they are.
* **The computer.** Nothing in a file runs while it is read, previewed or imported. What a file's pads run, they run only when the person presses them, and only if the person chose to keep the pads that run commands, send requests or start programs.
* **Secrets.** A file never carries one out: export reads no secret, grant or plugin setting, and refuses values shaped like a credential. A file never brings one in: there is no secret input type, and an import never touches the credential store.

## Archive threats are impossible by construction

A file is one JSON document of at most 2 MiB. There is no archive, no compression and no embedded file, and no code path writes anything from a file to disk: the only file an import writes is `workspace.json`, through the app's own store, after the person confirms. LaunchDeck has no archive reader, and the artifact code decompresses nothing. So:

| Threat | Why it cannot happen |
| --- | --- |
| Zip-slip, absolute paths, `..` | No path is ever read from a file. The file's own name is only shown, cleaned of characters a person cannot see and cut to 128 characters, and is never used as a path. |
| Symbolic and hard links | There are no entries to be links. |
| Duplicate or case-colliding entry names | There are no entries. Inside the JSON, a member named twice in one object is refused (`duplicateKey`), also when one spelling is escaped. |
| Compression bombs | Nothing is decompressed. The size is counted in bytes before anything is parsed. |
| Extraction races | Nothing is extracted. |
| A ZIP renamed `.ldpack` | Refused as `notJson`. |
| Embedded files (an SVG with a script, an executable in base64, a name like `../../x`) | Any `assets` member is refused (`unsupportedAsset`) before the manifest is read. |

## What is checked, and in which order

Each step refuses with a code and nothing after it runs (details and codes in [FORMAT.md](FORMAT.md#how-a-file-is-read)):

1. **Size**, before parsing: empty, or over 2 MiB.
2. **UTF-8**, without a byte order mark.
3. **A pre-pass over the raw text**, before any value is typed. A typed parse would silently keep the last of two equal members and has no say on depth or length, so this pass refuses nesting over 32 levels (a file of ten thousand nested arrays is refused here, not by a stack overflow), a duplicate member name (compared after unescaping), a string over 64 KiB or a member name over 128 bytes, and anything that is not exactly one JSON value.
4. **The envelope**: a known `format`, no member but the four of the format (`signatures` and `assets` are refused by name), and exactly format version 1.
5. **The manifest**, typed with unknown members refused, then every rule: ids, text, versions, requirements, dependencies, inputs, the digest's form.
6. **Integrity**: the content as it is in the file has the digest the manifest names.
7. **The content**, typed with unknown members refused, then every rule: ids, text, duplicates, geometry as the editor checks it, arguments, feedback rules; and no hidden character in what an action runs, and no request address read two ways (below).
8. **Cross-checks**: what each kind may hold, requirements no weaker than the content, every plugin used declared, every reference resolved.

Every count is limited (see [limits](FORMAT.md#limits)), so a file cannot make the app allocate or loop without bound. A refusal's message repeats at most 40 characters of a value from the file, without control or direction characters.

After a file is read, planning and applying are checked again with the app's own rules: answers to inputs are checked in Rust on the importing system, every pad of an installed provider goes through the same check as a pad saved in the editor, context rules through the Context Engine's own check, and, before the new workspace is saved, every control of every surface the import adds or replaces through the editor's control check, and every surface id for uniqueness.

## Actions that run things

Three kinds of pad run something on the computer or the network: a **command** (`shell.execute`), a **request** (`http.request`, and any other action of LaunchDeck itself marked privileged), and a **launch** of a program (`system.launchApplication`, every one). A fourth, an **instruction to an agent** (`agent-control.newTask`, `agent-control.continue`), is shown too. The import lists each of them with exactly what it runs, and the person must choose to **keep** or **remove** the first three kinds; remove is proposed. Removing empties the slot and keeps the control's label, looks and feedback.

What the person reads is what would run. Each point below closes a way a file could show one thing and run another (the SF numbers are the security findings of the milestone that led to them):

* **Read with the runtime's own parsers.** The review reads a command, a request, a launch and an agent's instruction with the very functions the runtime runs them with, after answers are put in place and surface ids renamed.
* **Launches (SF-1).** Every launch is reviewed, on every system, a plain application name without arguments too: on Linux the name is looked up on `PATH`, and on Windows `cmd /C start` also finds programs on `PATH`, so a name such as `reboot` or `logoff` runs a program, not an application (how Linux and Windows find a name is traced from the code, not run there: NOT VERIFIED). The review shows the program and each argument as the runtime passes them, also for a launch the runtime would refuse. The runtime refuses NUL and line breaks in a launch, an application name starting with `-`, and on Windows the characters its command interpreter reads and an application name starting with `/`, which `start` would read as its own option (Windows: written and tested by passing the platform to the parser, not run on Windows, NOT VERIFIED).
* **Hidden characters (SF-3).** In every review text, each character a person cannot see (control characters other than a line feed or a tab, line and paragraph separators, every Unicode default-ignorable code point, which includes zero-width characters and the direction controls) is shown as an escape; a one-line text escapes line feeds and tabs too. A run of more than two blank lines in a command, a request body or an agent's instruction is shown as one line that counts them (`[1899 blank lines]`), and the import view gives each text's number of lines, so line breaks cannot push what a pad runs out of view below its first line. Beyond that, a file is refused outright (`invalidContent`, pointing at the argument) when any argument of a command, request, launch or agent instruction holds such a character, so the review never has to show an escape where the action would run the character.
* **Request addresses (SF-4).** A request's address is read once, by the function the runtime sends with, and the review states where it goes: host and port, and "on this computer" or "on your local network" when the host says so (`localhost`, an address of this computer or of a local network, a name without a dot or with a local suffix such as `.local`). A file is refused when an address in it is not `http` or `https`, carries a user name or password, holds a backslash, a blank or a hidden character, is read differently by the HTTP client and by a browser's parser (such as a host in another script or a numeric spelling), or names a host no request may go to (unspecified, broadcast, multicast). An empty address sends nothing and is allowed; an address asked of the importer is checked the same way once it is answered, and a wrong answer is a problem of that input.
* **No redirects (SF-5).** The built-in request follows no redirect, so it cannot end at a host no one reviewed; a redirect is a failure that names the location.
* **Where a name resolves, checked when sending.** The review labels a destination by its name, before anything is looked up. When the pad is pressed, the built-in request looks the name up through the plugin broker's own check and connects only to the addresses checked: every address must be where the name says it is. A public name that resolves to this computer or to the local network (DNS rebinding, or a name such as `localtest.me` or `192-168-1-1.nip.io`) is not sent to, with a sentence that says where it resolved; so is `localhost` resolving to the local network, or a local name resolving to the internet. A device at home is reached by its local address (an IP address, `localhost`, a name without a dot or ending in `.local`), not by a public name pointed at it. The request goes straight to its destination, through no proxy.
* **Exact ids.** A case variant such as `Shell.execute` is not reviewed because it can never run: every action passes a gate that knows ids exactly, and refuses one it does not have.
* **Plugin actions** are never review items: a plugin cannot be privileged, and its network access stays behind the grants the person gives on the Plugins screen, which an import never changes.

## Nothing runs during an import

Reading a file, planning it, previewing it with other decisions and applying it call no action, run no test, install nothing, rescan no folder, download nothing and change no grant. Planning asks the runtime only for its checks of a pad's arguments and of a state rule's address; no action of any plugin is called. Applying writes the workspace and nothing else.

* Context rules are opt-in: none is added unless the person selects it, and a local rule for the same application always wins. Auto Context is never turned on or off and its fallback never changes.
* A file cannot carry grants, plugin settings or secrets, so an import cannot give a plugin network access or a setting.
* The log records, for an applied import, only how many surfaces and rules were added or replaced, and for a failed save the system's reason; never the file's content.

## Missing plugins are never installed automatically

A Pack's dependencies are only ids and version requirements. On import each gets a status (compatible, disabled, incompatible, missing) and a sentence such as "Some pads will not work until you install …". LaunchDeck never downloads, installs or turns on a plugin because of a file, and a missing plugin never blocks an import: its pads stay unavailable until the person installs the plugin themselves, for example one of the bundled examples from the Plugins screen. An installed plugin then asks for network access as any plugin does; the exporter's grants are never in the file.

## What export never sends

Export reads the workspace and what the app knows about its providers and devices; it never reads a secret, a grant, a plugin's settings, an agent session or a runtime value, and it leaves out preferences, Auto Context's settings, rules of surfaces not exported, legacy data, device ids and times (the full list is in [FORMAT.md](FORMAT.md#export)).

* **Bound arguments (SF-2).** An argument the person chose to ask the importer for is written as the input's default, or empty: its value on the exporter's computer never leaves it, and the preview lists it without its value.
* **Credentials.** A value shaped like a credential (an address with a user name or password, a token-like parameter in an address's query or fragment, an argument or a JSON member named like a password, token, secret or API key, an authorization, cookie or API-key header, a Home Assistant webhook id, the secret part of a Slack, Discord, Zapier, IFTTT or Telegram address) blocks the file unless the argument is asked of the importer. It is named, never repeated. The exact list is in [FORMAT.md](FORMAT.md).
* **Addresses read two ways (SF-2b).** Every address is read as the HTTP client that sends requests reads it and as a browser does, and the credential shapes are looked for in each reading: a browser shortens `/x/../..` in a path, for example, where the client keeps the secret part of a webhook address. An address only one of them reads, or that they read with different hosts (a percent or a letter outside ASCII in the host, a backslash, a numeric host), blocks the file on its own sentence, because what it carries cannot be told; a `@` before the host's end counts as a user name even when neither reads the address.
* **Pads that run things** are listed before sharing, each with the warning to check it holds no password or token. The check above catches known shapes only, in request bodies and commands too (JSON, `name=value` pairs, headers, addresses of any scheme); a password written in another form (`curl -u user:pw`, prose) would still be exported, which is why the warning is there.
* **The file.** It is written into Downloads under a new name, atomically, and never replaces an existing file; a name whose temporary file already exists, as a file or a link, is skipped, so the write never goes through a link. The webview never sends a path.

## Integrity is not trust

`integrity.content` proves that the content is the one the manifest was written with: a file damaged or edited by hand without updating its digest is refused. It does not say who made the file. Anyone can change a file and compute a new digest, and a manifest's `author` is just text. There is no publisher identity in format version 1: the publisher of a file is never verified, and a file carrying a `signatures` member is refused rather than half-trusted. Import files from people you trust, and read what the review shows.

## Where the tests are

`src-tauri/tests/artifacts_security.rs` is the hostile suite, a release blocker. Every case goes through the artifact service of a runtime on a real store in a temporary folder, and asserts the rejection code and pointer, that `workspace.json`, `plugins.json` and `permissions.json` keep their bytes, that the workspace in memory is unchanged, and that nothing ran: a recording keyboard, a recording `shell` provider and a recording plugin (`acme.recorder`) see no call. The small cases are files in `src-tauri/tests/fixtures/artifacts/hostile/` (`.gitattributes` keeps their bytes); large or deep cases are built in code, and most others change one thing in the golden files of `src-tauri/tests/fixtures/artifacts/valid/` and seal them again.

| Group | Cases |
| --- | --- |
| Bytes | empty; 2 MiB + 1 byte; truncated; garbage; byte order mark; invalid UTF-8; ZIP header bytes named `.ldpack`; top-level array |
| Envelope | no `format`; `launchdeck.workspace`; format version 2, 0 and `"1"`; unknown top-level member; `signatures`; `assets` (SVG with a script, base64 executable, `../../x`); a raw `workspace.json` |
| JSON | duplicate `content`; duplicate key in a control; duplicate look through a `\u0061` escape; 10 000 nested arrays in arguments; a 70 KiB string; `1e400` |
| Integrity | a one-character edit; no integrity; uppercase hex; `md5:`; and, accepted on purpose, a whitespace-only reformat and CRLF line ends |
| Manifest | ids `../evil`, `Acme.Pack`, `single`, `/abs`, `C:\x`; versions `1.0`, `01.0.0`, `1.0.0+build`; empty and 61-character names; a direction override in a label; a control character in the description; author addresses `javascript:`, `http://`, `https://u:p@example.com` |
| Dependencies | invalid plugin id; `>>1`; a pre-release comparator; a duplicate; 33; the host provider `system`; an undeclared plugin |
| Limits and geometry | 17 surfaces; 33 pages; 129 controls on a page; 2049 controls; 33 looks; 33 feedback rules; 65 rules; 9 identities; 100 000 surface references in one press; column 16; span 9; a logo control; overlapping controls; duplicate surface, page and control ids |
| References | a rule to a surface not in the Pack; a rule page not in its surface; `surfaceRefs` to a missing argument, a non-text or an unknown surface; a binding to an undeclared input; an application input bound to an argument; a text input bound to a rule; `show` naming a missing look |
| Feedback | a 10 ms hold; 9 conditions; both `show` and `showFromValue`; an invalid state provider id |
| Assignments | `args` not an object; an extra member; arguments 5 deep; 33 KiB of arguments |
| Inputs | type `secret`; enum without options; a default that is not an option; a duplicate id; 33 inputs |
| Kind and requirements | requirements understated; a Surface file with rules, with inputs, with two surfaces |
| Kind and name | a Pack named `.ldsurface` is a Pack, with a warning; a hostile file name is shown cleaned and cut; an unknown category is a warning |
| What runs (SF-1, SF-3, SF-4) | launches that must be reviewed (`review-*`: blank or non-text arguments, command-interpreter characters, paths, a tilde, a dot, slash look-alikes, line breaks); case variants that never run (`never-runs-*`); hidden characters in a command, a launch, an agent instruction and a request address (`hidden-*`); request addresses with a user name, a backslash, a numeric or non-ASCII host (`request-*`); addresses the runtime would not send |
| Apply stage | a stale digest after a workspace change; the `runs` decision missing; a required input missing; invalid answers (a relative path, an `ftp:` address); replacing an edited import without confirmation; a full workspace; a failed save (a folder in the way of `workspace.json.tmp`), after which the import is still pending and goes through once the save can; an expired, discarded or replaced plan id |
| Positive controls | a valid Pack with a command and `runs: remove` imports with the slot emptied and runs nothing; with `runs: keep` the pad is imported as written, runs nothing during the import, and runs its command once when pressed afterwards, so "nothing ran" is not a blind check |

Other tests: the pre-pass, the digest and the canonical writer in `src-tauri/src/artifacts/json.rs`; every format rule in `src-tauri/src/artifacts/format/tests.rs`; the review against the runtime's own readers in `src-tauri/src/artifacts/review_tests.rs`; what export never writes, credentials and bound arguments in `src-tauri/src/artifacts/export/tests.rs`; the plan, apply and service beside their code; the contract fixtures in `src-tauri/tests/artifacts_contract.rs`; and a full export, reset, import without the plugin, install and restart in `src-tauri/tests/artifacts_roundtrip.rs`.

Status: these tests ran on macOS only, in this milestone. CI: NOT VERIFIED (GitHub Actions blocked by account billing). Linux and Windows: NOT VERIFIED (no such system here).

## What is not claimed

* **Only some pads are reviewed.** A pad that opens a web page or a `mailto:` address, presses a key combination or a media key, or switches surfaces is imported as the file has it, and does that when pressed. A command kept on purpose runs with the person's rights when pressed.
* **Answers to an agent are not reviewed.** A pad that approves, rejects or retries an agent's session (`agent-control.approve`, `reject`, `retry`) is imported as the file has it, and is not listed for review. With the Claude Code and Codex hooks adapters, approve answers the focused session's pending permission request with "allow", and the agent then runs the tool call it asked for, a shell command for example. Only `newTask` and `continue`, which give an agent an instruction, are shown.
* **Labels can lie.** A pad labelled "Build" may run anything, and one labelled "Volume up" may approve an agent's request; the review shows the real command, request or launch, not the label.
* **A dependency is only an id.** If the person installs a plugin with the declared id from someone else, the pads use that plugin.
* **Integrity is not authenticity**, as above.
* **The app window.** The import and export screens, the file picker and the Downloads prompt have not been used in the app's window by a person: NOT VERIFIED.
