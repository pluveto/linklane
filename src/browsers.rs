use anyhow::{Context, Result, bail};
use configparser::ini::Ini;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use time::OffsetDateTime;
use url::Url;

use crate::identity::IdentityId;

// Copy is appropriate for this finite, fieldless browser family identifier.
#[non_exhaustive]
#[derive(Clone, Copy)]
pub(crate) enum BrowserFamily {
    Chrome,
    Edge,
    Brave,
    Firefox,
}

impl BrowserFamily {
    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::Chrome => "chrome",
            Self::Edge => "edge",
            Self::Brave => "brave",
            Self::Firefox => "firefox",
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Chrome => "Chrome",
            Self::Edge => "Edge",
            Self::Brave => "Brave",
            Self::Firefox => "Firefox",
        }
    }

    fn installation(self) -> Option<BrowserInstallation> {
        let application = match self {
            Self::Chrome => "Google/Chrome/Application/chrome.exe",
            Self::Edge => "Microsoft/Edge/Application/msedge.exe",
            Self::Brave => "BraveSoftware/Brave-Browser/Application/brave.exe",
            Self::Firefox => "Mozilla Firefox/firefox.exe",
        };
        let executable = ["PROGRAMFILES", "PROGRAMFILES(X86)", "LOCALAPPDATA"]
            .iter()
            .filter_map(std::env::var_os)
            .map(|root| PathBuf::from(root).join(application))
            .find(|path| path.is_file())?;
        let root = if matches!(self, Self::Firefox) {
            dirs::config_dir()?.join("Mozilla/Firefox")
        } else {
            dirs::data_local_dir()?.join(match self {
                Self::Chrome => "Google/Chrome/User Data",
                Self::Edge => "Microsoft/Edge/User Data",
                Self::Brave => "BraveSoftware/Brave-Browser/User Data",
                Self::Firefox => unreachable!(),
            })
        };
        Some(BrowserInstallation {
            family: self,
            executable,
            root,
        })
    }
}

// A chosen profile is retained while the UI's recency order changes.
#[non_exhaustive]
#[derive(Clone)]
pub(crate) struct Profile {
    pub(crate) id: IdentityId,
    pub(crate) name: String,
    pub(crate) browser_name: String,
    pub(crate) fill: u32,
    pub(crate) stroke: u32,
    pub(crate) browser_active: Option<OffsetDateTime>,
    launch: Option<BrowserLaunch>,
}

#[derive(Clone)]
enum BrowserLaunch {
    Chromium {
        executable: PathBuf,
        root: PathBuf,
        profile: String,
    },
    Firefox {
        executable: PathBuf,
        profile: PathBuf,
    },
}

impl BrowserLaunch {
    fn arguments(&self, link: &Url) -> Vec<std::ffi::OsString> {
        match self {
            Self::Chromium { root, profile, .. } => vec![
                format!("--user-data-dir={}", root.display()).into(),
                format!("--profile-directory={profile}").into(),
                link.as_str().into(),
            ],
            Self::Firefox { profile, .. } => vec![
                "--profile".into(),
                profile.as_os_str().to_owned(),
                "--new-tab".into(),
                link.as_str().into(),
            ],
        }
    }
}

impl Profile {
    pub(crate) fn preview(family: BrowserFamily, id: &str, name: &str) -> Result<Self> {
        Ok(Self {
            id: IdentityId::new(family.id(), id)?,
            name: name.into(),
            browser_name: family.name().into(),
            fill: 0xffd3e3fd,
            stroke: 0xff0842a0,
            browser_active: None,
            launch: None,
        })
    }

    pub(crate) fn open(&self, url: &Url) -> Result<()> {
        let launch = self
            .launch
            .as_ref()
            .context("此身份用于界面预览，请选择本机身份")?;
        let (executable, directory) = match launch {
            BrowserLaunch::Chromium {
                executable,
                root,
                profile,
            } => (executable, root.join(profile)),
            BrowserLaunch::Firefox {
                executable,
                profile,
            } => (executable, profile.clone()),
        };
        if !directory.is_dir() {
            bail!("身份目录已移除，请重新查找浏览器身份");
        }
        Command::new(executable)
            .args(launch.arguments(url))
            .spawn()
            .with_context(|| format!("启动 {} 失败", self.browser_name))?;
        Ok(())
    }
}

#[non_exhaustive]
pub(crate) struct BrowserCatalog {
    pub(crate) profiles: Vec<Profile>,
    pub(crate) warnings: Vec<String>,
}

impl BrowserCatalog {
    pub(crate) fn discover() -> Self {
        let mut catalog = Self {
            profiles: vec![],
            warnings: vec![],
        };
        for family in [
            BrowserFamily::Chrome,
            BrowserFamily::Edge,
            BrowserFamily::Brave,
            BrowserFamily::Firefox,
        ] {
            if let Some(installation) = family.installation() {
                match installation.discover() {
                    Ok(profiles) => catalog.profiles.extend(profiles),
                    Err(error) => catalog
                        .warnings
                        .push(format!("{}：{error:#}", family.name())),
                }
            }
        }
        if catalog.profiles.is_empty() {
            catalog
                .warnings
                .push("请先在 Chrome、Edge、Brave 或 Firefox 中创建身份，然后重新查找。".into());
        }
        catalog
    }
}

struct BrowserInstallation {
    family: BrowserFamily,
    executable: PathBuf,
    root: PathBuf,
}

impl BrowserInstallation {
    fn discover(&self) -> Result<Vec<Profile>> {
        if matches!(self.family, BrowserFamily::Firefox) {
            self.firefox_profiles(
                &fs::read_to_string(self.root.join("profiles.ini")).context("读取身份列表失败")?,
            )
        } else {
            self.chromium_profiles(
                &fs::read(self.root.join("Local State")).context("读取身份列表失败")?,
            )
        }
    }

    fn chromium_profiles(&self, data: &[u8]) -> Result<Vec<Profile>> {
        let state: serde_json::Value = serde_json::from_slice(data)?;
        let cache = state
            .pointer("/profile/info_cache")
            .and_then(serde_json::Value::as_object)
            .context("请先打开浏览器并创建身份")?;
        let mut profiles = Vec::new();
        for (id, value) in cache {
            if id.is_empty()
                || id.contains(['/', '\\', ':'])
                || id == "."
                || id == ".."
                || !self.root.join(id).is_dir()
            {
                continue;
            }
            let fill = Self::color(value, "default_avatar_fill_color")
                .or_else(|| Self::color(value, "profile_highlight_color"))
                .unwrap_or(0xffdee1e6);
            let stroke = Self::color(value, "default_avatar_stroke_color").unwrap_or_else(|| {
                if ((fill >> 16 & 255) * 299 + (fill >> 8 & 255) * 587 + (fill & 255) * 114)
                    > 128_000
                {
                    0xff3c4043
                } else {
                    0xffffffff
                }
            });
            profiles.push(Profile {
                id: IdentityId::new(self.family.id(), id)?,
                name: value
                    .get("name")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(id)
                    .into(),
                browser_name: self.family.name().into(),
                fill,
                stroke,
                browser_active: value
                    .get("active_time")
                    .and_then(serde_json::Value::as_f64)
                    .and_then(|seconds| std::time::Duration::try_from_secs_f64(seconds).ok())
                    .and_then(|duration| time::Duration::try_from(duration).ok())
                    .and_then(|duration| OffsetDateTime::UNIX_EPOCH.checked_add(duration)),
                launch: Some(BrowserLaunch::Chromium {
                    executable: self.executable.clone(),
                    root: self.root.clone(),
                    profile: id.clone(),
                }),
            });
        }
        Ok(profiles)
    }

    fn firefox_profiles(&self, data: &str) -> Result<Vec<Profile>> {
        let mut ini = Ini::new();
        ini.read(data.to_owned()).map_err(anyhow::Error::msg)?;
        let mut profiles = Vec::new();
        for section in ini
            .sections()
            .iter()
            .filter(|section| section.starts_with("profile"))
        {
            let path = ini.get(section, "path").context("Firefox 身份缺少 Path")?;
            let name = ini.get(section, "name").context("Firefox 身份缺少 Name")?;
            let directory = match ini.get(section, "isrelative").as_deref() {
                Some("1") => self.root.join(&path),
                Some("0") if Path::new(&path).is_absolute() => PathBuf::from(&path),
                _ => bail!("Firefox 身份路径标记无效"),
            };
            if !directory.is_dir() {
                continue;
            }
            profiles.push(Profile {
                id: IdentityId::new("firefox", &path)?,
                name,
                browser_name: self.family.name().into(),
                fill: 0xffffd8b5,
                stroke: 0xff713900,
                browser_active: None,
                launch: Some(BrowserLaunch::Firefox {
                    executable: self.executable.clone(),
                    profile: directory,
                }),
            });
        }
        Ok(profiles)
    }

    fn color(value: &serde_json::Value, key: &str) -> Option<u32> {
        let number = value.get(key)?.as_i64()?;
        if !(i64::from(i32::MIN)..=i64::from(u32::MAX)).contains(&number) {
            return None;
        }
        let color = if number < 0 {
            u32::from_ne_bytes(i32::try_from(number).ok()?.to_ne_bytes())
        } else {
            u32::try_from(number).ok()?
        };
        (color >> 24 == 255).then_some(color)
    }
}

#[cfg(test)]
mod tests;
