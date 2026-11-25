use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// Font files bundled into release builds for `olymp-classic`.
///
/// Only the optical sizes and weights actually referenced by `olymp-classic.typ` are
/// included (Latin Modern Roman 10/12/17, Sans 10/12/17, Mono 10/12, Math, and
/// JetBrains Mono Regular/Italic/Bold/BoldItalic), omitting NL/Prop/Caps/variable
/// variants to keep binary size small.
const BUNDLED_FONTS: &[&str] = &[
    // Latin Modern Roman
    "lmroman10-regular.otf",
    "lmroman10-bold.otf",
    "lmroman10-italic.otf",
    "lmroman10-bolditalic.otf",
    "lmroman12-regular.otf",
    "lmroman12-bold.otf",
    "lmroman12-italic.otf",
    "lmroman17-regular.otf",
    // Latin Modern Sans
    "lmsans10-regular.otf",
    "lmsans10-bold.otf",
    "lmsans10-oblique.otf",
    "lmsans10-boldoblique.otf",
    "lmsans12-regular.otf",
    "lmsans12-oblique.otf",
    "lmsans17-regular.otf",
    "lmsans17-oblique.otf",
    // Latin Modern Mono
    "lmmono10-regular.otf",
    "lmmono10-italic.otf",
    "lmmono12-regular.otf",
    // Latin Modern Math
    "latinmodern-math.otf",
    // JetBrains Mono (TTF)
    "JetBrainsMono-Regular.ttf",
    "JetBrainsMono-Italic.ttf",
    "JetBrainsMono-Bold.ttf",
    "JetBrainsMono-BoldItalic.ttf",
];

fn main() {
    println!("cargo:rerun-if-env-changed=XCPC_FONT_DIRS");
    println!("cargo:rerun-if-env-changed=XCPC_SKIP_EMBED_FONTS");

    let is_release = env::var_os("CARGO_CFG_DEBUG_ASSERTIONS").is_none();
    let is_feature = env::var_os("CARGO_FEATURE_BUNDLED_FONTS").is_some();
    if !is_release && !is_feature {
        return;
    }

    let out_fonts = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR")).join("fonts");
    fs::create_dir_all(&out_fonts).expect("create OUT_DIR/fonts");

    if matches!(
        env::var("XCPC_SKIP_EMBED_FONTS").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE") | Ok("yes") | Ok("YES")
    ) {
        for &name in BUNDLED_FONTS {
            let dst = out_fonts.join(name);
            let _ = fs::remove_file(&dst);
            fs::write(&dst, [])
                .unwrap_or_else(|e| panic!("failed to write empty `{}`: {e}", dst.display()));
        }
        return;
    }

    let font_dirs = env::var_os("XCPC_FONT_DIRS").unwrap_or_else(|| {
        panic!(
            "XCPC_FONT_DIRS must be set when building xcpc-engine with bundled fonts \
             (run inside `nix develop`, or set XCPC_SKIP_EMBED_FONTS=1)"
        );
    });

    let mut found: HashMap<String, PathBuf> = HashMap::new();
    for dir in env::split_paths(&font_dirs) {
        if dir.is_dir() {
            collect_fonts(&dir, &mut found);
        }
    }

    for &name in BUNDLED_FONTS {
        let src = found.get(name).unwrap_or_else(|| {
            panic!("required bundled font `{name}` not found in XCPC_FONT_DIRS")
        });
        println!("cargo:rerun-if-changed={}", src.display());
        let dst = out_fonts.join(name);
        let _ = fs::remove_file(&dst);
        let bytes =
            fs::read(src).unwrap_or_else(|e| panic!("failed to read `{}`: {e}", src.display()));
        fs::write(&dst, bytes)
            .unwrap_or_else(|e| panic!("failed to write `{}`: {e}", dst.display()));
    }
}

fn collect_fonts(dir: &Path, found: &mut HashMap<String, PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_fonts(&path, found);
        } else if let Some(name) = path.file_name().and_then(|n| n.to_str())
            && BUNDLED_FONTS.contains(&name)
        {
            found.entry(name.to_string()).or_insert(path);
        }
    }
}
