// RATIONALE: These application-private functions are stateless URL and Windows/Chrome
// integration boundaries. They keep OS side effects explicit without singleton services.
// This module belongs to the executable; it is not a public library API.
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};
use time::OffsetDateTime;
use url::Url;

// Equality compares the entire discovered snapshot, including theme and activity.
// Clone is needed to retain the chosen identity while the display order changes.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Profile {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) fill: u32,
    pub(crate) stroke: u32,
    pub(crate) chrome_active: Option<OffsetDateTime>,
}

// Equality compares both the website and its selected identity.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Remembered {
    pub(crate) site: String,
    pub(crate) profile: String,
}

#[non_exhaustive]
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Config {
    pub(crate) names: BTreeMap<String, String>,
    pub(crate) sites: Vec<Remembered>,
    pub(crate) always_ask: bool,
    pub(crate) last_used: Vec<ProfileUse>,
}

#[non_exhaustive]
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ProfileUse {
    pub(crate) id: String,
    #[serde(with = "time::serde::timestamp::milliseconds")]
    pub(crate) at: OffsetDateTime,
}

impl Config {
    pub(crate) fn mark_used(&mut self, profile: &str) {
        let now = OffsetDateTime::now_utc();
        let latest = self.last_used.iter().map(|entry| entry.at).max();
        let at = latest
            .and_then(|time| time.checked_add(time::Duration::milliseconds(1)))
            .map_or(now, |next| now.max(next));

        self.last_used.retain(|entry| entry.id != profile);
        self.last_used.push(ProfileUse {
            id: profile.into(),
            at,
        });
    }

    pub(crate) fn last_use(&self, profile: &Profile) -> Option<OffsetDateTime> {
        self.last_used
            .iter()
            .find(|entry| entry.id == profile.id)
            .map(|entry| entry.at)
            .max(profile.chrome_active)
    }

    pub(crate) fn sort_profiles(&self, profiles: &mut [Profile]) {
        profiles.sort_by(|a, b| {
            self.last_use(b)
                .cmp(&self.last_use(a))
                .then_with(|| a.name.cmp(&b.name))
                .then_with(|| a.id.cmp(&b.id))
        });
    }
}

// Chromium stores SkColor (ARGB) in signed JSON integers. Preserve the bit pattern.
fn chrome_color(value: &serde_json::Value, key: &str) -> Option<u32> {
    let n = value.get(key)?.as_i64()?;
    if !(i64::from(i32::MIN)..=i64::from(u32::MAX)).contains(&n) {
        return None;
    }
    let color = if n < 0 {
        u32::from_ne_bytes(i32::try_from(n).ok()?.to_ne_bytes())
    } else {
        u32::try_from(n).ok()?
    };
    (color >> 24 == 255).then_some(color)
}

pub(crate) fn parse_link(raw: &str) -> Result<Url> {
    let link = Url::parse(raw.trim()).context("请输入完整的网址，例如 https://example.com")?;
    if !matches!(link.scheme(), "http" | "https") || link.host_str().is_none() {
        bail!("这里只能打开以 http:// 或 https:// 开头的网站");
    }
    if !link.username().is_empty() || link.password().is_some() {
        bail!("这个网址包含登录信息，请去掉登录信息后再试");
    }
    Ok(link)
}

pub(crate) fn site_key(url: &Url) -> String {
    url.origin().ascii_serialization()
}

pub(crate) fn remembered_profile<'a>(
    config: &'a Config,
    url: &Url,
    profiles: &[Profile],
) -> Option<&'a str> {
    if config.always_ask {
        return None;
    }
    config
        .sites
        .iter()
        .find(|s| s.site == site_key(url) && profiles.iter().any(|p| p.id == s.profile))
        .map(|s| s.profile.as_str())
}

pub(crate) fn remember(config: &mut Config, url: &Url, profile: &str) {
    let site = site_key(url);
    config.sites.retain(|s| s.site != site);
    config.sites.push(Remembered {
        site,
        profile: profile.into(),
    });
}

pub(crate) fn app_home() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("LINKLANE_HOME") {
        return Ok(path.into());
    }
    Ok(dirs::data_local_dir()
        .context("找不到此电脑的用户文件夹")?
        .join("Linklane"))
}

pub(crate) fn load_config(home: &Path) -> Result<Config> {
    match fs::read(home.join("settings.json")) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .context("保存的设置无法读取；原文件已保留，请先修复设置文件"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(e).context("无法读取已保存的设置"),
    }
}

// Each chooser can be a separate process. Serialize read-modify-write and replace
// atomically so simultaneous clicks cannot lose another window's preferences.
pub(crate) fn update_config(home: &Path, update: impl FnOnce(&mut Config)) -> Result<Config> {
    fs::create_dir_all(home)?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(home.join("settings.lock"))?;
    fs2::FileExt::lock_exclusive(&lock)?;
    let mut config = load_config(home)?;
    update(&mut config);
    let mut temp = tempfile::NamedTempFile::new_in(home)?;
    temp.write_all(&serde_json::to_vec_pretty(&config)?)?;
    temp.as_file().sync_all()?;
    temp.persist(home.join("settings.json"))?;
    Ok(config)
}

pub(crate) fn profiles_from_state(data: &[u8], root: &Path) -> Result<Vec<Profile>> {
    let state: serde_json::Value = serde_json::from_slice(data)?;
    let cache = state
        .pointer("/profile/info_cache")
        .and_then(|v| v.as_object())
        .context("Chrome 暂时没有可用的身份")?;
    let mut profiles: Vec<_> = cache
        .iter()
        .filter(|(id, _)| {
            // Never allow a profile directory to escape the Chrome data directory.
            !id.is_empty()
                && !id.contains(['/', '\\', ':'])
                && *id != "."
                && *id != ".."
                && root.join(id).is_dir()
        })
        .map(|(id, value)| Profile {
            id: id.clone(),
            name: value
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or(id)
                .to_owned(),
            fill: chrome_color(value, "default_avatar_fill_color")
                .or_else(|| chrome_color(value, "profile_highlight_color"))
                .unwrap_or(0xffdee1e6),
            stroke: chrome_color(value, "default_avatar_stroke_color").unwrap_or_else(|| {
                let c = chrome_color(value, "default_avatar_fill_color")
                    .or_else(|| chrome_color(value, "profile_highlight_color"))
                    .unwrap_or(0xffdee1e6);
                if ((c >> 16 & 255) * 299 + (c >> 8 & 255) * 587 + (c & 255) * 114) > 128000 {
                    0xff3c4043
                } else {
                    0xffffffff
                }
            }),
            chrome_active: value
                .get("active_time")
                .and_then(|v| v.as_f64())
                .and_then(|seconds| std::time::Duration::try_from_secs_f64(seconds).ok())
                .and_then(|duration| time::Duration::try_from(duration).ok())
                .and_then(|duration| OffsetDateTime::UNIX_EPOCH.checked_add(duration)),
        })
        .collect();
    profiles.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
    Ok(profiles)
}

pub(crate) fn discover_profiles() -> Result<Vec<Profile>> {
    let root = dirs::data_local_dir()
        .context("找不到用户文件夹")?
        .join("Google/Chrome/User Data");
    let data = fs::read(root.join("Local State"))
        .context("还没有找到 Chrome 身份。请先打开 Chrome，添加或登录一个身份，然后点“重新查找”")?;
    profiles_from_state(&data, &root)
}

pub(crate) fn chrome_executable() -> Result<PathBuf> {
    for (var, suffix) in [
        ("PROGRAMFILES", "Google/Chrome/Application/chrome.exe"),
        ("PROGRAMFILES(X86)", "Google/Chrome/Application/chrome.exe"),
        ("LOCALAPPDATA", "Google/Chrome/Application/chrome.exe"),
    ] {
        if let Some(root) = std::env::var_os(var) {
            let candidate = PathBuf::from(root).join(suffix);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }
    bail!("没有找到 Chrome。请先安装 Chrome，再回来试试")
}

pub(crate) fn chrome_args(profile: &str, link: &Url) -> [String; 2] {
    [
        format!("--profile-directory={profile}"),
        link.as_str().to_owned(),
    ]
}

pub(crate) fn open_link(profile: &str, link: &Url) -> Result<()> {
    let profiles = discover_profiles()?;
    if !profiles.iter().any(|p| p.id == profile) {
        bail!("这个身份已经不在 Chrome 里了，请重新选择");
    }
    Command::new(chrome_executable()?)
        .args(chrome_args(profile, link))
        .spawn()
        .context("Chrome 没有打开，请稍后重试")?;
    Ok(())
}

#[cfg(windows)]
pub(crate) fn install_and_register() -> Result<()> {
    use winreg::{RegKey, enums::HKEY_CURRENT_USER};
    let home = app_home()?;
    let bin = home.join("bin");
    fs::create_dir_all(&bin)?;
    let exe = bin.join("linklane.exe");
    let current = std::env::current_exe()?;
    if !exe.exists() || fs::canonicalize(&current)? != fs::canonicalize(&exe)? {
        fs::copy(&current, &exe).context("无法安装更新。请关闭其他轻舟窗口后再试")?;
    }
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let cap_path = "Software\\Clients\\StartMenuInternet\\Linklane\\Capabilities";
    let (cap, _) = hkcu.create_subkey(cap_path)?;
    cap.set_value("ApplicationName", &"轻舟 Linklane")?;
    cap.set_value("ApplicationDescription", &"用合适的 Chrome 身份打开网站")?;
    let (assoc, _) = cap.create_subkey("URLAssociations")?;
    for scheme in ["http", "https"] {
        assoc.set_value(scheme, &"Linklane.Url")?;
    }
    let (registered, _) = hkcu.create_subkey("Software\\RegisteredApplications")?;
    registered.set_value("轻舟 Linklane", &cap_path)?;
    let (protocol, _) = hkcu.create_subkey("Software\\Classes\\Linklane.Url")?;
    protocol.set_value("", &"Linklane web link")?;
    protocol.set_value("URL Protocol", &"")?;
    let (icon, _) = protocol.create_subkey("DefaultIcon")?;
    icon.set_value("", &format!("\"{}\",0", exe.display()))?;
    let (command, _) = protocol.create_subkey("shell\\open\\command")?;
    command.set_value("", &format!("\"{}\" \"%1\"", exe.display()))?;
    let (client, _) = hkcu.create_subkey("Software\\Clients\\StartMenuInternet\\Linklane")?;
    client.set_value("", &"轻舟 Linklane")?;
    let (open, _) = client.create_subkey("shell\\open\\command")?;
    open.set_value("", &format!("\"{}\"", exe.display()))?;
    Ok(())
}

#[cfg(not(windows))]
pub(crate) fn install_and_register() -> Result<()> {
    bail!("目前只支持 Windows")
}

pub(crate) fn open_default_settings() -> Result<()> {
    Command::new("explorer.exe")
        .arg("ms-settings:defaultapps")
        .spawn()?;
    Ok(())
}

#[cfg(windows)]
pub(crate) fn is_default() -> bool {
    use winreg::{RegKey, enums::HKEY_CURRENT_USER};
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    ["http", "https"].iter().all(|scheme| {
        hkcu.open_subkey(format!("Software\\Microsoft\\Windows\\Shell\\Associations\\UrlAssociations\\{scheme}\\UserChoice"))
            .and_then(|key| key.get_value::<String, _>("ProgId")).is_ok_and(|id| id == "Linklane.Url")
    })
}
#[cfg(not(windows))]
pub(crate) fn is_default() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    fn url(s: &str) -> Url {
        parse_link(s).unwrap()
    }
    fn profiles() -> Vec<Profile> {
        vec![Profile {
            id: "Default".into(),
            name: "工作".into(),
            ..Profile::default()
        }]
    }
    #[test]
    fn matches_only_same_origin() {
        let mut c = Config::default();
        remember(
            &mut c,
            &url("https://EXAMPLE.com/a?token=private"),
            "Default",
        );
        assert_eq!(
            remembered_profile(&c, &url("https://example.com:443/b"), &profiles()),
            Some("Default")
        );
        for s in [
            "https://example.com.evil.test",
            "https://sub.example.com",
            "http://example.com",
            "https://example.com:8443",
        ] {
            assert_eq!(remembered_profile(&c, &url(s), &profiles()), None);
        }
        assert!(!serde_json::to_string(&c).unwrap().contains("private"));
    }
    #[test]
    fn replacement_missing_profile_and_pause() {
        let mut c = Config::default();
        let u = url("http://localhost:3000");
        remember(&mut c, &u, "missing");
        assert_eq!(remembered_profile(&c, &u, &profiles()), None);
        remember(&mut c, &u, "Default");
        assert_eq!(c.sites.len(), 1);
        assert_eq!(
            remembered_profile(&c, &url("http://localhost:3001"), &profiles()),
            None
        );
        c.always_ask = true;
        assert_eq!(remembered_profile(&c, &u, &profiles()), None);
    }
    #[test]
    fn rejects_non_web_and_credentials() {
        for s in [
            "file:///C:/test",
            "javascript:alert(1)",
            "--incognito",
            "https://user:password@example.com",
        ] {
            assert!(parse_link(s).is_err());
        }
        assert!(parse_link(" https://example.com/中文 ").is_ok());
    }
    #[test]
    fn launch_preserves_single_url_argument() {
        let u = url("https://example.com/?a=1&b=hello%20world");
        assert_eq!(
            chrome_args("Profile 2", &u),
            ["--profile-directory=Profile 2", u.as_str()]
        );
    }
    #[test]
    fn concurrent_preferences_are_not_lost() {
        let temp = tempfile::tempdir().unwrap();
        std::thread::scope(|scope| {
            for n in 0..8 {
                let home = temp.path();
                scope.spawn(move || {
                    update_config(home, |c| {
                        remember(c, &url(&format!("https://site{n}.test")), "Default")
                    })
                    .unwrap();
                });
            }
        });
        assert_eq!(load_config(temp.path()).unwrap().sites.len(), 8);
    }
    #[test]
    fn corrupt_preferences_are_preserved() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("settings.json"), "broken").unwrap();
        assert!(update_config(temp.path(), |c| c.always_ask = true).is_err());
        assert_eq!(
            fs::read_to_string(temp.path().join("settings.json")).unwrap(),
            "broken"
        );
    }
    #[test]
    fn discovers_only_existing_safe_profiles() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("Default")).unwrap();
        let data =
            br#"{"profile":{"info_cache":{"Default":{"name":"Work"},"Missing":{},"..":{}}}}"#;
        assert_eq!(
            profiles_from_state(data, temp.path()).unwrap(),
            vec![Profile {
                id: "Default".into(),
                name: "Work".into(),
                fill: 0xffdee1e6,
                stroke: 0xff3c4043,
                chrome_active: None,
            }]
        );
    }

    #[test]
    fn chrome_signed_colors_and_activity_are_preserved() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("Default")).unwrap();
        let data = br#"{"profile":{"info_cache":{"Default":{"name":"Work","default_avatar_fill_color":-15261648,"default_avatar_stroke_color":-4274468,"active_time":1000.25}}}}"#;
        let p = profiles_from_state(data, temp.path()).unwrap().remove(0);
        assert_eq!(p.fill, u32::from_ne_bytes((-15261648i32).to_ne_bytes()));
        assert_eq!(p.stroke, u32::from_ne_bytes((-4274468i32).to_ne_bytes()));
        assert_eq!(
            p.chrome_active.unwrap().unix_timestamp_nanos() / 1_000_000,
            1000250
        );
    }

    #[test]
    fn lru_survives_reload_and_uses_chrome_for_initial_order() {
        let temp = tempfile::tempdir().unwrap();
        let mut p = vec![
            Profile {
                id: "a".into(),
                chrome_active: Some(OffsetDateTime::UNIX_EPOCH + time::Duration::milliseconds(100)),
                ..Profile::default()
            },
            Profile {
                id: "b".into(),
                chrome_active: Some(OffsetDateTime::UNIX_EPOCH + time::Duration::milliseconds(200)),
                ..Profile::default()
            },
        ];
        Config::default().sort_profiles(&mut p);
        assert_eq!(p[0].id, "b");
        update_config(temp.path(), |c| {
            c.mark_used("b");
            c.mark_used("a");
        })
        .unwrap();
        load_config(temp.path()).unwrap().sort_profiles(&mut p);
        assert_eq!(p[0].id, "a");
        let old: Config = serde_json::from_str(r#"{"sites":[],"always_ask":false}"#).unwrap();
        assert!(old.last_used.is_empty());
    }
}
