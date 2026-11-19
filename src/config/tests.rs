use super::*;
use crate::{browsers::BrowserFamily, rules::RuleSet};

const WORK: &str = r#"
# Custom handoff note
[[rules]]
name = "Work"
enabled = true
all = [{ kind = "host", value = "example.com" }]
action = { kind = "open", identity = "chrome:Default" }
"#;
const BROKEN: &str = r#"
[[rules]]
name = "Invalid ports"
enabled = true
all = [{ kind = "port", min = 4000, max = 3000 }]
action = { kind = "ask" }
"#;

#[test]
fn invalid_reload_keeps_routing_after_restart_and_leaves_the_draft_for_repair() {
    let directory = tempfile::tempdir().unwrap();
    let store = ConfigStore::at(directory.path());
    fs::write(store.rules_path(), WORK).unwrap();
    assert!(store.load().unwrap().rules.is_empty());
    store.reload_rules().unwrap();
    fs::write(store.rules_path(), BROKEN).unwrap();
    let error = store.reload_rules().err().unwrap();
    assert!(format!("{error:#}").contains("Invalid ports"));
    assert_eq!(fs::read_to_string(store.rules_path()).unwrap(), BROKEN);
    let profile = Profile::preview(BrowserFamily::Chrome, "Default", "Work").unwrap();
    let restarted = ConfigStore::at(directory.path()).load().unwrap();
    let url = url::Url::parse("https://example.com/project").unwrap();
    assert_eq!(
        RuleSet::compile(&restarted.rules)
            .unwrap()
            .decide(&url, &[profile], false)
            .identity,
        Some(IdentityId::new("chrome", "Default").unwrap())
    );
    fs::write(store.rules_path(), "rules = []").unwrap();
    assert!(store.reload_rules().unwrap().rules.is_empty());
}

#[test]
fn replace_validates_before_writing_and_preserves_personal_preferences() {
    let directory = tempfile::tempdir().unwrap();
    let store = ConfigStore::at(directory.path());
    let identity = IdentityId::new("chrome", "Default").unwrap();
    store
        .update(|config| {
            config.names.insert(identity.clone(), "Office".into());
            config.mark_used(&identity);
            config.always_ask = true;
            Ok(())
        })
        .unwrap();
    let first = store.replace_document(WORK, None).unwrap();
    for invalid in [BROKEN, "rules = [", "rules = []\nextra = true"] {
        let before = fs::read(store.state_path()).unwrap();
        assert!(store.replace_document(invalid, None).is_err());
        assert_eq!(fs::read(store.state_path()).unwrap(), before);
        assert_eq!(
            fs::read_to_string(store.rules_path()).unwrap(),
            first.routing_text
        );
    }
    let config = store.replace_document("rules = []", None).unwrap();
    assert_eq!(config.names[&identity], "Office");
    assert_eq!(config.last_used[0].at, first.last_used[0].at);
    assert!(config.always_ask);
    assert!(config.rules.is_empty());
}

#[test]
fn disk_drafts_and_other_windows_cannot_be_overwritten_by_stale_window_edits() {
    let directory = tempfile::tempdir().unwrap();
    let store = ConfigStore::at(directory.path());
    let old = store.replace_document(WORK, None).unwrap();
    fs::write(store.rules_path(), BROKEN).unwrap();
    assert!(
        store
            .replace_document("rules = []", Some(&old.routing_text))
            .is_err()
    );
    assert!(store.forget(0, &old.routing_text).is_err());
    store
        .update(|config| {
            config.mark_used(&IdentityId::new("edge", "Default")?);
            Ok(())
        })
        .unwrap();
    assert_eq!(fs::read_to_string(store.rules_path()).unwrap(), BROKEN);
    store.replace_document("rules = []", None).unwrap();
    assert!(store.forget(0, &old.routing_text).is_err());
}

#[test]
fn concurrent_remember_and_forget_preserve_other_rules_and_comments() {
    let directory = tempfile::tempdir().unwrap();
    let store = ConfigStore::at(directory.path());
    store.replace_document(WORK, None).unwrap();
    std::thread::scope(|scope| {
        for index in 0..8 {
            let store = ConfigStore::at(directory.path());
            scope.spawn(move || {
                store
                    .update(|config| {
                        config.remember(
                            &url::Url::parse(&format!("https://site{index}.test")).unwrap(),
                            &IdentityId::new("chrome", "Default")?,
                        );
                        Ok(())
                    })
                    .unwrap();
            });
        }
    });
    let config = store.load().unwrap();
    assert_eq!(config.rules.len(), 9);
    let config = store.forget(1, &config.routing_text).unwrap();
    assert_eq!(config.rules.len(), 8);
    assert!(config.routing_text.contains("# Custom handoff note"));
    assert!(config.routing_text.starts_with("# 轻舟 Linklane"));
    assert_eq!(config.rules[0].name, "Work");
}

#[test]
fn remember_updates_an_existing_origin_in_each_toml_style_and_removes_the_last_rule() {
    for text in [
        r#"rules = [{ name = "Site", enabled = false, all = [{ kind = "origin", value = "https://example.com" }], action = { kind = "ask" } }]"#,
        "[[rules]]\nname = 'Site'\nenabled = false\nall = [{kind = 'origin', value = 'https://example.com'}]\n[rules.action]\nkind = 'ask'\n",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let store = ConfigStore::at(directory.path());
        store.replace_document(text, None).unwrap();
        let next = store
            .update(|config| {
                config.remember(
                    &url::Url::parse("https://example.com/p?secret=private").unwrap(),
                    &IdentityId::new("edge", "Default")?,
                );
                Ok(())
            })
            .unwrap();
        assert_eq!(next.rules.len(), 1);
        assert!(next.rules[0].enabled);
        assert!(
            matches!(&next.rules[0].action, crate::rules::RuleAction::Open { identity } if identity.to_string() == "edge:Default")
        );
        assert!(!next.routing_text.contains("secret=private"));
        assert!(
            store
                .forget(0, &next.routing_text)
                .unwrap()
                .rules
                .is_empty()
        );
    }
}

#[test]
fn annotated_header_examples_validate_and_resolution_settings_round_trip() {
    let uncomment = |start: &str| {
        crate::rule_document::GUIDE
            .lines()
            .skip_while(|line| *line != start)
            .take_while(|line| *line != "#")
            .map(|line| line.strip_prefix("# ").unwrap())
            .collect::<Vec<_>>()
            .join("\n")
    };
    let text = format!(
        "{}\n{}",
        uncomment("# [[rules]]"),
        uncomment("# [resolution]")
    );
    let document = RuleDocument::parse(&text).unwrap();
    let encoded = document.encode().unwrap();
    assert!(RuleDocument::parse(&encoded).unwrap() == document);
}
