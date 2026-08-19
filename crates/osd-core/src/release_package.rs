//! Builds the versioned Happy Science research release package from validated mission state.

use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use crate::claim_passport::{ClaimPassport, ClaimStatus};
use crate::missions::{MissionCheck, MissionRecord};
use crate::runtime::workspace_dir;
use crate::Env;

const FORMAT: &str = "happy-science-research-release";
const MANIFEST_NAME: &str = "HAPPY-SCIENCE-RELEASE.json";
const RELEASE_DIR: &str = "releases";
const IMPORT_DIR: &str = "imports";
const MAX_PAYLOAD_BYTES: u64 = 512 * 1024 * 1024;
const MAX_MANIFEST_BYTES: u64 = 5 * 1024 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 5_000;
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReleaseFile {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReleaseManifest {
    pub format: String,
    pub schema_version: u32,
    pub fingerprint: String,
    pub created_at: u64,
    pub mission: MissionRecord,
    pub claim_passports: Vec<ClaimPassport>,
    pub files: Vec<ReleaseFile>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResearchRelease {
    pub schema_version: u32,
    pub path: String,
    pub file_name: String,
    pub fingerprint: String,
    pub payload_files: usize,
    pub payload_bytes: u64,
    pub claim_passports: usize,
    pub created_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReleaseVerification {
    pub schema_version: u32,
    pub path: String,
    pub valid: bool,
    pub fingerprint: Option<String>,
    pub mission_id: Option<String>,
    pub payload_files: usize,
    pub payload_bytes: u64,
    pub claim_passports: usize,
    pub issues: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReleaseImport {
    pub schema_version: u32,
    pub source_path: String,
    pub destination_path: String,
    pub fingerprint: String,
    pub mission_id: String,
    pub payload_files: usize,
    pub payload_bytes: u64,
    pub imported_at: u64,
}

struct HashingReader<R> {
    inner: R,
    hasher: Sha256,
}

impl<R> HashingReader<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            hasher: Sha256::new(),
        }
    }

    fn finish(self) -> String {
        format!("{:x}", self.hasher.finalize())
    }
}

impl<R: Read> Read for HashingReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buffer)?;
        self.hasher.update(&buffer[..read]);
        Ok(read)
    }
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn nonce() -> String {
    let mut bytes = [0_u8; 4];
    getrandom::fill(&mut bytes).expect("OS random source unavailable");
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn normalized_relative(value: &str) -> Result<(PathBuf, String), String> {
    let path = Path::new(value);
    if value.trim().is_empty() || path.is_absolute() {
        return Err(format!("release path must be workspace-relative: {value}"));
    }
    let mut normalized = PathBuf::new();
    let mut archive = Vec::new();
    for component in path.components() {
        let Component::Normal(part) = component else {
            return Err(format!(
                "release path contains an unsafe component: {value}"
            ));
        };
        let part = part
            .to_str()
            .ok_or_else(|| format!("release path is not UTF-8: {value}"))?;
        normalized.push(part);
        archive.push(part);
    }
    Ok((normalized, archive.join("/")))
}

fn resolved_file(root: &Path, value: &str) -> Result<(PathBuf, String, u64), String> {
    let (relative, archive) = normalized_relative(value)?;
    let canonical_root =
        fs::canonicalize(root).map_err(|error| format!("workspace cannot be resolved: {error}"))?;
    let canonical_file = fs::canonicalize(root.join(&relative))
        .map_err(|error| format!("release file cannot be resolved ({value}): {error}"))?;
    if !canonical_file.starts_with(&canonical_root) {
        return Err(format!(
            "release file resolves outside the workspace: {value}"
        ));
    }
    let metadata = canonical_file
        .metadata()
        .map_err(|error| format!("release file cannot be inspected ({value}): {error}"))?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Err(format!(
            "release file must be a non-empty regular file: {value}"
        ));
    }
    Ok((canonical_file, archive, metadata.len()))
}

fn fingerprint_field(hasher: &mut Sha256, value: &str) {
    hasher.update((value.len() as u64).to_le_bytes());
    hasher.update(value.as_bytes());
}

fn content_fingerprint(
    mission: &MissionRecord,
    passports: &[ClaimPassport],
    files: &[ReleaseFile],
) -> String {
    let mut hasher = Sha256::new();
    fingerprint_field(&mut hasher, FORMAT);
    fingerprint_field(&mut hasher, &SCHEMA_VERSION.to_string());
    fingerprint_field(&mut hasher, &mission.contract_version.to_string());
    fingerprint_field(&mut hasher, &mission.mission_id);
    fingerprint_field(
        &mut hasher,
        &serde_json::to_string(&mission.kind).expect("mission kind serializes"),
    );
    fingerprint_field(
        &mut hasher,
        &serde_json::to_string(&mission.rigor).expect("mission rigor serializes"),
    );
    for passport in passports {
        fingerprint_field(&mut hasher, &passport.claim_id);
        fingerprint_field(&mut hasher, &passport.fingerprint);
    }
    for file in files {
        fingerprint_field(&mut hasher, &file.path);
        fingerprint_field(&mut hasher, &file.bytes.to_string());
        fingerprint_field(&mut hasher, &file.sha256);
    }
    format!("{:x}", hasher.finalize())
}

fn sha256_valid(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn verification(path: &str) -> ReleaseVerification {
    ReleaseVerification {
        schema_version: SCHEMA_VERSION,
        path: path.into(),
        valid: false,
        fingerprint: None,
        mission_id: None,
        payload_files: 0,
        payload_bytes: 0,
        claim_passports: 0,
        issues: Vec::new(),
    }
}

fn inspect_at(
    root: &Path,
    path: &str,
) -> Result<(ReleaseVerification, Option<ReleaseManifest>), String> {
    let (archive_path, normalized_path, archive_bytes) = resolved_file(root, path)?;
    let mut result = verification(&normalized_path);
    if archive_bytes > MAX_PAYLOAD_BYTES + MAX_MANIFEST_BYTES {
        result.issues.push(format!(
            "archive exceeds the {} MiB limit",
            (MAX_PAYLOAD_BYTES + MAX_MANIFEST_BYTES) / 1024 / 1024
        ));
        return Ok((result, None));
    }

    let file = File::open(&archive_path)
        .map_err(|error| format!("release package could not be opened: {error}"))?;
    let mut zip = match zip::ZipArchive::new(file) {
        Ok(zip) => zip,
        Err(error) => {
            result
                .issues
                .push(format!("archive is not a readable ZIP package: {error}"));
            return Ok((result, None));
        }
    };
    if zip.len() > MAX_ARCHIVE_ENTRIES {
        result.issues.push(format!(
            "archive contains more than {MAX_ARCHIVE_ENTRIES} entries"
        ));
        return Ok((result, None));
    }

    let mut names = HashSet::new();
    let mut manifest_indices = Vec::new();
    let mut archive_entries = BTreeMap::<String, usize>::new();
    let mut declared_archive_bytes = 0_u64;
    for index in 0..zip.len() {
        let entry = zip
            .by_index(index)
            .map_err(|error| format!("release entry could not be inspected: {error}"))?;
        let name = entry.name().to_string();
        if !names.insert(name.clone()) {
            result
                .issues
                .push(format!("archive contains duplicate entry: {name}"));
            continue;
        }
        if name == MANIFEST_NAME {
            manifest_indices.push(index);
            continue;
        }
        match normalized_relative(&name) {
            Ok((_, normalized)) if normalized == name && name != MANIFEST_NAME => {}
            _ => {
                result
                    .issues
                    .push(format!("archive contains unsafe entry path: {name}"));
                continue;
            }
        }
        if entry.is_dir() {
            result
                .issues
                .push(format!("archive entry must be a regular file: {name}"));
            continue;
        }
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            result
                .issues
                .push(format!("archive entry must not be a symbolic link: {name}"));
            continue;
        }
        declared_archive_bytes = declared_archive_bytes.saturating_add(entry.size());
        archive_entries.insert(name, index);
    }
    if declared_archive_bytes > MAX_PAYLOAD_BYTES {
        result.issues.push(format!(
            "archive payload exceeds the {} MiB limit",
            MAX_PAYLOAD_BYTES / 1024 / 1024
        ));
    }
    if manifest_indices.len() != 1 {
        result.issues.push(format!(
            "archive must contain exactly one {MANIFEST_NAME} manifest"
        ));
        return Ok((result, None));
    }

    let manifest_index = manifest_indices[0];
    let manifest = {
        let mut entry = zip
            .by_index(manifest_index)
            .map_err(|error| format!("release manifest could not be opened: {error}"))?;
        if entry.is_dir() || entry.size() == 0 || entry.size() > MAX_MANIFEST_BYTES {
            result.issues.push(format!(
                "release manifest must be between 1 byte and {} MiB",
                MAX_MANIFEST_BYTES / 1024 / 1024
            ));
            return Ok((result, None));
        }
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry
            .by_ref()
            .take(MAX_MANIFEST_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| format!("release manifest could not be read: {error}"))?;
        match serde_json::from_slice::<ReleaseManifest>(&bytes) {
            Ok(manifest) => manifest,
            Err(error) => {
                result
                    .issues
                    .push(format!("release manifest is invalid: {error}"));
                return Ok((result, None));
            }
        }
    };

    result.fingerprint = Some(manifest.fingerprint.clone());
    result.mission_id = Some(manifest.mission.mission_id.clone());
    result.payload_files = manifest.files.len();
    result.claim_passports = manifest.claim_passports.len();
    if manifest.format != FORMAT {
        result.issues.push(format!(
            "manifest format must be {FORMAT}, found {}",
            manifest.format
        ));
    }
    if manifest.schema_version != SCHEMA_VERSION {
        result.issues.push(format!(
            "manifest schema version must be {SCHEMA_VERSION}, found {}",
            manifest.schema_version
        ));
    }
    if !sha256_valid(&manifest.fingerprint) {
        result
            .issues
            .push("manifest fingerprint must be 64 lowercase hexadecimal characters".into());
    }
    if !crate::missions::valid_mission_id(&manifest.mission.mission_id) {
        result.issues.push("manifest mission ID is invalid".into());
    }

    let mut expected = BTreeMap::<String, &ReleaseFile>::new();
    let mut expected_bytes = 0_u64;
    let mut previous_path: Option<&str> = None;
    for file in &manifest.files {
        if previous_path.is_some_and(|previous| previous >= file.path.as_str()) {
            result
                .issues
                .push("manifest files must be uniquely sorted by path".into());
        }
        previous_path = Some(&file.path);
        match normalized_relative(&file.path) {
            Ok((_, normalized)) if normalized == file.path && file.path != MANIFEST_NAME => {}
            _ => result
                .issues
                .push(format!("manifest contains unsafe file path: {}", file.path)),
        }
        if file.bytes == 0 {
            result
                .issues
                .push(format!("manifest file must not be empty: {}", file.path));
        }
        if !sha256_valid(&file.sha256) {
            result
                .issues
                .push(format!("manifest file hash is invalid: {}", file.path));
        }
        if expected.insert(file.path.clone(), file).is_some() {
            result
                .issues
                .push(format!("manifest contains duplicate file: {}", file.path));
        }
        expected_bytes = expected_bytes.saturating_add(file.bytes);
    }
    result.payload_bytes = expected_bytes;
    if expected_bytes > MAX_PAYLOAD_BYTES {
        result.issues.push(format!(
            "manifest payload exceeds the {} MiB limit",
            MAX_PAYLOAD_BYTES / 1024 / 1024
        ));
    }

    for name in archive_entries.keys() {
        if !expected.contains_key(name) {
            result
                .issues
                .push(format!("archive contains unmanifested payload: {name}"));
        }
    }
    for (path, file) in &expected {
        let Some(index) = archive_entries.get(path).copied() else {
            result
                .issues
                .push(format!("manifested payload is missing: {path}"));
            continue;
        };
        let entry = zip
            .by_index(index)
            .map_err(|error| format!("release payload could not be opened ({path}): {error}"))?;
        if entry.size() != file.bytes {
            result
                .issues
                .push(format!("payload size does not match manifest: {path}"));
        }
        let mut reader = HashingReader::new(entry);
        let copied = std::io::copy(
            &mut reader.by_ref().take(file.bytes + 1),
            &mut std::io::sink(),
        )
        .map_err(|error| format!("release payload could not be read ({path}): {error}"))?;
        let hash = reader.finish();
        if copied != file.bytes {
            result.issues.push(format!(
                "payload byte count does not match manifest: {path}"
            ));
        }
        if hash != file.sha256 {
            result
                .issues
                .push(format!("payload hash does not match manifest: {path}"));
        }
    }

    let mut claim_ids = HashSet::new();
    for passport in &manifest.claim_passports {
        if passport.schema_version != crate::claim_passport::SCHEMA_VERSION {
            result.issues.push(format!(
                "Claim Passport {} uses an unsupported schema version",
                passport.claim_id
            ));
        }
        if !claim_ids.insert(passport.claim_id.as_str()) || passport.claim_id.trim().is_empty() {
            result.issues.push(format!(
                "Claim Passport ID is empty or duplicated: {}",
                passport.claim_id
            ));
        }
        if !matches!(
            passport.status,
            ClaimStatus::Supported | ClaimStatus::Qualified
        ) {
            result.issues.push(format!(
                "Claim Passport {} is not cleared for release",
                passport.claim_id
            ));
        }
        if !sha256_valid(&passport.fingerprint) {
            result.issues.push(format!(
                "Claim Passport {} has an invalid fingerprint",
                passport.claim_id
            ));
        }
    }

    let fingerprint = content_fingerprint(
        &manifest.mission,
        &manifest.claim_passports,
        &manifest.files,
    );
    if fingerprint != manifest.fingerprint {
        result
            .issues
            .push("content fingerprint does not match the release manifest".into());
    }
    result.valid = result.issues.is_empty();
    Ok((result, Some(manifest)))
}

pub fn verify_at(root: &Path, path: &str) -> Result<ReleaseVerification, String> {
    inspect_at(root, path).map(|(verification, _)| verification)
}

pub fn import_at(root: &Path, path: &str) -> Result<ReleaseImport, String> {
    let (verified, manifest) = inspect_at(root, path)?;
    if !verified.valid {
        return Err(format!(
            "release verification failed: {}",
            verified.issues.join("; ")
        ));
    }
    let manifest = manifest.ok_or("release manifest is unavailable")?;
    let (source, source_path, _) = resolved_file(root, path)?;
    let import_root = root.join(IMPORT_DIR);
    fs::create_dir_all(&import_root)
        .map_err(|error| format!("release import directory could not be created: {error}"))?;
    let nonce = nonce();
    let directory_name = format!(
        "happy-science-{}-{}-{}",
        manifest.mission.mission_id,
        &manifest.fingerprint[..12],
        nonce
    );
    let temp = import_root.join(format!(".pending-{nonce}"));
    let destination = import_root.join(&directory_name);
    fs::create_dir(&temp).map_err(|error| {
        format!("release import staging directory could not be created: {error}")
    })?;

    let result = (|| {
        let input = File::open(source)
            .map_err(|error| format!("release package could not be reopened: {error}"))?;
        let mut zip = zip::ZipArchive::new(input)
            .map_err(|error| format!("release package changed before import: {error}"))?;
        for expected in &manifest.files {
            let (relative, _) = normalized_relative(&expected.path)?;
            let output_path = temp.join(relative);
            if let Some(parent) = output_path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|error| format!("release import directory failed: {error}"))?;
            }
            let entry = zip.by_name(&expected.path).map_err(|error| {
                format!(
                    "release payload changed before import ({}): {error}",
                    expected.path
                )
            })?;
            let mut reader = HashingReader::new(entry);
            let mut output = File::create_new(&output_path)
                .map_err(|error| format!("release import file could not be created: {error}"))?;
            let copied = std::io::copy(&mut reader.by_ref().take(expected.bytes + 1), &mut output)
                .map_err(|error| format!("release payload could not be imported: {error}"))?;
            output
                .sync_all()
                .map_err(|error| format!("release import file could not be synced: {error}"))?;
            if copied != expected.bytes || reader.finish() != expected.sha256 {
                return Err(format!(
                    "release payload changed before import: {}",
                    expected.path
                ));
            }
        }

        let mut archived_manifest = zip
            .by_name(MANIFEST_NAME)
            .map_err(|error| format!("release manifest changed before import: {error}"))?;
        let mut manifest_bytes = Vec::with_capacity(archived_manifest.size() as usize);
        archived_manifest
            .by_ref()
            .take(MAX_MANIFEST_BYTES + 1)
            .read_to_end(&mut manifest_bytes)
            .map_err(|error| format!("release manifest could not be imported: {error}"))?;
        let current_manifest: ReleaseManifest = serde_json::from_slice(&manifest_bytes)
            .map_err(|error| format!("release manifest changed before import: {error}"))?;
        if current_manifest != manifest {
            return Err("release manifest changed before import".into());
        }
        fs::write(temp.join(MANIFEST_NAME), &manifest_bytes)
            .map_err(|error| format!("release manifest could not be written: {error}"))?;
        fs::rename(&temp, &destination)
            .map_err(|error| format!("release import could not be finalized: {error}"))?;
        Ok(ReleaseImport {
            schema_version: SCHEMA_VERSION,
            source_path,
            destination_path: format!("{IMPORT_DIR}/{directory_name}"),
            fingerprint: manifest.fingerprint.clone(),
            mission_id: manifest.mission.mission_id.clone(),
            payload_files: manifest.files.len(),
            payload_bytes: manifest.files.iter().map(|file| file.bytes).sum(),
            imported_at: now(),
        })
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(temp);
    }
    result
}

fn release_paths(check: &MissionCheck) -> BTreeSet<String> {
    let mut paths = check
        .mission
        .deliverables
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    if let Some(sources) = &check.source_manifest {
        paths.extend(
            sources
                .entries
                .iter()
                .map(|source| source.snapshot_path.clone()),
        );
    }
    if let Some(review) = &check.evidence_review {
        paths.insert(review.path.clone());
    }
    if check.decision_log.records > 0 {
        paths.insert(check.decision_log.path.clone());
    }
    if let Some(corpus) = &check.literature_corpus {
        if corpus.records > 0 {
            paths.insert(corpus.path.clone());
        }
    }
    paths
}

fn releasable(check: &MissionCheck) -> Result<Vec<ClaimPassport>, String> {
    if !check.ready_for_review || !check.issues.is_empty() {
        return Err("mission must pass its deterministic review gate before release".into());
    }
    let passports = check.claim_passports.clone().unwrap_or_default();
    let unresolved = passports
        .iter()
        .filter(|passport| {
            !matches!(
                passport.status,
                ClaimStatus::Supported | ClaimStatus::Qualified
            )
        })
        .count();
    if unresolved > 0 {
        return Err(format!(
            "{unresolved} claim passport(s) remain unresolved; only supported or qualified claims can be released"
        ));
    }
    if let Some(review) = &check.evidence_review {
        if !review.complete() {
            return Err(
                "every evidence relation must receive a final human decision before release".into(),
            );
        }
    }
    Ok(passports)
}

pub fn create_at(root: &Path, check: &MissionCheck) -> Result<ResearchRelease, String> {
    let passports = releasable(check)?;
    let paths = release_paths(check);
    let mut resolved = Vec::with_capacity(paths.len());
    let mut expected_bytes = 0_u64;
    for path in paths {
        let file = resolved_file(root, &path)?;
        expected_bytes = expected_bytes
            .checked_add(file.2)
            .ok_or("release payload size overflow")?;
        if expected_bytes > MAX_PAYLOAD_BYTES {
            return Err(format!(
                "release payload exceeds the {} MiB limit",
                MAX_PAYLOAD_BYTES / 1024 / 1024
            ));
        }
        resolved.push(file);
    }

    let release_dir = root.join(RELEASE_DIR);
    fs::create_dir_all(&release_dir)
        .map_err(|error| format!("release directory could not be created: {error}"))?;
    let created_at = now();
    let nonce = nonce();
    let temp_path = release_dir.join(format!(".pending-{nonce}.zip"));
    let result = (|| {
        let output = File::create_new(&temp_path)
            .map_err(|error| format!("release package could not be created: {error}"))?;
        let mut zip = zip::ZipWriter::new(output);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(0o644);
        let mut files = Vec::with_capacity(resolved.len());
        let mut payload_bytes = 0_u64;
        for (full_path, archive_path, _) in &resolved {
            zip.start_file(archive_path, options)
                .map_err(|error| format!("release entry could not be started: {error}"))?;
            let input = File::open(full_path)
                .map_err(|error| format!("release file could not be opened: {error}"))?;
            let mut input = HashingReader::new(input);
            let bytes = std::io::copy(&mut input, &mut zip)
                .map_err(|error| format!("release file could not be archived: {error}"))?;
            payload_bytes += bytes;
            files.push(ReleaseFile {
                path: archive_path.clone(),
                bytes,
                sha256: input.finish(),
            });
        }
        let fingerprint = content_fingerprint(&check.mission, &passports, &files);
        let manifest = ReleaseManifest {
            format: FORMAT.into(),
            schema_version: SCHEMA_VERSION,
            fingerprint: fingerprint.clone(),
            created_at,
            mission: check.mission.clone(),
            claim_passports: passports.clone(),
            files,
        };
        zip.start_file(MANIFEST_NAME, options)
            .map_err(|error| format!("release manifest could not be started: {error}"))?;
        let manifest = serde_json::to_vec_pretty(&manifest)
            .map_err(|error| format!("release manifest could not be serialized: {error}"))?;
        zip.write_all(&manifest)
            .map_err(|error| format!("release manifest could not be archived: {error}"))?;
        zip.finish()
            .map_err(|error| format!("release package could not be finalized: {error}"))?;

        let file_name = format!(
            "happy-science-{}-{}-{}-{}.zip",
            check.mission.mission_id,
            created_at,
            &fingerprint[..12],
            nonce
        );
        let final_path = release_dir.join(&file_name);
        fs::rename(&temp_path, &final_path)
            .map_err(|error| format!("release package could not be sealed: {error}"))?;
        Ok(ResearchRelease {
            schema_version: SCHEMA_VERSION,
            path: format!("{RELEASE_DIR}/{file_name}"),
            file_name,
            fingerprint,
            payload_files: resolved.len(),
            payload_bytes,
            claim_passports: passports.len(),
            created_at,
        })
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp_path);
    }
    result
}

/// Validate the latest mission state, then seal only its declared research payload.
pub fn create(env: &Env, mission_id: &str) -> Result<ResearchRelease, String> {
    let check = crate::missions::check_mission(env, mission_id)?;
    let root = workspace_dir(env)?;
    create_at(&root, &check)
}

/// Independently verify a release package already stored inside the workspace.
pub fn verify(env: &Env, path: &str) -> Result<ReleaseVerification, String> {
    let root = workspace_dir(env)?;
    verify_at(&root, path)
}

/// Verify and extract a release into a new, isolated workspace import directory.
pub fn import(env: &Env, path: &str) -> Result<ReleaseImport, String> {
    let root = workspace_dir(env)?;
    import_at(&root, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::missions::{GateStatus, MissionKind, MissionStatus, QualityGate, RigorLevel};

    fn root() -> PathBuf {
        let path = std::env::temp_dir().join(format!("happy-science-release-{}", nonce()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn ready_check(path: &str) -> MissionCheck {
        MissionCheck {
            mission: MissionRecord {
                contract_version: 3,
                mission_id: "hsm_0123456789abcdef".into(),
                kind: MissionKind::StudyLaunch,
                rigor: RigorLevel::Research,
                status: MissionStatus::Running,
                status_reason: None,
                session_id: Some("ses_1".into()),
                deliverables: vec![path.into()],
                quality_gates: vec![QualityGate {
                    key: "deliverables-present".into(),
                    status: GateStatus::Passed,
                }],
                created_at: 1,
                updated_at: 2,
            },
            ready_for_review: true,
            missing_deliverables: Vec::new(),
            evidence_ledger: None,
            source_manifest: None,
            evidence_review: None,
            claim_passports: None,
            literature_corpus: None,
            protocol: None,
            decision_log: crate::decisions::DecisionLogCheck {
                path: crate::decisions::log_path("hsm_0123456789abcdef"),
                records: 0,
                decisions: Vec::new(),
                issues: Vec::new(),
            },
            issues: Vec::new(),
            next_actions: Vec::new(),
            run_integrity: None,
        }
    }

    #[test]
    fn creates_a_standard_zip_with_a_versioned_manifest_and_content_hashes() {
        let root = root();
        fs::create_dir_all(root.join("research")).unwrap();
        fs::write(root.join("research/protocol.md"), "# Protocol\n").unwrap();

        let release = create_at(&root, &ready_check("research/protocol.md")).unwrap();
        assert_eq!(release.payload_files, 1);
        assert_eq!(release.claim_passports, 0);
        assert_eq!(release.fingerprint.len(), 64);

        let file = File::open(root.join(&release.path)).unwrap();
        let mut zip = zip::ZipArchive::new(file).unwrap();
        assert_eq!(zip.len(), 2);
        assert_eq!(
            zip.by_name("research/protocol.md").unwrap().size(),
            "# Protocol\n".len() as u64
        );
        let manifest: ReleaseManifest =
            serde_json::from_reader(zip.by_name(MANIFEST_NAME).unwrap()).unwrap();
        assert_eq!(manifest.format, FORMAT);
        assert_eq!(manifest.fingerprint, release.fingerprint);
        assert_eq!(manifest.files[0].path, "research/protocol.md");
        assert_eq!(manifest.files[0].sha256.len(), 64);

        let mut later_check = ready_check("research/protocol.md");
        later_check.mission.updated_at = 99;
        let later = create_at(&root, &later_check).unwrap();
        assert_eq!(later.fingerprint, release.fingerprint);
        assert_ne!(later.path, release.path);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_workspace_escape_and_unresolved_claims() {
        let root = root();
        let escape = ready_check("../outside.txt");
        assert!(create_at(&root, &escape)
            .unwrap_err()
            .contains("unsafe component"));

        let mut unresolved = ready_check("research/protocol.md");
        fs::create_dir_all(root.join("research")).unwrap();
        fs::write(root.join("research/protocol.md"), "content").unwrap();
        unresolved.claim_passports = Some(vec![ClaimPassport {
            schema_version: 1,
            claim_id: "cl_1".into(),
            claim: "A claim".into(),
            status: ClaimStatus::Contested,
            supports: 1,
            contradicts: 1,
            qualifies: 0,
            accepted: 2,
            rejected: 0,
            needs_review: 0,
            unreviewed: 0,
            source_count: 2,
            verified_sources: 2,
            fingerprint: "a".repeat(64),
        }]);
        assert!(create_at(&root, &unresolved)
            .unwrap_err()
            .contains("remain unresolved"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn independently_verifies_and_imports_without_overwriting() {
        let root = root();
        fs::create_dir_all(root.join("research")).unwrap();
        fs::write(root.join("research/protocol.md"), "# Protocol\n").unwrap();
        let release = create_at(&root, &ready_check("research/protocol.md")).unwrap();

        let verified = verify_at(&root, &release.path).unwrap();
        assert!(verified.valid, "{:?}", verified.issues);
        assert_eq!(
            verified.fingerprint.as_deref(),
            Some(release.fingerprint.as_str())
        );
        assert_eq!(verified.payload_files, 1);

        let first = import_at(&root, &release.path).unwrap();
        let second = import_at(&root, &release.path).unwrap();
        assert_ne!(first.destination_path, second.destination_path);
        assert_eq!(
            fs::read_to_string(
                root.join(&first.destination_path)
                    .join("research/protocol.md")
            )
            .unwrap(),
            "# Protocol\n"
        );
        assert!(root
            .join(&first.destination_path)
            .join(MANIFEST_NAME)
            .is_file());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reports_invalid_zip_and_rejects_unsafe_entries() {
        let root = root();
        fs::create_dir_all(root.join(RELEASE_DIR)).unwrap();
        fs::write(root.join(RELEASE_DIR).join("invalid.zip"), "not a zip").unwrap();
        let invalid = verify_at(&root, "releases/invalid.zip").unwrap();
        assert!(!invalid.valid);
        assert!(invalid.issues[0].contains("readable ZIP"));

        let unsafe_path = root.join(RELEASE_DIR).join("unsafe.zip");
        let output = File::create(&unsafe_path).unwrap();
        let mut zip = zip::ZipWriter::new(output);
        zip.start_file("../escape.txt", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"escape").unwrap();
        zip.start_file(MANIFEST_NAME, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"{}").unwrap();
        zip.finish().unwrap();

        let unsafe_release = verify_at(&root, "releases/unsafe.zip").unwrap();
        assert!(!unsafe_release.valid);
        assert!(unsafe_release
            .issues
            .iter()
            .any(|issue| issue.contains("unsafe entry path")));
        assert!(import_at(&root, "releases/unsafe.zip")
            .unwrap_err()
            .contains("verification failed"));
        assert!(!root.join("escape.txt").exists());

        let _ = fs::remove_dir_all(root);
    }
}
