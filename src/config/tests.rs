use super::*;
use crate::{
    browsers::BrowserFamily,
    rules::{Condition, RuleAction},
};

struct Fixture {
    directory: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        Self {
            directory: tempfile::tempdir().unwrap(),
        }
    }
    fn store(&self) -> ConfigStore {
        ConfigStore::at(self.directory.path())
    }
    fn path(&self) -> PathBuf {
        self.directory.path().join("settings.json")
    }
    fn legacy(&self) -> Vec<u8> {
        let bytes = br#"{"names":{"Default":"Work"},"sites":[{"site":"https://example.com","profile":"Default"}],"always_ask":false,"last_used":[{"id":"Default","at":1000}]}"#.to_vec();
        fs::write(self.path(), &bytes).unwrap();
        bytes
    }
}

#[test]
fn v2_upgrade_preserves_rules_and_backs_up_before_enabling_query_resolution() {
    let fixture = Fixture::new();
    let bytes = br#"{"version":2,"names":{"edge:Default":"Office"},"rules":[{"name":"Work","enabled":true,"all":[{"kind":"origin","value":"https://example.com"}],"action":{"kind":"open","identity":"edge:Default"}}],"rules_revision":7,"always_ask":false,"last_used":[{"id":"edge:Default","at":1000}]}"#;
    fs::write(fixture.path(), bytes).unwrap();
    let migrated = fixture.store().load().unwrap();
    assert_eq!(migrated.version, 3);
    assert_eq!(migrated.rules_revision, 7);
    assert_eq!(
        migrated.names[&IdentityId::new("edge", "Default").unwrap()],
        "Office"
    );
    assert_eq!(
        fs::read(fixture.directory.path().join("settings.v2.backup.json")).unwrap(),
        bytes
    );
    let profiles = [Profile::preview(BrowserFamily::Edge, "Default", "Office").unwrap()];
    let resolved = migrated.resolution.resolve(
        &url::Url::parse("https://wrap.test/?url=https%3A%2F%2Fexample.com%2Fproject").unwrap(),
    );
    assert_eq!(
        resolved
            .decide(
                &RuleSet::compile(&migrated.rules).unwrap(),
                &profiles,
                false
            )
            .identity,
        Some(profiles[0].id.clone())
    );
    let mut changed = migrated.resolution.clone();
    changed.parameters.clear();
    fixture
        .store()
        .replace_routing(migrated.rules.clone(), changed, migrated.rules_revision)
        .unwrap();
    assert!(
        fixture
            .store()
            .replace_routing(migrated.rules, migrated.resolution, migrated.rules_revision)
            .is_err()
    );
    assert!(
        fixture
            .store()
            .load()
            .unwrap()
            .resolution
            .parameters
            .is_empty()
    );
}

#[test]
fn browser_recency_and_user_names_stay_independent_across_restarts() {
    let fixture = Fixture::new();
    let mut chrome = Profile::preview(BrowserFamily::Chrome, "Default", "Work").unwrap();
    let edge = Profile::preview(BrowserFamily::Edge, "Default", "Work").unwrap();
    chrome.browser_active = Some(OffsetDateTime::UNIX_EPOCH);
    let mut profiles = [edge.clone(), chrome.clone()];
    Config::default().sort_profiles(&mut profiles);
    assert_eq!(profiles[0].id, chrome.id);
    fixture
        .store()
        .update(|config| {
            config.mark_used(&chrome.id);
            config.mark_used(&edge.id);
            config.names.insert(edge.id.clone(), "Testing".into());
            Ok(())
        })
        .unwrap();
    let reopened = fixture.store().load().unwrap();
    reopened.sort_profiles(&mut profiles);
    assert_eq!(profiles[0].id, edge.id);
    assert_eq!(reopened.names.get(&chrome.id), None);
    assert_eq!(reopened.names[&edge.id], "Testing");
}

#[cfg(windows)]
#[test]
fn failed_migration_write_preserves_old_file_and_can_retry() {
    let fixture = Fixture::new();
    let original = fixture.legacy();
    let initial_permissions = fs::metadata(fixture.path()).unwrap().permissions();
    let mut readonly = initial_permissions.clone();
    readonly.set_readonly(true);
    fs::set_permissions(fixture.path(), readonly).unwrap();
    let result = fixture.store().load();
    fs::set_permissions(fixture.path(), initial_permissions).unwrap();
    assert!(result.is_err());
    assert_eq!(fs::read(fixture.path()).unwrap(), original);
    assert_eq!(
        fs::read(fixture.directory.path().join("settings.v1.backup.json")).unwrap(),
        original
    );
    assert_eq!(fixture.store().load().unwrap().rules.len(), 1);
}

#[test]
fn upgrade_preserves_old_routes_names_recency_and_exact_backup_bytes() {
    let fixture = Fixture::new();
    let bytes = fixture.legacy();
    let config = fixture.store().load().unwrap();
    let identity = IdentityId::new("chrome", "Default").unwrap();
    assert_eq!(config.names[&identity], "Work");
    assert_eq!(config.last_used[0].at.unix_timestamp(), 1);
    let profiles = [
        Profile::preview(BrowserFamily::Chrome, "Default", "Original").unwrap(),
        Profile::preview(BrowserFamily::Edge, "Default", "Other").unwrap(),
    ];
    let decision = RuleSet::compile(&config.rules).unwrap().decide(
        &url::Url::parse("https://example.com/a").unwrap(),
        &profiles,
        false,
    );
    assert_eq!(decision.identity, Some(identity));
    assert_eq!(
        fs::read(fixture.directory.path().join("settings.v1.backup.json")).unwrap(),
        bytes
    );
    assert_eq!(fixture.store().load().unwrap().rules, config.rules);
}

#[test]
fn migration_keeps_large_legacy_site_lists_and_long_hosts_routable() {
    let fixture = Fixture::new();
    let host = format!("{}.{}.example.com", "a".repeat(63), "b".repeat(63));
    let mut sites = (0..600)
        .map(|n| format!(r#"{{"site":"https://site{n}.test","profile":"Default"}}"#))
        .collect::<Vec<_>>();
    sites.push(format!(
        r#"{{"site":"https://{host}","profile":"Default"}}"#
    ));
    fs::write(
        fixture.path(),
        format!(r#"{{"sites":[{}]}}"#, sites.join(",")),
    )
    .unwrap();
    let config = fixture.store().load().unwrap();
    let profiles = [Profile::preview(BrowserFamily::Chrome, "Default", "Work").unwrap()];
    assert_eq!(config.rules.len(), 601);
    let decision = RuleSet::compile(&config.rules).unwrap().decide(
        &url::Url::parse(&format!("https://{host}/project")).unwrap(),
        &profiles,
        false,
    );
    assert_eq!(decision.identity, Some(profiles[0].id.clone()));
    assert_eq!(fixture.store().load().unwrap().rules, config.rules);
}

#[test]
fn corrupt_unknown_and_conflicting_backups_preserve_original_settings() {
    for bytes in [
        b"broken".as_slice(),
        br#"{"version":99}"#,
        br#"{"sites":[],"typo":true}"#,
    ] {
        let fixture = Fixture::new();
        fs::write(fixture.path(), bytes).unwrap();
        assert!(
            fixture
                .store()
                .update(|config| {
                    config.always_ask = true;
                    Ok(())
                })
                .is_err()
        );
        assert_eq!(fs::read(fixture.path()).unwrap(), bytes);
    }
    let fixture = Fixture::new();
    let bytes = fixture.legacy();
    fs::write(
        fixture.directory.path().join("settings.v1.backup.json"),
        b"previous backup",
    )
    .unwrap();
    assert!(fixture.store().load().is_err());
    assert_eq!(fs::read(fixture.path()).unwrap(), bytes);
}

#[test]
fn concurrent_remember_operations_merge_and_recency_updates_allow_rule_edits() {
    let fixture = Fixture::new();
    std::thread::scope(|scope| {
        for n in 0..8 {
            let store = fixture.store();
            scope.spawn(move || {
                store
                    .update(|config| {
                        config.remember(
                            &url::Url::parse(&format!("https://site{n}.test")).unwrap(),
                            &IdentityId::new("chrome", "Default").unwrap(),
                        );
                        Ok(())
                    })
                    .unwrap()
            });
        }
    });
    let config = fixture.store().load().unwrap();
    assert_eq!(config.rules.len(), 8);
    fixture
        .store()
        .update(|config| {
            config.mark_used(&IdentityId::new("edge", "Default").unwrap());
            Ok(())
        })
        .unwrap();
    fixture
        .store()
        .replace_rules(vec![], config.rules_revision)
        .unwrap();
    assert_eq!(
        fixture.store().load().unwrap().last_used[0].id.to_string(),
        "edge:Default"
    );
}

#[test]
fn stale_or_invalid_rules_leave_saved_configuration_intact() {
    let fixture = Fixture::new();
    fixture.legacy();
    let initial = fixture.store().load().unwrap();
    fixture
        .store()
        .update(|config| {
            config.remember(
                &url::Url::parse("https://other.test").unwrap(),
                &IdentityId::new("edge", "Default").unwrap(),
            );
            Ok(())
        })
        .unwrap();
    let before = fs::read(fixture.path()).unwrap();
    assert!(
        fixture
            .store()
            .replace_rules(vec![], initial.rules_revision)
            .is_err()
    );
    let revision = fixture.store().load().unwrap().rules_revision;
    let invalid = Rule {
        name: "invalid".into(),
        enabled: true,
        all: vec![Condition::Port { min: 2, max: 1 }],
        any: vec![],
        exclude: vec![],
        action: RuleAction::Ask,
    };
    assert!(
        fixture
            .store()
            .replace_rules(vec![invalid], revision)
            .is_err()
    );
    assert_eq!(fs::read(fixture.path()).unwrap(), before);
}

#[test]
fn legacy_optional_fields_and_remembered_replacement_keep_working() {
    let fixture = Fixture::new();
    fs::write(fixture.path(), br#"{"sites":[],"always_ask":true}"#).unwrap();
    assert!(fixture.store().load().unwrap().always_ask);
    let url = url::Url::parse("https://example.com/path?token=private").unwrap();
    fixture
        .store()
        .update(|config| {
            config.remember(&url, &IdentityId::new("chrome", "Default")?);
            config.remember(&url, &IdentityId::new("edge", "Default")?);
            Ok(())
        })
        .unwrap();
    let config = fixture.store().load().unwrap();
    assert_eq!(config.rules.len(), 1);
    assert!(
        matches!(&config.rules[0].action, RuleAction::Open { identity } if identity.to_string() == "edge:Default")
    );
    assert!(
        !fs::read_to_string(fixture.path())
            .unwrap()
            .contains("private")
    );
}
