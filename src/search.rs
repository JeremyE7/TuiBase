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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct MatchScore {
    kind: u8,
    distance: usize,
    start: usize,
}

impl Default for MatchScore {
    fn default() -> Self {
        Self {
            kind: 0,
            distance: 0,
            start: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PreparedText {
    whole: Vec<char>,
    tokens: Vec<Vec<char>>,
}

#[derive(Debug, Clone)]
pub struct PreparedQuery {
    chars: Vec<char>,
    has_whitespace: bool,
}

pub fn prepare_text(value: &str) -> PreparedText {
    let whole = normalize(value);
    let tokens = value
        .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
        .filter(|token| !token.is_empty())
        .map(normalize)
        .collect();

    PreparedText { whole, tokens }
}

pub fn prepare_query(query: &str) -> PreparedQuery {
    let query = query.trim();
    PreparedQuery {
        chars: normalize(query),
        has_whitespace: query.chars().any(char::is_whitespace),
    }
}

pub fn score(value: &str, query: &str) -> Option<MatchScore> {
    let value = prepare_text(value);
    let query = prepare_query(query);
    score_prepared(&value, &query)
}

pub fn score_prepared(value: &PreparedText, query: &PreparedQuery) -> Option<MatchScore> {
    score_normalized(&value.whole, &query.chars)
}

pub fn best_prepared_match_score<'a, I>(values: I, query: &PreparedQuery) -> Option<MatchScore>
where
    I: IntoIterator<Item = &'a PreparedText>,
{
    let mut best = None;
    for value in values {
        update_best_score(&mut best, score_normalized(&value.whole, &query.chars));
        if !query.has_whitespace {
            for token in &value.tokens {
                update_best_score(&mut best, score_normalized(token, &query.chars));
            }
        }
    }
    best
}

fn update_best_score(best: &mut Option<MatchScore>, candidate: Option<MatchScore>) {
    if candidate.is_some_and(|candidate| best.is_none_or(|current| candidate < current)) {
        *best = candidate;
    }
}

fn score_normalized(value: &[char], query: &[char]) -> Option<MatchScore> {
    if query.is_empty() {
        return Some(MatchScore {
            kind: 0,
            distance: 0,
            start: 0,
        });
    }
    if value == query {
        return Some(MatchScore {
            kind: 0,
            distance: 0,
            start: 0,
        });
    }
    if value.starts_with(&query) {
        return Some(MatchScore {
            kind: 1,
            distance: value.len().saturating_sub(query.len()),
            start: 0,
        });
    }
    if let Some(start) = contiguous_start(&value, &query) {
        return Some(MatchScore {
            kind: 2,
            distance: value.len().saturating_sub(query.len()),
            start,
        });
    }
    if let Some((gaps, start)) = subsequence_match(&value, &query) {
        return Some(MatchScore {
            kind: 3,
            distance: gaps,
            start,
        });
    }

    if query.len() >= 3 {
        let maximum_distance = (query.len() / 3).clamp(1, 3);
        if value.len().abs_diff(query.len()) > maximum_distance {
            return None;
        }
        let distance = levenshtein_distance(&value, &query);
        if distance <= maximum_distance {
            return Some(MatchScore {
                kind: 4,
                distance,
                start: 0,
            });
        }
    }

    None
}

pub fn best_match_score<'a, I>(values: I, query: &str) -> Option<MatchScore>
where
    I: IntoIterator<Item = &'a str>,
{
    let query = prepare_query(query);
    values
        .into_iter()
        .map(prepare_text)
        .filter_map(|value| best_prepared_match_score(std::iter::once(&value), &query))
        .min()
}

fn normalize(value: &str) -> Vec<char> {
    value.chars().flat_map(char::to_lowercase).collect()
}

fn contiguous_start(value: &[char], query: &[char]) -> Option<usize> {
    value
        .windows(query.len())
        .position(|window| window == query)
}

fn subsequence_match(value: &[char], query: &[char]) -> Option<(usize, usize)> {
    let mut cursor = 0;
    let mut first = None;
    let mut last = 0;

    for character in query {
        let relative = value[cursor..]
            .iter()
            .position(|candidate| candidate == character)?;
        let position = cursor + relative;
        first.get_or_insert(position);
        last = position;
        cursor = position + 1;
    }

    let first = first?;
    Some((last + 1 - first - query.len(), first))
}

fn levenshtein_distance(value: &[char], query: &[char]) -> usize {
    let mut previous = (0..=query.len()).collect::<Vec<_>>();
    for (value_index, value_character) in value.iter().enumerate() {
        let mut current = vec![value_index + 1; query.len() + 1];
        for (query_index, query_character) in query.iter().enumerate() {
            current[query_index + 1] = if value_character == query_character {
                previous[query_index]
            } else {
                1 + previous[query_index]
                    .min(previous[query_index + 1])
                    .min(current[query_index])
            };
        }
        previous = current;
    }
    previous[query.len()]
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

    #[test]
    fn ranks_exact_prefix_substring_and_fuzzy_matches() {
        let exact = score("status", "status").expect("exact match");
        let prefix = score("status_code", "status").expect("prefix match");
        let substring = score("current_status", "status").expect("substring match");
        let subsequence = score("customer_state", "custa").expect("subsequence match");
        let typo = score("customer", "custmer").expect("small typo match");

        assert!(exact < prefix);
        assert!(prefix < substring);
        assert!(substring < subsequence);
        assert!(typo < subsequence);
    }

    #[test]
    fn best_match_score_checks_tokens_for_typo_tolerance() {
        let score = best_match_score(["dbo.customer_status"], "custmer");

        assert!(score.is_some());
    }

    #[test]
    fn rejects_large_typographical_distance() {
        assert!(score("customer", "zzzzzz").is_none());
    }
}
