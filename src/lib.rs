use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Seek, Write};
use std::path::{Component, Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, anyhow, bail};
use apk_info_axml::{ARSC, AXML};
use chrono::Local;
use regex::Regex;
use serde::Deserialize;
use tempfile::NamedTempFile;
use walkdir::WalkDir;
use zip::CompressionMethod;
use zip::read::ZipArchive;
use zip::write::{SimpleFileOptions, ZipWriter};

const LARK_EXTENSION: &str = "lark";
const DEFAULT_RULES_FILE: &str = "copy.json";
const LOG_FILE_NAME: &str = "lark-pack-tool.log";
const SDCARD_PREFIX: &str = "/sdcard/";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CopyConfig {
    #[serde(default)]
    launch_package: String,
    #[serde(default)]
    wait_seconds: i64,
    #[serde(default)]
    description: Option<String>,
    rules: Option<Vec<CopyRule>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CopyRule {
    #[serde(default)]
    source: String,
    #[serde(default)]
    device_dest: String,
}

#[derive(Debug)]
struct PackageValidation {
    package_name: String,
    version_name: String,
    rules_file: String,
    apk_file: String,
}

#[derive(Debug)]
struct ApkPackageInfo {
    package_name: String,
    version_name: String,
}

#[derive(Debug)]
struct ExistingBackup {
    original_path: PathBuf,
    backup_path: PathBuf,
    is_directory: bool,
}

pub fn check_package(input_path: &Path, ignore_uncovered: bool) -> Result<String> {
    if input_path.is_dir() {
        let package_name = directory_name(input_path)?;
        let package = validate_package_directory(input_path, &package_name, ignore_uncovered)?;
        return Ok(format!(
            "OK: valid package directory: {}",
            package.package_name
        ));
    }
    if is_lark_file(input_path) {
        let package = validate_archive_file(input_path, ignore_uncovered)?;
        return Ok(format!("OK: valid .lark package: {}", package.package_name));
    }
    bail!(
        "input must be an existing directory or .lark file: {}",
        input_path.display()
    )
}

pub fn pack_directory(package_directory: &Path, ignore_uncovered: bool) -> Result<String> {
    let started = Instant::now();
    let package_name = directory_name(package_directory)?;
    let package = validate_package_directory(package_directory, &package_name, ignore_uncovered)?;
    let parent = package_directory.parent().ok_or_else(|| {
        anyhow!(
            "package directory must have a parent: {}",
            package_directory.display()
        )
    })?;
    let archive_name = expected_archive_name(&package);
    let output_path = parent.join(format!("{archive_name}.{LARK_EXTENSION}"));
    let temp_path = parent.join(format!(".{archive_name}.{}.tmp", std::process::id()));

    let result = (|| {
        write_stored_archive(&temp_path, package_directory, &package)?;
        let backup = backup_existing_path(&output_path)?;
        if let Err(error) = fs::rename(&temp_path, &output_path) {
            restore_backup(backup.as_ref())?;
            return Err(error)
                .with_context(|| format!("failed to move package to {}", output_path.display()));
        }
        Ok((backup, output_path))
    })();

    if temp_path.exists() {
        let _ = fs::remove_file(&temp_path);
    }

    let (backup, output_path) = result?;
    Ok(format!(
        "{}OK: {} (elapsed {})",
        backup_message(backup.as_ref()),
        output_path.display(),
        format_elapsed(started.elapsed())
    ))
}

pub fn unpack_archive(archive_path: &Path, ignore_uncovered: bool) -> Result<String> {
    let started = Instant::now();
    let package = validate_archive_file(archive_path, ignore_uncovered)?;
    let destination = archive_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(&package.package_name);
    let backup = backup_existing_path(&destination)?;

    let result = (|| {
        fs::create_dir_all(&destination)
            .with_context(|| format!("failed to create {}", destination.display()))?;
        let file = File::open(archive_path)?;
        let mut archive = ZipArchive::new(BufReader::new(file))?;
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index)?;
            if entry.is_dir() {
                continue;
            }
            let target = safe_destination_path(&destination, entry.name())?;
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut output = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&target)
                .with_context(|| {
                    format!(
                        "archive entry collides with an existing output path: {}",
                        entry.name()
                    )
                })?;
            std::io::copy(&mut entry, &mut output)?;
        }
        Ok(())
    })();

    if let Err(error) = result {
        if destination.exists() {
            fs::remove_dir_all(&destination).with_context(|| {
                format!("failed to remove partial output {}", destination.display())
            })?;
        }
        restore_backup(backup.as_ref())?;
        return Err(error);
    }

    Ok(format!(
        "{}OK: {} (elapsed {})",
        backup_message(backup.as_ref()),
        destination.display(),
        format_elapsed(started.elapsed())
    ))
}

pub fn write_failure_log(operation: &str, input: &Path, error: &str) -> Result<()> {
    let executable = std::env::current_exe().context("failed to locate current executable")?;
    let directory = executable.parent().unwrap_or_else(|| Path::new("."));
    let log_path = directory.join(LOG_FILE_NAME);
    let message = format!(
        "[{}] {operation} failed\nInput: {}\nError: {error}\n\n",
        Local::now().format("%Y-%m-%d %H:%M:%S %:z"),
        input.display()
    );
    let mut options = fs::OpenOptions::new();
    options.create(true).append(true);
    options
        .open(&log_path)
        .and_then(|mut file| file.write_all(message.as_bytes()))
        .with_context(|| format!("failed to write {}", log_path.display()))?;
    eprintln!("Log: {}", log_path.display());
    Ok(())
}

fn validate_package_directory(
    package_directory: &Path,
    expected_package_name: &str,
    ignore_uncovered: bool,
) -> Result<PackageValidation> {
    let files = enumerate_files(package_directory)?;
    let apk_files = files_with_extension(&files, "apk");
    let obb_files = files_with_extension(&files, "obb");
    let xmpk_files = files_with_extension(&files, "xmpk");
    let mut package = validate_package_files(
        Some(expected_package_name),
        &apk_files,
        &obb_files,
        &xmpk_files,
        false,
        |apk| read_apk_info(&package_directory.join(entry_to_path(apk))),
    )?;

    let rules_file = find_rules_json_file(&files, &package.package_name)?;
    let config = read_copy_config(
        &package_directory.join(entry_to_path(&rules_file)),
        &rules_file,
    )?;
    validate_copy_config(&config, &package.package_name, &rules_file)?;
    if !ignore_uncovered {
        validate_resource_coverage(
            &files,
            &apk_files,
            &obb_files,
            &xmpk_files,
            &rules_file,
            &config,
        )?;
    }
    package.rules_file = rules_file;
    Ok(package)
}

fn validate_archive_file(archive_path: &Path, ignore_uncovered: bool) -> Result<PackageValidation> {
    let archive_name = archive_path
        .file_stem()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| anyhow!("invalid .lark file name: {}", archive_path.display()))?;
    let file = File::open(archive_path)
        .with_context(|| format!("failed to open {}", archive_path.display()))?;
    let mut archive = ZipArchive::new(BufReader::new(file))
        .with_context(|| format!("invalid ZIP archive: {}", archive_path.display()))?;
    validate_archive_entries(&mut archive)?;
    validate_archive_package(&mut archive, archive_name, ignore_uncovered)
}

fn validate_archive_package<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    archive_name: &str,
    ignore_uncovered: bool,
) -> Result<PackageValidation> {
    let files = archive_file_names(archive)?;
    let apk_files = files_with_extension(&files, "apk");
    let obb_files = files_with_extension(&files, "obb");
    let xmpk_files = files_with_extension(&files, "xmpk");

    let mut package = validate_package_files(
        None,
        &apk_files,
        &obb_files,
        &xmpk_files,
        true,
        |apk_name| read_apk_info_from_archive(archive, apk_name),
    )?;
    if archive_name != expected_archive_name(&package) {
        bail!(
            ".lark file must be named '{}.{}'.",
            expected_archive_name(&package),
            LARK_EXTENSION
        );
    }

    let rules_file = find_rules_json_file(&files, &package.package_name)?;
    let config = {
        let mut entry = archive.by_name(&rules_file)?;
        read_copy_config_from_reader(&mut entry, &rules_file)?
    };
    validate_copy_config(&config, &package.package_name, &rules_file)?;
    if !ignore_uncovered {
        validate_resource_coverage(
            &files,
            &apk_files,
            &obb_files,
            &xmpk_files,
            &rules_file,
            &config,
        )?;
    }
    package.rules_file = rules_file;
    Ok(package)
}

#[allow(clippy::too_many_arguments)]
fn validate_package_files<F>(
    expected_package_name: Option<&str>,
    apk_files: &[String],
    obb_files: &[String],
    xmpk_files: &[String],
    require_canonical_apk_name: bool,
    mut read_apk: F,
) -> Result<PackageValidation>
where
    F: FnMut(&str) -> Result<ApkPackageInfo>,
{
    if apk_files.len() != 1 {
        bail!(
            "package must contain exactly one .apk file, found {}",
            apk_files.len()
        );
    }
    if obb_files.len() > 1 {
        bail!(
            "package can contain at most one .obb file, found {}",
            obb_files.len()
        );
    }
    if xmpk_files.len() > 1 {
        bail!(
            "package can contain at most one .xmpk file, found {}",
            xmpk_files.len()
        );
    }

    let apk_path = &apk_files[0];
    let apk = read_apk(apk_path)?;
    if apk.package_name.trim().is_empty() {
        bail!("APK package name is empty: {apk_path}");
    }
    if let Some(expected) = expected_package_name
        && apk.package_name != expected
    {
        bail!(
            "APK package name '{}' must match package name '{}'",
            apk.package_name,
            expected
        );
    }
    if apk.version_name.trim().is_empty() {
        bail!("APK versionName is empty: {apk_path}");
    }
    if contains_invalid_filename_character(&apk.version_name) {
        bail!(
            "APK versionName contains characters that cannot be used in a .lark file name: {}",
            apk.version_name
        );
    }

    let expected_apk_name = format!("{}.apk", apk.package_name);
    if require_canonical_apk_name && apk_path != &expected_apk_name {
        bail!("APK must be at package root and named '{expected_apk_name}'");
    }
    if let Some(obb) = obb_files.first() {
        validate_obb_file_name(obb, &apk.package_name)?;
    }
    if let Some(xmpk) = xmpk_files.first() {
        validate_xmpk_file_name(xmpk, &apk.package_name)?;
    }

    Ok(PackageValidation {
        package_name: apk.package_name,
        version_name: apk.version_name,
        rules_file: String::new(),
        apk_file: apk_path.clone(),
    })
}

fn read_apk_info(path: &Path) -> Result<ApkPackageInfo> {
    let file =
        File::open(path).with_context(|| format!("failed to open APK {}", path.display()))?;
    read_apk_info_from_reader(BufReader::new(file), &path.display().to_string())
}

fn read_apk_info_from_reader<R: Read + Seek>(
    reader: R,
    source_name: &str,
) -> Result<ApkPackageInfo> {
    let mut archive = ZipArchive::new(reader)
        .with_context(|| format!("failed to open APK ZIP: {source_name}"))?;
    let manifest = read_zip_entry(&mut archive, "AndroidManifest.xml")
        .with_context(|| format!("failed to read AndroidManifest.xml from {source_name}"))?;
    let resources = match archive.by_name("resources.arsc") {
        Ok(mut entry) => {
            let mut bytes = Vec::with_capacity(entry.size().try_into().unwrap_or(0));
            entry.read_to_end(&mut bytes)?;
            Some(bytes)
        }
        Err(zip::result::ZipError::FileNotFound) => None,
        Err(error) => return Err(error.into()),
    };
    let arsc = resources
        .as_deref()
        .map(|bytes| ARSC::new(&mut &bytes[..]))
        .transpose()
        .with_context(|| format!("failed to parse resources.arsc from {source_name}"))?;
    let axml = AXML::new(&mut &manifest[..], arsc.as_ref())
        .with_context(|| format!("failed to parse AndroidManifest.xml from {source_name}"))?;
    let package_name = axml
        .get_attribute_value("manifest", "package", arsc.as_ref())
        .ok_or_else(|| anyhow!("APK package name is missing: {source_name}"))?;
    let version_name = axml
        .get_attribute_value("manifest", "versionName", arsc.as_ref())
        .ok_or_else(|| anyhow!("APK versionName is missing: {source_name}"))?;
    Ok(ApkPackageInfo {
        package_name,
        version_name,
    })
}

fn read_apk_info_from_archive<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    apk_name: &str,
) -> Result<ApkPackageInfo> {
    let mut entry = archive
        .by_name(apk_name)
        .with_context(|| format!("APK entry not found: {apk_name}"))?;
    let mut temporary = NamedTempFile::new().context("failed to create temporary APK file")?;
    std::io::copy(&mut entry, &mut temporary)
        .with_context(|| format!("failed to stream APK entry: {apk_name}"))?;
    temporary.flush()?;
    read_apk_info_from_reader(temporary.reopen()?, apk_name)
}

fn read_zip_entry<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    entry_name: &str,
) -> Result<Vec<u8>> {
    let mut entry = archive.by_name(entry_name)?;
    let mut bytes = Vec::with_capacity(entry.size().try_into().unwrap_or(0));
    entry.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn validate_obb_file_name(entry_name: &str, package_name: &str) -> Result<()> {
    let pattern = Regex::new(&format!(
        r"^main\.[1-9][0-9]*\.{}\.obb$",
        regex::escape(package_name)
    ))?;
    if !pattern.is_match(entry_name) {
        bail!(
            "OBB must be at package root and match 'main.<number>.{package_name}.obb': {entry_name}"
        );
    }
    Ok(())
}

fn validate_xmpk_file_name(entry_name: &str, package_name: &str) -> Result<()> {
    let product_name = package_name
        .rsplit_once('.')
        .map_or(package_name, |(_, product)| product);
    if !product_name
        .chars()
        .all(|character| character.is_ascii_alphabetic())
    {
        bail!(
            "package product name must contain only English letters to use .xmpk: {product_name}"
        );
    }
    let expected_name = format!("{product_name}.xmpk");
    if entry_name != expected_name {
        bail!("XMPK must be at package root and named '{expected_name}'");
    }
    Ok(())
}

fn find_rules_json_file(files: &[String], package_name: &str) -> Result<String> {
    let allowed = allowed_rules_file_names(package_name);
    let mut matches: Vec<_> = files
        .iter()
        .filter(|file| !file.contains('/') && allowed.contains(file.as_str()))
        .cloned()
        .collect();
    matches.sort();
    match matches.as_slice() {
        [only] => Ok(only.clone()),
        [] => {
            let mut names: Vec<_> = allowed.into_iter().collect();
            names.sort();
            bail!(
                "missing rules JSON at package root; expected one of: {}",
                names.join(", ")
            )
        }
        _ => bail!(
            "package must contain exactly one rules JSON at package root, found {}: {}",
            matches.len(),
            matches.join(", ")
        ),
    }
}

fn allowed_rules_file_names(package_name: &str) -> HashSet<String> {
    [
        DEFAULT_RULES_FILE.to_owned(),
        "main.json".to_owned(),
        "index.json".to_owned(),
        "manifest.json".to_owned(),
        format!("{package_name}.json"),
    ]
    .into_iter()
    .collect()
}

fn read_copy_config(path: &Path, source_name: &str) -> Result<CopyConfig> {
    let file =
        File::open(path).with_context(|| format!("missing rules JSON: {}", path.display()))?;
    read_copy_config_from_reader(BufReader::new(file), source_name)
}

fn read_copy_config_from_reader<R: Read>(mut reader: R, source_name: &str) -> Result<CopyConfig> {
    let mut json = String::new();
    reader
        .read_to_string(&mut json)
        .with_context(|| format!("failed to read rules JSON: {source_name}"))?;
    serde_json::from_str(&json).map_err(|error| {
        let hint = if contains_json_smart_punctuation(&json) {
            " Use standard JSON punctuation: property names must use ASCII double quotes (\") and separators must use colon (:)."
        } else {
            ""
        };
        anyhow!("invalid rules JSON in {source_name}: {error}.{hint}")
    })
}

fn validate_copy_config(config: &CopyConfig, package_name: &str, source_name: &str) -> Result<()> {
    if config.launch_package.trim().is_empty() {
        bail!("{source_name} launchPackage is required");
    }
    if config.launch_package != package_name {
        bail!(
            "{source_name} launchPackage '{}' must match APK package name '{package_name}'",
            config.launch_package
        );
    }
    if config.wait_seconds < 0 {
        bail!("{source_name} waitSeconds cannot be negative");
    }
    let rules = config
        .rules
        .as_ref()
        .ok_or_else(|| anyhow!("{source_name} rules is required"))?;
    for rule in rules {
        validate_copy_rule(rule, source_name)?;
    }
    let _ = &config.description;
    Ok(())
}

fn validate_copy_rule(rule: &CopyRule, source_name: &str) -> Result<()> {
    if rule.source.trim().is_empty() {
        bail!("{source_name} rule source is required");
    }
    if is_rooted_path(&rule.source) || contains_parent_traversal(&rule.source) {
        bail!(
            "{source_name} rule source must be a relative path pattern without '..': {}",
            rule.source
        );
    }
    if !rule.device_dest.starts_with(SDCARD_PREFIX) {
        bail!(
            "{source_name} rule deviceDest must start with {SDCARD_PREFIX}: {}",
            rule.device_dest
        );
    }
    Ok(())
}

fn validate_resource_coverage(
    files: &[String],
    apk_files: &[String],
    obb_files: &[String],
    xmpk_files: &[String],
    rules_file: &str,
    config: &CopyConfig,
) -> Result<()> {
    let mut known = HashSet::from([rules_file.to_owned()]);
    known.extend(apk_files.iter().cloned());
    known.extend(obb_files.iter().cloned());
    known.extend(xmpk_files.iter().cloned());
    let rules = config.rules.as_ref().expect("rules were validated");

    let mut uncovered: Vec<_> = files
        .iter()
        .filter(|file| !known.contains(*file))
        .filter(|file| !rules.iter().any(|rule| glob_matches(&rule.source, file)))
        .cloned()
        .collect();
    uncovered.sort();
    if !uncovered.is_empty() {
        bail!(
            "package contains files not covered by rules in {rules_file}: {}",
            uncovered.join(", ")
        );
    }
    Ok(())
}

fn glob_matches(pattern: &str, path: &str) -> bool {
    let pattern = normalize_entry_name(pattern);
    let path = normalize_entry_name(path);
    let mut expression = String::from("^");
    let characters: Vec<_> = pattern.chars().collect();
    let mut index = 0;
    while index < characters.len() {
        match characters[index] {
            '*' if characters.get(index + 1) == Some(&'*') => {
                if characters.get(index + 2) == Some(&'/') {
                    expression.push_str("(?:.*/)?");
                    index += 3;
                } else {
                    expression.push_str(".*");
                    index += 2;
                }
            }
            '*' => {
                expression.push_str("[^/]*");
                index += 1;
            }
            '?' => {
                expression.push_str("[^/]");
                index += 1;
            }
            character => {
                expression.push_str(&regex::escape(&character.to_string()));
                index += 1;
            }
        }
    }
    expression.push('$');
    Regex::new(&expression).is_ok_and(|regex| regex.is_match(&path))
}

fn validate_archive_entries<R: Read + Seek>(archive: &mut ZipArchive<R>) -> Result<()> {
    let mut names = HashSet::new();
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        let normalized = normalize_entry_name(entry.name());
        if normalized.trim().is_empty() {
            continue;
        }
        if is_unsafe_entry_name(entry.name()) {
            bail!("unsafe archive entry path: {}", entry.name());
        }
        if !entry.is_dir() && !names.insert(normalized) {
            bail!("duplicate archive entry path: {}", entry.name());
        }
    }
    Ok(())
}

fn archive_file_names<R: Read + Seek>(archive: &mut ZipArchive<R>) -> Result<Vec<String>> {
    let mut files = Vec::new();
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        if !entry.is_dir() {
            files.push(normalize_entry_name(entry.name()));
        }
    }
    Ok(files)
}

fn write_stored_archive(
    output_path: &Path,
    package_directory: &Path,
    package: &PackageValidation,
) -> Result<()> {
    let output = File::create_new(output_path)
        .with_context(|| format!("failed to create {}", output_path.display()))?;
    let mut archive = ZipWriter::new(BufWriter::new(output));
    let files = enumerate_file_paths(package_directory)?;
    let mut buffer = vec![0_u8; 1024 * 1024];

    for source in files {
        let relative = source.strip_prefix(package_directory)?;
        let entry_name = normalize_entry_name(&relative.to_string_lossy());
        let packed_name = packed_entry_name(&entry_name, package);
        let size = source.metadata()?.len();
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Stored)
            .large_file(size > u32::MAX as u64);
        archive.start_file(packed_name, options)?;

        let mut input = File::open(&source)?;
        loop {
            let read = input.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            archive.write_all(&buffer[..read])?;
        }
    }
    archive.finish()?;
    Ok(())
}

fn packed_entry_name(entry_name: &str, package: &PackageValidation) -> String {
    if entry_name == package.rules_file {
        "main.json".to_owned()
    } else if entry_name == package.apk_file {
        format!("{}.apk", package.package_name)
    } else {
        entry_name.to_owned()
    }
}

fn enumerate_files(root: &Path) -> Result<Vec<String>> {
    enumerate_file_paths(root).map(|paths| {
        paths
            .into_iter()
            .filter_map(|path| path.strip_prefix(root).ok().map(Path::to_path_buf))
            .map(|path| normalize_entry_name(&path.to_string_lossy()))
            .collect()
    })
}

fn enumerate_file_paths(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in WalkDir::new(root).follow_links(false) {
        let entry = entry.with_context(|| format!("failed to enumerate {}", root.display()))?;
        if entry.file_type().is_file() {
            files.push(entry.into_path());
        }
    }
    files.sort_by(|left, right| {
        normalize_entry_name(&left.strip_prefix(root).unwrap_or(left).to_string_lossy()).cmp(
            &normalize_entry_name(&right.strip_prefix(root).unwrap_or(right).to_string_lossy()),
        )
    });
    Ok(files)
}

fn files_with_extension(files: &[String], extension: &str) -> Vec<String> {
    files
        .iter()
        .filter(|file| {
            Path::new(file)
                .extension()
                .is_some_and(|value| value.eq_ignore_ascii_case(extension))
        })
        .cloned()
        .collect()
}

fn safe_destination_path(root: &Path, entry_name: &str) -> Result<PathBuf> {
    if is_unsafe_entry_name(entry_name) {
        bail!("unsafe archive entry path: {entry_name}");
    }
    let mut destination = root.to_path_buf();
    for segment in normalize_entry_name(entry_name).split('/') {
        destination.push(segment);
    }
    Ok(destination)
}

fn is_unsafe_entry_name(entry_name: &str) -> bool {
    entry_name.starts_with('/')
        || entry_name.starts_with('\\')
        || is_rooted_path(entry_name)
        || contains_parent_traversal(entry_name)
}

fn is_rooted_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/");
    normalized.starts_with('/')
        || normalized
            .as_bytes()
            .get(1)
            .is_some_and(|character| *character == b':')
        || Path::new(path)
            .components()
            .any(|component| matches!(component, Component::RootDir | Component::Prefix(_)))
}

fn contains_parent_traversal(path: &str) -> bool {
    path.split(['/', '\\']).any(|part| part == "..")
}

fn normalize_entry_name(path: &str) -> String {
    path.replace('\\', "/").trim_matches('/').to_owned()
}

fn entry_to_path(entry: &str) -> PathBuf {
    entry.split('/').collect()
}

fn contains_json_smart_punctuation(json: &str) -> bool {
    json.contains(['“', '”', '：'])
}

fn contains_invalid_filename_character(value: &str) -> bool {
    value.chars().any(|character| {
        character == '\0'
            || character == '/'
            || character == '\\'
            || (cfg!(windows)
                && (character.is_control()
                    || ['<', '>', ':', '"', '|', '?', '*'].contains(&character)))
    })
}

fn directory_name(path: &Path) -> Result<String> {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("invalid package directory: {}", path.display()))
}

fn expected_archive_name(package: &PackageValidation) -> String {
    format!("{}.{}", package.package_name, package.version_name)
}

fn is_lark_file(path: &Path) -> bool {
    path.is_file()
        && path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case(LARK_EXTENSION))
}

fn backup_existing_path(path: &Path) -> Result<Option<ExistingBackup>> {
    if !path.exists() {
        return Ok(None);
    }
    let is_directory = path.is_dir();
    let backup_path = create_backup_path(path, is_directory)?;
    fs::rename(path, &backup_path).with_context(|| {
        format!(
            "failed to back up {} to {}",
            path.display(),
            backup_path.display()
        )
    })?;
    Ok(Some(ExistingBackup {
        original_path: path.to_owned(),
        backup_path,
        is_directory,
    }))
}

fn create_backup_path(path: &Path, is_directory: bool) -> Result<PathBuf> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let extension = if is_directory {
        None
    } else {
        path.extension().and_then(|value| value.to_str())
    };
    let name = if is_directory {
        path.file_name().and_then(|value| value.to_str())
    } else {
        path.file_stem().and_then(|value| value.to_str())
    }
    .ok_or_else(|| anyhow!("invalid output path: {}", path.display()))?;
    let suffix = Local::now().format(".backup-%Y%m%d-%H%M%S%3f");

    for index in 0_u32.. {
        let counter = if index == 0 {
            String::new()
        } else {
            format!("-{index}")
        };
        let filename = match extension {
            Some(extension) => format!("{name}{suffix}{counter}.{extension}"),
            None => format!("{name}{suffix}{counter}"),
        };
        let candidate = parent.join(filename);
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    unreachable!()
}

fn restore_backup(backup: Option<&ExistingBackup>) -> Result<()> {
    if let Some(backup) = backup {
        fs::rename(&backup.backup_path, &backup.original_path).with_context(|| {
            format!(
                "failed to restore backup {} to {}",
                backup.backup_path.display(),
                backup.original_path.display()
            )
        })?;
    }
    Ok(())
}

fn backup_message(backup: Option<&ExistingBackup>) -> String {
    backup.map_or_else(String::new, |backup| {
        let kind = if backup.is_directory {
            "directory"
        } else {
            "file"
        };
        format!("Backup ({kind}): {}\n", backup.backup_path.display())
    })
}

fn format_elapsed(elapsed: std::time::Duration) -> String {
    let total_millis = elapsed.as_millis();
    let millis = total_millis % 1_000;
    let total_seconds = total_millis / 1_000;
    let seconds = total_seconds % 60;
    let total_minutes = total_seconds / 60;
    if total_minutes >= 60 {
        format!(
            "{}:{:02}:{:02}.{:03}",
            total_minutes / 60,
            total_minutes % 60,
            seconds,
            millis
        )
    } else {
        format!("{total_minutes}:{seconds:02}.{millis:03}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_supports_double_star_and_single_segment_wildcards() {
        assert!(glob_matches("Movies/**/*", "Movies/demo.mp4"));
        assert!(glob_matches("Movies/**/*", "Movies/nested/demo.mp4"));
        assert!(glob_matches("file?.txt", "file1.txt"));
        assert!(!glob_matches("Movies/*", "Movies/nested/demo.mp4"));
    }

    #[test]
    fn rejects_path_traversal_and_rooted_paths() {
        assert!(is_unsafe_entry_name("../outside.txt"));
        assert!(is_unsafe_entry_name(r"folder\..\outside.txt"));
        assert!(is_unsafe_entry_name("/absolute.txt"));
        assert!(is_unsafe_entry_name(r"C:\absolute.txt"));
        assert!(!is_unsafe_entry_name("folder/file.txt"));
    }

    #[test]
    fn validates_rule_paths_and_destinations() {
        let valid = CopyRule {
            source: "Movies/**/*".to_owned(),
            device_dest: "/sdcard/.Dubnium/Movies/".to_owned(),
        };
        assert!(validate_copy_rule(&valid, "main.json").is_ok());

        let invalid = CopyRule {
            source: "../Movies/**/*".to_owned(),
            device_dest: "/data/local/tmp".to_owned(),
        };
        assert!(validate_copy_rule(&invalid, "main.json").is_err());
    }

    #[test]
    fn identifies_allowed_rules_file_names() {
        let files = vec!["main.json".to_owned(), "asset.bin".to_owned()];
        assert_eq!(
            find_rules_json_file(&files, "com.example.demo").unwrap(),
            "main.json"
        );
    }
}
