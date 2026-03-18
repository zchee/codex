use crate::config::ConfigToml;
use crate::config::types::RawMcpServerConfig;
use codex_features::FEATURES;
use codex_features::Feature;
use codex_features::legacy_feature_aliases;
use codex_protocol::openai_models::ModelsResponse;
use schemars::r#gen::SchemaGenerator;
use schemars::r#gen::SchemaSettings;
use schemars::schema::InstanceType;
use schemars::schema::ObjectValidation;
use schemars::schema::RootSchema;
use schemars::schema::Schema;
use schemars::schema::SchemaObject;
use serde_json::Map;
use serde_json::Value;
use std::path::Path;

/// Schema for the `[features]` map with known + legacy keys only.
pub(crate) fn features_schema(_schema_gen: &mut SchemaGenerator) -> Schema {
    let mut object = SchemaObject {
        instance_type: Some(InstanceType::Object.into()),
        ..Default::default()
    };

    let mut validation = ObjectValidation::default();
    for feature in FEATURES {
        validation.properties.insert(
            feature.key.to_string(),
            feature_toggle_schema(feature.id, /*legacy*/ false),
        );
    }
    for (legacy_key, feature) in legacy_feature_aliases() {
        validation.properties.insert(
            legacy_key.to_string(),
            feature_toggle_schema(feature, /*legacy*/ true),
        );
    }
    validation.additional_properties = Some(Box::new(Schema::Bool(false)));
    object.object = Some(Box::new(validation));

    Schema::Object(object)
}

fn feature_toggle_schema(feature: Feature, legacy: bool) -> Schema {
    let mut object = Map::from_iter([
        ("type".to_string(), Value::String("boolean".to_string())),
        (
            "description".to_string(),
            Value::String(feature_toggle_description(feature, legacy)),
        ),
    ]);
    if let Some(default) = feature.schema_default() {
        object.insert("default".to_string(), Value::Bool(default));
    }
    serde_json::from_value(Value::Object(object)).expect("valid feature toggle schema")
}

fn feature_toggle_description(feature: Feature, legacy: bool) -> String {
    let mut parts = Vec::new();
    if legacy {
        parts.push(format!(
            "Deprecated alias for `[features].{}`.",
            feature.key()
        ));
    }
    parts.push(feature.schema_description().to_string());
    if let Some(menu_description) = feature.stage().experimental_menu_description()
        && menu_description != feature.schema_description()
    {
        parts.push(menu_description.to_string());
    }
    parts.push(feature.schema_stage_description().to_string());
    parts.push(feature.schema_default_description().to_string());
    parts.join(" ")
}

/// Schema for the `[mcp_servers]` map using the raw input shape.
pub(crate) fn mcp_servers_schema(schema_gen: &mut SchemaGenerator) -> Schema {
    let mut object = SchemaObject {
        instance_type: Some(InstanceType::Object.into()),
        ..Default::default()
    };

    let validation = ObjectValidation {
        additional_properties: Some(Box::new(schema_gen.subschema_for::<RawMcpServerConfig>())),
        ..Default::default()
    };
    object.object = Some(Box::new(validation));

    Schema::Object(object)
}

/// Build the config schema for `config.toml`.
pub fn config_schema() -> RootSchema {
    SchemaSettings::draft07()
        .with(|settings| {
            settings.option_add_null_type = false;
        })
        .into_generator()
        .into_root_schema_for::<ConfigToml>()
}

/// Canonicalize a JSON value by sorting its keys.
fn canonicalize(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(canonicalize).collect()),
        Value::Object(map) => {
            let mut entries: Vec<_> = map.iter().collect();
            entries.sort_by(|(left, _), (right, _)| left.cmp(right));
            let mut sorted = Map::with_capacity(map.len());
            for (key, child) in entries {
                sorted.insert(key.clone(), canonicalize(child));
            }
            Value::Object(sorted)
        }
        _ => value.clone(),
    }
}

fn resolve_ref_definition<'a>(root: &'a Value, reference: &str) -> Option<&'a Value> {
    let definition = reference.strip_prefix("#/definitions/")?;
    root.get("definitions")?.get(definition)
}

fn copy_ref_enum_keywords(root: &Value, object: &mut Map<String, Value>) {
    if object.contains_key("enum") || object.contains_key("oneOf") || object.contains_key("anyOf") {
        return;
    }

    let reference = object.get("$ref").and_then(Value::as_str).or_else(|| {
        object
            .get("allOf")
            .and_then(Value::as_array)
            .and_then(|entries| {
                if entries.len() == 1 {
                    entries[0].get("$ref").and_then(Value::as_str)
                } else {
                    None
                }
            })
    });
    let Some(reference) = reference else {
        return;
    };
    let Some(definition) = resolve_ref_definition(root, reference) else {
        return;
    };
    let Some(definition_object) = definition.as_object() else {
        return;
    };

    if !object.contains_key("type")
        && let Some(value) = definition_object.get("type")
    {
        object.insert("type".to_string(), value.clone());
    }
    if let Some(value) = definition_object.get("enum") {
        object.insert("enum".to_string(), value.clone());
        return;
    }
    if let Some(value) = definition_object.get("oneOf") {
        object.insert("oneOf".to_string(), value.clone());
        return;
    }
    if let Some(value) = definition_object.get("anyOf") {
        object.insert("anyOf".to_string(), value.clone());
    }
}

fn supplement_ref_enum_choices(root: &Value, original: &Value, target: &mut Value) {
    match (original, target) {
        (Value::Array(original_items), Value::Array(target_items)) => {
            for (original_item, target_item) in original_items.iter().zip(target_items.iter_mut()) {
                supplement_ref_enum_choices(root, original_item, target_item);
            }
        }
        (Value::Object(original_object), Value::Object(target_object)) => {
            if let Some(original_properties) =
                original_object.get("properties").and_then(Value::as_object)
                && let Some(target_properties) = target_object
                    .get_mut("properties")
                    .and_then(Value::as_object_mut)
            {
                for (key, original_child) in original_properties {
                    let Some(target_child) = target_properties.get_mut(key) else {
                        continue;
                    };
                    if let Value::Object(target_child_object) = target_child {
                        copy_ref_enum_keywords(root, target_child_object);
                    }
                    supplement_ref_enum_choices(root, original_child, target_child);
                }
            }

            for (key, original_child) in original_object {
                if key == "properties" {
                    continue;
                }
                let Some(target_child) = target_object.get_mut(key) else {
                    continue;
                };
                supplement_ref_enum_choices(root, original_child, target_child);
            }
        }
        _ => {}
    }
}

fn bundled_model_slugs() -> anyhow::Result<Vec<String>> {
    let response: ModelsResponse = serde_json::from_str(include_str!("../../models.json"))?;
    Ok(response
        .models
        .into_iter()
        .map(|model| model.slug)
        .collect())
}

fn value_at_path_mut<'a>(value: &'a mut Value, path: &[&str]) -> Option<&'a mut Value> {
    let mut current = value;
    for key in path {
        current = current.as_object_mut()?.get_mut(*key)?;
    }
    Some(current)
}

fn insert_bundled_model_slug_choices(value: &mut Value, model_slugs: &[String]) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    if object.contains_key("enum") || object.contains_key("oneOf") || object.contains_key("anyOf") {
        return;
    }

    let known_values = Value::Array(
        model_slugs
            .iter()
            .cloned()
            .map(Value::String)
            .collect::<Vec<_>>(),
    );
    object.insert(
        "oneOf".to_string(),
        Value::Array(vec![
            Value::Object(Map::from_iter([
                (
                    "description".to_string(),
                    Value::String("Bundled model slugs from `core/models.json`.".to_string()),
                ),
                ("enum".to_string(), known_values.clone()),
                ("type".to_string(), Value::String("string".to_string())),
            ])),
            Value::Object(Map::from_iter([
                (
                    "description".to_string(),
                    Value::String(
                        "Any other model slug from a custom model catalog or provider.".to_string(),
                    ),
                ),
                (
                    "not".to_string(),
                    Value::Object(Map::from_iter([("enum".to_string(), known_values.clone())])),
                ),
                ("type".to_string(), Value::String("string".to_string())),
            ])),
        ]),
    );
}

fn supplement_bundled_model_choices(value: &mut Value) -> anyhow::Result<()> {
    let model_slugs = bundled_model_slugs()?;
    for path in [
        ["properties", "model"].as_slice(),
        ["properties", "review_model"].as_slice(),
        ["definitions", "ConfigProfile", "properties", "model"].as_slice(),
    ] {
        if let Some(property) = value_at_path_mut(value, path) {
            insert_bundled_model_slug_choices(property, &model_slugs);
        }
    }
    Ok(())
}

/// Render the config schema as pretty-printed JSON.
pub fn config_schema_json() -> anyhow::Result<Vec<u8>> {
    let schema = config_schema();
    let mut value = serde_json::to_value(schema)?;
    let root = value.clone();
    supplement_ref_enum_choices(&root, &root, &mut value);
    supplement_bundled_model_choices(&mut value)?;
    let value = canonicalize(&value);
    let json = serde_json::to_vec_pretty(&value)?;
    Ok(json)
}

/// Write the config schema fixture to disk.
pub fn write_config_schema(out_path: &Path) -> anyhow::Result<()> {
    let json = config_schema_json()?;
    std::fs::write(out_path, json)?;
    Ok(())
}

#[cfg(test)]
#[path = "schema_tests.rs"]
mod tests;
