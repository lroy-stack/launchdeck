//! The export, step by step: choose the surfaces, carry each one, add the
//! context rules and the input bindings, look over every assignment as it
//! will be written, derive the dependencies and the manifest, run the
//! format's own checks on the result, and write it with the canonical writer.

use super::bindings::bind;
use super::dependencies::dependencies;
use super::metadata::{file_name, tested_with, words};
use super::preview::{
    Collected, ExportCode, ExportPreview, ExportResult, ExportedRule, ExportedSurface,
};
use super::request::ExportRequest;
use super::rules::export_rules;
use super::select::{choose, id_table, Chosen};
use super::surfaces::{export_surface, look_over};
use crate::artifacts::env::Environment;
use crate::artifacts::format::{
    check_content, check_cross, check_manifest, compute_requirements, ArtifactKind, ArtifactV1,
    ContentV1, CreatedWith, DependencyV1, Integrity, ManifestV1, CREATED_WITH_APP,
};
use crate::artifacts::limits::MAX_FILE_BYTES;
use crate::artifacts::review::check_visible;
use crate::core::model::Workspace;

/// Export the surfaces `request` names. Pure and deterministic: the same
/// workspace, environment and request give the same preview and the same
/// bytes. The bytes are there only when the preview has no error.
pub fn export(workspace: &Workspace, env: &Environment, request: &ExportRequest) -> ExportResult {
    let mut found = Collected::default();
    let chosen = choose(workspace, &request.surface_ids, &mut found);
    let ids = id_table(&chosen);
    let mut surfaces: Vec<_> = chosen
        .iter()
        .enumerate()
        .filter_map(|(i, c)| export_surface(c, i, &ids, env, &mut found))
        .collect();
    let (context_rules, rules_preview) =
        export_rules(workspace, request, &chosen, &ids, &mut found);
    check_kind(request, chosen.len(), !context_rules.is_empty(), &mut found);
    bind(&mut surfaces, request, &mut found);
    look_over(&surfaces, env, &mut found);
    let content = ContentV1 {
        surfaces,
        context_rules,
    };
    let dependencies = dependencies(&content, env, &mut found);
    let manifest = manifest(request, env, &chosen, &content, dependencies, &mut found);
    if !chosen.is_empty() {
        check_format(request.kind, &manifest, &content, &mut found);
    }
    let artifact = ArtifactV1 {
        kind: request.kind,
        manifest,
        content,
    };
    let text = artifact.to_canonical().unwrap_or_else(|e| {
        found.error(
            ExportCode::InvalidContent,
            format!("The file could not be written: {e}."),
            None,
        );
        String::new()
    });
    if text.len() > MAX_FILE_BYTES {
        found.error(
            ExportCode::TooLarge,
            format!(
                "The file would take {} KiB; a file takes at most {} KiB.",
                text.len().div_ceil(1024),
                MAX_FILE_BYTES / 1024
            ),
            None,
        );
    }
    preview(artifact, &chosen, rules_preview, found, text)
}

/// A Surface file holds one surface and nothing of a Pack: no context rules
/// and no inputs (bindings are refused where they are recorded).
fn check_kind(request: &ExportRequest, surfaces: usize, has_rules: bool, found: &mut Collected) {
    if request.kind != ArtifactKind::Surface {
        return;
    }
    let mut refuse = |why: &str| found.error(ExportCode::KindMismatch, why.into(), None);
    if surfaces > 1 {
        refuse("A Surface file holds one surface; export several as a Pack.");
    }
    if has_rules {
        refuse("A Surface file has no context rules; export a Pack to include them.");
    }
    if !request.inputs.is_empty() || !request.rule_inputs.is_empty() {
        refuse("A Surface file asks the importer for nothing; export a Pack to use inputs.");
    }
}

fn manifest(
    request: &ExportRequest,
    env: &Environment,
    chosen: &[Chosen<'_>],
    content: &ContentV1,
    dependencies: Vec<DependencyV1>,
    found: &mut Collected,
) -> ManifestV1 {
    let first = chosen.first().map(|c| c.surface.name.as_str());
    let words = words(&request.metadata, first, request.kind);
    let digest = content.digest().unwrap_or_else(|e| {
        found.error(
            ExportCode::InvalidContent,
            format!("The content could not be written: {e}."),
            None,
        );
        String::new()
    });
    let metadata = &request.metadata;
    ManifestV1 {
        id: words.id,
        name: words.name,
        version: words.version,
        description: metadata.description.clone().unwrap_or_default(),
        author: metadata.author.clone(),
        license: metadata.license.clone().unwrap_or_default(),
        created_with: CreatedWith {
            app: CREATED_WITH_APP.into(),
            version: env.app_version.clone(),
        },
        requirements: compute_requirements(content),
        tested_with: tested_with(env),
        dependencies,
        inputs: request.inputs.clone(),
        integrity: Integrity { content: digest },
    }
}

/// What an importer would refuse is an error here: the manifest and the
/// content each on their own (an action that runs things hiding a character
/// included), then, when nothing else is wrong, the checks across them
/// (which would repeat a kind or binding problem already said).
fn check_format(
    kind: ArtifactKind,
    manifest: &ManifestV1,
    content: &ContentV1,
    found: &mut Collected,
) {
    let manifest_ok = check_manifest(manifest)
        .map_err(|r| found.rejected(r))
        .is_ok();
    let content_ok = check_content(content)
        .and_then(|()| check_visible(content))
        .map_err(|r| found.rejected(r))
        .is_ok();
    if manifest_ok && content_ok && found.errors.is_empty() {
        if let Err(r) = check_cross(kind, manifest, content) {
            found.rejected(r);
        }
    }
}

fn preview(
    artifact: ArtifactV1,
    chosen: &[Chosen<'_>],
    context_rules: Vec<ExportedRule>,
    found: Collected,
    text: String,
) -> ExportResult {
    let surfaces = chosen
        .iter()
        .map(|c| ExportedSurface {
            local_id: c.surface.id.clone(),
            artifact_id: c.artifact_id.clone(),
            name: c.surface.name.clone(),
            pages: c.surface.pages.len(),
            controls: c.surface.pages.iter().map(|p| p.controls.len()).sum(),
        })
        .collect();
    let manifest = artifact.manifest;
    let preview = ExportPreview {
        file_name: file_name(&manifest.name, &manifest.version, artifact.kind),
        size_bytes: text.len(),
        content_digest: manifest.integrity.content.clone(),
        dependencies: manifest.dependencies.clone(),
        requirements: manifest.requirements,
        tested_with: manifest.tested_with.clone(),
        manifest,
        surfaces,
        context_rules,
        review: found.review,
        stripped: found.stripped,
        suggestions: found.suggestions,
        parameters: found.parameters,
        warnings: found.warnings,
        errors: found.errors,
    };
    let bytes = preview.errors.is_empty().then(|| text.into_bytes());
    ExportResult { preview, bytes }
}
