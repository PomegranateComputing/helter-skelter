//! Proves that bridge/protocol/*.schema.json, the fixtures in
//! bridge/messages/fixtures/, and this crate's Rust types all agree.
//!
//! For every fixture:
//! 1. It must validate against envelope.schema.json.
//! 2. Its `payload` must validate against the schema matching its `kind`.
//! 3. For `command.request`, `payload.params` must validate against the
//!    schema matching its `action`.
//! 4. It must deserialize into `common::protocol::Envelope`, pass
//!    `Envelope::validate()`, and re-serialize back to the exact same JSON.
//!
//! A schema change without a matching Rust type change (or vice versa) fails
//! step 2/3 or step 4 -- that's the "stay in sync" check.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use common::protocol::Envelope;
use jsonschema::Validator;
use serde_json::Value;

fn protocol_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bridge/protocol")
}

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bridge/messages/fixtures")
}

fn load_schema(relative_path: &str) -> Value {
    let path = protocol_dir().join(relative_path);
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("reading schema {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("parsing schema {}: {e}", path.display()))
}

fn validator_for(relative_path: &str) -> Validator {
    let schema = load_schema(relative_path);
    Validator::new(&schema).unwrap_or_else(|e| panic!("compiling schema {relative_path}: {e}"))
}

fn message_schema_path(kind: &str) -> &'static str {
    match kind {
        "hello" => "messages/hello.schema.json",
        "heartbeat" => "messages/heartbeat.schema.json",
        "observation.snapshot" => "messages/observation_snapshot.schema.json",
        "command.request" => "messages/command_request.schema.json",
        "command.result" => "messages/command_result.schema.json",
        "shutdown" => "messages/shutdown.schema.json",
        "ack" => "messages/ack.schema.json",
        other => panic!("unknown message kind in fixture: {other}"),
    }
}

fn command_schema_path(action: &str) -> String {
    format!("commands/{action}.schema.json")
}

fn load_fixtures() -> Vec<(String, Value)> {
    let dir = fixtures_dir();
    let mut entries: Vec<_> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("reading fixtures dir {}: {e}", dir.display()))
        .map(|entry| entry.expect("dir entry").path())
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("json"))
        .collect();
    entries.sort();

    entries
        .into_iter()
        .map(|path| {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap()
                .to_string();
            let text =
                fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let value: Value = serde_json::from_str(&text)
                .unwrap_or_else(|e| panic!("parsing {}: {e}", path.display()));
            (name, value)
        })
        .collect()
}

#[test]
fn every_fixture_validates_and_round_trips() {
    let envelope_validator = validator_for("envelope.schema.json");
    let mut message_validators: HashMap<&'static str, Validator> = HashMap::new();
    let mut command_validators: HashMap<String, Validator> = HashMap::new();

    for (name, fixture) in load_fixtures() {
        envelope_validator
            .validate(&fixture)
            .unwrap_or_else(|e| panic!("{name}: fails envelope.schema.json: {e}"));

        let kind = fixture["kind"]
            .as_str()
            .unwrap_or_else(|| panic!("{name}: missing/non-string kind"));
        let payload = &fixture["payload"];

        let msg_schema_path = message_schema_path(kind);
        let message_validator = message_validators
            .entry(msg_schema_path)
            .or_insert_with(|| validator_for(msg_schema_path));
        message_validator
            .validate(payload)
            .unwrap_or_else(|e| panic!("{name}: payload fails {msg_schema_path}: {e}"));

        if kind == "command.request" {
            let action = payload["action"]
                .as_str()
                .unwrap_or_else(|| panic!("{name}: command.request missing action"));
            let cmd_schema_path = command_schema_path(action);
            let command_validator = command_validators
                .entry(cmd_schema_path.clone())
                .or_insert_with(|| validator_for(&cmd_schema_path));
            command_validator
                .validate(&payload["params"])
                .unwrap_or_else(|e| panic!("{name}: params fail {cmd_schema_path}: {e}"));
        }

        let envelope: Envelope = serde_json::from_value(fixture.clone())
            .unwrap_or_else(|e| panic!("{name}: does not deserialize into Envelope: {e}"));
        envelope
            .validate()
            .unwrap_or_else(|e| panic!("{name}: Envelope::validate() failed: {e}"));

        let round_tripped = serde_json::to_value(&envelope)
            .unwrap_or_else(|e| panic!("{name}: re-serializing: {e}"));
        assert_eq!(
            round_tripped, fixture,
            "{name}: round-tripped Envelope does not match original fixture"
        );
    }
}

#[test]
fn command_result_without_correlation_id_is_rejected() {
    use common::protocol::{CommandResult, Envelope, Payload};
    use uuid::Uuid;

    let payload = Payload::CommandResult(CommandResult {
        engine_cost: Some(0),
        engine_error: None,
    });
    let err = Envelope::new(Uuid::now_v7(), payload, None, None, None)
        .expect_err("command.result with correlation_id=None must be rejected");
    assert!(matches!(
        err,
        common::protocol::ProtocolError::MissingCorrelationId(
            common::protocol::Kind::CommandResult
        )
    ));
}

#[test]
fn ack_without_correlation_id_is_rejected() {
    use common::protocol::{Ack, Envelope, Payload};
    use uuid::Uuid;

    let err = Envelope::new(
        Uuid::now_v7(),
        Payload::Ack(Ack::default()),
        None,
        None,
        None,
    )
    .expect_err("ack with correlation_id=None must be rejected");
    assert!(matches!(
        err,
        common::protocol::ProtocolError::MissingCorrelationId(common::protocol::Kind::Ack)
    ));
}

#[test]
fn hello_does_not_require_correlation_id() {
    use common::protocol::{Envelope, Hello, Payload, Role};
    use uuid::Uuid;

    let payload = Payload::Hello(Hello {
        role: Role::Orchestrator,
        bridge_version: "0.1.0".to_string(),
        openrct2_version: "0.4.13".to_string(),
    });
    Envelope::new(
        Uuid::now_v7(),
        payload,
        None,
        Some(common::protocol::Status::Ok),
        None,
    )
    .expect("hello must not require a correlation_id");
}
