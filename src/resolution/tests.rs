use super::*;
use crate::{browsers::BrowserFamily, identity::IdentityId, rules::Rule};

fn wrapped(target: &str) -> Url {
    let mut url = Url::parse("https://wrapper.test/out?signature=keep").unwrap();
    url.query_pairs_mut().append_pair("target", target);
    url
}

#[test]
fn nested_targets_route_by_destination_and_preserve_the_opening_link() {
    let destination = "https://github.com/acme/repo?signature=a%2Bb&path=%2Fdocs#readme";
    let original = wrapped(wrapped(destination).as_str());
    let resolved = ResolutionSettings::default().resolve(&original);
    assert_eq!(resolved.matching_url().unwrap().as_str(), destination);
    assert_eq!(resolved.original, original);
    assert_eq!(resolved.steps.len(), 2);
    let identity = IdentityId::new("edge", "Default").unwrap();
    let rules = RuleSet::compile(&[Rule::remembered(
        "https://github.com".into(),
        identity.clone(),
    )])
    .unwrap();
    let profiles = [Profile::preview(BrowserFamily::Edge, "Default", "Work").unwrap()];
    assert_eq!(
        resolved.decide(&rules, &profiles, false).identity,
        Some(identity)
    );
    assert!(resolved.decide(&rules, &profiles, true).identity.is_none());
}

#[test]
fn encoded_targets_preserve_url_encoding_and_real_plus_signs() {
    let destination = "https://example.com/a%2Fb?token=a+b%2Fc";
    let double_encoded =
        percent_encoding::utf8_percent_encode(destination, percent_encoding::NON_ALPHANUMERIC)
            .to_string();
    let resolved = ResolutionSettings::default().resolve(&wrapped(&double_encoded));
    assert_eq!(resolved.matching_url().unwrap().as_str(), destination);
    let unicode = "https://例子.测试/中文?x=一";
    let resolved = ResolutionSettings::default().resolve(&wrapped(unicode));
    assert_eq!(
        resolved.matching_url().unwrap(),
        &Url::parse(unicode).unwrap()
    );
}

#[test]
fn ambiguous_or_unusable_targets_require_a_choice() {
    for suffix in [
        "&url=https%3A%2F%2Fother.test",
        "&target=https%3A%2F%2Fother.test",
    ] {
        let original = Url::parse(&format!("{}{suffix}", wrapped("https://example.com"))).unwrap();
        let resolved = ResolutionSettings::default().resolve(&original);
        assert!(resolved.matching_url().is_none());
        assert!(
            resolved
                .decide(&RuleSet::compile(&[]).unwrap(), &[], false)
                .identity
                .is_none()
        );
    }
    let original = wrapped("https://user:secret@example.com");
    assert!(
        ResolutionSettings::default()
            .resolve(&original)
            .matching_url()
            .is_none()
    );
    let repeated =
        Url::parse("https://wrap.test/?url=https://example.com&target=https://example.com")
            .unwrap();
    assert_eq!(
        ResolutionSettings::default()
            .resolve(&repeated)
            .matching_url()
            .unwrap()
            .host_str(),
        Some("example.com")
    );
}

#[test]
fn site_overrides_allow_custom_parameters_and_exempt_specific_sites() {
    let settings = ResolutionSettings {
        sites: vec![
            SiteResolution {
                host: "Login.Example.com".into(),
                subdomains: false,
                parameters: vec![],
            },
            SiteResolution {
                host: "example.com".into(),
                subdomains: true,
                parameters: vec!["to".into()],
            },
        ],
        ..ResolutionSettings::default()
    };
    settings.validate().unwrap();
    for (source, expected) in [
        (
            "https://go.example.com/?to=https://target.test",
            "target.test",
        ),
        (
            "https://login.example.com/?url=https://target.test",
            "login.example.com",
        ),
        (
            "https://example.com.evil.test/?to=https://target.test",
            "example.com.evil.test",
        ),
    ] {
        let resolved = settings.resolve(&Url::parse(source).unwrap());
        assert_eq!(resolved.matching_url().unwrap().host_str(), Some(expected));
    }
}

#[test]
fn ordinary_queries_and_disabled_resolution_keep_existing_rules() {
    let source =
        Url::parse("https://example.com/search?q=https://other.test&target=_blank").unwrap();
    assert!(!ResolutionSettings::default().resolve(&source).changed());
    let settings = ResolutionSettings {
        parameters: vec![],
        sites: vec![],
    };
    let source = wrapped("https://target.test");
    assert_eq!(settings.resolve(&source).matching_url(), Some(&source));
}

#[test]
fn relative_targets_and_depth_limits_are_explicit() {
    let settings = ResolutionSettings::default();
    assert_eq!(
        settings
            .resolve(&wrapped("/acme/project"))
            .matching_url()
            .unwrap()
            .as_str(),
        "https://wrapper.test/acme/project"
    );
    let mut source = Url::parse("https://example.com").unwrap();
    for _ in 0..8 {
        source = wrapped(source.as_str());
    }
    assert_eq!(settings.resolve(&source).steps.len(), 8);
    assert!(settings.resolve(&source).matching_url().is_some());
    source = wrapped(source.as_str());
    assert!(settings.resolve(&source).matching_url().is_none());
}

#[test]
fn invalid_configuration_is_rejected_before_use() {
    let mut settings = ResolutionSettings::default();
    settings.parameters.push("url".into());
    assert!(settings.validate().is_err());
    settings.parameters.clear();
    settings.sites.push(SiteResolution {
        host: "*.example.com".into(),
        subdomains: false,
        parameters: vec![],
    });
    assert!(settings.validate().is_err());
}
