#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anyhow::{Result, bail};
// RATIONALE: main wires callbacks and process lifetime; render is the explicit
// UI boundary projecting the application state into Slint's generated model.
mod routing;
use routing::{
    Config, Profile, app_home, discover_profiles, install_and_register, is_default, load_config,
    open_default_settings, open_link, parse_link, remember, remembered_profile, site_key,
    update_config,
};
use slint::winit_030::WinitWindowAccessor;
use slint::{ComponentHandle, ModelRc, VecModel};
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
            .sites
            .iter()
            .map(|s| {
                let name = state
                    .config
                    .names
                    .get(&s.profile)
                    .cloned()
                    .or_else(|| {
                        state
                            .profiles
                            .iter()
                            .find(|p| p.id == s.profile)
                            .map(|p| p.name.clone())
                    })
                    .unwrap_or("已移除的身份，请忘记后重新选择".into());
                Website {
                    address: s.site.as_str().into(),
                    identity: name.into(),
                }
            })
            .collect::<Vec<_>>(),
    )));
    ui.set_always_ask(state.config.always_ask);
    ui.set_connected(!state.demo && is_default());
}

fn edit_config(state: &mut State, update: impl FnOnce(&mut Config)) -> Result<()> {
    if state.demo {
        update(&mut state.config);
    } else {
        state.config = update_config(&state.home, update)?;
    }
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let demo = args.iter().any(|a| a == "--demo");
    let dry = args.iter().any(|a| a == "--dry-run");
    let choose = args.iter().any(|a| a == "--choose");
    let screenshot = args
        .iter()
        .position(|a| a == "--screenshot")
        .and_then(|i| args.get(i + 1))
        .cloned();
    if args.iter().any(|a| a == "--help") {
        println!(
            "Linklane [--choose] [--dry-run] [https://example.com]\n  --demo         Isolated sample identities; never opens Chrome or writes settings\n  --screenshot PATH  Save a preview and exit (requires --demo)\n  --choose       Ask even for a remembered website\n  --dry-run      Print routing decision without opening anything"
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
            "--demo" | "--dry-run" | "--choose" => {}
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
        load_config(&home).unwrap_or_else(|e| {
            message = e.to_string();
            Config::default()
        })
    };
    let profiles = if demo {
        vec![
            Profile {
                id: "demo-work".into(),
                name: "工作".into(),
                fill: 0xffd3e3fd,
                stroke: 0xff0842a0,
                ..Profile::default()
            },
            Profile {
                id: "demo-personal".into(),
                name: "自己的".into(),
                fill: 0xffc4eed0,
                stroke: 0xff0f5223,
                ..Profile::default()
            },
            Profile {
                id: "demo-test".into(),
                name: "测试用".into(),
                fill: 0xffffd8e4,
                stroke: 0xff633b48,
                ..Profile::default()
            },
        ]
    } else {
        discover_profiles().unwrap_or_else(|e| {
            message = e.to_string();
            vec![]
        })
    };
    let parsed = raw.as_deref().map(parse_link).transpose();
    if let Ok(Some(url)) = &parsed {
        let target = if choose {
            None
        } else {
            remembered_profile(&config, url, &profiles)
        };
        if dry {
            println!(
                "{}",
                target.map(|p| format!("OPEN {p}")).unwrap_or("ASK".into())
            );
            return Ok(());
        }
        if !demo && let Some(profile) = target {
            match open_link(profile, url) {
                Ok(()) => {
                    // Auto-open is a use too. A bookkeeping failure must not open the URL twice.
                    if let Err(e) = update_config(&home, |c| c.mark_used(profile)) {
                        eprintln!("{e}");
                    }
                    return Ok(());
                }
                Err(e) => message = e.to_string(),
            }
        }
    } else if let Err(e) = &parsed {
        if dry {
            bail!("{e}");
        }
        message = e.to_string();
    } else if dry {
        bail!("Provide a URL with --dry-run");
    }
    let ui = MainWindow::new()?;
    let initial = raw.clone().unwrap_or_else(|| {
        if demo {
            "https://www.figma.com/design/team-project".into()
        } else {
            String::new()
        }
    });
    ui.set_link(initial.as_str().into());
    ui.set_site(
        parse_link(&initial)
            .map(|u| site_key(&u))
            .unwrap_or_default()
            .into(),
    );
    ui.set_message(message.into());
    let state = Rc::new(RefCell::new(State {
        home,
        config,
        profiles,
        demo,
        incoming: raw.is_some(),
    }));
    render(&ui, &mut state.borrow_mut());
    ui.on_link_edited({
        let weak = ui.as_weak();
        move |value| {
            if let Some(ui) = weak.upgrade() {
                ui.set_site(
                    parse_link(value.as_str())
                        .map(|u| site_key(&u))
                        .unwrap_or_default()
                        .into(),
                );
                ui.set_remember_site(true);
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
                let profile = state
                    .profiles
                    .get(usize::try_from(index).unwrap_or(usize::MAX))
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("请重新选择身份"))?;
                if !state.demo {
                    open_link(&profile.id, &url)?;
                }
                edit_config(&mut state, |c| {
                    c.mark_used(&profile.id);
                    if ui.get_remember_site() {
                        remember(c, &url, &profile.id);
                    }
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
                        if ui.get_remember_site() {
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
            let Some(site) = state
                .config
                .sites
                .get(usize::try_from(index).unwrap_or(usize::MAX))
                .map(|s| s.site.clone())
            else {
                return;
            };
            match edit_config(&mut state, |c| c.sites.retain(|s| s.site != site)) {
                Ok(()) => {
                    render(&ui, &mut state);
                    ui.set_message("已忘记，下次打开这个网站时会重新问你。".into());
                }
                Err(e) => ui.set_message(e.to_string().into()),
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
            }) {
                Ok(()) => {
                    render(&ui, &mut state);
                    ui.set_message("名字已保存。Chrome 中的名字不会改变。".into());
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
            if let Err(e) = edit_config(&mut state, |c| c.always_ask = paused) {
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
                match discover_profiles() {
                    Ok(profiles) => {
                        state.profiles = profiles;
                        ui.set_message("已重新查找。".into());
                    }
                    Err(e) => ui.set_message(e.to_string().into()),
                }
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
