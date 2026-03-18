use super::canonicalize;
use super::config_schema_json;
use super::write_config_schema;

use codex_protocol::openai_models::ModelsResponse;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use similar::TextDiff;
use tempfile::TempDir;

fn trim_single_trailing_newline(contents: &str) -> &str {
    contents.strip_suffix('\n').unwrap_or(contents)
}

fn config_schema_value() -> Value {
    let schema_json = config_schema_json().expect("serialize config schema");
    serde_json::from_slice(&schema_json).expect("decode schema json")
}

fn bundled_model_slug_values() -> Value {
    let response: ModelsResponse =
        serde_json::from_str(include_str!("../../models.json")).expect("parse bundled models.json");
    Value::Array(
        response
            .models
            .into_iter()
            .map(|model| Value::String(model.slug))
            .collect(),
    )
}

#[test]
fn config_schema_matches_fixture() {
    let fixture_path = codex_utils_cargo_bin::find_resource!("config.schema.json")
        .expect("resolve config schema fixture path");
    let fixture = std::fs::read_to_string(fixture_path).expect("read config schema fixture");
    let fixture_value: serde_json::Value =
        serde_json::from_str(&fixture).expect("parse config schema fixture");
    let schema_json = config_schema_json().expect("serialize config schema");
    let schema_value: serde_json::Value =
        serde_json::from_slice(&schema_json).expect("decode schema json");
    let fixture_value = canonicalize(&fixture_value);
    let schema_value = canonicalize(&schema_value);
    if fixture_value != schema_value {
        let expected =
            serde_json::to_string_pretty(&fixture_value).expect("serialize fixture json");
        let actual = serde_json::to_string_pretty(&schema_value).expect("serialize schema json");
        let diff = TextDiff::from_lines(&expected, &actual)
            .unified_diff()
            .header("fixture", "generated")
            .to_string();
        panic!(
            "Current schema for `config.toml` doesn't match the fixture. \
Run `just write-config-schema` to overwrite with your changes.\n\n{diff}"
        );
    }

    // Make sure the version in the repo matches exactly: https://github.com/openai/codex/pull/10977.
    let tmp = TempDir::new().expect("create temp dir");
    let tmp_path = tmp.path().join("config.schema.json");
    write_config_schema(&tmp_path).expect("write config schema to temp path");
    let tmp_contents =
        std::fs::read_to_string(&tmp_path).expect("read back config schema from temp path");
    #[cfg(windows)]
    let fixture = fixture.replace("\r\n", "\n");
    #[cfg(windows)]
    let tmp_contents = tmp_contents.replace("\r\n", "\n");

    assert_eq!(
        trim_single_trailing_newline(&fixture),
        trim_single_trailing_newline(&tmp_contents),
        "fixture should match exactly with generated schema"
    );
}

#[test]
fn config_schema_includes_documented_top_level_metadata() {
    let schema = config_schema_value();
    let properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .expect("root schema properties");

    assert_eq!(properties["allow_login_shell"]["default"], json!(true));
    assert_eq!(
        properties["background_terminal_max_timeout"]["default"],
        json!(300_000)
    );
    assert_eq!(
        properties["check_for_update_on_startup"]["default"],
        json!(true)
    );
    assert_eq!(
        properties["project_root_markers"]["default"],
        json!([".git"])
    );
    assert_eq!(
        properties["cli_auth_credentials_store"]["default"],
        json!("file")
    );
    assert_eq!(
        properties["mcp_oauth_credentials_store"]["default"],
        json!("auto")
    );
    assert_eq!(
        properties["experimental_use_freeform_apply_patch"]["default"],
        json!(false)
    );
    assert!(
        properties["projects"]["description"]
            .as_str()
            .expect("projects description")
            .contains("Per-project trust settings")
    );
    assert!(
        properties["model_reasoning_effort"]["description"]
            .as_str()
            .expect("model_reasoning_effort description")
            .contains("Reasoning effort")
    );
}

#[test]
fn config_schema_includes_feature_flag_metadata() {
    let schema = config_schema_value();
    let features = schema
        .get("properties")
        .and_then(Value::as_object)
        .and_then(|properties| properties.get("features"))
        .and_then(|features| features.get("properties"))
        .and_then(Value::as_object)
        .expect("features properties");

    let multi_agent = &features["multi_agent"];
    assert_eq!(multi_agent["default"], json!(true));
    assert!(
        multi_agent["description"]
            .as_str()
            .expect("multi_agent description")
            .contains("Enable collab tools.")
    );
    assert!(
        multi_agent["description"]
            .as_str()
            .expect("multi_agent description")
            .contains("Status: stable.")
    );

    let collab = &features["collab"];
    assert_eq!(collab["default"], json!(true));
    assert!(
        collab["description"]
            .as_str()
            .expect("collab description")
            .contains("Deprecated alias for `[features].multi_agent`.")
    );

    let apps = &features["apps"];
    assert_eq!(apps["default"], json!(false));
    let apps_description = apps["description"].as_str().expect("apps description");
    assert!(apps_description.contains("Enable apps."));
    assert!(apps_description.contains("Status: experimental."));
    assert!(apps_description.contains("Use a connected ChatGPT App using \"$\"."));

    let unified_exec = &features["unified_exec"];
    assert!(unified_exec.get("default").is_none());
    assert!(
        unified_exec["description"]
            .as_str()
            .expect("unified_exec description")
            .contains(
                "Enabled by default on non-Windows platforms and disabled by default on Windows."
            )
    );
}

#[test]
fn config_schema_copies_enum_choices_to_ref_backed_properties() {
    let schema = config_schema_value();
    let properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .expect("root schema properties");

    assert_eq!(
        properties["model_reasoning_effort"]["enum"],
        json!(["none", "minimal", "low", "medium", "high", "xhigh"])
    );
    assert_eq!(
        properties["model_verbosity"]["enum"],
        json!(["low", "medium", "high"])
    );
    assert_eq!(properties["service_tier"]["enum"], json!(["fast", "flex"]));
    assert_eq!(
        properties["personality"]["enum"],
        json!(["none", "friendly", "pragmatic"])
    );

    let summary_variants = properties["model_reasoning_summary"]["oneOf"]
        .as_array()
        .expect("model_reasoning_summary oneOf");
    assert!(summary_variants.iter().any(|entry| {
        entry
            .get("enum")
            .is_some_and(|variants| variants == &json!(["auto", "concise", "detailed"]))
    }));
}

#[test]
fn config_schema_includes_bundled_model_slug_choices() {
    let schema = config_schema_value();
    let expected_model_slugs = bundled_model_slug_values();
    let properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .expect("root schema properties");

    let model_choices = properties["model"]["oneOf"]
        .as_array()
        .expect("model oneOf");
    assert_eq!(model_choices[0]["enum"], expected_model_slugs);
    assert_eq!(
        properties["review_model"]["oneOf"][0]["enum"],
        model_choices[0]["enum"]
    );
    assert_eq!(model_choices[1]["not"]["enum"], model_choices[0]["enum"]);

    assert_eq!(
        schema["definitions"]["ConfigProfile"]["properties"]["model"]["oneOf"][0]["enum"],
        model_choices[0]["enum"]
    );
}
