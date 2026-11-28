//! Typst external package resolution (`@preview` and `@local`).
//!
//! Loading order for `VirtualRoot::Package(spec)`:
//! 1. Built-in offline fallback bundles (`@preview/cetz:0.5.3`, `@preview/oxifmt:1.0.0`,
//!    `@preview/pinit:0.2.2`, `@preview/mannot:0.4.0`, `@preview/tiptoe:0.4.0`,
//!    `@preview/touying:0.8.0`, `@preview/uniwarn:0.1.1`, `@preview/meander:0.4.4`) so the
//!    example workspace and common drawing/annotation/presentation packages compile
//!    instantaneously and deterministically even without internet access.
//! 2. Standard Typst package directories (`TYPST_PACKAGE_PATH` / `$XDG_DATA_HOME/typst/packages`
//!    for `@local`, `TYPST_PACKAGE_CACHE_PATH` / `$XDG_CACHE_HOME/typst/packages` for cached
//!    `@preview` packages).
//! 3. On-demand download from Typst Universe (`packages.typst.org`) via `typst-kit`'s
//!    `SystemDownloader`, cached into the standard package cache directory. Setting
//!    `XCPC_OFFLINE=1` disables network downloads.

use std::any::Any;
use std::collections::HashMap;
use std::io::{self, Cursor, Read};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use flate2::read::GzDecoder;
use tar::Archive;
use typst::diag::{FileError, FileResult};
use typst::foundations::Bytes;
use typst::syntax::package::{PackageSpec, PackageVersion};
use typst::syntax::{FileId, Source, VirtualPath};
use typst_kit::downloader::{Downloader, SystemDownloader};
use typst_kit::packages::{FsPackages, SystemPackages, UniversePackages};

struct BundledPackage {
    name: &'static str,
    version: PackageVersion,
    tar_gz: &'static [u8],
}

const BUNDLED_PACKAGES: &[BundledPackage] = &[
    BundledPackage {
        name: "cetz",
        version: PackageVersion {
            major: 0,
            minor: 5,
            patch: 3,
        },
        tar_gz: include_bytes!("../packages/preview/cetz-0.5.3.tar.gz"),
    },
    BundledPackage {
        name: "oxifmt",
        version: PackageVersion {
            major: 1,
            minor: 0,
            patch: 0,
        },
        tar_gz: include_bytes!("../packages/preview/oxifmt-1.0.0.tar.gz"),
    },
    BundledPackage {
        name: "pinit",
        version: PackageVersion {
            major: 0,
            minor: 2,
            patch: 2,
        },
        tar_gz: include_bytes!("../packages/preview/pinit-0.2.2.tar.gz"),
    },
    BundledPackage {
        name: "mannot",
        version: PackageVersion {
            major: 0,
            minor: 4,
            patch: 0,
        },
        tar_gz: include_bytes!("../packages/preview/mannot-0.4.0.tar.gz"),
    },
    BundledPackage {
        name: "tiptoe",
        version: PackageVersion {
            major: 0,
            minor: 4,
            patch: 0,
        },
        tar_gz: include_bytes!("../packages/preview/tiptoe-0.4.0.tar.gz"),
    },
    BundledPackage {
        name: "touying",
        version: PackageVersion {
            major: 0,
            minor: 8,
            patch: 0,
        },
        tar_gz: include_bytes!("../packages/preview/touying-0.8.0.tar.gz"),
    },
    BundledPackage {
        name: "uniwarn",
        version: PackageVersion {
            major: 0,
            minor: 1,
            patch: 1,
        },
        tar_gz: include_bytes!("../packages/preview/uniwarn-0.1.1.tar.gz"),
    },
    BundledPackage {
        name: "meander",
        version: PackageVersion {
            major: 0,
            minor: 4,
            patch: 4,
        },
        tar_gz: include_bytes!("../packages/preview/meander-0.4.4.tar.gz"),
    },
];

/// Lazy-unpacked contents of bundled `@preview` packages keyed by `(name, version)`.
fn bundled_files(spec: &PackageSpec) -> Option<&'static HashMap<String, Bytes>> {
    if spec.namespace != UniversePackages::NAMESPACE {
        return None;
    }
    let idx = BUNDLED_PACKAGES
        .iter()
        .position(|pkg| pkg.name == spec.name.as_str() && pkg.version == spec.version)?;

    static CACHE: OnceLock<Vec<OnceLock<HashMap<String, Bytes>>>> = OnceLock::new();
    let slots = CACHE.get_or_init(|| {
        (0..BUNDLED_PACKAGES.len())
            .map(|_| OnceLock::new())
            .collect()
    });

    Some(slots[idx].get_or_init(|| unpack_tar_gz(BUNDLED_PACKAGES[idx].tar_gz)))
}

fn unpack_tar_gz(bytes: &[u8]) -> HashMap<String, Bytes> {
    let mut map = HashMap::new();
    let decoder = GzDecoder::new(Cursor::new(bytes));
    let mut archive = Archive::new(decoder);
    let Ok(entries) = archive.entries() else {
        return map;
    };
    for entry in entries.flatten() {
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let Ok(path) = entry.path() else {
            continue;
        };
        let normalized = normalize_archive_path(&path.to_string_lossy());
        let mut buf = Vec::new();
        let mut reader = entry;
        if reader.read_to_end(&mut buf).is_ok() {
            map.insert(normalized, Bytes::new(buf));
        }
    }
    map
}

fn normalize_archive_path(raw: &str) -> String {
    let s = raw.replace('\\', "/");
    s.trim_start_matches("./")
        .trim_start_matches('/')
        .to_owned()
}

/// Downloader that respects `XCPC_OFFLINE=1` at call time.
struct ConfigurableDownloader {
    inner: SystemDownloader,
}

impl ConfigurableDownloader {
    fn new() -> Self {
        Self {
            inner: SystemDownloader::new(concat!("xcpc-workstation/", env!("CARGO_PKG_VERSION"))),
        }
    }

    fn is_offline() -> bool {
        std::env::var("XCPC_OFFLINE").ok().is_some_and(|v| {
            let trimmed = v.trim();
            trimmed == "1"
                || trimmed.eq_ignore_ascii_case("true")
                || trimmed.eq_ignore_ascii_case("yes")
        })
    }
}

impl Downloader for ConfigurableDownloader {
    fn stream(&self, key: &dyn Any, url: &str) -> io::Result<(Option<usize>, Box<dyn Read>)> {
        if Self::is_offline() {
            return Err(io::Error::other("offline mode enabled (XCPC_OFFLINE=1)"));
        }
        self.inner.stream(key, url)
    }

    fn download(&self, key: &dyn Any, url: &str) -> io::Result<Vec<u8>> {
        if Self::is_offline() {
            return Err(io::Error::other("offline mode enabled (XCPC_OFFLINE=1)"));
        }
        self.inner.download(key, url)
    }
}

fn system_packages() -> &'static SystemPackages {
    static SYSTEM_PACKAGES: OnceLock<SystemPackages> = OnceLock::new();
    SYSTEM_PACKAGES.get_or_init(|| {
        let data = std::env::var_os("TYPST_PACKAGE_PATH")
            .filter(|v| !v.is_empty())
            .map(FsPackages::new)
            .or_else(FsPackages::system_data);
        let cache = std::env::var_os("TYPST_PACKAGE_CACHE_PATH")
            .filter(|v| !v.is_empty())
            .map(FsPackages::new)
            .or_else(FsPackages::system_cache);
        let universe = UniversePackages::new(ConfigurableDownloader::new());
        SystemPackages::from_parts(data, cache, universe)
    })
}

/// Loads raw file bytes from a Typst package (`@preview` or `@local`).
pub(crate) fn load_package_file(spec: &PackageSpec, vpath: &VirtualPath) -> FileResult<Bytes> {
    if let Some(files) = bundled_files(spec) {
        let rel = normalize_archive_path(vpath.get_without_slash());
        return files
            .get(&rel)
            .cloned()
            .ok_or_else(|| FileError::NotFound(PathBuf::from(rel)));
    }
    let root = system_packages().obtain(spec)?;
    root.load(vpath)
}

fn strip_utf8_bom(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

/// Loads a Typst [`Source`] from a package.
///
/// Immutable `@preview` sources are cached process-wide so repeated live-preview
/// compilations do not re-parse large packages like `cetz`. Local (`@local`) packages
/// are parsed fresh so edits take effect immediately.
pub(crate) fn load_package_source(id: FileId, spec: &PackageSpec) -> FileResult<Source> {
    if spec.namespace == UniversePackages::NAMESPACE {
        static PREVIEW_SOURCES: OnceLock<Mutex<HashMap<FileId, Source>>> = OnceLock::new();
        let cache = PREVIEW_SOURCES.get_or_init(Default::default);
        if let Some(source) = cache.lock().unwrap().get(&id).cloned() {
            return Ok(source);
        }
        let bytes = load_package_file(spec, id.vpath())?;
        let text = strip_utf8_bom(std::str::from_utf8(&bytes).map_err(|_| FileError::InvalidUtf8)?);
        let source = Source::new(id, text.into());
        cache.lock().unwrap().insert(id, source.clone());
        return Ok(source);
    }

    let bytes = load_package_file(spec, id.vpath())?;
    let text = strip_utf8_bom(std::str::from_utf8(&bytes).map_err(|_| FileError::InvalidUtf8)?);
    Ok(Source::new(id, text.into()))
}
