//! In-memory ZIP archive reading/writing and SHA-256 helpers exposed to QuickJS plugins.
//!
//! Enforces strict Zip Slip path traversal protection, symlink rejection, compressed input
//! limit (20 MiB), decompressed total limit (50 MiB, anti-ZIP-bomb), and max entry count (1000).

use std::collections::HashSet;
use std::io::{Cursor, Read, Write};
use std::path::{Component, Path};

use base64::Engine as _;
use serde::{Deserialize, Serialize};

use crate::error::{PluginError, Result};
use crate::manager::sha256_hex;

/// Maximum encoded ZIP archive byte size accepted as input (20 MiB).
pub const MAX_ARCHIVE_INPUT_BYTES: usize = 20 * 1024 * 1024;
/// Maximum total uncompressed output bytes allowed when reading or creating a ZIP (50 MiB).
pub const MAX_ARCHIVE_UNPACKED_BYTES: u64 = 50 * 1024 * 1024;
/// Maximum number of entries in a single ZIP archive.
pub const MAX_ARCHIVE_ENTRIES: usize = 1000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ZipEntryMeta {
    pub path: String,
    pub size: u64,
    pub is_dir: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ZipEntryData {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_base64: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateZipEntry {
    pub path: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default, alias = "base64", alias = "contentBase64")]
    pub content_base64: Option<String>,
}

fn validate_archive_entry_path(raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || raw.contains('\\') || raw.starts_with('/') {
        return Err(PluginError::ZipSlip(raw.to_owned()));
    }
    let rel = Path::new(raw);
    if rel.is_absolute() {
        return Err(PluginError::ZipSlip(raw.to_owned()));
    }
    for comp in rel.components() {
        match comp {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(PluginError::ZipSlip(raw.to_owned()));
            }
        }
    }
    Ok(raw.to_owned())
}

fn decode_zip_base64(zip_base64: &str) -> Result<Vec<u8>> {
    // Reject overly large base64 strings before allocating decoded buffer.
    let max_b64 = (MAX_ARCHIVE_INPUT_BYTES / 3 + 2) * 4 + 1024;
    if zip_base64.len() > max_b64 {
        return Err(PluginError::InvalidPackage(format!(
            "ZIP input exceeds maximum size of {MAX_ARCHIVE_INPUT_BYTES} bytes"
        )));
    }
    let cleaned: String = zip_base64.chars().filter(|c| !c.is_whitespace()).collect();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(cleaned)
        .map_err(|e| PluginError::InvalidArgument(format!("invalid base64 ZIP payload: {e}")))?;
    if bytes.len() > MAX_ARCHIVE_INPUT_BYTES {
        return Err(PluginError::InvalidPackage(format!(
            "ZIP input ({} bytes) exceeds maximum size of {MAX_ARCHIVE_INPUT_BYTES} bytes",
            bytes.len()
        )));
    }
    Ok(bytes)
}

/// Lists all entries in a Base64-encoded ZIP archive with Zip Slip and entry-count validation.
pub fn list_zip(zip_base64: &str) -> Result<Vec<ZipEntryMeta>> {
    let bytes = decode_zip_base64(zip_base64)?;
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| PluginError::InvalidPackage(format!("invalid zip archive: {e}")))?;
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err(PluginError::InvalidPackage(format!(
            "ZIP archive has {} entries (maximum is {MAX_ARCHIVE_ENTRIES})",
            archive.len()
        )));
    }
    let mut out = Vec::with_capacity(archive.len());
    for i in 0..archive.len() {
        let file = archive
            .by_index(i)
            .map_err(|e| PluginError::InvalidPackage(format!("zip entry error: {e}")))?;
        let raw_name = file.name().to_owned();
        if let Some(mode) = file.unix_mode()
            && (mode & 0o170000) == 0o120000
        {
            return Err(PluginError::ZipSlip(format!("symlink entry '{raw_name}'")));
        }
        let path = validate_archive_entry_path(&raw_name)?;
        out.push(ZipEntryMeta {
            path,
            size: file.size(),
            is_dir: file.is_dir(),
        });
    }
    Ok(out)
}

/// Reads selected (or all) file entries from a Base64-encoded ZIP archive into memory,
/// enforcing Zip Slip, symlink rejection, max entry count, and 50 MiB total decompressed limit.
pub fn read_zip(
    zip_base64: &str,
    paths: Option<&[String]>,
    as_text: bool,
) -> Result<Vec<ZipEntryData>> {
    let bytes = decode_zip_base64(zip_base64)?;
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| PluginError::InvalidPackage(format!("invalid zip archive: {e}")))?;
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err(PluginError::InvalidPackage(format!(
            "ZIP archive has {} entries (maximum is {MAX_ARCHIVE_ENTRIES})",
            archive.len()
        )));
    }
    let filter: Option<HashSet<&str>> = paths.map(|list| list.iter().map(String::as_str).collect());

    let mut total_unpacked: u64 = 0;
    let mut out = Vec::new();
    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| PluginError::InvalidPackage(format!("zip entry error: {e}")))?;
        let raw_name = file.name().to_owned();
        if let Some(mode) = file.unix_mode()
            && (mode & 0o170000) == 0o120000
        {
            return Err(PluginError::ZipSlip(format!("symlink entry '{raw_name}'")));
        }
        let path = validate_archive_entry_path(&raw_name)?;
        if file.is_dir() {
            continue;
        }
        if let Some(ref set) = filter
            && !set.contains(path.as_str())
        {
            continue;
        }
        let remaining = MAX_ARCHIVE_UNPACKED_BYTES.saturating_sub(total_unpacked);
        let mut buf = Vec::new();
        let mut limited = (&mut file).take(remaining.saturating_add(1));
        limited.read_to_end(&mut buf)?;
        if (buf.len() as u64) > remaining {
            return Err(PluginError::InvalidPackage(format!(
                "ZIP decompressed payload exceeds maximum size of {MAX_ARCHIVE_UNPACKED_BYTES} bytes"
            )));
        }
        total_unpacked = total_unpacked.saturating_add(buf.len() as u64);
        if as_text {
            let text = String::from_utf8(buf).map_err(|e| {
                PluginError::InvalidPackage(format!("ZIP entry '{path}' is not valid UTF-8: {e}"))
            })?;
            out.push(ZipEntryData {
                path,
                text: Some(text),
                content_base64: None,
            });
        } else {
            let b64 = base64::engine::general_purpose::STANDARD.encode(&buf);
            out.push(ZipEntryData {
                path,
                text: None,
                content_base64: Some(b64),
            });
        }
    }
    Ok(out)
}

/// Creates a ZIP archive in memory from `entries` and returns it as a Base64 string.
pub fn create_zip(entries: &[CreateZipEntry]) -> Result<String> {
    if entries.len() > MAX_ARCHIVE_ENTRIES {
        return Err(PluginError::InvalidPackage(format!(
            "too many ZIP entries ({}, maximum is {MAX_ARCHIVE_ENTRIES})",
            entries.len()
        )));
    }
    let mut total_unpacked: u64 = 0;
    let mut buf = Cursor::new(Vec::new());
    {
        let mut writer = zip::ZipWriter::new(&mut buf);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(0o644);
        for entry in entries {
            let path = validate_archive_entry_path(&entry.path)?;
            if path.ends_with('/') {
                return Err(PluginError::InvalidArgument(format!(
                    "ZIP file entry path '{path}' cannot end with '/'"
                )));
            }
            let data: Vec<u8> = if let Some(ref b64) = entry.content_base64 {
                let cleaned: String = b64.chars().filter(|c| !c.is_whitespace()).collect();
                base64::engine::general_purpose::STANDARD
                    .decode(cleaned)
                    .map_err(|e| {
                        PluginError::InvalidArgument(format!(
                            "invalid base64 for entry '{path}': {e}"
                        ))
                    })?
            } else if let Some(ref text) = entry.text {
                text.as_bytes().to_vec()
            } else {
                Vec::new()
            };
            total_unpacked = total_unpacked.saturating_add(data.len() as u64);
            if total_unpacked > MAX_ARCHIVE_UNPACKED_BYTES {
                return Err(PluginError::InvalidPackage(format!(
                    "total ZIP entry payload exceeds {MAX_ARCHIVE_UNPACKED_BYTES} bytes"
                )));
            }
            writer
                .start_file(&path, options)
                .map_err(|e| PluginError::InvalidPackage(format!("zip write error: {e}")))?;
            writer.write_all(&data)?;
        }
        writer
            .finish()
            .map_err(|e| PluginError::InvalidPackage(format!("zip finish error: {e}")))?;
    }
    let zip_bytes = buf.into_inner();
    Ok(base64::engine::general_purpose::STANDARD.encode(zip_bytes))
}

/// Computes lowercase hex SHA-256 of `input` decoded as `"utf8"` (default) or `"base64"`.
pub fn sha256_encoded(input: &str, encoding: Option<&str>) -> Result<String> {
    match encoding.map(str::trim).unwrap_or("utf8") {
        "" | "utf8" | "utf-8" => Ok(sha256_hex(input.as_bytes())),
        "base64" => {
            let cleaned: String = input.chars().filter(|c| !c.is_whitespace()).collect();
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(cleaned)
                .map_err(|e| PluginError::InvalidArgument(format!("invalid base64 input: {e}")))?;
            Ok(sha256_hex(&bytes))
        }
        other => Err(PluginError::InvalidArgument(format!(
            "unsupported encoding '{other}' (expected 'utf8' or 'base64')"
        ))),
    }
}
