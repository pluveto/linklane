use anyhow::{Context, Result, bail};
use percent_encoding::percent_decode_str;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::{
    browsers::Profile,
    routing::parse_link,
    rules::{Decision, RuleSet},
};

// Equality detects edits to the complete URL resolution policy.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResolutionSettings {
    pub(crate) parameters: Vec<String>,
    #[serde(default)]
    pub(crate) sites: Vec<SiteResolution>,
}

// Site policies override the default parameter list, in their displayed order.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SiteResolution {
    pub(crate) host: String,
    #[serde(default)]
    pub(crate) subdomains: bool,
    pub(crate) parameters: Vec<String>,
}

impl Default for ResolutionSettings {
    fn default() -> Self {
        Self {
            parameters: [
                "url",
                "target",
                "redirect",
                "redirect_url",
                "redirect_uri",
                "redirectUrl",
                "destination",
                "dest",
                "return_url",
                "returnUrl",
                "continue",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            sites: vec![],
        }
    }
}

impl ResolutionSettings {
    pub(crate) fn validate(&self) -> Result<()> {
        for parameters in
            std::iter::once(&self.parameters).chain(self.sites.iter().map(|site| &site.parameters))
        {
            let mut seen = std::collections::BTreeSet::new();
            for parameter in parameters {
                if parameter.trim().is_empty()
                    || parameter.chars().any(char::is_control)
                    || !seen.insert(parameter)
                {
                    bail!("跳转参数名须填写且互异");
                }
            }
        }
        for site in &self.sites {
            if site.host.contains(['*', '?']) {
                bail!("跳转域名使用精确 host；包含子域时设置 subdomains");
            }
            url::Host::parse(&site.host).context("跳转规则 host 无效")?;
        }
        Ok(())
    }

    fn parameters_for(&self, url: &Url) -> &[String] {
        for site in &self.sites {
            let Ok(host) = url::Host::parse(&site.host) else {
                continue;
            };
            let host = host.to_string();
            if url.host_str().is_some_and(|actual| {
                actual == host
                    || (site.subdomains
                        && actual
                            .strip_suffix(&host)
                            .is_some_and(|prefix| prefix.ends_with('.')))
            }) {
                return &site.parameters;
            }
        }
        &self.parameters
    }

    pub(crate) fn resolve(&self, original: &Url) -> ResolvedLink {
        let mut result = ResolvedLink {
            original: original.clone(),
            target: original.clone(),
            steps: vec![],
            issue: None,
        };
        let mut visited = std::collections::BTreeSet::from([original.to_string()]);
        loop {
            let parameters = self.parameters_for(&result.target);
            let mut candidate: Option<(String, Url)> = None;
            for (key, value) in result.target.query_pairs() {
                if !parameters.iter().any(|parameter| parameter == &key) {
                    continue;
                }
                let target = match ResolvedLink::parameter_target(&result.target, &value) {
                    Ok(Some(target)) => target,
                    Ok(None) => continue,
                    Err(error) => {
                        result.issue = Some(format!("参数 {key}：{error:#}"));
                        return result;
                    }
                };
                if let Some((previous_key, previous)) = &candidate {
                    if *previous != target {
                        result.issue = Some(format!(
                            "参数 {previous_key} 与 {key} 指向不同目标，请手动选择身份"
                        ));
                        return result;
                    }
                } else {
                    candidate = Some((key.into_owned(), target));
                }
            }
            let Some((parameter, target)) = candidate else {
                return result;
            };
            if result.steps.len() == 8 {
                result.issue = Some("跳转嵌套超过 8 层，请手动选择身份".into());
                return result;
            }
            if !visited.insert(target.to_string()) {
                result.issue = Some("跳转目标形成循环，请手动选择身份".into());
                return result;
            }
            result.steps.push(ResolutionStep {
                parameter,
                target: target.clone(),
            });
            result.target = target;
        }
    }
}

struct ResolutionStep {
    parameter: String,
    target: Url,
}

#[non_exhaustive]
pub(crate) struct ResolvedLink {
    original: Url,
    target: Url,
    steps: Vec<ResolutionStep>,
    issue: Option<String>,
}

impl ResolvedLink {
    fn parameter_target(base: &Url, value: &str) -> Result<Option<Url>> {
        let mut decoded = value.to_owned();
        // query_pairs has decoded the outer form layer already. Stop decoding as soon
        // as a URL is recognized, preserving encoded slashes, plus signs and signatures.
        for _ in 0..4 {
            let trimmed = decoded.trim();
            if trimmed.starts_with('/')
                || trimmed.to_ascii_lowercase().starts_with("http:")
                || trimmed.to_ascii_lowercase().starts_with("https:")
            {
                let target = base.join(trimmed).context("目标网址无效")?;
                return parse_link(target.as_str()).map(Some);
            }
            let next = percent_decode_str(&decoded)
                .decode_utf8()
                .context("目标编码须为 UTF-8")?
                .into_owned();
            if next == decoded {
                return Ok(None);
            }
            decoded = next;
        }
        bail!("目标编码超过支持层数，请手动选择身份")
    }

    pub(crate) fn matching_url(&self) -> Option<&Url> {
        self.issue.is_none().then_some(&self.target)
    }

    pub(crate) fn changed(&self) -> bool {
        !self.steps.is_empty() || self.issue.is_some()
    }

    pub(crate) fn summary(&self) -> String {
        self.issue.clone().unwrap_or_else(|| {
            format!(
                "已解析 {} 层：按目标匹配，打开时保留原始链接",
                self.steps.len()
            )
        })
    }

    pub(crate) fn explain(&self) -> String {
        let mut lines = vec![format!("原始链接：{}", self.original)];
        for (index, step) in self.steps.iter().enumerate() {
            lines.push(format!(
                "{}. 参数 {} → {}",
                index + 1,
                step.parameter,
                step.target
            ));
        }
        if let Some(issue) = &self.issue {
            lines.push(format!("解析待确认：{issue}"));
        } else {
            lines.push(format!("匹配网址：{}", self.target));
            lines.push("实际打开：原始链接（保留网站的中转流程）".into());
        }
        lines.join("\n")
    }

    pub(crate) fn decide(&self, rules: &RuleSet, profiles: &[Profile], paused: bool) -> Decision {
        let mut decision = if let Some(url) = self.matching_url() {
            rules.decide(url, profiles, paused)
        } else {
            Decision {
                identity: None,
                explanation: "显示身份选择器；解析待确认时暂停自动记忆".into(),
                notice: None,
            }
        };
        decision.explanation = format!("{}\n\n{}", self.explain(), decision.explanation);
        decision
    }
}

#[cfg(test)]
mod tests;
