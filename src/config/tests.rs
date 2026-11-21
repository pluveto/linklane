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
                            0,
                            &IdentityId::new("chrome", "Default")?,
                        )
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
                    0,
                    &IdentityId::new("edge", "Default")?,
                )
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
fn nested_path_choices_persist_as_exceptions_to_an_existing_host_preference() {
    for site in [
        r#"rules = [{ name = "Host", enabled = true, all = [{ kind = "host", value = "example.test" }], action = { kind = "open", identity = "chrome:Default" } }]"#,
        "# Website default\n[[rules]]\nname = 'Host'\nenabled = true\nall = [{kind = 'host', value = 'example.test'}]\n[rules.action]\nkind = 'open'\nidentity = 'chrome:Default'\n",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let store = ConfigStore::at(directory.path());
        store.replace_document(site, None).unwrap();
        let target =
            url::Url::parse("https://example.test/Team/One.Utils/path/1?secret=private#comment")
                .unwrap();
        let chrome = Profile::preview(BrowserFamily::Chrome, "Default", "General").unwrap();
        let edge = Profile::preview(BrowserFamily::Edge, "Default", "Team").unwrap();
        let brave = Profile::preview(BrowserFamily::Brave, "Default", "Project").unwrap();
        let profiles = [chrome, edge, brave];
        for (scope, profile) in [(2, &profiles[2]), (1, &profiles[1]), (0, &profiles[0])] {
            store
                .update(|config| config.remember(&target, scope, &profile.id))
                .unwrap();
        }
        let saved = store.load().unwrap();
        assert_eq!(saved.rules.len(), 4);
        assert!(!saved.routing_text.contains("secret=private"));
        assert!(!saved.routing_text.contains("#comment"));
        if site.contains("# Website default") {
            assert!(saved.routing_text.contains("# Website default"));
        }
        let rules = RuleSet::compile(&saved.rules).unwrap();
        for (path, expected) in [
            ("/Team/One.Utils/path/2", Some(&profiles[2].id)),
            ("/Team/One.Utils-extra", Some(&profiles[1].id)),
            ("/Team/one.utils", Some(&profiles[1].id)),
            ("/Team", Some(&profiles[1].id)),
            ("/team/One.Utils", Some(&profiles[0].id)),
            ("/Team-extra/One.Utils", Some(&profiles[0].id)),
        ] {
            let url = url::Url::parse(&format!("https://example.test{path}")).unwrap();
            assert_eq!(
                rules.decide(&url, &profiles, false).identity.as_ref(),
                expected,
                "{url}"
            );
        }
    }
}

#[test]
fn suggestions_use_literal_uri_segments_and_keep_scheme_host_and_port() {
    let labels = |url| {
        RememberScope::suggest(&url::Url::parse(url).unwrap())
            .into_iter()
            .map(|scope| scope.label())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        labels("http://[::1]:8080//A%2FB/C/?token=private#fragment"),
        [
            "http://[::1]:8080 下的所有链接",
            "//A%2FB 及下级",
            "//A%2FB/C 及下级"
        ]
    );
    assert_eq!(
        labels("https://example.test/path"),
        ["https://example.test 下的所有链接", "/path 及下级"]
    );
    assert_eq!(
        labels("https://example.test/?key=value"),
        ["https://example.test 下的所有链接"]
    );
}

#[test]
fn generic_section_overrides_the_remembered_site_at_segment_and_origin_boundaries() {
    let directory = tempfile::tempdir().unwrap();
    let store = ConfigStore::at(directory.path());
    store.replace_document(WORK, None).unwrap();
    let url = url::Url::parse("https://example.com:8443/docs/start").unwrap();
    let site = Profile::preview(BrowserFamily::Edge, "Default", "Site").unwrap();
    let section = Profile::preview(BrowserFamily::Brave, "Default", "Docs").unwrap();
    let fallback = Profile::preview(BrowserFamily::Chrome, "Default", "Other").unwrap();
    store
        .update(|config| config.remember(&url, 0, &site.id))
        .unwrap();
    let config = store
        .update(|config| config.remember(&url, 1, &section.id))
        .unwrap();
    let profiles = [site, section, fallback];
    let rules = RuleSet::compile(&config.rules).unwrap();
    for (address, expected) in [
        ("https://example.com:8443/docs", &profiles[1].id),
        ("https://example.com:8443/docs/api", &profiles[1].id),
        ("https://example.com:8443/docs-v2/start", &profiles[0].id),
        ("https://example.com/docs/start", &profiles[2].id),
    ] {
        let url = url::Url::parse(address).unwrap();
        assert_eq!(
            rules.decide(&url, &profiles, false).identity.as_ref(),
            Some(expected)
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
