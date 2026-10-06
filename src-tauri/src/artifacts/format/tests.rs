//! Accept and reject per rule, on whole files read in the inspect order;
//! conversion of the starter surfaces; the frozen feedback rule.

use serde_json::{json, Value};

use super::*;
use crate::artifacts::{content_digest, to_canonical};
use crate::platform::Platform;
use crate::state::feedback::FeedbackRule;
use crate::surfaces::default_workspace_for;

use RejectionCode as R;

/// A whole file read as an importer reads it: every check of the format
/// in the inspect order (plan 7.3).
fn read(raw: &str) -> Result<ArtifactV1, Rejection> {
    crate::artifacts::testkit::read(raw)
}

/// Set the digest the content has.
fn seal(v: &mut Value) {
    v["manifest"]["integrity"]["content"] = json!(content_digest(&to_canonical(&v["content"])));
}

fn pack() -> Value {
    let mut v = json!({
        "format": "launchdeck.pack",
        "formatVersion": 1,
        "manifest": {
            "id": "acme.video-editing",
            "name": "Video editing",
            "version": "1.2.0-beta.1",
            "description": "Cut, mark and export.\nFrom the pads.",
            "author": { "name": "Acme", "url": "https://example.com/acme" },
            "license": "MIT",
            "createdWith": { "app": "launchdeck", "version": "0.3.0" },
            "requirements": {
                "grid": { "minWidth": 8, "minHeight": 8 },
                "aux": { "top": 1, "side": 1 },
                "rgb": true
            },
            "testedWith": [ { "model": "launchpad-x", "kind": "hardware" } ],
            "dependencies": [
                { "pluginId": "acme.notes", "name": "Notes", "version": "^1.2", "required": true },
                { "pluginId": "acme.timer", "version": "*", "required": false }
            ],
            "inputs": [
                { "id": "project", "type": "path", "label": "Project folder" },
                { "id": "editor", "type": "application", "label": "Video editor", "required": false },
                { "id": "quality", "type": "enum", "label": "Quality",
                  "options": [ { "value": "high", "label": "High" }, { "value": "low", "label": "Low" } ],
                  "default": { "kind": "text", "value": "high" } },
                { "id": "note", "type": "string", "label": "Note", "maxLength": 80 },
                { "id": "site", "type": "url", "label": "Site",
                  "default": { "kind": "text", "value": "https://example.com/docs" } }
            ],
            "integrity": { "content": "" }
        },
        "content": {
            "surfaces": [
                { "id": "editing", "name": "Editing", "icon": "✂", "category": "video", "pages": [
                    { "id": "home", "name": "Home", "controls": [
                        { "id": "g.0.0", "pos": { "kind": "grid", "col": 0, "row": 0 }, "label": "Cut",
                          "visual": { "color": "#ff0000" },
                          "looks": { "busy": { "color": "#00ff00", "animation": "pulse" } },
                          "press": { "provider": "acme.notes", "action": "mark",
                                     "args": { "folder": "/tmp/x", "count": 2 },
                                     "inputs": { "folder": "project" } },
                          "feedback": [
                              { "when": { "state": { "provider": "acme.timer", "id": "running" },
                                          "is": { "op": "equals", "value": true } },
                                "show": "busy" }
                          ] },
                        { "id": "g.6.7", "pos": { "kind": "grid", "col": 6, "row": 7 },
                          "span": { "w": 2, "h": 1 }, "label": "Next", "visual": { "color": "#808080" },
                          "press": { "provider": "navigation", "action": "openSurface",
                                     "args": { "surface": "color" }, "surfaceRefs": [ "surface" ] } },
                        { "id": "t.0", "pos": { "kind": "top", "index": 0 }, "label": "Up",
                          "visual": { "color": "#202020" },
                          "feedback": [
                              { "when": { "event": { "provider": "ci", "id": "build.finished" },
                                          "where": [ { "field": "result", "equals": "failed" } ] },
                                "show": "idle", "hold": { "ms": 4000 } }
                          ] }
                    ] },
                    { "id": "two", "name": "Two", "controls": [] }
                ] },
                { "id": "color", "name": "Colour", "category": "video", "pages": [
                    { "id": "home", "name": "Home", "controls": [
                        { "id": "s.0", "pos": { "kind": "side", "index": 0 }, "label": "Back",
                          "visual": { "color": "#000000" },
                          "longPress": { "provider": "navigation", "action": "openSurface",
                                         "args": { "surface": "editing" }, "surfaceRefs": [ "surface" ] } }
                    ] }
                ] }
            ],
            "contextRules": [
                { "application": { "name": "Resolve",
                                   "identities": [ { "namespace": "macos", "id": "com.blackmagic-design.DaVinciResolve" } ] },
                  "surface": "editing", "page": "two" },
                { "application": { "input": "editor" }, "surface": "color" }
            ]
        }
    });
    seal(&mut v);
    v
}

fn surface_file() -> Value {
    let mut v = json!({
        "format": "launchdeck.surface",
        "formatVersion": 1,
        "manifest": {
            "id": "local.solo",
            "name": "Solo",
            "version": "1.0.0",
            "createdWith": { "app": "launchdeck", "version": "0.3.0" },
            "requirements": { "grid": { "minWidth": 1, "minHeight": 1 }, "aux": { "top": 0, "side": 0 }, "rgb": false },
            "integrity": { "content": "" }
        },
        "content": {
            "surfaces": [
                { "id": "solo", "name": "Solo", "category": "custom", "pages": [
                    { "id": "home", "name": "Home", "controls": [
                        { "id": "g.0.0", "pos": { "kind": "grid", "col": 0, "row": 0 }, "label": "Open",
                          "visual": { "color": "#ffffff" },
                          "press": { "provider": "system", "action": "openUrl",
                                     "args": { "url": "https://example.com" } } }
                    ] }
                ] }
            ]
        }
    });
    seal(&mut v);
    v
}

/// Change the pack, seal it again, read it: the code and the place.
fn refused(mutate: impl FnOnce(&mut Value)) -> (RejectionCode, Option<String>) {
    let mut v = pack();
    mutate(&mut v);
    seal(&mut v);
    let r = read(&v.to_string()).expect_err("should be refused");
    (r.code, r.at)
}

fn at(code: RejectionCode, at: &str) -> (RejectionCode, Option<String>) {
    (code, Some(at.to_string()))
}

const CONTROL: &str = "/content/surfaces/0/pages/0/controls/0";

fn control(v: &mut Value) -> &mut Value {
    &mut v["content"]["surfaces"][0]["pages"][0]["controls"][0]
}

#[test]
fn the_fixtures_are_valid() {
    let pack = read(&pack().to_string()).unwrap();
    assert_eq!(pack.kind, ArtifactKind::Pack);
    assert_eq!(pack.content.surfaces.len(), 2);
    let surface = read(&surface_file().to_string()).unwrap();
    assert_eq!(surface.kind, ArtifactKind::Surface);
}

#[test]
fn a_whitespace_only_reformat_keeps_the_digest() {
    let v = pack();
    let pretty = serde_json::to_string_pretty(&v)
        .unwrap()
        .replace('\n', "\r\n");
    assert!(read(&pretty).is_ok());
    // One character of the content changed, the digest not: refused.
    let edited = v
        .to_string()
        .replacen("\"label\":\"Cut\"", "\"label\":\"Cup\"", 1);
    assert_eq!(read(&edited).unwrap_err().code, R::Integrity);
}

#[test]
fn the_manifest_id_name_and_versions() {
    for id in [
        "../evil",
        "Acme.Pack",
        "single",
        "/abs",
        "C:\\x",
        "launchdeck.pack",
    ] {
        assert_eq!(
            refused(|v| v["manifest"]["id"] = json!(id)),
            at(R::InvalidManifest, "/manifest/id"),
            "{id}"
        );
    }
    for version in ["1.0", "01.0.0", "1.0.0+build", "v1.0.0", ""] {
        assert_eq!(
            refused(|v| v["manifest"]["version"] = json!(version)),
            at(R::InvalidManifest, "/manifest/version"),
            "{version}"
        );
    }
    for name in [String::new(), "x".repeat(61), " Padded".into()] {
        assert_eq!(
            refused(|v| v["manifest"]["name"] = json!(name)),
            at(R::InvalidManifest, "/manifest/name")
        );
    }
    assert_eq!(
        refused(|v| v["manifest"]["description"] = json!("bell \u{7}")),
        at(R::InvalidManifest, "/manifest/description")
    );
    assert_eq!(
        refused(|v| v["manifest"]["license"] = json!("x".repeat(65))),
        at(R::InvalidManifest, "/manifest/license")
    );
    assert_eq!(
        refused(|v| v["manifest"]["createdWith"]["app"] = json!("other")),
        at(R::InvalidManifest, "/manifest/createdWith/app")
    );
    assert_eq!(
        refused(|v| v["manifest"]["unknown"] = json!(1)),
        at(R::InvalidManifest, "/manifest")
    );
}

#[test]
fn the_author_address_is_https_without_credentials() {
    for url in [
        "javascript:alert(1)",
        "http://example.com",
        "https://u:p@example.com",
        "https://u@example.com",
        "https://",
        &format!("https://example.com/{}", "x".repeat(240)),
    ] {
        assert_eq!(
            refused(|v| v["manifest"]["author"]["url"] = json!(url)),
            at(R::InvalidManifest, "/manifest/author/url"),
            "{url}"
        );
    }
    assert_eq!(
        refused(|v| v["manifest"]["author"]["name"] = json!("")),
        at(R::InvalidManifest, "/manifest/author/name")
    );
}

#[test]
fn the_integrity_is_a_lowercase_sha256() {
    let mut v = pack();
    let digest = v["manifest"]["integrity"]["content"]
        .as_str()
        .unwrap()
        .to_string();
    for bad in [
        digest.to_uppercase(),
        digest.replace("sha256:", "md5:"),
        digest[..digest.len() - 1].to_string(),
    ] {
        v["manifest"]["integrity"]["content"] = json!(bad);
        assert_eq!(
            read(&v.to_string()).unwrap_err().at.as_deref(),
            Some("/manifest/integrity/content")
        );
    }
    v["manifest"].as_object_mut().unwrap().remove("integrity");
    assert_eq!(read(&v.to_string()).unwrap_err().code, R::InvalidManifest);
}

#[test]
fn dependencies() {
    let dep =
        |id: &str, version: &str| json!({ "pluginId": id, "version": version, "required": true });
    for (deps, expected) in [
        (vec![dep("Acme", "*")], "/manifest/dependencies/0/pluginId"),
        (
            vec![dep("system", "*")],
            "/manifest/dependencies/0/pluginId",
        ),
        (
            vec![dep("acme.notes", ">>1")],
            "/manifest/dependencies/0/version",
        ),
        (
            vec![dep("acme.notes", "^1.0.0-beta")],
            "/manifest/dependencies/0/version",
        ),
        (
            vec![dep("acme.notes", ">=1, <2, !=1.5, >0.1, <3")],
            "/manifest/dependencies/0/version",
        ),
        (
            vec![dep("acme.notes", &format!(">={}", "1".repeat(64)))],
            "/manifest/dependencies/0/version",
        ),
        (
            vec![dep("acme.notes", "*"), dep("acme.notes", "*")],
            "/manifest/dependencies/1/pluginId",
        ),
        (
            vec![dep("acme.timer", "*"), dep("acme.notes", "*")],
            "/manifest/dependencies/1/pluginId",
        ),
    ] {
        assert_eq!(
            refused(|v| v["manifest"]["dependencies"] = json!(deps)),
            at(R::InvalidManifest, expected),
            "{deps:?}"
        );
    }
    let many: Vec<Value> = (0..33)
        .map(|i| dep(&format!("acme.d{i:02}"), "*"))
        .collect();
    assert_eq!(
        refused(|v| v["manifest"]["dependencies"] = json!(many)),
        at(R::TooMany, "/manifest/dependencies")
    );
    // Accepted requirements, Cargo semantics.
    for version in ["*", "1.2.3", "^0.4", "~1", ">=1.0, <2", "=1.2.3"] {
        let mut v = pack();
        v["manifest"]["dependencies"][0]["version"] = json!(version);
        seal(&mut v);
        assert!(read(&v.to_string()).is_ok(), "{version}");
    }
}

#[test]
fn inputs() {
    let inputs = "/manifest/inputs";
    assert_eq!(
        refused(|v| v["manifest"]["inputs"][0]["type"] = json!("secret")),
        at(R::InvalidManifest, "/manifest")
    );
    assert_eq!(
        refused(|v| v["manifest"]["inputs"][2]["options"] = json!([])),
        at(R::InvalidManifest, &format!("{inputs}/2/options"))
    );
    assert_eq!(
        refused(|v| v["manifest"]["inputs"][0]["options"] = json!([{ "value": "a", "label": "A" }])),
        at(R::InvalidManifest, &format!("{inputs}/0/options"))
    );
    assert_eq!(
        refused(|v| v["manifest"]["inputs"][2]["default"]["value"] = json!("medium")),
        at(R::InvalidManifest, &format!("{inputs}/2/default"))
    );
    assert_eq!(
        refused(|v| v["manifest"]["inputs"][2]["options"][1]["value"] = json!("high")),
        at(R::InvalidManifest, &format!("{inputs}/2/options/1/value"))
    );
    assert_eq!(
        refused(|v| v["manifest"]["inputs"][1]["id"] = json!("project")),
        at(R::InvalidManifest, &format!("{inputs}/1/id"))
    );
    assert_eq!(
        refused(|v| v["manifest"]["inputs"][1]["id"] = json!("Editor")),
        at(R::InvalidManifest, &format!("{inputs}/1/id"))
    );
    assert_eq!(
        refused(|v| v["manifest"]["inputs"][3]["maxLength"] = json!(2049)),
        at(R::InvalidManifest, &format!("{inputs}/3/maxLength"))
    );
    assert_eq!(
        refused(|v| v["manifest"]["inputs"][0]["maxLength"] = json!(10)),
        at(R::InvalidManifest, &format!("{inputs}/0/maxLength"))
    );
    assert_eq!(
        refused(|v| v["manifest"]["inputs"][4]["default"]["value"] = json!("ftp://example.com")),
        at(R::InvalidManifest, &format!("{inputs}/4/default"))
    );
    assert_eq!(
        refused(|v| v["manifest"]["inputs"][1]["default"] = json!({ "kind": "text", "value": "x" })),
        at(R::InvalidManifest, &format!("{inputs}/1/default"))
    );
    assert_eq!(
        refused(|v| v["manifest"]["inputs"][3]["default"] =
            json!({ "kind": "text", "value": "x".repeat(81) })),
        at(R::InvalidManifest, &format!("{inputs}/3/default"))
    );
    assert_eq!(
        refused(|v| v["manifest"]["inputs"][0]["label"] = json!("Folder\u{202E}")),
        at(R::InvalidManifest, &format!("{inputs}/0/label"))
    );
    let many: Vec<Value> = (0..33)
        .map(|i| json!({ "id": format!("i{i}"), "type": "string", "label": "L" }))
        .collect();
    assert_eq!(
        refused(|v| v["manifest"]["inputs"] = json!(many)),
        at(R::TooMany, inputs)
    );
    // An application default, and an input that is not required.
    let mut v = pack();
    v["manifest"]["inputs"][1]["default"] = json!({
        "kind": "application", "namespace": "macos", "id": "com.example.Editor", "displayName": "Editor"
    });
    seal(&mut v);
    assert!(read(&v.to_string()).is_ok());
}

#[test]
fn requirements_and_tested_devices() {
    let req = "/manifest/requirements";
    assert_eq!(
        refused(|v| v["manifest"]["requirements"]["grid"]["minWidth"] = json!(0)),
        at(R::InvalidManifest, &format!("{req}/grid"))
    );
    assert_eq!(
        refused(|v| v["manifest"]["requirements"]["grid"]["minHeight"] = json!(17)),
        at(R::InvalidManifest, &format!("{req}/grid"))
    );
    assert_eq!(
        refused(|v| v["manifest"]["requirements"]["aux"]["side"] = json!(17)),
        at(R::InvalidManifest, &format!("{req}/aux"))
    );
    // Understated: weaker than the content needs.
    assert_eq!(
        refused(|v| v["manifest"]["requirements"]["grid"]["minWidth"] = json!(7)),
        at(R::InvalidManifest, &format!("{req}/grid"))
    );
    assert_eq!(
        refused(|v| {
            v["manifest"]["requirements"]
                .as_object_mut()
                .unwrap()
                .remove("grid");
        }),
        at(R::InvalidManifest, &format!("{req}/grid"))
    );
    assert_eq!(
        refused(|v| v["manifest"]["requirements"]["aux"]["top"] = json!(0)),
        at(R::InvalidManifest, &format!("{req}/aux"))
    );
    assert_eq!(
        refused(|v| v["manifest"]["requirements"]["rgb"] = json!(false)),
        at(R::InvalidManifest, &format!("{req}/rgb"))
    );
    // Stronger than needed is fine.
    let mut v = pack();
    v["manifest"]["requirements"] = json!({ "grid": { "minWidth": 16, "minHeight": 9 }, "aux": { "top": 8, "side": 8 }, "rgb": true });
    seal(&mut v);
    assert!(read(&v.to_string()).is_ok());
    let tested: Vec<Value> = (0..9)
        .map(|_| json!({ "model": "launchpad-x", "kind": "virtual" }))
        .collect();
    assert_eq!(
        refused(|v| v["manifest"]["testedWith"] = json!(tested)),
        at(R::TooMany, "/manifest/testedWith")
    );
    assert_eq!(
        refused(|v| v["manifest"]["testedWith"][0]["model"] = json!("Launchpad X")),
        at(R::InvalidManifest, "/manifest/testedWith/0/model")
    );
}

#[test]
fn computed_requirements() {
    let pack = read(&pack().to_string()).unwrap();
    assert_eq!(
        compute_requirements(&pack.content),
        Requirements {
            grid: Some(GridRequirement {
                min_width: 8,
                min_height: 8
            }),
            aux: AuxRequirement { top: 1, side: 1 },
            rgb: true,
        }
    );
    let surface = read(&surface_file().to_string()).unwrap();
    let needed = compute_requirements(&surface.content);
    assert_eq!(needed, surface.manifest.requirements);
    assert!(!needed.rgb, "white is a grey");
    // No grid control, no grid requirement; a grey look needs no colour.
    let mut content = surface.content.clone();
    let c = &mut content.surfaces[0].pages[0].controls[0];
    c.pos = Pos::Side { index: 3 };
    c.looks.insert(
        "on".into(),
        VisualV1 {
            color: Color(crate::core::model::Rgb::new(9, 9, 9)),
            animation: AnimationV1::Flash,
        },
    );
    assert_eq!(
        compute_requirements(&content),
        Requirements {
            grid: None,
            aux: AuxRequirement { top: 0, side: 4 },
            rgb: false,
        }
    );
}

#[test]
fn counts_over_a_limit_are_too_many() {
    let surfaces = "/content/surfaces";
    assert_eq!(
        refused(|v| {
            let one = v["content"]["surfaces"][1].clone();
            let more: Vec<Value> = (0..17)
                .map(|i| {
                    let mut s = one.clone();
                    s["id"] = json!(format!("s{i}"));
                    s
                })
                .collect();
            v["content"]["surfaces"] = json!(more);
        }),
        at(R::TooMany, surfaces)
    );
    assert_eq!(
        refused(|v| v["content"]["surfaces"] = json!([])),
        at(R::InvalidContent, surfaces)
    );
    let page = pack()["content"]["surfaces"][1]["pages"][0].clone();
    assert_eq!(
        refused(|v| v["content"]["surfaces"][1]["pages"] = json!(vec![page.clone(); 33])),
        at(R::TooMany, "/content/surfaces/1/pages")
    );
    assert_eq!(
        refused(|v| v["content"]["surfaces"][1]["pages"] = json!([])),
        at(R::InvalidContent, "/content/surfaces/1/pages")
    );
    let one = pack()["content"]["surfaces"][1]["pages"][0]["controls"][0].clone();
    assert_eq!(
        refused(
            |v| v["content"]["surfaces"][1]["pages"][0]["controls"] = json!(vec![one.clone(); 129])
        ),
        at(R::TooMany, "/content/surfaces/1/pages/0/controls")
    );
    // 2049 controls in one file, 121 on each of 17 pages.
    assert_eq!(
        refused(|v| {
            let mut full = page.clone();
            full["controls"] = json!(vec![one.clone(); 121]);
            v["content"]["surfaces"][1]["pages"] = json!(vec![full; 17]);
        }),
        at(R::TooMany, surfaces)
    );
    let look = json!({ "color": "#ffffff" });
    let looks: serde_json::Map<String, Value> =
        (0..33).map(|i| (format!("l{i}"), look.clone())).collect();
    assert_eq!(
        refused(|v| control(v)["looks"] = Value::Object(looks)),
        at(R::TooMany, &format!("{CONTROL}/looks"))
    );
    let rule = pack()["content"]["surfaces"][0]["pages"][0]["controls"][0]["feedback"][0].clone();
    assert_eq!(
        refused(|v| control(v)["feedback"] = json!(vec![rule; 33])),
        at(R::TooMany, &format!("{CONTROL}/feedback"))
    );
    let context = pack()["content"]["contextRules"][0].clone();
    assert_eq!(
        refused(|v| v["content"]["contextRules"] = json!(vec![context; 65])),
        at(R::TooMany, "/content/contextRules")
    );
    let identities: Vec<Value> = (0..9)
        .map(|i| json!({ "namespace": format!("ns{i}"), "id": "app" }))
        .collect();
    assert_eq!(
        refused(
            |v| v["content"]["contextRules"][0]["application"]["identities"] = json!(identities)
        ),
        at(R::TooMany, "/content/contextRules/0/application/identities")
    );
    let args: serde_json::Map<String, Value> =
        (0..17).map(|i| (format!("a{i}"), json!(1))).collect();
    assert_eq!(
        refused(|v| control(v)["press"]["args"] = Value::Object(args)),
        at(R::TooMany, &format!("{CONTROL}/press/args"))
    );
    // Each reference names a distinct argument, so there are no more of
    // them than arguments; counted before they are compared.
    let refs: Vec<String> = (0..17).map(|i| format!("a{i}")).collect();
    assert_eq!(
        refused(|v| control(v)["press"]["surfaceRefs"] = json!(refs)),
        at(R::TooMany, &format!("{CONTROL}/press/surfaceRefs"))
    );
}

#[test]
fn geometry_is_the_editors() {
    let grid = |col: u8, row: u8| json!({ "kind": "grid", "col": col, "row": row });
    assert_eq!(
        refused(|v| control(v)["pos"] = grid(16, 0)),
        at(R::InvalidContent, CONTROL)
    );
    assert_eq!(
        refused(|v| control(v)["span"] = json!({ "w": 9, "h": 1 })),
        at(R::InvalidContent, &format!("{CONTROL}/span"))
    );
    assert_eq!(
        refused(|v| control(v)["span"] = json!({ "w": 0, "h": 1 })),
        at(R::InvalidContent, &format!("{CONTROL}/span"))
    );
    // Reaching past the 16 by 16 grid.
    let (code, place) = refused(|v| {
        control(v)["pos"] = grid(12, 0);
        control(v)["span"] = json!({ "w": 5, "h": 1 });
        v["manifest"]["requirements"]["grid"]["minWidth"] = json!(16);
    });
    assert_eq!((code, place.as_deref()), (R::InvalidContent, Some(CONTROL)));
    assert_eq!(
        refused(|v| {
            let c = &mut v["content"]["surfaces"][0]["pages"][0]["controls"][2];
            c["span"] = json!({ "w": 1, "h": 1 });
        }),
        at(
            R::InvalidContent,
            "/content/surfaces/0/pages/0/controls/2/span"
        )
    );
    assert_eq!(
        refused(|v| control(v)["pos"] = json!({ "kind": "logo" })),
        at(R::InvalidContent, "/content")
    );
    // Overlap: the second control is moved onto the first.
    assert_eq!(
        refused(|v| v["content"]["surfaces"][0]["pages"][0]["controls"][1]["pos"] = grid(0, 0)),
        at(R::InvalidContent, "/content/surfaces/0/pages/0/controls/0")
    );
}

#[test]
fn ids_text_and_colours() {
    let page = "/content/surfaces/0/pages/0";
    assert_eq!(
        refused(|v| v["content"]["surfaces"][1]["id"] = json!("editing")),
        at(R::InvalidContent, "/content/surfaces/1/id")
    );
    assert_eq!(
        refused(|v| v["content"]["surfaces"][0]["pages"][1]["id"] = json!("home")),
        at(R::InvalidContent, "/content/surfaces/0/pages/1/id")
    );
    assert_eq!(
        refused(|v| v["content"]["surfaces"][0]["pages"][0]["controls"][1]["id"] = json!("g.0.0")),
        at(R::InvalidContent, &format!("{page}/controls/1/id"))
    );
    assert_eq!(
        refused(|v| v["content"]["surfaces"][0]["id"] = json!("Editing")),
        at(R::InvalidContent, "/content/surfaces/0/id")
    );
    assert_eq!(
        refused(|v| control(v)["id"] = json!("a b")),
        at(R::InvalidContent, &format!("{CONTROL}/id"))
    );
    assert_eq!(
        refused(|v| control(v)["label"] = json!("Cut\u{202E}")),
        at(R::InvalidContent, &format!("{CONTROL}/label"))
    );
    assert_eq!(
        refused(|v| control(v)["label"] = json!("x".repeat(41))),
        at(R::InvalidContent, &format!("{CONTROL}/label"))
    );
    assert_eq!(
        refused(|v| control(v)["icon"] = json!("123456789")),
        at(R::InvalidContent, &format!("{CONTROL}/icon"))
    );
    assert_eq!(
        refused(|v| v["content"]["surfaces"][0]["category"] = json!("Video")),
        at(R::InvalidContent, "/content/surfaces/0/category")
    );
    assert_eq!(
        refused(|v| control(v)["looks"] = json!({ " busy": { "color": "#000000" } })),
        at(R::InvalidContent, &format!("{CONTROL}/looks/ busy"))
    );
    for colour in ["#FF0000", "#ff00", "ff0000", "#+fffff", "red"] {
        assert_eq!(
            refused(|v| control(v)["visual"]["color"] = json!(colour)),
            at(R::InvalidContent, "/content"),
            "{colour}"
        );
    }
    // A description may break lines; a label may not.
    let mut v = pack();
    control(&mut v)["description"] = json!("Line one\nline two");
    seal(&mut v);
    assert!(read(&v.to_string()).is_ok());
    assert_eq!(
        refused(|v| control(v)["unknown"] = json!(1)),
        at(R::InvalidContent, "/content")
    );
}

#[test]
fn assignments() {
    let press = format!("{CONTROL}/press");
    assert_eq!(
        refused(|v| control(v)["press"]["args"] = json!("not an object")),
        at(R::InvalidContent, "/content")
    );
    assert_eq!(
        refused(|v| control(v)["press"]["extra"] = json!(1)),
        at(R::InvalidContent, "/content")
    );
    assert_eq!(
        refused(
            |v| control(v)["press"]["args"]["deep"] = json!({ "a": { "b": { "c": { "d": 1 } } } })
        ),
        at(R::InvalidContent, &format!("{press}/args"))
    );
    let mut v = pack();
    control(&mut v)["press"]["args"]["deep"] = json!({ "a": { "b": [1] } });
    seal(&mut v);
    assert!(read(&v.to_string()).is_ok(), "depth 4 is accepted");
    assert_eq!(
        refused(|v| control(v)["press"]["args"]["big"] = json!(vec!["x".repeat(1100); 30])),
        at(R::InvalidContent, &format!("{press}/args"))
    );
    assert_eq!(
        refused(|v| control(v)["press"]["args"]["long"] = json!("x".repeat(16 * 1024 + 1))),
        at(R::InvalidContent, &format!("{press}/args"))
    );
    assert_eq!(
        refused(|v| control(v)["press"]["args"]["not-a-name"] = json!(1)),
        at(R::InvalidContent, &format!("{press}/args"))
    );
    assert_eq!(
        refused(|v| control(v)["press"]["provider"] = json!("")),
        at(R::InvalidContent, &format!("{press}/provider"))
    );
    assert_eq!(
        refused(|v| control(v)["press"]["surfaceRefs"] = json!(["folder", "folder"])),
        at(R::InvalidContent, &format!("{press}/surfaceRefs/1"))
    );
}

#[test]
fn feedback_rules() {
    let rule = format!("{CONTROL}/feedback/0");
    let event = |extra: Value| {
        let mut r =
            json!({ "when": { "event": { "provider": "ci", "id": "done" } }, "show": "busy" });
        r.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        r
    };
    assert_eq!(
        refused(|v| control(v)["feedback"] = json!([event(json!({ "hold": { "ms": 10 } }))])),
        at(R::InvalidContent, &rule)
    );
    let nine: Vec<Value> = (0..9)
        .map(|i| json!({ "field": format!("f{i}"), "equals": "x" }))
        .collect();
    assert_eq!(
        refused(|v| {
            let mut r = event(json!({}));
            r["when"]["where"] = json!(nine);
            control(v)["feedback"] = json!([r]);
        }),
        at(R::InvalidContent, &rule)
    );
    assert_eq!(
        refused(|v| control(v)["feedback"][0]["showFromValue"] = json!(true)),
        at(R::InvalidContent, &rule)
    );
    assert_eq!(
        refused(|v| control(v)["feedback"][0]["when"]["state"]["provider"] = json!("Acme")),
        at(R::InvalidContent, &rule)
    );
    // A value from the file is echoed short and plain.
    let mut v = pack();
    let hostile = format!("Acme\u{202E}{}", "x".repeat(500));
    control(&mut v)["feedback"][0]["when"]["state"]["provider"] = json!(hostile);
    seal(&mut v);
    let message = read(&v.to_string()).unwrap_err().message;
    assert!(
        message.len() < 120 && !message.contains('\u{202E}'),
        "{message}"
    );
    // Members the workspace type would drop are refused.
    assert_eq!(
        refused(|v| control(v)["feedback"][0]["when"]["is"]["extra"] = json!(1)),
        at(R::InvalidContent, "/content")
    );
    assert_eq!(
        refused(
            |v| control(v)["feedback"] = json!([event(json!({ "hold": { "ms": 400, "x": 1 } }))])
        ),
        at(R::InvalidContent, "/content")
    );
    assert_eq!(
        refused(|v| control(v)["feedback"][0]["colour"] = json!("red")),
        at(R::InvalidContent, "/content")
    );
}

#[test]
fn kind_rules() {
    let mut v = surface_file();
    v["content"]["contextRules"] = pack()["content"]["contextRules"].clone();
    v["content"]["contextRules"][1]["surface"] = json!("solo");
    v["content"]["contextRules"]
        .as_array_mut()
        .unwrap()
        .remove(1);
    v["content"]["contextRules"][0]["surface"] = json!("solo");
    v["content"]["contextRules"][0]
        .as_object_mut()
        .unwrap()
        .remove("page");
    seal(&mut v);
    assert_eq!(
        read(&v.to_string()).unwrap_err().code,
        R::KindMismatch,
        "a surface file with rules"
    );
    let mut v = surface_file();
    v["manifest"]["inputs"] = json!([{ "id": "url", "type": "url", "label": "Address" }]);
    seal(&mut v);
    let r = read(&v.to_string()).unwrap_err();
    assert_eq!(
        (r.code, r.at.as_deref()),
        (R::KindMismatch, Some("/manifest/inputs"))
    );
    let mut v = surface_file();
    let second = {
        let mut s = v["content"]["surfaces"][0].clone();
        s["id"] = json!("second");
        s
    };
    v["content"]["surfaces"]
        .as_array_mut()
        .unwrap()
        .push(second);
    seal(&mut v);
    let r = read(&v.to_string()).unwrap_err();
    assert_eq!(
        (r.code, r.at.as_deref()),
        (R::KindMismatch, Some("/content/surfaces"))
    );
    let mut v = surface_file();
    v["content"]["surfaces"][0]["pages"][0]["controls"][0]["press"]["inputs"] =
        json!({ "url": "site" });
    seal(&mut v);
    let r = read(&v.to_string()).unwrap_err();
    assert_eq!(
        (r.code, r.at.as_deref()),
        (
            R::KindMismatch,
            Some("/content/surfaces/0/pages/0/controls/0/press/inputs")
        )
    );
}

#[test]
fn every_plugin_used_is_declared() {
    assert_eq!(
        refused(|v| {
            v["manifest"]["dependencies"]
                .as_array_mut()
                .unwrap()
                .remove(0);
        }),
        at(R::InvalidManifest, &format!("{CONTROL}/press/provider"))
    );
    assert_eq!(
        refused(|v| {
            v["manifest"]["dependencies"]
                .as_array_mut()
                .unwrap()
                .remove(1);
        }),
        at(
            R::InvalidManifest,
            &format!("{CONTROL}/feedback/0/when/state/provider")
        )
    );
    // Host providers and event sources are never dependencies.
    let mut v = pack();
    control(&mut v)["press"] =
        json!({ "provider": "agent-control", "action": "newTask", "args": {} });
    control(&mut v)["feedback"][0]["when"]["state"]["provider"] = json!("agent-control");
    v["manifest"]["dependencies"] = json!([]);
    v["manifest"]["inputs"]
        .as_array_mut()
        .unwrap()
        .retain(|i| i["id"] != "project");
    seal(&mut v);
    assert!(read(&v.to_string()).is_ok());
}

#[test]
fn references_resolve() {
    let rules = "/content/contextRules";
    assert_eq!(
        refused(|v| v["content"]["contextRules"][0]["surface"] = json!("missing")),
        at(R::InvalidContent, &format!("{rules}/0/surface"))
    );
    assert_eq!(
        refused(|v| v["content"]["contextRules"][0]["page"] = json!("missing")),
        at(R::InvalidContent, &format!("{rules}/0/page"))
    );
    assert_eq!(
        refused(|v| v["content"]["contextRules"][1]["application"]["input"] = json!("ghost")),
        at(R::InvalidContent, &format!("{rules}/1/application/input"))
    );
    assert_eq!(
        refused(|v| v["content"]["contextRules"][1]["application"]["input"] = json!("project")),
        at(R::InvalidContent, &format!("{rules}/1/application/input"))
    );
    let next = "/content/surfaces/0/pages/0/controls/1/press";
    for args in [
        json!({}),
        json!({ "surface": 3 }),
        json!({ "surface": "elsewhere" }),
    ] {
        assert_eq!(
            refused(
                |v| v["content"]["surfaces"][0]["pages"][0]["controls"][1]["press"]["args"] =
                    args.clone()
            ),
            at(R::InvalidContent, &format!("{next}/surfaceRefs/0")),
            "{args}"
        );
    }
    let press = format!("{CONTROL}/press");
    assert_eq!(
        refused(|v| control(v)["press"]["inputs"]["folder"] = json!("ghost")),
        at(R::InvalidContent, &format!("{press}/inputs/folder"))
    );
    assert_eq!(
        refused(|v| control(v)["press"]["inputs"]["folder"] = json!("editor")),
        at(R::InvalidContent, &format!("{press}/inputs/folder"))
    );
    assert_eq!(
        refused(|v| control(v)["press"]["inputs"] = json!({ "count": "note" })),
        at(R::InvalidContent, &format!("{press}/inputs/count"))
    );
    assert_eq!(
        refused(|v| control(v)["press"]["inputs"] = json!({ "missing": "note" })),
        at(R::InvalidContent, &format!("{press}/inputs/missing"))
    );
    assert_eq!(
        refused(|v| control(v)["feedback"][0]["show"] = json!("missing")),
        at(R::InvalidContent, &format!("{CONTROL}/feedback/0/show"))
    );
}

/// The plugins a workspace's pads use, by an action or a state rule, each
/// once: the Home starter's pads are an example plugin's.
fn plugins_used(workspace: &crate::core::model::Workspace) -> Vec<String> {
    let mut plugins: Vec<String> = workspace
        .surfaces
        .iter()
        .flat_map(|s| &s.pages)
        .flat_map(|p| &p.controls)
        .flat_map(|c| {
            let actions = c.press.iter().chain(&c.long_press).map(|a| &a.provider);
            let states = c.feedback.iter().filter_map(|r| match &r.when {
                crate::state::feedback::When::State(s) => Some(&s.state.provider),
                crate::state::feedback::When::Event(_) => None,
            });
            actions.chain(states)
        })
        .filter(|provider| provider.contains('.'))
        .cloned()
        .collect();
    plugins.sort();
    plugins.dedup();
    plugins
}

/// The five starter surfaces, to a file's types and back, lose nothing a
/// file carries, and as one Pack pass every structural check.
#[test]
fn the_starter_surfaces_convert_both_ways() {
    let workspace = default_workspace_for(Platform::Linux);
    assert_eq!(workspace.surfaces.len(), 5);
    let mut surfaces = Vec::new();
    for surface in &workspace.surfaces {
        let dto = SurfaceV1::from_model(surface).unwrap();
        assert_eq!(&dto.to_model(), surface, "{}", surface.id);
        let json = serde_json::to_value(&dto).unwrap();
        let back: SurfaceV1 = serde_json::from_value(json).unwrap();
        assert_eq!(back, dto, "{}", surface.id);
        surfaces.push(dto);
    }
    let content = ContentV1 {
        surfaces,
        context_rules: Vec::new(),
    };
    check_content(&content).unwrap();
    let manifest = ManifestV1 {
        id: "local.starters".into(),
        name: "Starters".into(),
        version: "1.0.0".into(),
        description: String::new(),
        author: None,
        license: String::new(),
        created_with: CreatedWith {
            app: CREATED_WITH_APP.into(),
            version: "0.3.0".into(),
        },
        requirements: compute_requirements(&content),
        tested_with: Vec::new(),
        dependencies: plugins_used(&workspace)
            .into_iter()
            .map(|plugin_id| DependencyV1 {
                plugin_id,
                name: None,
                version: "*".into(),
                required: true,
            })
            .collect(),
        inputs: Vec::new(),
        integrity: Integrity {
            content: content.digest().unwrap(),
        },
    };
    check_manifest(&manifest).unwrap();
    check_cross(ArtifactKind::Pack, &manifest, &content).unwrap();
    // Written and read back: the same file, the same bytes.
    let artifact = ArtifactV1 {
        kind: ArtifactKind::Pack,
        manifest,
        content,
    };
    let text = artifact.to_canonical().unwrap();
    let again = read(&text).unwrap();
    assert_eq!(again, artifact);
    assert_eq!(again.to_canonical().unwrap(), text);
}

#[test]
fn what_a_file_never_carries() {
    let mut surface = default_workspace_for(Platform::Linux).surfaces[1].clone();
    surface.context = Some(crate::core::model::ContextHint {
        foreground_apps: vec!["code".into()],
    });
    surface.pages[0].controls[0].legacy_bindings = vec![json!({ "old": true })];
    surface.pages[0].controls[0].icon = Some(String::new());
    let dto = SurfaceV1::from_model(&surface).unwrap();
    let text = serde_json::to_string(&dto).unwrap();
    assert!(
        !text.contains("foregroundApps") && !text.contains("legacy") && !text.contains("\"old\"")
    );
    let back = dto.to_model();
    assert_eq!(back.context, None);
    assert!(back.pages[0].controls[0].legacy_bindings.is_empty());
    assert_eq!(back.pages[0].controls[0].icon, None);
    // A control on the logo LED cannot be carried.
    surface.pages[0].controls[0].pos = crate::core::model::ControlPos::Logo;
    assert!(SurfaceV1::from_model(&surface)
        .unwrap_err()
        .contains("logo"));
}

#[test]
fn defaults_are_not_written() {
    let artifact = read(&pack().to_string()).unwrap();
    let text = artifact.to_canonical().unwrap();
    assert!(text.starts_with(
        "{\n  \"format\": \"launchdeck.pack\",\n  \"formatVersion\": 1,\n  \"manifest\": {"
    ));
    assert!(text.ends_with("}\n"));
    for absent in [
        "\"animation\": \"none\"",
        "\"required\": true,\n        \"type\"",
        "\"span\": {\n                \"h\": 1,\n                \"w\": 1",
        "\"contextRules\": []",
        "\"surfaceRefs\": []",
        "\"inputs\": {}",
        "\"description\": \"\"",
    ] {
        assert!(!text.contains(absent), "{absent}");
    }
    // Written twice, the same bytes; read back, the same file.
    assert_eq!(artifact.to_canonical().unwrap(), text);
    assert_eq!(read(&text).unwrap(), artifact);
    // The content digest is the one the manifest names.
    assert_eq!(
        artifact.content.digest().unwrap(),
        artifact.manifest.integrity.content
    );
}

#[test]
fn the_kinds() {
    for (kind, format, extension, media) in [
        (
            ArtifactKind::Surface,
            "launchdeck.surface",
            "ldsurface",
            "application/vnd.launchdeck.surface+json",
        ),
        (
            ArtifactKind::Pack,
            "launchdeck.pack",
            "ldpack",
            "application/vnd.launchdeck.pack+json",
        ),
    ] {
        assert_eq!(ArtifactKind::from_format(format), Some(kind));
        assert_eq!(
            (kind.format(), kind.extension(), kind.media_type()),
            (format, extension, media)
        );
    }
    assert_eq!(ArtifactKind::from_format("launchdeck.workspace"), None);
    assert_eq!(
        serde_json::to_value(ArtifactKind::Pack).unwrap(),
        json!("pack")
    );
}

/// The feedback rule is the workspace's own type: a change to how it is
/// written changes the file format, and must be made on purpose.
#[test]
fn the_feedback_rule_is_frozen() {
    let golden = json!([
        { "when": { "state": { "provider": "example.counter", "id": "level", "args": { "slot": 2 } },
                    "is": { "op": "equals", "value": "high" } },
          "show": "warning" },
        { "when": { "state": { "provider": "example.counter", "id": "value" },
                    "is": { "op": "atLeast", "value": 5.0 } },
          "show": "high" },
        { "when": { "state": { "provider": "agent-control", "id": "slot.status" },
                    "is": { "op": "hasValue" } },
          "showFromValue": true },
        { "when": { "event": { "provider": "ci", "id": "build.finished" },
                    "where": [ { "field": "result", "equals": "failed" } ] },
          "show": "failed", "hold": { "ms": 4000 } },
        { "when": { "event": { "provider": "ci", "id": "x" } },
          "showFromField": "status", "hold": "latch" }
    ]);
    let rules: Vec<FeedbackRule> = serde_json::from_value(golden.clone()).unwrap();
    assert_eq!(serde_json::to_value(&rules).unwrap(), golden);
    assert_eq!(
        to_canonical(&serde_json::to_value(&rules).unwrap()),
        to_canonical(&golden)
    );
}
