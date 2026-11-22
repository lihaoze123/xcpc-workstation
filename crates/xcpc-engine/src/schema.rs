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
    TemplateChoice, TemplateField, TemplateFieldScope, TemplateFieldType, TemplateGroup,
    TemplateOptionValue, TemplateSchema,
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

fn parse_schema(dict: &Dict) -> TemplateSchema {
    let name = get_str(dict, "name");
    let description = get_str(dict, "description");
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
        name,
        description,
        groups,
        fields,
    }
}

fn parse_group(dict: &Dict) -> Option<TemplateGroup> {
    let id = get_str(dict, "id")?;
    let label = get_str(dict, "label").unwrap_or_else(|| id.clone());
    Some(TemplateGroup { id, label })
}

fn parse_field(dict: &Dict) -> Option<TemplateField> {
    let key = get_str(dict, "key")?;
    let label = get_str(dict, "label").unwrap_or_else(|| key.clone());
    let description = get_str(dict, "description");
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
                            let label = get_str(d, "label").unwrap_or_else(|| value.clone());
                            Some(TemplateChoice { value, label })
                        }
                        Value::Str(s) => {
                            let v = s.as_str().to_owned();
                            Some(TemplateChoice {
                                label: v.clone(),
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
    let placeholder = get_str(dict, "placeholder");
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
