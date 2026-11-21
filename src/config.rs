use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
};
use time::OffsetDateTime;

use crate::{
    browsers::Profile,
    identity::IdentityId,
    remember::RememberScope,
    resolution::ResolutionSettings,
    rule_document::RuleDocument,
    rules::{Rule, RuleAction, RuleSet},
};

#[non_exhaustive]
#[derive(Default)]
pub(crate) struct Config {
    pub(crate) rules: Vec<Rule>,
    pub(crate) resolution: ResolutionSettings,
    pub(crate) names: BTreeMap<IdentityId, String>,
    pub(crate) always_ask: bool,
    pub(crate) last_used: Vec<ProfileUse>,
    pub(crate) routing_text: String,
}

#[non_exhaustive]
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProfileUse {
    pub(crate) id: IdentityId,
    #[serde(with = "time::serde::timestamp::milliseconds")]
    pub(crate) at: OffsetDateTime,
}

impl Config {
    pub(crate) fn mark_used(&mut self, profile: &IdentityId) {
        let now = OffsetDateTime::now_utc();
        let latest = self.last_used.iter().map(|entry| entry.at).max();
        let at = latest
            .and_then(|time| time.checked_add(time::Duration::milliseconds(1)))
            .map_or(now, |next| now.max(next));
        self.last_used.retain(|entry| entry.id != *profile);
        self.last_used.push(ProfileUse {
            id: profile.clone(),
            at,
        });
    }

    pub(crate) fn last_use(&self, profile: &Profile) -> Option<OffsetDateTime> {
        self.last_used
            .iter()
            .find(|entry| entry.id == profile.id)
            .map(|entry| entry.at)
            .max(profile.browser_active)
    }

    pub(crate) fn sort_profiles(&self, profiles: &mut [Profile]) {
        profiles.sort_by(|a, b| {
            self.last_use(b)
                .cmp(&self.last_use(a))
                .then_with(|| a.name.cmp(&b.name))
                .then_with(|| a.id.cmp(&b.id))
        });
    }

    pub(crate) fn remember(
        &mut self,
        url: &url::Url,
        index: usize,
        identity: &IdentityId,
    ) -> Result<()> {
        let scope = RememberScope::suggest(url)
            .into_iter()
            .nth(index)
            .context("请重新选择要记住的范围")?;
        let conditions = scope.conditions();
        let action = RuleAction::Open {
            identity: identity.clone(),
        };
        if let Some(rule) = self
            .rules
            .iter_mut()
            .find(|rule| rule.all == conditions && rule.any.is_empty() && rule.exclude.is_empty())
        {
            rule.enabled = true;
            rule.action = action;
        } else {
            let position = RuleSet::compile(&self.rules)?.first_match(url, |rule| {
                if scope.is_origin() {
                    // Keep existing path exceptions ahead of the origin-wide fallback.
                    rule.is_site_fallback()
                } else {
                    !scope.is_broader_than(rule)
                }
            });
            let mut rule = Rule {
                name: scope.name(),
                enabled: true,
                all: conditions,
                any: vec![],
                exclude: vec![],
                action,
            };
            while self.rules.iter().any(|existing| existing.name == rule.name) {
                rule.name.push('·');
            }
            self.rules
                .insert(position.unwrap_or(self.rules.len()), rule);
        }
        Ok(())
    }

    fn validate_preferences(&self) -> Result<()> {
        if self
            .names
            .values()
            .any(|name| name.trim().is_empty() || name.chars().count() > 30)
        {
            bail!("身份名称须为 1–30 个字");
        }
        let mut seen = BTreeSet::new();
        if self.last_used.iter().any(|entry| !seen.insert(&entry.id)) {
            bail!("最近使用记录含重复身份");
        }
        Ok(())
    }
}

#[non_exhaustive]
pub(crate) struct ConfigStore {
    home: PathBuf,
}

impl ConfigStore {
    pub(crate) fn at(home: &Path) -> Self {
        Self {
            home: home.to_owned(),
        }
    }

    fn lock(&self) -> Result<fs::File> {
        fs::create_dir_all(&self.home)?;
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.home.join("settings.lock"))?;
        fs2::FileExt::lock_exclusive(&lock)?;
        Ok(lock)
    }

    pub(crate) fn load(&self) -> Result<Config> {
        let _lock = self.lock()?;
        self.read_locked()
    }

    pub(crate) fn rules_path(&self) -> PathBuf {
        self.home.join("rules.toml")
    }

    fn state_path(&self) -> PathBuf {
        self.home.join("settings.toml")
    }

    fn read_locked(&self) -> Result<Config> {
        let text = match fs::read_to_string(self.state_path()) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let config = Config {
                    routing_text: RuleDocument::default().encode()?,
                    ..Config::default()
                };
                let path = self.rules_path();
                if !path.try_exists()? {
                    self.write_atomic(&path, config.routing_text.as_bytes())?;
                }
                self.write_state(&config)?;
                return Ok(config);
            }
            Err(error) => return Err(error).context("读取轻舟设置失败"),
        };
        let saved: StateFile<String, BTreeMap<IdentityId, String>, Vec<ProfileUse>> =
            toml::from_str(&text).context("轻舟设置文件有误")?;
        let document =
            RuleDocument::parse(&saved.active_rules).context("上次生效的网站设置有误")?;
        let config = Config {
            rules: document.rules,
            resolution: document.resolution,
            names: saved.names,
            always_ask: saved.always_ask,
            last_used: saved.last_used,
            routing_text: saved.active_rules,
        };
        config.validate_preferences()?;
        Ok(config)
    }

    fn write_atomic(&self, path: &Path, data: &[u8]) -> Result<()> {
        let mut temp = tempfile::NamedTempFile::new_in(&self.home)?;
        temp.write_all(data)?;
        temp.as_file().sync_all()?;
        temp.persist(path)?;
        Ok(())
    }

    fn write_state(&self, config: &Config) -> Result<()> {
        let saved = StateFile {
            active_rules: config.routing_text.as_str(),
            names: &config.names,
            always_ask: config.always_ask,
            last_used: &config.last_used,
        };
        self.write_atomic(&self.state_path(), toml::to_string(&saved)?.as_bytes())
            .context("保存轻舟设置失败")
    }

    fn read_source(&self) -> Result<Option<Vec<u8>>> {
        match fs::read(self.rules_path()) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error).context("读取网站设置文件失败"),
        }
    }

    fn publish(&self, config: &Config, previous_source: Option<&[u8]>) -> Result<()> {
        let path = self.rules_path();
        let next = config.routing_text.as_bytes();
        let source_changed = previous_source != Some(next);
        if source_changed {
            self.write_atomic(&path, next)
                .context("写入网站设置文件失败")?;
        }
        if let Err(error) = self.write_state(config) {
            if source_changed {
                let rollback = match previous_source {
                    Some(previous) => self.write_atomic(&path, previous),
                    None => fs::remove_file(path).map_err(Into::into),
                };
                if let Err(rollback_error) = rollback {
                    return Err(error).context(format!(
                        "已保留生效设置；无法恢复磁盘草稿：{rollback_error:#}"
                    ));
                }
            }
            return Err(error);
        }
        Ok(())
    }

    pub(crate) fn reload_rules(&self) -> Result<Config> {
        let _lock = self.lock()?;
        let mut config = self.read_locked()?;
        let bytes = self.read_source()?.context("找不到网站设置文件")?;
        let text = std::str::from_utf8(&bytes).context("网站设置文件需使用 UTF-8 编码")?;
        let document = RuleDocument::parse(text)?;
        config.set_document(document, text);
        self.publish(&config, Some(&bytes))?;
        Ok(config)
    }

    pub(crate) fn replace_document(&self, text: &str, expected: Option<&str>) -> Result<Config> {
        let document = RuleDocument::parse(text)?;
        let _lock = self.lock()?;
        let mut config = self.read_locked()?;
        let source = self.read_source()?;
        if let Some(expected) = expected {
            config.check_current(expected, source.as_deref())?;
        }
        config.set_document(document, text);
        self.publish(&config, source.as_deref())?;
        Ok(config)
    }

    pub(crate) fn update(&self, update: impl FnOnce(&mut Config) -> Result<()>) -> Result<Config> {
        let _lock = self.lock()?;
        let mut config = self.read_locked()?;
        let previous = config.rules.clone();
        update(&mut config)?;
        self.write_update(config, &previous)
    }

    pub(crate) fn forget(&self, index: usize, expected: &str) -> Result<Config> {
        let _lock = self.lock()?;
        let mut config = self.read_locked()?;
        let source = self.read_source()?;
        config.check_current(expected, source.as_deref())?;
        let previous = config.rules.clone();
        if index >= config.rules.len() {
            bail!("找不到要移除的网站设置");
        }
        config.rules.remove(index);
        self.write_update(config, &previous)
    }

    fn write_update(&self, mut config: Config, previous: &[Rule]) -> Result<Config> {
        config.validate_preferences()?;
        if config.rules == previous {
            self.write_state(&config)?;
        } else {
            let source = self.read_source()?;
            config.check_current(&config.routing_text, source.as_deref())?;
            let text = RuleDocument::update_rules(&config.routing_text, previous, &config.rules)?;
            let document = RuleDocument::parse(&text)?;
            config.set_document(document, &text);
            self.publish(&config, source.as_deref())?;
        }
        Ok(config)
    }
}

impl Config {
    fn set_document(&mut self, document: RuleDocument, text: &str) {
        self.rules = document.rules;
        self.resolution = document.resolution;
        self.routing_text = RuleDocument::with_guide(text);
    }

    fn check_current(&self, expected: &str, source: Option<&[u8]>) -> Result<()> {
        if self.routing_text != expected || source != Some(expected.as_bytes()) {
            bail!("网站设置文件已有改动。请先从文件重新加载，再应用当前改动。");
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StateFile<Text, Names, Uses> {
    active_rules: Text,
    names: Names,
    always_ask: bool,
    last_used: Uses,
}

#[cfg(test)]
mod tests;
