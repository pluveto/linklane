use super::*;
use crate::browsers::BrowserFamily;

struct Fixture {
    profiles: Vec<Profile>,
}

impl Fixture {
    fn new() -> Self {
        Self {
            profiles: vec![
                Profile::preview(BrowserFamily::Chrome, "Default", "work").unwrap(),
                Profile::preview(BrowserFamily::Edge, "Default", "test").unwrap(),
            ],
        }
    }
    fn route(&self, rules: &[Rule], url: &str) -> Decision {
        RuleSet::compile(rules).unwrap().decide(
            &crate::routing::parse_link(url).unwrap(),
            &self.profiles,
            false,
        )
    }
    fn rule(&self, condition: Condition) -> Rule {
        Rule {
            name: "test".into(),
            enabled: true,
            all: vec![condition],
            any: vec![],
            exclude: vec![],
            action: RuleAction::Open {
                identity: self.profiles[0].id.clone(),
            },
        }
    }
}

#[test]
fn remembered_origins_keep_protocol_and_port_and_exclude_other_hosts() {
    let fixture = Fixture::new();
    let rules = [Rule::remembered(
        "https://example.com".into(),
        fixture.profiles[0].id.clone(),
    )];
    assert!(
        fixture
            .route(&rules, "https://EXAMPLE.com:443/path?token=secret")
            .identity
            .is_some()
    );
    for url in [
        "http://example.com",
        "https://example.com:8443",
        "https://sub.example.com",
        "https://example.com.evil.test",
    ] {
        assert!(fixture.route(&rules, url).identity.is_none(), "{url}");
    }
    assert!(
        !fixture
            .route(&rules, "https://example.com?token=secret")
            .explanation
            .contains("secret")
    );
}

#[test]
fn host_subdomains_idn_and_path_boundaries_use_url_semantics() {
    let fixture = Fixture::new();
    let mut rule = fixture.rule(Condition::Host {
        value: "EXAMPLE.com".into(),
        subdomains: true,
    });
    rule.all.push(Condition::PathPrefix {
        value: "/work".into(),
    });
    for url in ["https://example.com/work", "https://a.example.com/work/a"] {
        assert!(
            fixture
                .route(std::slice::from_ref(&rule), url)
                .identity
                .is_some()
        );
    }
    for url in [
        "https://example.com/workshop",
        "https://example.com.evil.test/work",
        "https://example.com/WORK",
    ] {
        assert!(
            fixture
                .route(std::slice::from_ref(&rule), url)
                .identity
                .is_none()
        );
    }
    let idn = fixture.rule(Condition::Host {
        value: "例子.测试".into(),
        subdomains: false,
    });
    assert!(
        fixture
            .route(&[idn], "https://例子.测试")
            .identity
            .is_some()
    );
}

#[test]
fn port_ranges_query_decoding_and_any_exclusions_combine() {
    let fixture = Fixture::new();
    let mut rule = fixture.rule(Condition::Port {
        min: 3000,
        max: 3999,
    });
    rule.any = vec![
        Condition::Host {
            value: "localhost".into(),
            subdomains: false,
        },
        Condition::Host {
            value: "127.0.0.1".into(),
            subdomains: false,
        },
    ];
    rule.all.push(Condition::Query {
        key: "tenant".into(),
        value: Some(TextPattern::Exact("测试 x".into())),
    });
    rule.exclude.push(Condition::Query {
        key: "private".into(),
        value: None,
    });
    let rules = [rule];
    assert!(
        fixture
            .route(
                &rules,
                "http://localhost:3000/?tenant=no&tenant=%E6%B5%8B%E8%AF%95+x"
            )
            .identity
            .is_some()
    );
    assert!(
        fixture
            .route(&rules, "http://127.0.0.1:3999/?tenant=测试+x")
            .identity
            .is_some()
    );
    for url in [
        "http://localhost:4000/?tenant=测试+x",
        "http://localhost:3000/?Tenant=测试+x",
        "http://example.com:3000/?tenant=测试+x",
        "http://localhost:3000/?tenant=测试+x&private=",
    ] {
        assert!(fixture.route(&rules, url).identity.is_none());
    }
}

#[test]
fn glob_paths_and_regular_expressions_keep_their_documented_scope() {
    let fixture = Fixture::new();
    for (pattern, matches_nested) in [
        (TextPattern::Glob("/work/*".into()), false),
        (TextPattern::Glob("/work/**".into()), true),
    ] {
        let rule = fixture.rule(Condition::Path { pattern });
        assert!(
            fixture
                .route(std::slice::from_ref(&rule), "https://example.com/work/a")
                .identity
                .is_some()
        );
        assert_eq!(
            fixture
                .route(&[rule], "https://example.com/work/a/b")
                .identity
                .is_some(),
            matches_nested
        );
    }
    let rule = fixture.rule(Condition::Path {
        pattern: TextPattern::Regex("^/item/[0-9]+$".into()),
    });
    assert!(
        fixture
            .route(std::slice::from_ref(&rule), "https://example.com/item/42")
            .identity
            .is_some()
    );
    assert!(
        fixture
            .route(&[rule], "https://example.com/item/42/edit")
            .identity
            .is_none()
    );
}

#[test]
fn first_match_ask_pause_and_missing_targets_never_fall_through() {
    let fixture = Fixture::new();
    let mut first = fixture.rule(Condition::Host {
        value: "example.com".into(),
        subdomains: false,
    });
    first.action = RuleAction::Ask;
    let mut second = first.clone();
    second.name = "second".into();
    second.action = RuleAction::Open {
        identity: fixture.profiles[1].id.clone(),
    };
    assert!(
        fixture
            .route(&[first.clone(), second.clone()], "https://example.com")
            .identity
            .is_none()
    );
    first.enabled = false;
    assert_eq!(
        fixture
            .route(&[first.clone(), second.clone()], "https://example.com")
            .identity,
        Some(fixture.profiles[1].id.clone())
    );
    first.enabled = true;
    first.action = RuleAction::Open {
        identity: IdentityId::new("chrome", "removed").unwrap(),
    };
    let decision = fixture.route(&[first, second.clone()], "https://example.com");
    assert!(decision.identity.is_none());
    assert!(decision.explanation.contains("暂不可用"));
    let engine = RuleSet::compile(&[second]).unwrap();
    assert!(
        engine
            .decide(
                &Url::parse("https://example.com").unwrap(),
                &fixture.profiles,
                true
            )
            .identity
            .is_none()
    );
}

#[test]
fn invalid_patterns_and_ambiguous_config_are_rejected_before_activation() {
    let fixture = Fixture::new();
    for condition in [
        Condition::Path {
            pattern: TextPattern::Regex("[".into()),
        },
        Condition::Port {
            min: 4000,
            max: 3000,
        },
        Condition::Host {
            value: "*.example.com".into(),
            subdomains: true,
        },
    ] {
        assert!(RuleSet::compile(&[fixture.rule(condition)]).is_err());
    }
    let rule = fixture.rule(Condition::Scheme {
        value: "https".into(),
    });
    assert!(RuleSet::compile(&[rule.clone(), rule]).is_err());
    assert!(
        serde_json::from_str::<Rule>(
            r#"{"name":"typo","enabled":true,"all":[],"al":[],"action":{"kind":"ask"}}"#
        )
        .is_err()
    );
}
