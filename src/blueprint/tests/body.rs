use super::super::ExternalBody;

#[test]
fn external_body_round_trips_protocol_looking_markdown() {
    let body = ExternalBody {
        lines: vec![
            "## Notes".into(),
            "[[target]] ^evidence-9".into(),
            "literal \\n stays literal".into(),
        ],
    };

    let rendered = body.render().expect("render body");

    assert_eq!(ExternalBody::parse("Plan", &rendered).unwrap(), body);
}

#[test]
fn external_body_uses_a_longer_fence_when_body_contains_tildes() {
    let body = ExternalBody {
        lines: vec!["~~~".into(), "content".into()],
    };

    let rendered = body.render().expect("render body");

    assert!(rendered.starts_with("~~~~\n"), "{rendered}");
    assert!(rendered.ends_with("\n~~~~"), "{rendered}");
}

#[test]
fn external_body_rejects_non_fenced_and_empty_content() {
    assert!(ExternalBody::parse("Plan", "## escaped").is_err());
    assert!(ExternalBody { lines: vec![] }.render().is_err());
    assert!(
        ExternalBody {
            lines: vec!["   ".into()]
        }
        .render()
        .is_err()
    );
}
