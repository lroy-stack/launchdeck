//! The hostile fixtures of plan 7.20, a release blocker: every malformed,
//! oversized, archive-like or lying file is refused by `inspect` with a
//! stable code and, where the problem has a place, the JSON pointer to it.
//! The apply stage (stale plans, missing decisions, save failures) and the
//! run of every case through the artifact service on a real store are
//! package C5's.
//!
//! Small cases are files in `tests/fixtures/artifacts/hostile/`, named by
//! case (`.gitattributes` keeps their bytes). Large or deep cases are built
//! here, and so is every case that changes one thing in a valid file: the
//! golden Pack and Surface file of `tests/fixtures/artifacts/valid/` are
//! read, changed, and sealed again (their digest recomputed) unless the
//! case is about the digest. Files named `review-*` and `never-runs-*` are
//! valid on purpose: what they launch must be reviewed, or can never run.

use std::path::PathBuf;
use std::sync::Arc;

use launchdeck_lib::artifacts::{
    classify, content_digest, inspect, Environment, InspectWarningCode, Location, ParsedArtifact,
    RejectionCode as R, ReviewItem, ReviewKind, Slot,
};
use launchdeck_lib::core::runtime::{NullEmitter, Runtime, RuntimeConfig};
use launchdeck_lib::plugins::keyboard::{Combo, Keyboard};
use launchdeck_lib::plugins::ActionError;
use launchdeck_lib::surfaces::default_workspace;
use serde_json::{json, Value};

const MIB: usize = 1024 * 1024;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/artifacts")
}

fn hostile(name: &str) -> Vec<u8> {
    std::fs::read(fixtures().join("hostile").join(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn golden_text(name: &str) -> String {
    std::fs::read_to_string(fixtures().join("valid").join(name)).unwrap()
}

/// The golden Pack: the five starters, two context rules, six inputs.
fn pack() -> Value {
    serde_json::from_str(&golden_text("starters.ldpack")).unwrap()
}

/// The golden Surface file: Development.
fn surface_file() -> Value {
    serde_json::from_str(&golden_text("development.ldsurface")).unwrap()
}

/// A file's bytes, its digest set to what its content has (a case that is
/// not about the digest).
fn sealed(mut v: Value) -> Vec<u8> {
    let content = serde_json::to_string(&v["content"]).unwrap();
    v["manifest"]["integrity"]["content"] = json!(content_digest(&content));
    serde_json::to_vec_pretty(&v).unwrap()
}

fn refused(bytes: &[u8], file_name: &str) -> (R, Option<String>) {
    let r = inspect(bytes, file_name).expect_err("should be refused");
    assert!(!r.message.is_empty() && r.message.ends_with('.'), "{r:?}");
    (r.code, r.at)
}

fn accepted(bytes: &[u8], file_name: &str) -> ParsedArtifact {
    inspect(bytes, file_name).unwrap_or_else(|r| panic!("{file_name}: {r:?}"))
}

/// Change the golden Pack, seal it, inspect it.
fn pack_refused(change: impl FnOnce(&mut Value)) -> (R, Option<String>) {
    let mut v = pack();
    change(&mut v);
    refused(&sealed(v), "case.ldpack")
}

fn surface_refused(change: impl FnOnce(&mut Value)) -> (R, Option<String>) {
    let mut v = surface_file();
    change(&mut v);
    refused(&sealed(v), "case.ldsurface")
}

fn at(code: R, at: &str) -> (R, Option<String>) {
    (code, Some(at.to_string()))
}

fn whole(code: R) -> (R, Option<String>) {
    (code, None)
}

/// In the golden Pack: Development, its first page, the Terminal pad.
const DEV: &str = "/content/surfaces/1";
const TERMINAL: &str = "/content/surfaces/1/pages/0/controls/0";

fn dev(v: &mut Value) -> &mut Value {
    &mut v["content"]["surfaces"][1]
}

fn terminal(v: &mut Value) -> &mut Value {
    &mut dev(v)["pages"][0]["controls"][0]
}

/// The file with every space, tab and line end between tokens taken out.
fn compact(text: &str) -> String {
    let (mut out, mut in_string, mut escaped) = (String::new(), false, false);
    for c in text.chars() {
        if in_string {
            escaped = !escaped && c == '\\';
            in_string = escaped || c != '"';
        } else if c == '"' {
            in_string = true;
        } else if c.is_ascii_whitespace() {
            continue;
        }
        out.push(c);
    }
    out
}

// Positive controls.

#[test]
fn the_golden_files_are_accepted() {
    let parsed = accepted(golden_text("starters.ldpack").as_bytes(), "starters.ldpack");
    assert_eq!(parsed.content.surfaces.len(), 5);
    assert_eq!(parsed.format_version, 1);
    assert_eq!(parsed.content_digest, parsed.manifest.integrity.content);
    assert_eq!(parsed.size_bytes, golden_text("starters.ldpack").len());
    assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
    accepted(
        golden_text("development.ldsurface").as_bytes(),
        "development.ldsurface",
    );
}

#[test]
fn a_whitespace_only_reformat_is_accepted() {
    let text = golden_text("starters.ldpack");
    let tight = compact(&text);
    assert!(tight.len() < text.len() / 2);
    let tabs = text.replace("  ", "\t");
    for reformatted in [tight, tabs] {
        let parsed = accepted(reformatted.as_bytes(), "starters.ldpack");
        assert_eq!(
            parsed.content_digest,
            pack()["manifest"]["integrity"]["content"]
        );
    }
}

#[test]
fn the_crlf_version_is_accepted() {
    let crlf = golden_text("development.ldsurface").replace('\n', "\r\n");
    accepted(crlf.as_bytes(), "development.ldsurface");
}

// Bytes.

#[test]
fn bytes_empty() {
    assert_eq!(
        refused(&hostile("empty.ldpack"), "empty.ldpack"),
        whole(R::Empty)
    );
}

#[test]
fn bytes_two_mebibytes_and_one() {
    let mut text = golden_text("starters.ldpack");
    text.push_str(&" ".repeat(2 * MIB - text.len()));
    accepted(text.as_bytes(), "starters.ldpack");
    text.push(' ');
    assert_eq!(text.len(), 2 * MIB + 1);
    assert_eq!(
        refused(text.as_bytes(), "starters.ldpack"),
        whole(R::TooLarge)
    );
}

#[test]
fn bytes_truncated() {
    let (code, _) = refused(&hostile("truncated.ldsurface"), "truncated.ldsurface");
    assert_eq!(code, R::NotJson);
}

#[test]
fn bytes_garbage() {
    assert_eq!(
        refused(&hostile("garbage.ldpack"), "garbage.ldpack"),
        whole(R::NotJson)
    );
}

#[test]
fn bytes_utf8_byte_order_mark() {
    assert_eq!(
        refused(&hostile("bom.ldsurface"), "bom.ldsurface"),
        whole(R::NotUtf8)
    );
}

#[test]
fn bytes_invalid_utf8() {
    assert_eq!(
        refused(&hostile("invalid-utf8.ldpack"), "invalid-utf8.ldpack"),
        whole(R::NotUtf8)
    );
}

/// A ZIP archive renamed `.ldpack` (every byte ASCII, so it is text): there
/// is no archive reader to reach, it is simply not JSON.
#[test]
fn bytes_zip_renamed() {
    let zip = hostile("zip-renamed.ldpack");
    assert!(zip.starts_with(b"PK\x03\x04"));
    assert_eq!(refused(&zip, "zip-renamed.ldpack"), whole(R::NotJson));
}

#[test]
fn bytes_top_level_array() {
    assert_eq!(
        refused(&hostile("top-level-array.ldpack"), "top-level-array.ldpack"),
        whole(R::NotArtifact)
    );
}

// Envelope.

#[test]
fn envelope_missing_format() {
    assert_eq!(
        refused(&hostile("missing-format.ldpack"), "missing-format.ldpack"),
        whole(R::NotArtifact)
    );
}

#[test]
fn envelope_workspace_format() {
    assert_eq!(
        refused(&hostile("format-workspace.ldpack"), "x.ldpack"),
        at(R::NotArtifact, "/format")
    );
}

#[test]
fn envelope_format_version_2() {
    assert_eq!(
        refused(&hostile("format-version-2.ldpack"), "x.ldpack"),
        at(R::TooNew, "/formatVersion")
    );
}

#[test]
fn envelope_format_version_0() {
    assert_eq!(
        refused(&hostile("format-version-0.ldpack"), "x.ldpack"),
        at(R::UnsupportedFormatVersion, "/formatVersion")
    );
}

#[test]
fn envelope_format_version_as_text() {
    assert_eq!(
        refused(&hostile("format-version-string.ldpack"), "x.ldpack"),
        at(R::UnsupportedFormatVersion, "/formatVersion")
    );
}

#[test]
fn envelope_unknown_member() {
    assert_eq!(
        refused(&hostile("unknown-member.ldpack"), "x.ldpack"),
        at(R::UnknownField, "/installScript")
    );
}

#[test]
fn envelope_signatures() {
    assert_eq!(
        refused(&hostile("signatures.ldpack"), "x.ldpack"),
        at(R::UnsupportedSignature, "/signatures")
    );
}

#[test]
fn envelope_assets_svg_with_script() {
    assert_eq!(
        refused(&hostile("assets-svg-script.ldpack"), "x.ldpack"),
        at(R::UnsupportedAsset, "/assets")
    );
}

#[test]
fn envelope_assets_base64_executable() {
    assert_eq!(
        refused(&hostile("assets-base64-mz.ldpack"), "x.ldpack"),
        at(R::UnsupportedAsset, "/assets")
    );
}

#[test]
fn envelope_assets_path_traversal() {
    assert_eq!(
        refused(&hostile("assets-path-traversal.ldpack"), "x.ldpack"),
        at(R::UnsupportedAsset, "/assets")
    );
}

#[test]
fn envelope_raw_workspace_file() {
    let workspace = std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/default-workspace.json"),
    )
    .unwrap();
    assert_eq!(
        refused(&workspace, "workspace.ldpack"),
        whole(R::NotArtifact)
    );
}

// JSON.

#[test]
fn json_duplicate_content() {
    assert_eq!(
        refused(&hostile("duplicate-content.ldpack"), "x.ldpack"),
        at(R::DuplicateKey, "/content")
    );
}

#[test]
fn json_duplicate_key_in_a_control() {
    assert_eq!(
        refused(
            &hostile("duplicate-key-in-control.ldsurface"),
            "x.ldsurface"
        ),
        at(
            R::DuplicateKey,
            "/content/surfaces/0/pages/0/controls/0/label"
        )
    );
}

#[test]
fn json_duplicate_look_through_an_escape() {
    assert_eq!(
        refused(&hostile("duplicate-look-escaped.ldsurface"), "x.ldsurface"),
        at(
            R::DuplicateKey,
            "/content/surfaces/0/pages/0/controls/0/looks/a"
        )
    );
}

#[test]
fn json_ten_thousand_nested_arrays_in_args() {
    let mut v = pack();
    terminal(&mut v)["press"]["args"] = json!({ "app": "code" });
    let text = serde_json::to_string(&v).unwrap().replace(
        "\"app\":\"code\"",
        &format!("\"app\":{}{}", "[".repeat(10_000), "]".repeat(10_000)),
    );
    let (code, place) = refused(text.as_bytes(), "deep.ldpack");
    assert_eq!(code, R::TooDeep);
    let place = place.unwrap();
    assert!(
        place.starts_with(&format!("{TERMINAL}/press/args/app/0/0")),
        "{place}"
    );
}

#[test]
fn json_seventy_kibibyte_string() {
    assert_eq!(
        pack_refused(|v| terminal(v)["description"] = json!("x".repeat(70 * 1024))),
        at(R::StringTooLong, &format!("{TERMINAL}/description"))
    );
}

#[test]
fn json_number_1e400() {
    assert_eq!(
        refused(&hostile("number-1e400.ldpack"), "x.ldpack"),
        whole(R::NotJson)
    );
}

// Integrity.

#[test]
fn integrity_one_character_edit() {
    let text = golden_text("starters.ldpack").replacen(
        "\"label\": \"Terminal\"",
        "\"label\": \"Terminai\"",
        1,
    );
    assert_eq!(
        refused(text.as_bytes(), "starters.ldpack"),
        at(R::Integrity, "/manifest/integrity/content")
    );
}

#[test]
fn integrity_missing() {
    let mut v = pack();
    v["manifest"].as_object_mut().unwrap().remove("integrity");
    let bytes = serde_json::to_vec_pretty(&v).unwrap();
    assert_eq!(
        refused(&bytes, "x.ldpack"),
        at(R::InvalidManifest, "/manifest")
    );
}

#[test]
fn integrity_uppercase_hex() {
    let mut v = pack();
    let digest = v["manifest"]["integrity"]["content"].as_str().unwrap();
    let upper = format!("sha256:{}", digest["sha256:".len()..].to_uppercase());
    v["manifest"]["integrity"]["content"] = json!(upper);
    let bytes = serde_json::to_vec_pretty(&v).unwrap();
    assert_eq!(
        refused(&bytes, "x.ldpack"),
        at(R::InvalidManifest, "/manifest/integrity/content")
    );
}

#[test]
fn integrity_md5() {
    let mut v = pack();
    v["manifest"]["integrity"]["content"] = json!("md5:9e107d9d372bb6826bd81d3542a419d6");
    let bytes = serde_json::to_vec_pretty(&v).unwrap();
    assert_eq!(
        refused(&bytes, "x.ldpack"),
        at(R::InvalidManifest, "/manifest/integrity/content")
    );
}

// Manifest.

#[test]
fn manifest_ids() {
    for id in ["../evil", "Acme.Pack", "single", "/abs", "C:\\x"] {
        assert_eq!(
            pack_refused(|v| v["manifest"]["id"] = json!(id)),
            at(R::InvalidManifest, "/manifest/id"),
            "{id}"
        );
    }
}

#[test]
fn manifest_versions() {
    for version in ["1.0", "01.0.0", "1.0.0+build"] {
        assert_eq!(
            pack_refused(|v| v["manifest"]["version"] = json!(version)),
            at(R::InvalidManifest, "/manifest/version"),
            "{version}"
        );
    }
}

#[test]
fn manifest_name_empty_or_61_characters() {
    for name in [String::new(), "x".repeat(61)] {
        assert_eq!(
            pack_refused(|v| v["manifest"]["name"] = json!(name)),
            at(R::InvalidManifest, "/manifest/name")
        );
    }
}

#[test]
fn manifest_label_with_a_direction_override() {
    assert_eq!(
        pack_refused(|v| v["manifest"]["inputs"][0]["label"] = json!("Lights\u{202E}kooh")),
        at(R::InvalidManifest, "/manifest/inputs/0/label")
    );
    assert_eq!(
        pack_refused(|v| terminal(v)["label"] = json!("Term\u{202E}lanim")),
        at(R::InvalidContent, &format!("{TERMINAL}/label"))
    );
}

#[test]
fn manifest_control_character_in_description() {
    assert_eq!(
        pack_refused(|v| v["manifest"]["description"] = json!("Five\u{7} starters")),
        at(R::InvalidManifest, "/manifest/description")
    );
}

#[test]
fn manifest_author_url() {
    for url in [
        "javascript:alert(1)",
        "http://example.com",
        "https://u:p@example.com",
    ] {
        assert_eq!(
            pack_refused(|v| v["manifest"]["author"]["url"] = json!(url)),
            at(R::InvalidManifest, "/manifest/author/url"),
            "{url}"
        );
    }
}

// Dependencies.

fn dependency(plugin_id: &str, version: &str) -> Value {
    json!({ "pluginId": plugin_id, "version": version, "required": true })
}

#[test]
fn dependencies_invalid_plugin_id() {
    assert_eq!(
        pack_refused(|v| v["manifest"]["dependencies"] = json!([dependency("Acme Notes", "*")])),
        at(R::InvalidManifest, "/manifest/dependencies/0/pluginId")
    );
}

#[test]
fn dependencies_requirement_not_semver() {
    assert_eq!(
        pack_refused(|v| v["manifest"]["dependencies"] = json!([dependency("acme.notes", ">>1")])),
        at(R::InvalidManifest, "/manifest/dependencies/0/version")
    );
}

#[test]
fn dependencies_prerelease_comparator() {
    assert_eq!(
        pack_refused(|v| {
            v["manifest"]["dependencies"] = json!([dependency("acme.notes", "^1.0.0-beta.1")])
        }),
        at(R::InvalidManifest, "/manifest/dependencies/0/version")
    );
}

#[test]
fn dependencies_duplicate() {
    assert_eq!(
        pack_refused(|v| {
            v["manifest"]["dependencies"] = json!([
                dependency("acme.notes", "*"),
                dependency("acme.notes", "^1")
            ])
        }),
        at(R::InvalidManifest, "/manifest/dependencies/1/pluginId")
    );
}

#[test]
fn dependencies_thirty_three() {
    let deps: Vec<Value> = (0..33)
        .map(|i| dependency(&format!("acme.p{i:02}"), "*"))
        .collect();
    assert_eq!(
        pack_refused(|v| v["manifest"]["dependencies"] = json!(deps)),
        at(R::TooMany, "/manifest/dependencies")
    );
}

#[test]
fn dependencies_host_provider() {
    assert_eq!(
        pack_refused(|v| v["manifest"]["dependencies"] = json!([dependency("system", "*")])),
        at(R::InvalidManifest, "/manifest/dependencies/0/pluginId")
    );
}

#[test]
fn dependencies_undeclared_plugin() {
    assert_eq!(
        pack_refused(|v| {
            terminal(v)["press"] = json!({ "provider": "acme.notes", "action": "mark", "args": {} })
        }),
        at(R::InvalidManifest, &format!("{TERMINAL}/press/provider"))
    );
}

// Limits and geometry.

/// A control that parses, for the counts that are checked before anything
/// else about a control.
fn filler(i: usize) -> Value {
    json!({ "id": format!("f{i}"), "pos": { "kind": "grid", "col": 0, "row": 0 },
            "label": "F", "visual": { "color": "#000000" } })
}

#[test]
fn limits_seventeen_surfaces() {
    assert_eq!(
        pack_refused(|v| {
            let first = v["content"]["surfaces"][1].clone();
            let surfaces = v["content"]["surfaces"].as_array_mut().unwrap();
            for i in 0..12 {
                let mut copy = first.clone();
                copy["id"] = json!(format!("copy-{i}"));
                surfaces.push(copy);
            }
            assert_eq!(surfaces.len(), 17);
        }),
        at(R::TooMany, "/content/surfaces")
    );
}

#[test]
fn limits_thirty_three_pages() {
    assert_eq!(
        pack_refused(|v| {
            let page = dev(v)["pages"][1].clone();
            let pages = dev(v)["pages"].as_array_mut().unwrap();
            for i in 0..31 {
                let mut copy = page.clone();
                copy["id"] = json!(format!("p{i}"));
                pages.push(copy);
            }
        }),
        at(R::TooMany, &format!("{DEV}/pages"))
    );
}

#[test]
fn limits_129_controls_on_a_page() {
    assert_eq!(
        pack_refused(
            |v| dev(v)["pages"][0]["controls"] = json!((0..129).map(filler).collect::<Vec<_>>())
        ),
        at(R::TooMany, &format!("{DEV}/pages/0/controls"))
    );
}

#[test]
fn limits_2049_controls() {
    assert_eq!(
        pack_refused(|v| {
            let total: usize = v["content"]["surfaces"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|s| s["pages"].as_array().unwrap())
                .map(|p| p["controls"].as_array().unwrap().len())
                .sum();
            let controls = dev(v)["pages"][1]["controls"].as_array_mut().unwrap();
            controls.extend((0..2049 - total).map(filler));
        }),
        at(R::TooMany, "/content/surfaces")
    );
}

#[test]
fn limits_thirty_three_looks() {
    let looks: serde_json::Map<String, Value> = (0..33)
        .map(|i| (format!("look{i}"), json!({ "color": "#101010" })))
        .collect();
    assert_eq!(
        pack_refused(|v| terminal(v)["looks"] = Value::Object(looks)),
        at(R::TooMany, &format!("{TERMINAL}/looks"))
    );
}

fn event_rule(extra: Value) -> Value {
    let mut rule =
        json!({ "when": { "event": { "provider": "ci", "id": "done" } }, "show": "success" });
    rule.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    rule
}

#[test]
fn limits_thirty_three_feedback_rules() {
    let rules: Vec<Value> = (0..33).map(|_| event_rule(json!({}))).collect();
    assert_eq!(
        pack_refused(|v| terminal(v)["feedback"] = json!(rules)),
        at(R::TooMany, &format!("{TERMINAL}/feedback"))
    );
}

#[test]
fn limits_sixty_five_context_rules() {
    assert_eq!(
        pack_refused(|v| {
            let rule = v["content"]["contextRules"][0].clone();
            v["content"]["contextRules"] = json!(vec![rule; 65]);
        }),
        at(R::TooMany, "/content/contextRules")
    );
}

#[test]
fn limits_nine_identities() {
    let identities: Vec<Value> = (0..9)
        .map(|i| json!({ "namespace": format!("ns{i}"), "id": "app" }))
        .collect();
    assert_eq!(
        pack_refused(
            |v| v["content"]["contextRules"][0]["application"]["identities"] = json!(identities)
        ),
        at(R::TooMany, "/content/contextRules/0/application/identities")
    );
}

#[test]
fn geometry_column_16() {
    assert_eq!(
        pack_refused(|v| terminal(v)["pos"] = json!({ "kind": "grid", "col": 16, "row": 0 })),
        at(R::InvalidContent, TERMINAL)
    );
}

#[test]
fn geometry_span_9() {
    assert_eq!(
        pack_refused(|v| terminal(v)["span"] = json!({ "w": 9, "h": 1 })),
        at(R::InvalidContent, &format!("{TERMINAL}/span"))
    );
}

#[test]
fn geometry_logo_control() {
    assert_eq!(
        pack_refused(|v| terminal(v)["pos"] = json!({ "kind": "logo" })),
        at(R::InvalidContent, "/content")
    );
}

#[test]
fn geometry_overlap() {
    assert_eq!(
        pack_refused(|v| dev(v)["pages"][0]["controls"][1]["pos"] =
            json!({ "kind": "grid", "col": 0, "row": 0 })),
        at(R::InvalidContent, TERMINAL)
    );
}

#[test]
fn duplicate_surface_id() {
    assert_eq!(
        pack_refused(|v| dev(v)["id"] = json!("agent-control")),
        at(R::InvalidContent, &format!("{DEV}/id"))
    );
}

#[test]
fn duplicate_page_id() {
    assert_eq!(
        pack_refused(|v| dev(v)["pages"][1]["id"] = json!("home")),
        at(R::InvalidContent, &format!("{DEV}/pages/1/id"))
    );
}

#[test]
fn duplicate_control_id() {
    assert_eq!(
        pack_refused(|v| dev(v)["pages"][0]["controls"][1]["id"] = json!("g.0.0")),
        at(R::InvalidContent, &format!("{DEV}/pages/0/controls/1/id"))
    );
}

// References.

/// The Lights pad of Smart Home, whose `url` asks the importer for
/// `lights_webhook`.
const LIGHTS: &str = "/content/surfaces/3/pages/0/controls/0/press";

#[test]
fn references_rule_to_a_surface_not_in_the_pack() {
    assert_eq!(
        pack_refused(|v| v["content"]["contextRules"][0]["surface"] = json!("elsewhere")),
        at(R::InvalidContent, "/content/contextRules/0/surface")
    );
}

#[test]
fn references_rule_page_not_in_its_surface() {
    assert_eq!(
        pack_refused(|v| v["content"]["contextRules"][0]["page"] = json!("nowhere")),
        at(R::InvalidContent, "/content/contextRules/0/page")
    );
}

#[test]
fn references_surface_refs() {
    for args in [
        json!({ "surfaceId": "productivity" }),
        json!({ "surfaceId": 3 }),
        json!({ "surfaceId": "elsewhere" }),
    ] {
        let refs = if args["surfaceId"] == "productivity" {
            "missing"
        } else {
            "surfaceId"
        };
        assert_eq!(
            pack_refused(|v| {
                terminal(v)["press"] = json!({ "provider": "navigation", "action": "openSurface",
                                               "args": args, "surfaceRefs": [refs] })
            }),
            at(
                R::InvalidContent,
                &format!("{TERMINAL}/press/surfaceRefs/0")
            ),
            "{args}"
        );
    }
}

#[test]
fn references_binding_to_an_undeclared_input() {
    assert_eq!(
        pack_refused(
            |v| v["content"]["surfaces"][3]["pages"][0]["controls"][0]["press"]["inputs"]["url"] =
                json!("nowhere")
        ),
        at(R::InvalidContent, &format!("{LIGHTS}/inputs/url"))
    );
}

#[test]
fn references_application_input_bound_to_an_argument() {
    assert_eq!(
        pack_refused(|v| v["manifest"]["inputs"][0]["type"] = json!("application")),
        at(R::InvalidContent, &format!("{LIGHTS}/inputs/url"))
    );
}

#[test]
fn references_text_input_bound_to_a_rule() {
    assert_eq!(
        pack_refused(|v| v["content"]["contextRules"][0]["application"] =
            json!({ "input": "lights_webhook" })),
        at(
            R::InvalidContent,
            "/content/contextRules/0/application/input"
        )
    );
}

#[test]
fn references_show_naming_a_missing_look() {
    assert_eq!(
        pack_refused(|v| terminal(v)["feedback"] = json!([event_rule(json!({ "show": "nope" }))])),
        at(R::InvalidContent, &format!("{TERMINAL}/feedback/0/show"))
    );
}

// Feedback.

#[test]
fn feedback_hold_10_ms() {
    assert_eq!(
        pack_refused(
            |v| terminal(v)["feedback"] = json!([event_rule(json!({ "hold": { "ms": 10 } }))])
        ),
        at(R::InvalidContent, &format!("{TERMINAL}/feedback/0"))
    );
}

#[test]
fn feedback_nine_where_entries() {
    let nine: Vec<Value> = (0..9)
        .map(|i| json!({ "field": format!("f{i}"), "equals": "x" }))
        .collect();
    let mut rule = event_rule(json!({}));
    rule["when"]["where"] = json!(nine);
    assert_eq!(
        pack_refused(|v| terminal(v)["feedback"] = json!([rule])),
        at(R::InvalidContent, &format!("{TERMINAL}/feedback/0"))
    );
}

#[test]
fn feedback_show_and_show_from_value() {
    assert_eq!(
        pack_refused(
            |v| terminal(v)["feedback"] = json!([event_rule(json!({ "showFromValue": true }))])
        ),
        at(R::InvalidContent, &format!("{TERMINAL}/feedback/0"))
    );
}

#[test]
fn feedback_invalid_state_provider_id() {
    let rule = json!({
        "when": { "state": { "provider": "Acme Notes", "id": "x" }, "is": { "op": "hasValue" } },
        "show": "success"
    });
    assert_eq!(
        pack_refused(|v| terminal(v)["feedback"] = json!([rule])),
        at(R::InvalidContent, &format!("{TERMINAL}/feedback/0"))
    );
}

// Assignments.

#[test]
fn assignments_args_not_an_object() {
    assert_eq!(
        pack_refused(|v| terminal(v)["press"]["args"] = json!("x-terminal-emulator")),
        at(R::InvalidContent, "/content")
    );
}

#[test]
fn assignments_extra_member() {
    assert_eq!(
        pack_refused(|v| terminal(v)["press"]["runAs"] = json!("root")),
        at(R::InvalidContent, "/content")
    );
}

#[test]
fn assignments_args_depth_5() {
    assert_eq!(
        pack_refused(
            |v| terminal(v)["press"]["args"] = json!({ "a": { "b": { "c": { "d": {} } } } })
        ),
        at(R::InvalidContent, &format!("{TERMINAL}/press/args"))
    );
}

#[test]
fn assignments_args_of_33_kibibytes() {
    let text = "x".repeat(11 * 1024);
    assert_eq!(
        pack_refused(|v| terminal(v)["press"]["args"] = json!({ "a": text, "b": text, "c": text })),
        at(R::InvalidContent, &format!("{TERMINAL}/press/args"))
    );
}

// Inputs.

#[test]
fn inputs_type_secret() {
    assert_eq!(
        pack_refused(|v| v["manifest"]["inputs"][0]["type"] = json!("secret")),
        at(R::InvalidManifest, "/manifest")
    );
}

#[test]
fn inputs_enum_without_options() {
    assert_eq!(
        pack_refused(|v| v["manifest"]["inputs"][0]["type"] = json!("enum")),
        at(R::InvalidManifest, "/manifest/inputs/0/options")
    );
}

#[test]
fn inputs_default_not_an_option() {
    assert_eq!(
        pack_refused(|v| {
            let input = &mut v["manifest"]["inputs"][0];
            input["type"] = json!("enum");
            input["options"] = json!([{ "value": "on", "label": "On" }]);
            input["default"] = json!({ "kind": "text", "value": "off" });
        }),
        at(R::InvalidManifest, "/manifest/inputs/0/default")
    );
}

#[test]
fn inputs_duplicate_id() {
    assert_eq!(
        pack_refused(|v| v["manifest"]["inputs"][1]["id"] = json!("lights_webhook")),
        at(R::InvalidManifest, "/manifest/inputs/1/id")
    );
}

#[test]
fn inputs_thirty_three() {
    assert_eq!(
        pack_refused(|v| {
            let inputs = v["manifest"]["inputs"].as_array_mut().unwrap();
            for i in inputs.len()..33 {
                inputs.push(
                    json!({ "id": format!("extra_{i}"), "type": "string", "label": "Extra" }),
                );
            }
        }),
        at(R::TooMany, "/manifest/inputs")
    );
}

// Requirements and kind.

#[test]
fn requirements_understated() {
    assert_eq!(
        pack_refused(|v| v["manifest"]["requirements"]["grid"]["minWidth"] = json!(4)),
        at(R::InvalidManifest, "/manifest/requirements/grid")
    );
}

#[test]
fn kind_surface_file_with_rules() {
    let rules = pack()["content"]["contextRules"].clone();
    assert_eq!(
        surface_refused(|v| v["content"]["contextRules"] = rules),
        at(R::KindMismatch, "/content/contextRules")
    );
}

#[test]
fn kind_surface_file_with_inputs() {
    let input = pack()["manifest"]["inputs"][0].clone();
    assert_eq!(
        surface_refused(|v| v["manifest"]["inputs"] = json!([input])),
        at(R::KindMismatch, "/manifest/inputs")
    );
}

#[test]
fn kind_surface_file_with_two_surfaces() {
    assert_eq!(
        surface_refused(|v| {
            let mut second = v["content"]["surfaces"][0].clone();
            second["id"] = json!("development-2");
            v["content"]["surfaces"]
                .as_array_mut()
                .unwrap()
                .push(second);
        }),
        at(R::KindMismatch, "/content/surfaces")
    );
}

// A file that lies about itself is believed only where it cannot hurt.

#[test]
fn the_format_decides_the_kind_and_a_wrong_extension_is_a_warning() {
    let parsed = accepted(
        golden_text("starters.ldpack").as_bytes(),
        "starters.ldsurface",
    );
    assert_eq!(parsed.kind, launchdeck_lib::artifacts::ArtifactKind::Pack);
    let codes: Vec<_> = parsed.warnings.iter().map(|w| w.code).collect();
    assert_eq!(codes, [InspectWarningCode::ExtensionMismatch]);
}

#[test]
fn the_file_name_is_shown_plain_and_short() {
    let name = format!("evil\u{202E}kcap\u{0}\u{7}{}.ldpack", "x".repeat(300));
    let parsed = accepted(golden_text("starters.ldpack").as_bytes(), &name);
    assert_eq!(parsed.file_name.chars().count(), 128);
    assert!(parsed.file_name.starts_with("evilkcapxxx"));
    assert!(!parsed
        .file_name
        .chars()
        .any(|c| c.is_control() || c == '\u{202E}'));
}

#[test]
fn an_unknown_category_is_a_warning() {
    let mut v = pack();
    dev(&mut v)["category"] = json!("weird-stuff");
    let parsed = accepted(&sealed(v), "x.ldpack");
    let warnings: Vec<_> = parsed
        .warnings
        .iter()
        .map(|w| (w.code, w.at.as_deref()))
        .collect();
    assert_eq!(
        warnings,
        [(
            InspectWarningCode::UnknownCategory,
            Some("/content/surfaces/1/category")
        )]
    );
    assert_eq!(parsed.content.surfaces[1].category, "weird-stuff");
}

// Actions that run things (security findings SF-1 and SF-3).

/// Every press of a file, with the review the import would show.
fn presses(parsed: &ParsedArtifact) -> Vec<(String, Option<ReviewItem>)> {
    let env = Environment::default();
    let surface = &parsed.content.surfaces[0];
    let page = &surface.pages[0];
    page.controls
        .iter()
        .map(|c| {
            let assignment = c.press.as_ref().unwrap().to_model();
            let item = classify(&assignment, &env).map(|kind| {
                let at = Location {
                    surface_source_id: surface.id.clone(),
                    page_id: page.id.clone(),
                    control_id: c.id.clone(),
                    slot: Slot::Press,
                };
                ReviewItem::new(at, &c.label, kind, &assignment)
            });
            (assignment.args.to_string(), item)
        })
        .collect()
}

fn every_launch_is_reviewed(name: &str) {
    let parsed = accepted(&hostile(name), name);
    let presses = presses(&parsed);
    assert!(!presses.is_empty());
    for (args, item) in presses {
        let item = item.unwrap_or_else(|| panic!("{name}: {args} is not reviewed"));
        assert_eq!(item.kind, ReviewKind::LaunchesProgram, "{args}");
        let hidden = item
            .text
            .chars()
            .any(|c| (c.is_control() && c != '\n') || ('\u{202A}'..='\u{202E}').contains(&c));
        assert!(!hidden, "{args}: {:?}", item.text);
    }
}

#[test]
fn launch_arguments_that_are_blank_are_reviewed() {
    every_launch_is_reviewed("review-launch-args-blank.ldsurface");
}

#[test]
fn launch_arguments_that_are_not_text_are_reviewed() {
    every_launch_is_reviewed("review-launch-args-not-text.ldsurface");
}

#[test]
fn launch_of_command_interpreter_characters_is_reviewed() {
    every_launch_is_reviewed("review-launch-command-chars.ldsurface");
}

#[test]
fn launch_by_path_tilde_dot_or_slash_look_alike_is_reviewed() {
    every_launch_is_reviewed("review-launch-paths.ldsurface");
}

#[test]
fn launch_with_a_line_break_is_reviewed() {
    every_launch_is_reviewed("review-launch-line-break.ldsurface");
}

struct NoKeyboard;

impl Keyboard for NoKeyboard {
    fn press(&self, _combo: &Combo) -> Result<(), ActionError> {
        panic!("no key may be pressed while a file is inspected");
    }
}

/// A case variant of an action that runs things is no review item, because
/// it never runs: the gate every run passes knows ids exactly.
#[tokio::test]
async fn a_case_variant_never_runs() {
    let name = "never-runs-case-variants.ldsurface";
    let parsed = accepted(&hostile(name), name);
    let rt = Runtime::start(RuntimeConfig {
        workspace: default_workspace(),
        store: None,
        emitter: Arc::new(NullEmitter),
        keyboard: Arc::new(NoKeyboard),
        scan_hardware: false,
        serve_external: false,
        demo_pace: 1.0,
        plugin_dirs: vec![],
        secrets: None,
        observer: None,
    })
    .await;
    let presses = presses(&parsed);
    assert_eq!(presses.len(), 8);
    for (control, (args, item)) in parsed.content.surfaces[0].pages[0]
        .controls
        .iter()
        .zip(presses)
    {
        assert!(item.is_none(), "{args}");
        let assignment = control.press.as_ref().unwrap().to_model();
        let issues = rt.validate_assignment(&assignment).unwrap_err();
        let unknown = issues[0]
            .message
            .ends_with("is not installed or was removed")
            || issues[0].message.contains("has no action");
        assert!(
            unknown,
            "{}: {}",
            assignment.definition_id(),
            issues[0].message
        );
    }
}

#[test]
fn an_action_that_runs_things_hides_no_character() {
    for (name, arg) in [
        ("hidden-shell-bidi.ldsurface", "command"),
        ("hidden-launch-nul.ldsurface", "app"),
        ("hidden-agent-escape.ldsurface", "text"),
        ("hidden-http-carriage-return.ldsurface", "url"),
        ("hidden-http-zero-width.ldsurface", "url"),
        ("hidden-shell-zero-width.ldsurface", "command"),
    ] {
        assert_eq!(
            refused(&hostile(name), name),
            at(
                R::InvalidContent,
                &format!("/content/surfaces/0/pages/0/controls/0/press/args/{arg}")
            ),
            "{name}"
        );
    }
}

/// A request the runtime would send elsewhere than its review shows (SF-4):
/// a user name before the host, a backslash, a host two parsers read
/// differently, a host in other scripts.
#[test]
fn a_request_goes_only_where_it_is_shown() {
    for name in [
        "request-userinfo.ldsurface",
        "request-backslash.ldsurface",
        "request-numeric-host.ldsurface",
        "request-idn-host.ldsurface",
    ] {
        assert_eq!(
            refused(&hostile(name), name),
            at(
                R::InvalidContent,
                "/content/surfaces/0/pages/0/controls/0/press/args/url"
            ),
            "{name}"
        );
    }
}

/// Every file of the hostile folder is refused, except the valid ones whose
/// launches must be reviewed or can never run.
#[test]
fn every_hostile_fixture_is_refused() {
    let mut names: Vec<String> = std::fs::read_dir(fixtures().join("hostile"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert!(names.len() >= 30, "{names:?}");
    for name in names {
        let bytes = hostile(&name);
        let valid = name.starts_with("review-") || name.starts_with("never-runs-");
        assert_eq!(inspect(&bytes, &name).is_ok(), valid, "{name}");
    }
}
