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
    resolution::ResolutionSettings,
    rules::{Rule, RuleSet},
};

#[non_exhaustive]
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Config {
    pub(crate) version: u32,
    pub(crate) names: BTreeMap<IdentityId, String>,
    pub(crate) rules: Vec<Rule>,
    pub(crate) resolution: ResolutionSettings,
    pub(crate) rules_revision: u64,
    pub(crate) always_ask: bool,
    pub(crate) last_used: Vec<ProfileUse>,
}

#[non_exhaustive]
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProfileUse {
    pub(crate) id: IdentityId,
    #[serde(with = "time::serde::timestamp::milliseconds")]
    pub(crate) at: OffsetDateTime,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: 3,
            names: BTreeMap::new(),
            rules: vec![],
            resolution: ResolutionSettings::default(),
            rules_revision: 0,
            always_ask: false,
            last_used: vec![],
        }
    }
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

    pub(crate) fn remember(&mut self, url: &url::Url, identity: &IdentityId) {
        let origin = url.origin().ascii_serialization();
        if let Some(rule) = self
            .rules
            .iter_mut()
            .find(|rule| rule.origin() == Some(origin.as_str()))
        {
            rule.enabled = true;
            rule.action = crate::rules::RuleAction::Open {
                identity: identity.clone(),
            };
        } else {
            let mut rule = Rule::remembered(origin, identity.clone());
            while self.rules.iter().any(|existing| existing.name == rule.name) {
                rule.name.push('·');
            }
            self.rules.push(rule);
        }
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if self.version != 3 {
            bail!(
                "设置版本 {} 尚无迁移路径，请使用对应版本的轻舟",
                self.version
            );
        }
        RuleSet::compile(&self.rules)?;
        self.resolution.validate()?;
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

    fn read_locked(&self) -> Result<Config> {
        let bytes = match fs::read(self.home.join("settings.json")) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Config::default());
            }
            Err(error) => return Err(error).context("读取设置失败"),
        };
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).context("设置 JSON 无效；原文件已保留")?;
        let (config, previous_version) = match value.get("version") {
            Some(version) if version == 3 => {
                let config: Config =
                    serde_json::from_value(value).context("设置字段无效；原文件已保留")?;
                config.validate()?;
                return Ok(config);
            }
            Some(version) if version == 2 => {
                let legacy: VersionTwo =
                    serde_json::from_value(value).context("v2 设置字段无效；原文件已保留")?;
                (legacy.migrate(), 2)
            }
            None => {
                let legacy: LegacyConfig =
                    serde_json::from_value(value).context("旧版设置无效；原文件已保留")?;
                (legacy.migrate()?, 1)
            }
            Some(version) => bail!("设置版本 {version} 尚无迁移路径；原文件已保留"),
        };
        config.validate()?;
        let backup_name = format!("settings.v{previous_version}.backup.json");
        let backup = self.home.join(&backup_name);
        match fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&backup)
        {
            Ok(mut file) => {
                file.write_all(&bytes)?;
                file.sync_all()?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                if fs::read(&backup)? != bytes {
                    bail!(
                        "已有不同的旧版备份，请先将 {backup_name} 移到备份目录后重试；当前设置已保留"
                    );
                }
            }
            Err(error) => return Err(error).context("创建升级备份失败"),
        }
        self.write_locked(&config)
            .context("迁移保存失败；可继续使用旧版设置")?;
        Ok(config)
    }

    fn write_locked(&self, config: &Config) -> Result<()> {
        let mut temp = tempfile::NamedTempFile::new_in(&self.home)?;
        temp.write_all(&serde_json::to_vec_pretty(config)?)?;
        temp.as_file().sync_all()?;
        temp.persist(self.home.join("settings.json"))?;
        Ok(())
    }

    pub(crate) fn update(&self, update: impl FnOnce(&mut Config) -> Result<()>) -> Result<Config> {
        let _lock = self.lock()?;
        let mut config = self.read_locked()?;
        let previous = config.rules.clone();
        let previous_resolution = config.resolution.clone();
        update(&mut config)?;
        config.validate()?;
        if config.rules != previous || config.resolution != previous_resolution {
            config.rules_revision = config
                .rules_revision
                .checked_add(1)
                .context("规则版本已达到上限")?;
        }
        self.write_locked(&config)?;
        Ok(config)
    }

    pub(crate) fn replace_rules(&self, rules: Vec<Rule>, expected_revision: u64) -> Result<Config> {
        self.update(|config| {
            if config.rules_revision != expected_revision {
                bail!("其他窗口已修改规则。请先保留当前草稿，再载入已保存规则进行合并。");
            }
            config.rules = rules;
            Ok(())
        })
    }

    pub(crate) fn replace_routing(
        &self,
        rules: Vec<Rule>,
        resolution: ResolutionSettings,
        expected_revision: u64,
    ) -> Result<Config> {
        self.update(|config| {
            if config.rules_revision != expected_revision {
                bail!("其他窗口已修改规则或跳转设置。请先保留草稿，再载入已保存规则进行合并。");
            }
            config.rules = rules;
            config.resolution = resolution;
            Ok(())
        })
    }
}

// Legacy DTOs exist only at the migration boundary; runtime decisions use Config v3.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VersionTwo {
    version: u32,
    names: BTreeMap<IdentityId, String>,
    rules: Vec<Rule>,
    rules_revision: u64,
    always_ask: bool,
    last_used: Vec<ProfileUse>,
}

impl VersionTwo {
    fn migrate(self) -> Config {
        debug_assert_eq!(self.version, 2);
        Config {
            names: self.names,
            rules: self.rules,
            rules_revision: self.rules_revision,
            always_ask: self.always_ask,
            last_used: self.last_used,
            ..Config::default()
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct LegacyConfig {
    names: BTreeMap<String, String>,
    sites: Vec<LegacySite>,
    always_ask: bool,
    last_used: Vec<LegacyUse>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacySite {
    site: String,
    profile: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyUse {
    id: String,
    #[serde(with = "time::serde::timestamp::milliseconds")]
    at: OffsetDateTime,
}

impl LegacyConfig {
    fn migrate(self) -> Result<Config> {
        let mut config = Config {
            always_ask: self.always_ask,
            ..Config::default()
        };
        for (id, name) in self.names {
            config.names.insert(IdentityId::new("chrome", &id)?, name);
        }
        for site in self.sites {
            config.rules.push(Rule::remembered(
                site.site,
                IdentityId::new("chrome", &site.profile)?,
            ));
        }
        for entry in self.last_used {
            config.last_used.push(ProfileUse {
                id: IdentityId::new("chrome", &entry.id)?,
                at: entry.at,
            });
        }
        Ok(config)
    }
}

#[cfg(test)]
mod tests;
