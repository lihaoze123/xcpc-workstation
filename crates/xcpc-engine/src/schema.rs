//! Declarative `#let schema = (...)` extraction from Typst templates.
//!
//! Evaluates a tiny virtual wrapper against an existing [`Engine`] (without mutating its
//! main source or invalidating preview caches) to query `<xcpc-schema>` metadata exported
//! by built-in (`/xcpc/*.typ`) or workspace (`/templates/*.typ`) templates.

use std::collections::BTreeMap;

use typst::diag::FileResult;
use typst::foundations::{
    Bytes, Datetime, Dict, Duration as TypstDuration, Label, Selector, Value,
};
use typst::introspection::Introspector;
use typst::syntax::{FileId, Source};
use typst::text::{Font, FontBook};
use typst::utils::{LazyHash, PicoStr};
use typst::{Library, World};
use typst_layout::PagedDocument;
use xcpc_domain::{
    LocalizedText, TemplateChoice, TemplateField, TemplateFieldScope, TemplateFieldType,
    TemplateGroup, TemplateKind, TemplateOptionValue, TemplateSchema,
};

use crate::{Engine, project_id};

const SCHEMA_MAIN_PATH: &str = "/__xcpc_schema__.typ";

struct SchemaWorld<'a> {
    engine: &'a Engine,
    main: Source,
}

impl World for SchemaWorld<'_> {
    fn library(&self) -> &LazyHash<Library> {
        self.engine.library()
    }

    fn book(&self) -> &LazyHash<FontBook> {
        self.engine.book()
    }

    fn main(&self) -> FileId {
        self.main.id()
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main.id() {
            return Ok(self.main.clone());
        }
        self.engine.source(id)
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        self.engine.file(id)
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.engine.font(index)
    }

    fn today(&self, offset: Option<TypstDuration>) -> Option<Datetime> {
        self.engine.today(offset)
    }
}

/// Evaluates `import_path` (e.g. `"/xcpc/olymp-classic.typ"` or `"/templates/custom.typ"`)
/// and extracts its exported `#let schema = (...)` dictionary.
///
/// Returns an empty [`TemplateSchema`] if the template does not export `schema` or fails
/// to evaluate (graceful fallback for legacy/minimal custom templates).
pub(crate) fn inspect(engine: &Engine, import_path: &str) -> TemplateSchema {
    let escaped = import_path.replace('\\', "\\\\").replace('"', "\\\"");
    let src = format!(
        "#import \"{escaped}\" as __tpl\n\
         #let __s = if \"schema\" in dictionary(__tpl) {{ __tpl.schema }} else {{ none }}\n\
         #metadata(__s) <xcpc-schema>\n"
    );
    let world = SchemaWorld {
        engine,
        main: Source::new(project_id(SCHEMA_MAIN_PATH), src),
    };
    let warned = typst::compile::<PagedDocument>(&world);
    let Ok(doc) = warned.output else {
        return TemplateSchema::default();
    };
    let Some(label) = Label::new(PicoStr::intern("xcpc-schema")) else {
        return TemplateSchema::default();
    };
    let introspector = doc.introspector();
    let Some(elem) = introspector
        .query(&Selector::Label(label))
        .into_iter()
        .next()
    else {
        return TemplateSchema::default();
    };
    let Ok(value) = elem.get_by_name("value") else {
        return TemplateSchema::default();
    };
    match value {
        Value::Dict(dict) => parse_schema(&dict),
        _ => TemplateSchema::default(),
    }
}

/// Validates that `import_path` compiles cleanly as a Typst template module, exports a
/// callable `render` function, and returns its parsed `#let schema` (if any).
pub(crate) fn validate_module(
    engine: &Engine,
    import_path: &str,
) -> Result<TemplateSchema, Vec<crate::Diagnostic>> {
    let escaped = import_path.replace('\\', "\\\\").replace('"', "\\\"");
    let src = format!(
        "#import \"{escaped}\" as __tpl\n\
         #let __dict = dictionary(__tpl)\n\
         #let __has_render = \"render\" in __dict and type(__tpl.render) == function\n\
         #let __s = if \"schema\" in __dict {{ __tpl.schema }} else {{ none }}\n\
         #metadata((has_render: __has_render, schema: __s)) <xcpc-schema-check>\n"
    );
    let world = SchemaWorld {
        engine,
        main: Source::new(project_id(SCHEMA_MAIN_PATH), src),
    };
    let warned = typst::compile::<PagedDocument>(&world);
    let doc = warned.output.map_err(|errs| engine.diagnostics(&errs))?;
    let Some(label) = Label::new(PicoStr::intern("xcpc-schema-check")) else {
        return Ok(TemplateSchema::default());
    };
    let introspector = doc.introspector();
    let Some(elem) = introspector
        .query(&Selector::Label(label))
        .into_iter()
        .next()
    else {
        return Ok(TemplateSchema::default());
    };
    let Ok(Value::Dict(dict)) = elem.get_by_name("value") else {
        return Ok(TemplateSchema::default());
    };
    let has_render = matches!(dict.get("has_render"), Ok(Value::Bool(true)));
    if !has_render {
        return Err(vec![crate::Diagnostic {
            error: true,
            message: format!("template '{import_path}' must export `#let render(doc) = ...`"),
            file: Some(import_path.to_owned()),
            line: Some(1),
            column: Some(1),
            offset: Some(0),
            page: None,
            hints: Vec::new(),
        }]);
    }
    let schema = match dict.get("schema") {
        Ok(Value::Dict(s)) => parse_schema(s),
        _ => TemplateSchema::default(),
    };
    Ok(schema)
}

fn parse_schema(dict: &Dict) -> TemplateSchema {
    let kind = match get_str(dict, "kind").as_deref() {
        Some("trd") => TemplateKind::Trd,
        _ => TemplateKind::Statement,
    };
    let name = get_localized(dict, "name");
    let description = get_localized(dict, "description");
    let groups = dict
        .get("groups")
        .ok()
        .and_then(|v| match v {
            Value::Array(arr) => Some(
                arr.iter()
                    .filter_map(|item| match item {
                        Value::Dict(d) => parse_group(d),
                        _ => None,
                    })
                    .collect(),
            ),
            _ => None,
        })
        .unwrap_or_default();
    let fields = dict
        .get("fields")
        .ok()
        .and_then(|v| match v {
            Value::Array(arr) => Some(
                arr.iter()
                    .filter_map(|item| match item {
                        Value::Dict(d) => parse_field(d),
                        _ => None,
                    })
                    .collect(),
            ),
            _ => None,
        })
        .unwrap_or_default();
    TemplateSchema {
        kind,
        name,
        description,
        groups,
        fields,
    }
}

fn parse_group(dict: &Dict) -> Option<TemplateGroup> {
    let id = get_str(dict, "id")?;
    let label = get_localized(dict, "label").unwrap_or_else(|| LocalizedText::Plain(id.clone()));
    Some(TemplateGroup { id, label })
}

fn parse_field(dict: &Dict) -> Option<TemplateField> {
    let key = get_str(dict, "key")?;
    let label = get_localized(dict, "label").unwrap_or_else(|| LocalizedText::Plain(key.clone()));
    let description = get_localized(dict, "description");
    let field_type = match get_str(dict, "type").as_deref() {
        Some("bool") => TemplateFieldType::Bool,
        Some("enum") => TemplateFieldType::Enum,
        Some("lines") => TemplateFieldType::Lines,
        Some("number") => TemplateFieldType::Number,
        Some("length") => TemplateFieldType::Length,
        Some("color") => TemplateFieldType::Color,
        Some("font") => TemplateFieldType::Font,
        Some("image") => TemplateFieldType::Image,
        _ => TemplateFieldType::String,
    };
    let scope = match get_str(dict, "scope").as_deref() {
        Some("problem") => TemplateFieldScope::Problem,
        _ => TemplateFieldScope::Book,
    };
    let group = get_str(dict, "group");
    let default = dict
        .get("default")
        .ok()
        .and_then(parse_option_value)
        .unwrap_or_else(|| default_for_type(field_type));
    let choices = dict
        .get("choices")
        .ok()
        .and_then(|v| match v {
            Value::Array(arr) => Some(
                arr.iter()
                    .filter_map(|item| match item {
                        Value::Dict(d) => {
                            let value = get_str(d, "value")?;
                            let label = get_localized(d, "label")
                                .unwrap_or_else(|| LocalizedText::Plain(value.clone()));
                            Some(TemplateChoice { value, label })
                        }
                        Value::Str(s) => {
                            let v = s.as_str().to_owned();
                            Some(TemplateChoice {
                                label: LocalizedText::Plain(v.clone()),
                                value: v,
                            })
                        }
                        _ => None,
                    })
                    .collect(),
            ),
            _ => None,
        })
        .unwrap_or_default();
    let presets = dict
        .get("presets")
        .ok()
        .and_then(|v| match v {
            Value::Array(arr) => Some(
                arr.iter()
                    .filter_map(|item| match item {
                        Value::Str(s) => Some(s.as_str().to_owned()),
                        _ => None,
                    })
                    .collect(),
            ),
            _ => None,
        })
        .unwrap_or_default();
    let min = get_f64(dict, "min");
    let max = get_f64(dict, "max");
    let step = get_f64(dict, "step");
    let placeholder = get_localized(dict, "placeholder");
    let mut when = BTreeMap::new();
    if let Ok(Value::Dict(when_dict)) = dict.get("when") {
        for (k, v) in when_dict.iter() {
            if let Some(opt) = parse_option_value(v) {
                when.insert(k.as_str().to_owned(), opt);
            }
        }
    }
    Some(TemplateField {
        key,
        label,
        description,
        field_type,
        scope,
        group,
        default,
        choices,
        presets,
        min,
        max,
        step,
        placeholder,
        when,
    })
}

fn parse_option_value(value: &Value) -> Option<TemplateOptionValue> {
    match value {
        Value::Bool(b) => Some(TemplateOptionValue::Bool(*b)),
        Value::Int(i) => Some(TemplateOptionValue::Int(*i)),
        Value::Float(f) => Some(TemplateOptionValue::Float(*f)),
        Value::Str(s) => Some(TemplateOptionValue::String(s.as_str().to_owned())),
        Value::Array(arr) => {
            let lines: Vec<String> = arr
                .iter()
                .filter_map(|item| match item {
                    Value::Str(s) => Some(s.as_str().to_owned()),
                    _ => None,
                })
                .collect();
            Some(TemplateOptionValue::Lines(lines))
        }
        _ => None,
    }
}

fn default_for_type(ty: TemplateFieldType) -> TemplateOptionValue {
    match ty {
        TemplateFieldType::Bool => TemplateOptionValue::Bool(false),
        TemplateFieldType::Lines => TemplateOptionValue::Lines(Vec::new()),
        TemplateFieldType::Number => TemplateOptionValue::Float(0.0),
        TemplateFieldType::Enum
        | TemplateFieldType::String
        | TemplateFieldType::Length
        | TemplateFieldType::Color
        | TemplateFieldType::Font
        | TemplateFieldType::Image => TemplateOptionValue::String(String::new()),
    }
}

fn get_localized(dict: &Dict, key: &str) -> Option<LocalizedText> {
    match dict.get(key).ok()? {
        Value::Str(s) => Some(LocalizedText::Plain(s.as_str().to_owned())),
        Value::Dict(d) => {
            let mut map = BTreeMap::new();
            for (k, v) in d.iter() {
                if let Value::Str(s) = v {
                    map.insert(k.as_str().to_owned(), s.as_str().to_owned());
                }
            }
            (!map.is_empty()).then_some(LocalizedText::Map(map))
        }
        _ => None,
    }
}

fn get_str(dict: &Dict, key: &str) -> Option<String> {
    match dict.get(key).ok()? {
        Value::Str(s) => Some(s.as_str().to_owned()),
        _ => None,
    }
}

fn get_f64(dict: &Dict, key: &str) -> Option<f64> {
    match dict.get(key).ok()? {
        Value::Float(f) => Some(*f),
        Value::Int(i) => Some(*i as f64),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Engine, FontOptions};

    #[test]
    fn parses_plain_and_bilingual_localized_schema_fields() {
        let dir = tempfile::tempdir().unwrap();
        let tpl_dir = dir.path().join("templates");
        std::fs::create_dir_all(&tpl_dir).unwrap();
        std::fs::write(
            tpl_dir.join("test-i18n.typ"),
            r#"
#let schema = (
  name: (zh: "测试模板", en: "Test Template"),
  description: "Plain description",
  groups: (
    (id: "g1", label: (zh: "分组一", en: "Group 1")),
    (id: "g2", label: "Plain Group"),
    (id: "g3", label: (:)),
  ),
  fields: (
    (
      key: "cover",
      label: (zh: "封面", en: "Cover"),
      description: (zh: "显示封面", en: "Show cover"),
      type: "enum",
      scope: "book",
      group: "g1",
      default: "auto",
      placeholder: (zh: "选择模式", en: "Select mode"),
      choices: (
        (value: "auto", label: (zh: "自动", en: "Auto")),
        (value: "off", label: "Off"),
        "raw-choice",
      ),
    ),
    (
      key: "note",
      label: "Plain Label",
      description: (:),
      type: "string",
      default: "",
      placeholder: "Type here",
    ),
  ),
)
"#,
        )
        .unwrap();
        let engine = Engine::new(&FontOptions::from_env(), Some(dir.path().to_path_buf()));
        let schema = inspect(&engine, "/templates/test-i18n.typ");
        assert_eq!(
            schema.name,
            Some(LocalizedText::bilingual("测试模板", "Test Template"))
        );
        assert_eq!(
            schema.description,
            Some(LocalizedText::Plain("Plain description".to_owned()))
        );
        assert_eq!(schema.groups.len(), 3);
        assert_eq!(
            schema.groups[0].label,
            LocalizedText::bilingual("分组一", "Group 1")
        );
        assert_eq!(
            schema.groups[1].label,
            LocalizedText::Plain("Plain Group".to_owned())
        );
        // Empty dict treated as missing -> falls back to group id.
        assert_eq!(
            schema.groups[2].label,
            LocalizedText::Plain("g3".to_owned())
        );

        assert_eq!(schema.fields.len(), 2);
        let cover = &schema.fields[0];
        assert_eq!(cover.label.resolve("zh-CN"), "封面");
        assert_eq!(cover.label.resolve("en"), "Cover");
        assert_eq!(
            cover.description.as_ref().map(|d| d.resolve("en")),
            Some("Show cover")
        );
        assert_eq!(
            cover.placeholder.as_ref().map(|p| p.resolve("zh-CN")),
            Some("选择模式")
        );
        assert_eq!(cover.choices.len(), 3);
        assert_eq!(cover.choices[0].label.resolve("en"), "Auto");
        assert_eq!(cover.choices[0].label.resolve("zh-CN"), "自动");
        assert_eq!(cover.choices[1].label.resolve("zh-CN"), "Off");
        assert_eq!(cover.choices[2].label.resolve("en"), "raw-choice");

        let note = &schema.fields[1];
        assert_eq!(note.label, LocalizedText::Plain("Plain Label".to_owned()));
        // Empty dict treated as missing -> None.
        assert_eq!(note.description, None);
        assert_eq!(
            note.placeholder,
            Some(LocalizedText::Plain("Type here".to_owned()))
        );
        assert_eq!(schema.kind, TemplateKind::Statement);
    }

    #[test]
    fn parses_trd_template_kind_and_preset_schemas() {
        let engine = Engine::new(&FontOptions::from_env(), None);
        let compact = inspect(&engine, "/xcpc/trd-compact.typ");
        assert_eq!(compact.kind, TemplateKind::Trd);
        assert!(compact.fields.iter().any(|f| f.key == "columns"));
        assert!(compact.fields.iter().any(|f| f.key == "font-size"));
        assert!(compact.fields.iter().any(|f| f.key == "hash-interval"));
        assert!(compact.fields.iter().any(|f| f.key == "strip-comments"));
        assert!(!compact.fields.iter().any(|f| f.key == "margin"));
        assert!(!compact.fields.iter().any(|f| f.key == "show-verify-cmd"));
        assert!(!compact.fields.iter().any(|f| f.key == "show-toc"));

        let book = inspect(&engine, "/xcpc/trd-book.typ");
        assert_eq!(book.kind, TemplateKind::Trd);
        assert!(book.fields.iter().any(|f| f.key == "columns"));
        assert!(book.fields.iter().any(|f| f.key == "font-size"));
        assert!(book.fields.iter().any(|f| f.key == "hash-interval"));
        assert!(book.fields.iter().any(|f| f.key == "strip-comments"));
        assert!(!book.fields.iter().any(|f| f.key == "margin"));
        assert!(!book.fields.iter().any(|f| f.key == "show-verify-cmd"));
        assert!(!book.fields.iter().any(|f| f.key == "show-toc"));
    }
}
