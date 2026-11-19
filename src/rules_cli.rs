// RATIONALE: this entry point translates console arguments and reports store results.
use anyhow::{Context, Result, bail};
use std::{ffi::OsString, fs, io::Read, path::Path};

use crate::{
    browsers::BrowserCatalog,
    config::ConfigStore,
    routing::{app_home, parse_link},
    rule_document::RuleDocument,
    rules::RuleSet,
};

const HELP: &str = "\
linklane-cli rules path                 Print the editable TOML path
linklane-cli rules show                 Print the active TOML (with examples)
linklane-cli rules identities           List available identity IDs
linklane-cli rules check [FILE|-]       Validate the editable file, FILE or stdin
linklane-cli rules reload               Validate and apply the editable file
linklane-cli rules replace FILE|-       Validate and apply a complete TOML document
linklane-cli rules explain URL          Explain how an incoming link would open";

pub(crate) fn run(args: Vec<OsString>) -> Result<()> {
    if args.is_empty() || args == ["--help"] || args == ["rules", "--help"] {
        println!("{HELP}");
        return Ok(());
    }
    let [group, command, tail @ ..] = args.as_slice() else {
        bail!("{HELP}");
    };
    if group != "rules" {
        bail!("{HELP}");
    }
    let store = ConfigStore::at(&app_home()?);
    match (command.to_str(), tail) {
        (Some("path"), []) => {
            store.load()?;
            println!("{}", store.rules_path().display());
        }
        (Some("show"), []) => print!("{}", store.load()?.routing_text),
        (Some("identities"), []) => {
            let catalog = BrowserCatalog::discover();
            for warning in catalog.warnings {
                eprintln!("{warning}");
            }
            for profile in catalog.profiles {
                println!(
                    "{}\t{} · {}",
                    profile.id, profile.browser_name, profile.name
                );
            }
        }
        (Some("check"), []) => {
            println!(
                "OK: {}",
                RuleDocument::parse(&fs::read_to_string(store.rules_path())?)?
                    .rules
                    .len()
            );
        }
        (Some("check"), [path]) => {
            println!(
                "OK: {}",
                RuleDocument::parse(&input(Path::new(path))?)?.rules.len()
            );
        }
        (Some("reload"), []) => {
            let config = store
                .reload_rules()
                .context("重载失败，仍使用上次生效的设置")?;
            println!("APPLIED: {}", config.rules.len());
        }
        (Some("replace"), [path]) => {
            let config = store
                .replace_document(&input(Path::new(path))?, None)
                .context("替换失败，仍使用上次生效的设置")?;
            println!("APPLIED: {}", config.rules.len());
        }
        (Some("explain"), [url]) => {
            let url = parse_link(url.to_str().context("网址需使用 UTF-8 编码")?)?;
            let config = store.load()?;
            let decision = config.resolution.resolve(&url).decide(
                &RuleSet::compile(&config.rules)?,
                &BrowserCatalog::discover().profiles,
                config.always_ask,
            );
            println!("{}", decision.explanation);
        }
        _ => bail!("{HELP}"),
    }
    Ok(())
}

fn input(path: &Path) -> Result<String> {
    if path == Path::new("-") {
        let mut text = String::new();
        std::io::stdin()
            .read_to_string(&mut text)
            .context("读取标准输入失败；请使用 UTF-8")?;
        Ok(text)
    } else {
        fs::read_to_string(path)
            .with_context(|| format!("读取 {} 失败；请使用 UTF-8", path.display()))
    }
}
