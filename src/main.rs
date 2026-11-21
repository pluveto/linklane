#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anyhow::{Result, bail};
// RATIONALE: main wires callbacks and process lifetime; render is the explicit
// UI boundary projecting the application state into Slint's generated model.
mod advanced;
mod browsers;
mod config;
mod identity;
mod remember;
mod resolution;
mod routing;
mod rule_document;
mod rules;
mod rules_cli;
use browsers::{BrowserCatalog, BrowserFamily, Profile};
use config::{Config, ConfigStore};
use remember::RememberScope;
use routing::{app_home, install_and_register, is_default, open_default_settings, parse_link};
use rules::{RuleAction, RuleSet};
use slint::winit_030::WinitWindowAccessor;
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::{cell::RefCell, rc::Rc};
slint::include_modules!();

struct State {
    home: std::path::PathBuf,
    config: Config,
    profiles: Vec<Profile>,
    demo: bool,
    incoming: bool,
}

fn render(ui: &MainWindow, state: &mut State) {
    state.config.sort_profiles(&mut state.profiles);
    let identities = state
        .profiles
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let name = state.config.names.get(&p.id).unwrap_or(&p.name);
            Identity {
                name: name.into(),
                original: p.name.as_str().into(),
                browser: p.browser_name.as_str().into(),
                initial: name.chars().next().unwrap_or('人').to_string().into(),
                tint: slint::Color::from_argb_encoded(p.fill),
                ink: slint::Color::from_argb_encoded(p.stroke),
                recent: i == 0 && state.config.last_use(p).is_some(),
            }
        })
        .collect::<Vec<_>>();
    ui.set_identities(ModelRc::new(VecModel::from(identities)));
    ui.set_websites(ModelRc::new(VecModel::from(
        state
            .config
            .rules
            .iter()
            .map(|rule| {
                let identity = match &rule.action {
                    RuleAction::Ask => "每次询问".to_owned(),
                    RuleAction::Open { identity } => state
                        .profiles
                        .iter()
                        .find(|profile| profile.id == *identity)
                        .map(|profile| {
                            format!(
                                "{} · {}",
                                profile.browser_name,
                                state.config.names.get(identity).unwrap_or(&profile.name)
                            )
                        })
                        .unwrap_or_else(|| format!("身份暂不可用：{identity}")),
                };
                Website {
                    address: rule.name.as_str().into(),
                    identity: format!(
                        "{}{}",
                        if rule.enabled { "" } else { "已停用 · " },
                        identity
                    )
                    .into(),
                }
            })
            .collect::<Vec<_>>(),
    )));
    ui.set_always_ask(state.config.always_ask);
    ui.set_connected(!state.demo && is_default());
    preview_resolution(ui, &state.config);
}

// RATIONALE: this UI boundary projects the resolved URL without changing the opening URL.
fn preview_resolution(ui: &MainWindow, config: &Config) {
    let url = parse_link(ui.get_link().as_str()).ok();
    let resolved = url.as_ref().map(|url| config.resolution.resolve(url));
    let target = resolved
        .as_ref()
        .and_then(|resolved| resolved.matching_url());
    let changed = resolved.as_ref().filter(|resolved| resolved.changed());
    ui.set_resolved_target(changed.and(target).map_or("", url::Url::as_str).into());
    ui.set_resolution_summary(
        changed
            .map(|resolved| resolved.summary())
            .unwrap_or_default()
            .into(),
    );
    let labels = target
        .map(RememberScope::suggest)
        .unwrap_or_default()
        .into_iter()
        .map(|scope| slint::SharedString::from(scope.label()))
        .collect::<Vec<_>>();
    if !ui.get_remember_scopes().iter().eq(labels.iter().cloned()) {
        ui.set_remember_scopes(ModelRc::new(VecModel::from(labels)));
        ui.set_remember_index(0);
    }
}

fn edit_config(state: &mut State, update: impl FnOnce(&mut Config) -> Result<()>) -> Result<()> {
    if state.demo {
        update(&mut state.config)?;
    } else {
        state.config = ConfigStore::at(&state.home).update(update)?;
    }
    Ok(())
}

fn main() -> Result<()> {
    let mut raw_args = std::env::args_os().skip(1);
    let first = raw_args.next();
    if first.as_deref() == Some(std::ffi::OsStr::new("--install")) && raw_args.len() == 0 {
        return install_and_register();
    }
    if first.as_deref() == Some(std::ffi::OsStr::new("--rules-cli")) {
        if let Err(error) = rules_cli::run(raw_args.collect()) {
            eprintln!("{error:#}");
            std::process::exit(1);
        }
        return Ok(());
    }
    let args: Vec<_> = std::env::args().skip(1).collect();
    let demo = args.iter().any(|a| a == "--demo");
    let dry = args.iter().any(|a| a == "--dry-run");
    let choose = args.iter().any(|a| a == "--choose");
    let explain = args.iter().any(|a| a == "--explain");
    let list_identities = args.iter().any(|a| a == "--list-identities");
    let screenshot = args
        .iter()
        .position(|a| a == "--screenshot")
        .and_then(|i| args.get(i + 1))
        .cloned();
    if args.iter().any(|a| a == "--help") {
        println!(
            "Linklane [--choose] [--dry-run] [https://example.com]\n  --demo         Isolated sample identities; never opens a browser or writes settings\n  --screenshot PATH  Save a preview and exit (requires --demo)\n  --choose       Ask even for a remembered website\n  --dry-run      Print routing decision without opening anything\n  --explain      Explain ordered rule matching without opening anything\n  --list-identities  Print available browser identity IDs"
        );
        return Ok(());
    }
    if screenshot.is_some() && !demo {
        bail!("--screenshot requires --demo");
    }
    let mut raw = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--demo" | "--dry-run" | "--choose" | "--explain" | "--list-identities" => {}
            "--screenshot" => {
                index += 1;
                if index == args.len() {
                    bail!("--screenshot needs a path");
                }
            }
            a if a.starts_with("--") => bail!("Unknown option: {a}"),
            a => {
                if raw.is_some() {
                    bail!("Only one URL at a time is supported");
                }
                raw = Some(a.to_owned());
            }
        }
        index += 1;
    }
    let home = app_home()?;
    let mut message = String::new();
    let config = if demo {
        Config::default()
    } else {
        match ConfigStore::at(&home).load() {
            Ok(config) => config,
            Err(error) if dry || explain || list_identities => return Err(error),
            Err(error) => {
                message = format!("{error:#}");
                Config::default()
            }
        }
    };
    let profiles = if demo {
        vec![
            Profile::preview(BrowserFamily::Chrome, "Default", "工作")?,
            Profile::preview(BrowserFamily::Edge, "Default", "测试")?,
            Profile::preview(BrowserFamily::Brave, "Default", "自己的")?,
            Profile::preview(BrowserFamily::Firefox, "Profiles/demo", "研究")?,
        ]
    } else {
        let catalog = BrowserCatalog::discover();
        if !catalog.warnings.is_empty() {
            if !message.is_empty() {
                message.push('\n');
            }
            message.push_str(&catalog.warnings.join("\n"));
        }
        catalog.profiles
    };
    if list_identities {
        for profile in &profiles {
            println!(
                "{}\t{} · {}",
                profile.id, profile.browser_name, profile.name
            );
        }
        if !message.is_empty() {
            eprintln!("{message}");
        }
        return Ok(());
    }
    let parsed = raw.as_deref().map(parse_link).transpose();
    if let Ok(Some(url)) = &parsed {
        let decision = config.resolution.resolve(url).decide(
            &RuleSet::compile(&config.rules)?,
            &profiles,
            choose || config.always_ask,
        );
        if dry || explain {
            if explain {
                println!("{}", decision.explanation);
            } else {
                println!(
                    "{}",
                    decision
                        .identity
                        .as_ref()
                        .map_or("ASK".into(), |identity| format!("OPEN {identity}"))
                );
            }
            return Ok(());
        }
        if !demo && let Some(identity) = decision.identity {
            let profile = profiles
                .iter()
                .find(|profile| profile.id == identity)
                .context("选中的身份已改变")?;
            match profile.open(url) {
                Ok(()) => {
                    match ConfigStore::at(&home).update(|config| {
                        config.mark_used(&identity);
                        Ok(())
                    }) {
                        Ok(_) => return Ok(()),
                        Err(error) => {
                            message = format!("网站已打开，最近使用记录保存失败：{error:#}");
                        }
                    }
                }
                Err(error) => message = format!("{error:#}"),
            }
        } else if let Some(notice) = decision.notice {
            message = notice;
        }
    } else if let Err(error) = &parsed {
        if dry || explain {
            bail!("{error}");
        }
        message = error.to_string();
    } else if dry || explain {
        bail!("请提供要测试的网址");
    }
    let ui = MainWindow::new()?;
    ui.on_resize_window({
        let weak = ui.as_weak();
        move |width, height| {
            if let Some(ui) = weak.upgrade() {
                ui.window().set_size(slint::LogicalSize::new(width, height));
            }
        }
    });
    let initial = raw.clone().unwrap_or_else(|| {
        if demo {
            "https://www.figma.com/design/team-project".into()
        } else {
            String::new()
        }
    });
    ui.set_link(initial.as_str().into());
    ui.set_message(message.into());
    let state = Rc::new(RefCell::new(State {
        home,
        config,
        profiles,
        demo,
        incoming: raw.is_some(),
    }));
    render(&ui, &mut state.borrow_mut());
    advanced::bind(&ui, state.clone())?;
    ui.on_link_edited({
        let weak = ui.as_weak();
        let state = state.clone();
        move |_value| {
            if let Some(ui) = weak.upgrade() {
                ui.set_remember_index(0);
                preview_resolution(&ui, &state.borrow().config);
                ui.set_message("".into());
            }
        }
    });
    ui.on_choose({
        let weak = ui.as_weak();
        let state = state.clone();
        move |index| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let outcome = (|| -> Result<()> {
                let url = parse_link(ui.get_link().as_str())?;
                let mut state = state.borrow_mut();
                let resolved = state.config.resolution.resolve(&url);
                let selection = if ui.get_remember_site() {
                    resolved
                        .matching_url()
                        .zip(usize::try_from(ui.get_remember_index()).ok())
                } else {
                    None
                };
                let profile = state
                    .profiles
                    .get(usize::try_from(index).unwrap_or(usize::MAX))
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("请重新选择身份"))?;
                if !state.demo {
                    profile.open(&url)?;
                }
                edit_config(&mut state, |c| {
                    c.mark_used(&profile.id);
                    if let Some((target, index)) = selection {
                        if c.resolution.resolve(&url).matching_url() != Some(target) {
                            bail!("目标网址的识别方式已变更，请重新打开轻舟后再保存。");
                        }
                        c.remember(target, index, &profile.id)?;
                    }
                    Ok(())
                })
                .context("网站已打开，但没能保存这次选择")?;
                render(&ui, &mut state);
                if state.incoming && !state.demo {
                    slint::quit_event_loop()?;
                }
                let name = state.config.names.get(&profile.id).unwrap_or(&profile.name);
                ui.set_message(
                    format!(
                        "{}“{}”打开{}",
                        if state.demo {
                            "预览：会用"
                        } else {
                            "已用"
                        },
                        name,
                        if selection.is_some() {
                            "，下次也会记住。"
                        } else {
                            "。"
                        }
                    )
                    .into(),
                );
                Ok(())
            })();
            if let Err(e) = outcome {
                ui.set_message(format!("{e:#}").into());
            }
        }
    });
    ui.on_forget({
        let weak = ui.as_weak();
        let state = state.clone();
        move |index| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let mut state = state.borrow_mut();
            let Ok(index) = usize::try_from(index) else {
                return;
            };
            if index >= state.config.rules.len() {
                return;
            }
            let result = if state.demo {
                state.config.rules.remove(index);
                Ok(())
            } else {
                ConfigStore::at(&state.home)
                    .forget(index, &state.config.routing_text)
                    .map(|config| {
                        state.config = config;
                    })
            };
            match result {
                Ok(()) => {
                    render(&ui, &mut state);
                    ui.set_message("已移除这条规则。其余规则仍按顺序匹配。".into());
                }
                Err(error) => ui.set_message(format!("{error:#}").into()),
            }
        }
    });
    ui.on_rename({
        let weak = ui.as_weak();
        let state = state.clone();
        move |index, name| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let name = name.trim();
            if name.is_empty() || name.chars().count() > 30 {
                ui.set_message("名字请填写 1 到 30 个字。".into());
                return;
            }
            let mut state = state.borrow_mut();
            let Some(id) = state
                .profiles
                .get(usize::try_from(index).unwrap_or(usize::MAX))
                .map(|p| p.id.clone())
            else {
                return;
            };
            match edit_config(&mut state, |c| {
                c.names.insert(id, name.to_owned());
                Ok(())
            }) {
                Ok(()) => {
                    render(&ui, &mut state);
                    ui.set_message("名字已保存在轻舟中。".into());
                }
                Err(e) => ui.set_message(e.to_string().into()),
            }
        }
    });
    ui.on_pause_changed({
        let weak = ui.as_weak();
        let state = state.clone();
        move |paused| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let mut state = state.borrow_mut();
            if let Err(e) = edit_config(&mut state, |c| {
                c.always_ask = paused;
                Ok(())
            }) {
                ui.set_message(e.to_string().into());
            }
            render(&ui, &mut state);
        }
    });
    ui.on_refresh({
        let weak = ui.as_weak();
        let state = state.clone();
        move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let mut state = state.borrow_mut();
            if !state.demo {
                let catalog = BrowserCatalog::discover();
                state.profiles = catalog.profiles;
                ui.set_message(if catalog.warnings.is_empty() {
                    "已重新查找。".into()
                } else {
                    catalog.warnings.join("\n").into()
                });
            }
            render(&ui, &mut state);
        }
    });
    ui.on_connect({ let weak = ui.as_weak(); let state = state.clone(); move || {
        let Some(ui) = weak.upgrade() else { return; };
        if state.borrow().demo { ui.set_message("预览模式：正式使用时，这里会带你去系统设置。".into()); return; }
        match install_and_register().and_then(|()| open_default_settings()) {
            Ok(()) => ui.set_message("在系统设置中搜索“轻舟”，选择它，再点“设为默认值”。如果分别显示 HTTP 和 HTTPS，请都选轻舟。".into()),
            Err(e) => ui.set_message(format!("{e:#}").into()),
        }
    }});
    ui.on_cancel(|| {
        let _ = slint::quit_event_loop();
    });
    ui.on_drag_window({
        let weak = ui.as_weak();
        move || {
            if let Some(ui) = weak.upgrade() {
                ui.window().with_winit_window(|w| {
                    let _ = w.drag_window();
                });
            }
        }
    });
    ui.on_minimize({
        let weak = ui.as_weak();
        move || {
            if let Some(ui) = weak.upgrade() {
                ui.window().set_minimized(true);
            }
        }
    });
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_secs(2),
        {
            let weak = ui.as_weak();
            move || {
                if !demo && let Some(ui) = weak.upgrade() {
                    ui.set_connected(is_default());
                }
            }
        },
    );
    let shot_timer = slint::Timer::default();
    if let Some(path) = screenshot {
        shot_timer.start(
            slint::TimerMode::SingleShot,
            std::time::Duration::from_millis(700),
            {
                let weak = ui.as_weak();
                move || {
                    let result = (|| -> Result<()> {
                        let ui = weak.upgrade().context("预览窗口已关闭")?;
                        let pixels = ui.window().take_snapshot()?;
                        image::save_buffer(
                            &path,
                            pixels.as_bytes(),
                            pixels.width(),
                            pixels.height(),
                            image::ColorType::Rgba8,
                        )?;
                        Ok(())
                    })();
                    if let Err(e) = result {
                        eprintln!("Screenshot failed: {e}");
                    }
                    let _ = slint::quit_event_loop();
                }
            },
        );
    }
    ui.run()?;
    Ok(())
}

use anyhow::Context;
