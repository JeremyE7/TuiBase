#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchQuery {
    Local(String),
    Global(GlobalPath),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GlobalPath {
    pub database: Option<String>,
    pub kind: Option<String>,
    pub object_prefix: Option<String>,
}

pub fn classify_query(input: &str) -> SearchQuery {
    if !input.starts_with('/') {
        return SearchQuery::Local(input.to_owned());
    }

    let parts: Vec<&str> = input
        .split('/')
        .skip(1)
        .filter(|part| !part.is_empty())
        .collect();

    SearchQuery::Global(GlobalPath {
        database: parts.first().map(|value| (*value).to_owned()),
        kind: parts.get(1).map(|value| (*value).to_owned()),
        object_prefix: parts.get(2).map(|value| (*value).to_owned()),
    })
}

pub fn matches(value: &str, query: &str) -> bool {
    value.to_lowercase().contains(&query.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_is_local_search() {
        assert_eq!(
            classify_query("proced"),
            SearchQuery::Local("proced".to_owned())
        );
    }

    #[test]
    fn leading_slash_is_global_path() {
        assert_eq!(
            classify_query("/meg_servicios/procedimientos/"),
            SearchQuery::Global(GlobalPath {
                database: Some("meg_servicios".to_owned()),
                kind: Some("procedimientos".to_owned()),
                object_prefix: None,
            })
        );
    }

    #[test]
    fn text_with_slash_inside_remains_local() {
        assert_eq!(
            classify_query("owner/object"),
            SearchQuery::Local("owner/object".to_owned())
        );
    }
}
