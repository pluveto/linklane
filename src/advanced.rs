// RATIONALE: bind wires Slint callbacks; rule validation, matching and persistence belong to their owners.
use anyhow::Result;
use slint::ComponentHandle;
use std::{cell::RefCell, rc::Rc};

use crate::{
    MainWindow, State, config::ConfigStore, routing::parse_link, rule_document::RuleDocument,
    rules::RuleSet,
};

pub(crate) fn bind(ui: &MainWindow, state: Rc<RefCell<State>>) -> Result<()> {
    let store = ConfigStore::at(&state.borrow().home);
    let path = store.rules_path();
    ui.set_rules_path(if state.borrow().demo {
        r"C:\Users\Demo\AppData\Local\Linklane\rules.toml".into()
    } else {
        path.display().to_string().into()
    });
    ui.set_can_copy_rules_path(!state.borrow().demo);
    if state.borrow().config.routing_text.is_empty() {
        state.borrow_mut().config.routing_text = RuleDocument::default().encode()?;
    }
    ui.set_rules_draft(state.borrow().config.routing_text.as_str().into());
    let editing_base = Rc::new(RefCell::new(state.borrow().config.routing_text.clone()));
    ui.on_copy_rules_path({
        let weak = ui.as_weak();
        move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            #[cfg(windows)]
            let result = arboard::Clipboard::new()
                .and_then(|mut clipboard| clipboard.set_text(path.display().to_string()));
            #[cfg(not(windows))]
            let result: Result<()> = Err(anyhow::anyhow!("请从地址栏选择并复制路径"));
            ui.set_rules_status(match result {
                Ok(()) => "路径已复制，可粘贴给 AI。".into(),
                Err(error) => format!("复制失败：{error}。你也可以选中路径后复制。").into(),
            });
        }
    });
    ui.on_enter_rules({
        let weak = ui.as_weak();
        let state = state.clone();
        let editing_base = editing_base.clone();
        move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            if !ui.get_rules_dirty() {
                let mut state = state.borrow_mut();
                if !state.demo {
                    match ConfigStore::at(&state.home).load() {
                        Ok(config) => state.config = config,
                        Err(error) => {
                            ui.set_rules_status(format!("读取生效设置失败：{error:#}").into())
                        }
                    }
                }
                *editing_base.borrow_mut() = state.config.routing_text.clone();
                ui.set_rules_draft(state.config.routing_text.as_str().into());
                crate::render(&ui, &mut state);
            }
        }
    });
    ui.on_reload_rules({
        let weak = ui.as_weak();
        let state = state.clone();
        let editing_base = editing_base.clone();
        move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let result = (|| -> Result<()> {
                let mut state = state.borrow_mut();
                if !state.demo {
                    let store = ConfigStore::at(&state.home);
                    state.config = store.reload_rules()?;
                }
                ui.set_rules_draft(state.config.routing_text.as_str().into());
                ui.set_rules_dirty(false);
                ui.set_test_result("".into());
                *editing_base.borrow_mut() = state.config.routing_text.clone();
                crate::render(&ui, &mut state);
                Ok(())
            })();
            ui.set_rules_status(match result {
                Ok(()) => "已从文件加载，后续链接将使用这份设置。".into(),
                Err(error) => format!("重载失败，仍使用上次生效的设置。\n{error:#}").into(),
            });
        }
    });
    ui.on_apply_rules({
        let weak = ui.as_weak();
        let state = state.clone();
        let editing_base = editing_base.clone();
        move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let result = (|| -> Result<()> {
                let text = ui.get_rules_draft();
                let mut state = state.borrow_mut();
                if state.demo {
                    let document = RuleDocument::parse(text.as_str())?;
                    state.config.rules = document.rules;
                    state.config.resolution = document.resolution;
                    state.config.routing_text = RuleDocument::with_guide(text.as_str());
                } else {
                    state.config = ConfigStore::at(&state.home)
                        .replace_document(text.as_str(), Some(&editing_base.borrow()))?;
                }
                *editing_base.borrow_mut() = state.config.routing_text.clone();
                ui.set_rules_draft(state.config.routing_text.as_str().into());
                ui.set_rules_dirty(false);
                crate::render(&ui, &mut state);
                Ok(())
            })();
            ui.set_rules_status(match result {
                Ok(()) => "设置已应用，后续链接将按此顺序匹配。".into(),
                Err(error) => format!("{error:#}；草稿已保留。更新规则前请先修正错误。").into(),
            });
        }
    });
    ui.on_test_rules({
        let weak = ui.as_weak();
        let state = state.clone();
        move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let result = (|| -> Result<String> {
                let document = RuleDocument::parse(ui.get_rules_draft().as_str())?;
                let url = parse_link(ui.get_test_url().as_str())?;
                let state = state.borrow();
                Ok(document
                    .resolution
                    .resolve(&url)
                    .decide(
                        &RuleSet::compile(&document.rules)?,
                        &state.profiles,
                        state.config.always_ask,
                    )
                    .explanation)
            })();
            ui.set_test_result(match result {
                Ok(explanation) => explanation.into(),
                Err(error) => format!("{error:#}").into(),
            });
        }
    });
    ui.on_rules_help({
        let weak = ui.as_weak();
        move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let state = state.borrow();
            let identities = state
                .profiles
                .iter()
                .map(|profile| {
                    format!(
                        "{}  →  {} · {}",
                        profile.id, profile.browser_name, profile.name
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            ui.set_help_text(
                format!(
                    "当前可用身份（复制左侧标识作为 identity）：\n{identities}\n\n{}",
                    include_str!("../docs/RULES.md")
                )
                .into(),
            );
            ui.set_show_help(true);
        }
    });
    Ok(())
}
