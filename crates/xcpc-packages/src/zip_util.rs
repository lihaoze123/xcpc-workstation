//! Safe ZIP reading (Zip Slip protection & decompression budget) and atomic ZIP writing.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{Read, Seek, Write};
use std::path::{Component, Path};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::error::{PackageError, Result, io_err};

/// Default maximum total uncompressed bytes allowed when reading an archive (512 MiB).
pub const DEFAULT_MAX_UNCOMPRESSED_BYTES: u64 = 512 * 1024 * 1024;

/// Default maximum entry count allowed when reading an archive.
pub const DEFAULT_MAX_ENTRY_COUNT: usize = 8192;

/// Validates a ZIP entry path against Zip Slip traversal, absolute paths, drive letters, and NUL bytes.
/// Returns a normalized `/`-separated relative path (without trailing `/`), or `None` for root/empty dirs.
pub fn sanitize_zip_path(raw: &str) -> Result<Option<String>> {
    if raw.contains('\0') {
        return Err(PackageError::ZipSlip(raw.to_owned()));
    }
    let trimmed = raw.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() || trimmed == "." {
        return Ok(None);
    }
    if trimmed.starts_with('/') || trimmed.starts_with('\\') {
        return Err(PackageError::ZipSlip(raw.to_owned()));
    }
    // Reject Windows drive prefixes like `C:...`
    let bytes = trimmed.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return Err(PackageError::ZipSlip(raw.to_owned()));
    }

    let normalized = trimmed.replace('\\', "/");
    let mut parts = Vec::new();
    for comp in Path::new(&normalized).components() {
        match comp {
            Component::Normal(p) => {
                let s = p.to_string_lossy();
                if s.is_empty() || s == "." || s == ".." {
                    return Err(PackageError::ZipSlip(raw.to_owned()));
                }
                parts.push(s.into_owned());
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(PackageError::ZipSlip(raw.to_owned()));
            }
        }
    }
    if parts.is_empty() {
        Ok(None)
    } else {
        Ok(Some(parts.join("/")))
    }
}

/// In-memory safely validated view of a ZIP archive's files (normalized `/`-separated paths -> bytes).
#[derive(Debug, Clone)]
pub struct SafeZipContents {
    pub files: BTreeMap<String, Vec<u8>>,
}

impl SafeZipContents {
    /// Opens and validates a ZIP file on disk, enforcing Zip Slip checks, symlink rejection,
    /// entry count cap, and streaming uncompressed byte budget.
    pub fn read_from_path(
        path: &Path,
        max_uncompressed_bytes: u64,
        max_entry_count: usize,
    ) -> Result<Self> {
        let file =
            File::open(path).map_err(|e| io_err(format!("open archive {}", path.display()), e))?;
        Self::read_from_reader(file, max_uncompressed_bytes, max_entry_count)
    }

    /// Reads and validates a ZIP archive from any `Read + Seek`.
    pub fn read_from_reader<R: Read + Seek>(
        reader: R,
        max_uncompressed_bytes: u64,
        max_entry_count: usize,
    ) -> Result<Self> {
        let mut archive =
            ZipArchive::new(reader).map_err(|e| PackageError::InvalidArchive(e.to_string()))?;

        if archive.len() > max_entry_count {
            return Err(PackageError::TooManyEntries {
                limit: max_entry_count,
                actual: archive.len(),
            });
        }

        let mut files = BTreeMap::new();
        let mut total_bytes: u64 = 0;

        for i in 0..archive.len() {
            let entry = archive
                .by_index(i)
                .map_err(|e| PackageError::InvalidArchive(e.to_string()))?;

            let raw_name = entry.name().to_owned();
            let is_dir = entry.is_dir() || raw_name.ends_with('/') || raw_name.ends_with('\\');

            // Reject symlinks encoded in Unix external attributes (S_IFMT == S_IFLNK = 0o120000).
            if let Some(mode) = entry.unix_mode()
                && (mode & 0o170000) == 0o120000
            {
                return Err(PackageError::ZipSlip(format!("symlink entry: {raw_name}")));
            }

            let Some(clean_path) = sanitize_zip_path(&raw_name)? else {
                continue;
            };

            if is_dir {
                continue;
            }

            // Streaming read with strict budget so forged ZIP headers cannot cause OOM.
            let remaining = max_uncompressed_bytes.saturating_sub(total_bytes);
            let mut limited = entry.take(remaining.saturating_add(1));
            let mut buf = Vec::new();
            limited
                .read_to_end(&mut buf)
                .map_err(|e| PackageError::InvalidArchive(format!("read {clean_path}: {e}")))?;

            let new_total = total_bytes.saturating_add(buf.len() as u64);
            if new_total > max_uncompressed_bytes {
                return Err(PackageError::ArchiveTooLarge {
                    limit_bytes: max_uncompressed_bytes,
                    actual_bytes: new_total,
                });
            }
            total_bytes = new_total;
            files.insert(clean_path, buf);
        }

        Ok(Self { files })
    }

    /// Strips a single common top-level wrapper directory if every file shares it
    /// (e.g. `problem-123/problem.xml` -> `problem.xml`), unless `problem.xml` or `contest.xml`
    /// is already at the root.
    pub fn strip_single_root_prefix(&self) -> Self {
        if self.files.contains_key("problem.xml") || self.files.contains_key("contest.xml") {
            return self.clone();
        }
        let mut prefix: Option<String> = None;
        for key in self.files.keys() {
            let Some((first, _rest)) = key.split_once('/') else {
                return self.clone();
            };
            match &prefix {
                None => prefix = Some(first.to_owned()),
                Some(p) if p == first => {}
                Some(_) => return self.clone(),
            }
        }
        let Some(p) = prefix else {
            return self.clone();
        };
        let strip = format!("{p}/");
        let mut stripped = BTreeMap::new();
        for (k, v) in &self.files {
            if let Some(rest) = k.strip_prefix(&strip)
                && !rest.is_empty()
            {
                stripped.insert(rest.to_owned(), v.clone());
            }
        }
        Self { files: stripped }
    }

    /// Returns UTF-8 text of a file inside the archive, if present.
    pub fn get_text(&self, path: &str) -> Option<String> {
        let bytes = self.files.get(path)?;
        Some(String::from_utf8_lossy(bytes).into_owned())
    }

    /// Returns raw bytes of a file inside the archive, if present.
    pub fn get_bytes(&self, path: &str) -> Option<&[u8]> {
        self.files.get(path).map(Vec::as_slice)
    }

    /// Returns a sub-view containing all files under `prefix/` with the prefix stripped.
    pub fn subtree(&self, prefix: &str) -> Self {
        let norm = prefix.trim_matches('/');
        let with_slash = format!("{norm}/");
        let mut sub = BTreeMap::new();
        for (k, v) in &self.files {
            if let Some(rest) = k.strip_prefix(&with_slash)
                && !rest.is_empty()
            {
                sub.insert(rest.to_owned(), v.clone());
            }
        }
        Self { files: sub }
    }
}

/// Atomic ZIP writer that stages into a sibling temp file and renames on finish.
pub struct AtomicZipWriter {
    target_path: std::path::PathBuf,
    temp_path: std::path::PathBuf,
    writer: Option<ZipWriter<File>>,
}

impl AtomicZipWriter {
    pub fn create(target_path: &Path) -> Result<Self> {
        if let Some(parent) = target_path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)
                .map_err(|e| io_err(format!("create directory {}", parent.display()), e))?;
        }
        let parent = target_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let file_name = target_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("archive.zip");
        let temp_path = parent.join(format!(".{file_name}.tmp-{}", std::process::id()));
        let file = File::create(&temp_path)
            .map_err(|e| io_err(format!("create temp zip {}", temp_path.display()), e))?;
        Ok(Self {
            target_path: target_path.to_path_buf(),
            temp_path,
            writer: Some(ZipWriter::new(file)),
        })
    }

    /// Adds a regular file with default 0o644 permissions.
    pub fn add_file(&mut self, name: &str, bytes: &[u8]) -> Result<()> {
        self.add_file_with_mode(name, bytes, 0o644)
    }

    /// Adds a file with explicit Unix mode bits (e.g. `0o755` for executable scripts).
    pub fn add_file_with_mode(&mut self, name: &str, bytes: &[u8], mode: u32) -> Result<()> {
        let writer = self
            .writer
            .as_mut()
            .ok_or_else(|| PackageError::InvalidArchive("zip writer already finished".into()))?;
        let opts = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .unix_permissions(mode);
        writer
            .start_file(name, opts)
            .map_err(|e| PackageError::InvalidArchive(format!("zip entry {name}: {e}")))?;
        writer
            .write_all(bytes)
            .map_err(|e| io_err(format!("write zip entry {name}"), e))?;
        Ok(())
    }

    /// Finishes writing, syncs to disk, atomically renames to target path, and returns file size.
    pub fn finish(mut self) -> Result<u64> {
        let writer = self
            .writer
            .take()
            .ok_or_else(|| PackageError::InvalidArchive("zip writer already finished".into()))?;
        let file = writer
            .finish()
            .map_err(|e| PackageError::InvalidArchive(format!("finish zip: {e}")))?;
        file.sync_all()
            .map_err(|e| io_err(format!("sync zip {}", self.temp_path.display()), e))?;
        let size = file
            .metadata()
            .map_err(|e| io_err(format!("stat zip {}", self.temp_path.display()), e))?
            .len();
        drop(file);
        fs::rename(&self.temp_path, &self.target_path).map_err(|e| {
            let _ = fs::remove_file(&self.temp_path);
            io_err(format!("rename zip to {}", self.target_path.display()), e)
        })?;
        Ok(size)
    }
}

impl Drop for AtomicZipWriter {
    fn drop(&mut self) {
        if self.writer.is_some() {
            let _ = self.writer.take();
            let _ = fs::remove_file(&self.temp_path);
        }
    }
}
