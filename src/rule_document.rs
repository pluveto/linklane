use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use toml_edit::{DocumentMut, Item, Table};

use crate::{
    resolution::ResolutionSettings,
    rules::{Rule, RuleSet},
};

pub(crate) const GUIDE: &str = include_str!("../docs/rules-header.toml");

#[non_exhaustive]
#[derive(Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuleDocument {
    pub(crate) rules: Vec<Rule>,
    #[serde(default)]
    pub(crate) resolution: ResolutionSettings,
}

impl RuleDocument {
    pub(crate) fn parse(text: &str) -> Result<Self> {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let document: Self = toml::from_str(text).context("网站设置 TOML 有误")?;
        RuleSet::compile(&document.rules)?;
        document.resolution.validate()?;
        Ok(document)
    }

    pub(crate) fn encode(&self) -> Result<String> {
        Ok(Self::with_guide(&toml::to_string_pretty(self)?))
    }

    pub(crate) fn with_guide(text: &str) -> String {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        if text.starts_with(GUIDE.lines().next().unwrap_or_default()) {
            text.to_owned()
        } else {
            format!("{GUIDE}\n{text}")
        }
    }

    pub(crate) fn update_rules(source: &str, previous: &[Rule], next: &[Rule]) -> Result<String> {
        let mut document: DocumentMut = source.parse()?;
        let rules = document.get_mut("rules").context("缺少 rules")?;
        for (index, old) in previous.iter().enumerate().rev() {
            let replacement = next.iter().find(|rule| rule.name == old.name);
            match (&mut *rules, replacement) {
                (_, Some(rule)) if rule == old => {}
                (Item::ArrayOfTables(tables), None) => {
                    tables.remove(index);
                }
                (Item::ArrayOfTables(tables), Some(rule)) => {
                    let table = tables.get_mut(index).context("找不到网站设置")?;
                    let enabled = table.get_mut("enabled").context("缺少 enabled")?;
                    *enabled = toml_edit::value(rule.enabled);
                    table["action"] = Self::rule_table(rule)?
                        .remove("action")
                        .context("缺少 action")?;
                }
                (Item::Value(toml_edit::Value::Array(array)), None) => {
                    array.remove(index);
                }
                (Item::Value(toml_edit::Value::Array(array)), Some(rule)) => {
                    array.replace(index, Self::rule_table(rule)?.into_inline_table());
                }
                _ => anyhow::bail!("rules 应为表数组"),
            }
        }
        for rule in next
            .iter()
            .filter(|rule| !previous.iter().any(|old| old.name == rule.name))
        {
            let table = Self::rule_table(rule)?;
            if let Some(tables) = rules.as_array_of_tables_mut() {
                tables.push(table);
            } else if let Some(array) = rules.as_array_mut() {
                if array.is_empty() {
                    let mut tables = toml_edit::ArrayOfTables::new();
                    tables.push(table);
                    *rules = Item::ArrayOfTables(tables);
                } else {
                    array.push(table.into_inline_table());
                }
            }
        }
        if rules
            .as_array_of_tables()
            .is_some_and(toml_edit::ArrayOfTables::is_empty)
        {
            *rules = toml_edit::value(toml_edit::Array::new());
        }
        Ok(Self::with_guide(&document.to_string()))
    }

    fn rule_table(rule: &Rule) -> Result<Table> {
        let serialized = toml::to_string_pretty(rule)?
            .parse::<DocumentMut>()?
            .into_table();
        let mut table = Table::new();
        for (key, item) in serialized {
            let value = item.into_value().ok().context("无效的网站设置字段")?;
            table.insert(&key, Item::Value(value));
        }
        Ok(table)
    }
}
