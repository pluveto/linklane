use anyhow::{Context, Result, bail};
use globset::{GlobBuilder, GlobMatcher};
use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::{browsers::Profile, identity::IdentityId};

// Structural equality detects concurrent edits and changes to the executable rule policy.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Rule {
    pub(crate) name: String,
    pub(crate) enabled: bool,
    #[serde(default)]
    pub(crate) all: Vec<Condition>,
    #[serde(default)]
    pub(crate) any: Vec<Condition>,
    #[serde(default)]
    pub(crate) exclude: Vec<Condition>,
    pub(crate) action: RuleAction,
}

// Equality compares action and destination, including the browser namespace.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum RuleAction {
    Open { identity: IdentityId },
    Ask,
}

// Equality compares the complete matching condition for revision checks.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum Condition {
    Origin {
        value: String,
    },
    Host {
        value: String,
        #[serde(default)]
        subdomains: bool,
    },
    Scheme {
        value: String,
    },
    Port {
        min: u16,
        max: u16,
    },
    Path {
        pattern: TextPattern,
    },
    PathPrefix {
        value: String,
    },
    Url {
        pattern: TextPattern,
    },
    Query {
        key: String,
        #[serde(default)]
        value: Option<TextPattern>,
    },
}

// Equality compares matching syntax and pattern text, preserving intentional case.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum TextPattern {
    Exact(String),
    Glob(String),
    Regex(String),
}

impl TextPattern {
    fn compile(&self, path: bool) -> Result<Pattern> {
        let (Self::Exact(value) | Self::Glob(value) | Self::Regex(value)) = self;
        if value.len() > 16_384 {
            bail!("单个匹配表达式上限为 16384 字节");
        }
        Ok(match self {
            Self::Exact(value) => Pattern::Exact(value.clone()),
            Self::Glob(value) => Pattern::Glob(
                GlobBuilder::new(value)
                    .literal_separator(path)
                    .backslash_escape(true)
                    .build()?
                    .compile_matcher(),
            ),
            Self::Regex(value) => Pattern::Regex(
                RegexBuilder::new(value)
                    .size_limit(2 * 1024 * 1024)
                    .build()
                    .context("匹配表达式无效")?,
            ),
        })
    }
}

enum Pattern {
    Exact(String),
    Glob(GlobMatcher),
    Regex(Regex),
}

impl Pattern {
    fn is_match(&self, text: &str) -> bool {
        match self {
            Self::Exact(value) => text == value,
            Self::Glob(pattern) => pattern.is_match(text),
            Self::Regex(pattern) => pattern.is_match(text),
        }
    }
}

enum Predicate {
    Origin(String),
    Host(String, bool),
    Scheme(String),
    Port(u16, u16),
    Path(Pattern),
    PathPrefix(String),
    Url(Pattern),
    Query(String, Option<Pattern>),
}

impl Condition {
    fn compile(&self) -> Result<Predicate> {
        Ok(match self {
            Self::Origin { value } => {
                let url = crate::routing::parse_link(value)?;
                if url.origin().ascii_serialization() != *value {
                    bail!("origin 须使用精确来源，例如 https://example.com，省略路径与默认端口");
                }
                Predicate::Origin(value.clone())
            }
            Self::Host { value, subdomains } => {
                if value.contains(['*', '?']) {
                    bail!("host 使用精确域名；包含子域请设置 subdomains，通配符请使用 url 条件");
                }
                let host =
                    url::Host::parse(value).context("host 须为域名或 IP；通配符请使用 url 条件")?;
                Predicate::Host(host.to_string(), *subdomains)
            }
            Self::Scheme { value } => {
                if !["http", "https"].contains(&value.as_str()) {
                    bail!("scheme 须为 http 或 https");
                }
                Predicate::Scheme(value.clone())
            }
            Self::Port { min, max } => {
                if min > max || *min == 0 {
                    bail!("端口范围须满足 1 ≤ min ≤ max ≤ 65535");
                }
                Predicate::Port(*min, *max)
            }
            Self::Path { pattern } => Predicate::Path(pattern.compile(true)?),
            Self::PathPrefix { value } => {
                if !value.starts_with('/') || value.contains(['?', '#']) {
                    bail!("pathPrefix 须以 / 开头，仅填写路径");
                }
                Predicate::PathPrefix(value.trim_end_matches('/').to_owned())
            }
            Self::Url { pattern } => Predicate::Url(pattern.compile(false)?),
            Self::Query { key, value } => {
                if key.is_empty() {
                    bail!("查询参数名称须填写");
                }
                Predicate::Query(
                    key.clone(),
                    value
                        .as_ref()
                        .map(|pattern| pattern.compile(false))
                        .transpose()?,
                )
            }
        })
    }
}

impl Predicate {
    fn matches(&self, url: &Url) -> bool {
        match self {
            Self::Origin(value) => url.origin().ascii_serialization() == *value,
            Self::Host(value, subdomains) => url.host_str().is_some_and(|host| {
                host == value
                    || (*subdomains
                        && host
                            .strip_suffix(value)
                            .is_some_and(|prefix| prefix.ends_with('.')))
            }),
            Self::Scheme(value) => url.scheme() == value,
            Self::Port(min, max) => url
                .port_or_known_default()
                .is_some_and(|port| (*min..=*max).contains(&port)),
            Self::Path(pattern) => pattern.is_match(url.path()),
            Self::PathPrefix(value) => {
                url.path() == value
                    || url
                        .path()
                        .strip_prefix(value)
                        .is_some_and(|suffix| suffix.starts_with('/'))
            }
            Self::Url(pattern) => pattern.is_match(url.as_str()),
            Self::Query(key, value) => url.query_pairs().any(|(name, content)| {
                name == key.as_str()
                    && value
                        .as_ref()
                        .is_none_or(|pattern| pattern.is_match(&content))
            }),
        }
    }
}

struct PreparedRule {
    rule: Rule,
    all: Vec<Predicate>,
    any: Vec<Predicate>,
    exclude: Vec<Predicate>,
}

#[non_exhaustive]
pub(crate) struct RuleSet(Vec<PreparedRule>);

#[non_exhaustive]
pub(crate) struct Decision {
    pub(crate) identity: Option<IdentityId>,
    pub(crate) explanation: String,
    pub(crate) notice: Option<String>,
}

impl RuleSet {
    pub(crate) fn compile(rules: &[Rule]) -> Result<Self> {
        let mut names = std::collections::BTreeSet::new();
        let mut prepared = Vec::new();
        for rule in rules {
            if rule.name.trim().is_empty() || !names.insert(&rule.name) {
                bail!("规则名称须填写且唯一");
            }
            if rule.all.is_empty() && rule.any.is_empty() {
                bail!("规则“{}”至少需要一个 all 或 any 条件", rule.name);
            }
            if rule.all.len() + rule.any.len() + rule.exclude.len() > 64 {
                bail!("单条规则最多支持 64 个条件");
            }
            let compile = |conditions: &[Condition]| {
                conditions
                    .iter()
                    .map(Condition::compile)
                    .collect::<Result<Vec<_>>>()
            };
            prepared.push(PreparedRule {
                rule: rule.clone(),
                all: compile(&rule.all)
                    .with_context(|| format!("规则“{}”的 all 条件", rule.name))?,
                any: compile(&rule.any)
                    .with_context(|| format!("规则“{}”的 any 条件", rule.name))?,
                exclude: compile(&rule.exclude)
                    .with_context(|| format!("规则“{}”的 exclude 条件", rule.name))?,
            });
        }
        Ok(Self(prepared))
    }

    pub(crate) fn decide(&self, url: &Url, profiles: &[Profile], paused: bool) -> Decision {
        let mut trace = Vec::new();
        if paused {
            return Decision {
                identity: None,
                explanation: "已开启“每次都让我选”，本次显示身份选择器。".into(),
                notice: None,
            };
        }
        for (index, prepared) in self.0.iter().enumerate() {
            let rule = &prepared.rule;
            let label = format!("{}. {}", index + 1, rule.name);
            if !rule.enabled {
                trace.push(format!("{label}：已停用"));
                continue;
            }
            if let Some(failed) = prepared
                .all
                .iter()
                .position(|condition| !condition.matches(url))
            {
                trace.push(format!("{label}：all 第 {} 项未命中", failed + 1));
                continue;
            }
            if !prepared.any.is_empty()
                && !prepared.any.iter().any(|condition| condition.matches(url))
            {
                trace.push(format!("{label}：any 条件均未命中"));
                continue;
            }
            if let Some(excluded) = prepared
                .exclude
                .iter()
                .position(|condition| condition.matches(url))
            {
                trace.push(format!("{label}：被 exclude 第 {} 项排除", excluded + 1));
                continue;
            }
            let mut notice = None;
            let identity = match &rule.action {
                RuleAction::Ask => {
                    trace.push(format!("{label}：首条命中 → 每次询问"));
                    None
                }
                RuleAction::Open { identity } => {
                    if let Some(profile) = profiles.iter().find(|profile| profile.id == *identity) {
                        trace.push(format!(
                            "{label}：首条命中 → {} · {}\n身份标识：{identity}",
                            profile.browser_name, profile.name
                        ));
                        Some(identity.clone())
                    } else {
                        notice = Some(format!(
                            "规则“{}”的目标身份暂不可用，请重新选择。",
                            rule.name
                        ));
                        trace.push(format!(
                            "{label}：首条命中，但身份 {identity} 暂不可用 → 请重新选择"
                        ));
                        None
                    }
                }
            };
            return Decision {
                identity,
                explanation: trace.join("\n"),
                notice,
            };
        }
        trace.push("其余规则均未命中 → 显示身份选择器".into());
        Decision {
            identity: None,
            explanation: trace.join("\n"),
            notice: None,
        }
    }
}

impl Rule {
    pub(crate) fn remembered(origin: String, identity: IdentityId) -> Self {
        Self {
            name: format!("网站：{origin}"),
            enabled: true,
            all: vec![Condition::Origin { value: origin }],
            any: vec![],
            exclude: vec![],
            action: RuleAction::Open { identity },
        }
    }

    pub(crate) fn origin(&self) -> Option<&str> {
        match self.all.as_slice() {
            [Condition::Origin { value }] if self.any.is_empty() && self.exclude.is_empty() => {
                Some(value)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
