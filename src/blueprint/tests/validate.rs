use super::super::model::TodoStatus;

#[test]
fn serializes_protocol_statuses() {
    assert_eq!(
        serde_json::to_string(&TodoStatus::Pending).unwrap(),
        "\"pending\""
    );
    assert_eq!(
        serde_json::to_string(&TodoStatus::InProgress).unwrap(),
        "\"in_progress\""
    );
    assert_eq!(TodoStatus::Blocked.marker(), "?");
}
