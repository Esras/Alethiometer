use super::*;

#[test]
fn rows_carry_labels_descriptions_and_paths() {
    let f = settings_fields();
    let quiet = f.iter().find(|r| r.key == "quiet_days").unwrap();
    assert_eq!(
        (quiet.section.as_str(), quiet.group.as_str(), quiet.label.as_str()),
        ("general", "Topics", "Quiet after")
    );
    assert_eq!(quiet.desc, "No new signal for this long moves a topic under Quiet.");
    assert_eq!(quiet.widget, "number");
    assert_eq!(quiet.opts["unit"], "days");
    let profile = f.iter().find(|r| r.key == "ai.aws_profile").unwrap();
    assert_eq!(profile.section, "ai");
    assert_eq!(
        profile.desc, "Empty uses AWS_PROFILE, then default.",
        "first paragraph only"
    );
    assert_eq!(profile.path, vec![Value::from("ai"), Value::from("aws_profile")]);
    assert!(f
        .iter()
        .any(|r| r.key == "people.suggest_days" && r.section == "people"));
    assert!(
        f.iter().all(|r| r.key != "day_end_hour" && r.key != "life_calendars"),
        "hidden rows stay out"
    );
    assert!(f.iter().find(|r| r.key == "ai.allowed_providers").unwrap().opts["readonly"] == true);
    assert!(f.iter().find(|r| r.key == "fake_now").unwrap().opts["dev"] == true);
    assert_eq!(f.iter().find(|r| r.key == "demo_data").unwrap().widget, "toggle");
}

#[test]
fn every_kind_has_a_page() {
    for (kind, _) in crate::sources::KINDS {
        let f = source_fields(kind);
        assert!(
            f.iter().any(|r| r.key == "sources[].name" && r.group == "identity"),
            "{kind}"
        );
        assert!(f.iter().any(|r| r.key.starts_with("sources[].config.")), "{kind}");
    }
    let slack = source_fields("slack");
    let ch = slack.iter().find(|r| r.key == "sources[].config.channels").unwrap();
    assert_eq!((ch.widget.as_str(), ch.group.as_str()), ("slack_channels", "reads"));
    let lb = slack
        .iter()
        .find(|r| r.key == "sources[].config.lookback_hours")
        .unwrap();
    assert_eq!(lb.path, vec![Value::from("config"), Value::from("lookback_hours")]);
}
