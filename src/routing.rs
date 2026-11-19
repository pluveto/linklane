// RATIONALE: stateless URL parsing and Windows registration are private OS boundaries.
use anyhow::{Context, Result, bail};
use std::{fs, path::PathBuf, process::Command};
use url::Url;

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

pub(crate) fn app_home() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("LINKLANE_HOME") {
        return Ok(std::path::absolute(path)?);
    }
    Ok(dirs::data_local_dir()
        .context("找不到此电脑的用户文件夹")?
        .join("Linklane"))
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
        let cli = current.with_file_name("linklane-cli.exe");
        for (source, destination) in [(current, exe.clone()), (cli, bin.join("linklane-cli.exe"))] {
            if source.try_exists()? {
                let mut temp = tempfile::NamedTempFile::new_in(&bin)?;
                std::io::copy(&mut fs::File::open(source)?, &mut temp)?;
                temp.persist(destination)
                    .context("无法安装更新。请关闭其他轻舟窗口后再试")?;
            }
        }
    }
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let cap_path = "Software\\Clients\\StartMenuInternet\\Linklane\\Capabilities";
    let (cap, _) = hkcu.create_subkey(cap_path)?;
    cap.set_value("ApplicationName", &"轻舟 Linklane")?;
    cap.set_value("ApplicationDescription", &"用合适的浏览器身份打开网站")?;
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

    #[test]
    fn rejects_non_web_urls_and_embedded_credentials() {
        for value in [
            "file:///C:/test",
            "javascript:alert(1)",
            "--incognito",
            "https://user:password@example.com",
        ] {
            assert!(parse_link(value).is_err());
        }
        assert!(parse_link(" https://example.com/中文 ").is_ok());
    }
}
