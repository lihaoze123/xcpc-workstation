pub struct BuiltinPluginFiles {
    pub id: &'static str,
    pub manifest_toml: &'static str,
    pub index_js: &'static str,
    pub ui_js: Option<&'static str>,
}

pub const BUILTIN_PLUGINS: &[BuiltinPluginFiles] = &[
    BuiltinPluginFiles {
        id: "codeforces",
        manifest_toml: include_str!("../plugins/codeforces/manifest.toml"),
        index_js: include_str!("../plugins/codeforces/index.js"),
        ui_js: None,
    },
    BuiltinPluginFiles {
        id: "atcoder",
        manifest_toml: include_str!("../plugins/atcoder/manifest.toml"),
        index_js: include_str!("../plugins/atcoder/index.js"),
        ui_js: None,
    },
];

pub fn find_builtin(id: &str) -> Option<&'static BuiltinPluginFiles> {
    BUILTIN_PLUGINS.iter().find(|p| p.id == id)
}
