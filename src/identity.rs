use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::fmt;

// Equality and ordering identify one browser installation's profile, not its display name.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub(crate) struct IdentityId(String);

impl IdentityId {
    pub(crate) fn new(browser: &str, profile: &str) -> Result<Self> {
        format!("{browser}:{profile}").try_into()
    }
}

impl TryFrom<String> for IdentityId {
    type Error = anyhow::Error;

    fn try_from(value: String) -> Result<Self> {
        let Some((browser, profile)) = value.split_once(':') else {
            bail!("身份标识应为 browser:profile，例如 chrome:Default");
        };
        if !["chrome", "edge", "brave", "firefox"].contains(&browser)
            || profile.trim().is_empty()
            || value.len() > 2048
            || value.chars().any(char::is_control)
        {
            bail!("无效的浏览器身份标识：{value}");
        }
        Ok(Self(value))
    }
}

impl From<IdentityId> for String {
    fn from(value: IdentityId) -> Self {
        value.0
    }
}

impl fmt::Display for IdentityId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}
