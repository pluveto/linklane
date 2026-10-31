use super::*;

struct Fixture {
    directory: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir(directory.path().join("Default")).unwrap();
        Self { directory }
    }
    fn installation(&self, family: BrowserFamily) -> BrowserInstallation {
        BrowserInstallation {
            family,
            executable: self.directory.path().join("browser.exe"),
            root: self.directory.path().to_owned(),
        }
    }
}

#[test]
fn identical_profile_directories_in_different_browsers_have_independent_identities() {
    let fixture = Fixture::new();
    let data = br#"{"profile":{"info_cache":{"Default":{"name":"Work"},"Missing":{},"..":{},"../Default":{}}}}"#;
    let chrome = fixture
        .installation(BrowserFamily::Chrome)
        .chromium_profiles(data)
        .unwrap();
    let edge = fixture
        .installation(BrowserFamily::Edge)
        .chromium_profiles(data)
        .unwrap();
    assert_eq!(chrome.len(), 1);
    assert_eq!(edge.len(), 1);
    assert_ne!(chrome[0].id, edge[0].id);
    assert_eq!(chrome[0].id.to_string(), "chrome:Default");
    assert_eq!(edge[0].id.to_string(), "edge:Default");
}

#[test]
fn chromium_colors_activity_and_launch_arguments_preserve_profile_and_url() {
    let fixture = Fixture::new();
    let data = br#"{"profile":{"info_cache":{"Default":{"name":"Work","default_avatar_fill_color":-15261648,"default_avatar_stroke_color":-4274468,"active_time":1000.25}}}}"#;
    let profile = fixture
        .installation(BrowserFamily::Chrome)
        .chromium_profiles(data)
        .unwrap()
        .remove(0);
    assert_eq!(
        profile.fill,
        u32::from_ne_bytes((-15_261_648_i32).to_ne_bytes())
    );
    assert_eq!(
        profile.stroke,
        u32::from_ne_bytes((-4_274_468_i32).to_ne_bytes())
    );
    assert_eq!(
        profile.browser_active.unwrap().unix_timestamp_nanos() / 1_000_000,
        1_000_250
    );
    let url = Url::parse("https://example.com/?a=1&b=hello%20world").unwrap();
    let arguments = profile.launch.unwrap().arguments(&url);
    assert_eq!(arguments[1], "--profile-directory=Default");
    assert_eq!(arguments[2], url.as_str());
    assert!(
        arguments[0]
            .to_string_lossy()
            .contains(&fixture.directory.path().display().to_string())
    );
}

#[test]
fn firefox_discovers_relative_and_absolute_profiles_with_separate_literal_arguments() {
    let fixture = Fixture::new();
    let external = tempfile::tempdir().unwrap();
    let data = format!(
        "[Profile0]\nName=Research\nIsRelative=1\nPath=Default\n[Profile1]\nName=External\nIsRelative=0\nPath={}\n",
        external.path().display()
    );
    let profiles = fixture
        .installation(BrowserFamily::Firefox)
        .firefox_profiles(&data)
        .unwrap();
    assert_eq!(profiles.len(), 2);
    let profile = profiles
        .iter()
        .find(|profile| profile.name == "Research")
        .unwrap();
    let url = Url::parse("https://example.com/?a=1&b=2").unwrap();
    let args = profile.launch.as_ref().unwrap().arguments(&url);
    assert_eq!(
        args,
        [
            std::ffi::OsString::from("--profile"),
            fixture.directory.path().join("Default").into_os_string(),
            "--new-tab".into(),
            url.as_str().into()
        ]
    );
}

#[test]
fn removed_profile_fails_before_spawning_a_browser() {
    let fixture = Fixture::new();
    let profile = fixture
        .installation(BrowserFamily::Chrome)
        .chromium_profiles(br#"{"profile":{"info_cache":{"Default":{"name":"Work"}}}}"#)
        .unwrap()
        .remove(0);
    fs::remove_dir(fixture.directory.path().join("Default")).unwrap();
    assert!(
        profile
            .open(&Url::parse("https://example.com").unwrap())
            .unwrap_err()
            .to_string()
            .contains("目录已移除")
    );
}
