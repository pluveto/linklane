// RATIONALE: bind wires Slint callbacks; rule validation, matching and persistence belong to their owners.
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use slint::ComponentHandle;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use crate::{
    MainWindow, State,
    config::ConfigStore,
    resolution::ResolutionSettings,
    routing::parse_link,
    rules::{Rule, RuleSet},
};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuleDocument {
    rules: Vec<Rule>,
    #[serde(default)]
    resolution: ResolutionSettings,
}

impl RuleDocument {
    fn read(text: &str) -> Result<Self> {
        let document: Self = serde_json::from_str(text).context("规则 JSON 无效")?;
        RuleSet::compile(&document.rules)?;
        document.resolution.validate()?;
        Ok(document)
    }
}

pub(crate) fn bind(ui: &MainWindow, state: Rc<RefCell<State>>) -> Result<()> {
    let revision = Rc::new(Cell::new(state.borrow().config.rules_revision));
    ui.set_rules_draft(
        serde_json::to_string_pretty(&RuleDocument {
            rules: state.borrow().config.rules.clone(),
            resolution: state.borrow().config.resolution.clone(),
        })?
        .into(),
    );
    ui.on_reload_rules({
        let weak = ui.as_weak();
        let state = state.clone();
        let revision = revision.clone();
        move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let result = (|| -> Result<()> {
                let mut state = state.borrow_mut();
                if !state.demo {
                    state.config = ConfigStore::at(&state.home).load()?;
                }
                let text = serde_json::to_string_pretty(&RuleDocument {
                    rules: state.config.rules.clone(),
                    resolution: state.config.resolution.clone(),
                })?;
                ui.set_rules_draft(text.into());
                ui.set_rules_dirty(false);
                revision.set(state.config.rules_revision);
                crate::render(&ui, &mut state);
                Ok(())
            })();
            ui.set_message(match result {
                Ok(()) => "已载入保存的规则。".into(),
                Err(error) => format!("{error:#}").into(),
            });
        }
    });
    ui.on_apply_rules({
        let weak = ui.as_weak();
        let state = state.clone();
        let revision = revision.clone();
        move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let result = (|| -> Result<()> {
                let document = RuleDocument::read(ui.get_rules_draft().as_str())?;
                let mut state = state.borrow_mut();
                if state.demo {
                    state.config.rules = document.rules;
                    state.config.resolution = document.resolution;
                } else {
                    state.config = ConfigStore::at(&state.home).replace_routing(
                        document.rules,
                        document.resolution,
                        revision.get(),
                    )?;
                }
                revision.set(state.config.rules_revision);
                ui.set_rules_dirty(false);
                crate::render(&ui, &mut state);
                Ok(())
            })();
            ui.set_message(match result {
                Ok(()) => "规则已应用，后续链接将按此顺序匹配。".into(),
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
                let document = RuleDocument::read(ui.get_rules_draft().as_str())?;
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
