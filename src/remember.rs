use url::Url;

use crate::rules::{Condition, Rule};

#[non_exhaustive]
pub(crate) struct RememberScope {
    origin: String,
    prefix: Option<String>,
}

impl RememberScope {
    pub(crate) fn suggest(url: &Url) -> Vec<Self> {
        const MAX_ANCESTORS: usize = 12;
        let origin = url.origin().ascii_serialization();
        let path = url.path().trim_end_matches('/');
        let mut scopes = vec![Self {
            origin: origin.clone(),
            prefix: None,
        }];
        // Split at literal separators; encoded separators stay inside their segment.
        let ancestors = path.match_indices('/').filter_map(|(end, _)| {
            let prefix = &path[..end];
            (!prefix.is_empty() && !prefix.ends_with('/')).then_some(prefix)
        });
        for prefix in ancestors
            .take(MAX_ANCESTORS)
            .chain((!path.is_empty()).then_some(path))
        {
            scopes.push(Self {
                origin: origin.clone(),
                prefix: Some(prefix.to_owned()),
            });
        }
        scopes
    }

    pub(crate) fn label(&self) -> String {
        match &self.prefix {
            Some(prefix) => format!("{prefix} 及下级"),
            None => format!("{} 下的所有链接", self.origin),
        }
    }

    pub(crate) fn name(&self) -> String {
        match &self.prefix {
            Some(prefix) => format!("路径：{}{prefix}", self.origin),
            None => format!("网站：{}", self.origin),
        }
    }

    pub(crate) fn conditions(&self) -> Vec<Condition> {
        let mut conditions = vec![Condition::Origin {
            value: self.origin.clone(),
        }];
        if let Some(prefix) = &self.prefix {
            conditions.push(Condition::PathPrefix {
                value: prefix.clone(),
            });
        }
        conditions
    }

    pub(crate) fn is_origin(&self) -> bool {
        self.prefix.is_none()
    }

    pub(crate) fn is_broader_than(&self, rule: &Rule) -> bool {
        let Some(prefix) = &self.prefix else {
            return false;
        };
        let [
            Condition::Origin { value: origin },
            Condition::PathPrefix { value: path },
        ] = rule.all.as_slice()
        else {
            return false;
        };
        let path = path.trim_end_matches('/');
        origin == &self.origin && path != prefix && Condition::path_has_prefix(path, prefix)
    }
}
