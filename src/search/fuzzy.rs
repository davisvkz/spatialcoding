//! Subsequence matching with a score, for the command palette and the file
//! search. Every query character must appear in the candidate, in order;
//! consecutive runs, word starts and matches in the file name score higher.

/// A successful match: higher `score` ranks first; `indices` are the matched
/// character positions (not bytes) of the candidate, for highlighting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FuzzyMatch {
    pub score: i32,
    pub indices: Vec<usize>,
}

const CONSECUTIVE_BONUS: i32 = 8;
const WORD_START_BONUS: i32 = 10;
const FILE_NAME_BONUS: i32 = 6;
const GAP_PENALTY: i32 = 1;
const BASE_MATCH: i32 = 4;

fn is_separator(c: char) -> bool {
    matches!(c, '/' | '\\' | '_' | '-' | '.' | ' ' | ':')
}

/// Matches `query` against `candidate`, ignoring case. An empty query matches
/// everything with score 0. Returns `None` when a query character is missing.
pub fn fuzzy_match(query: &str, candidate: &str) -> Option<FuzzyMatch> {
    let query: Vec<char> = query.chars().filter(|c| !c.is_whitespace()).flat_map(|c| c.to_lowercase()).collect();
    if query.is_empty() {
        return Some(FuzzyMatch { score: 0, indices: Vec::new() });
    }
    let chars: Vec<char> = candidate.chars().collect();
    let lower: Vec<char> = chars.iter().map(|c| c.to_lowercase().next().unwrap_or(*c)).collect();
    let name_start = chars.iter().rposition(|c| matches!(c, '/' | '\\')).map_or(0, |i| i + 1);

    // Greedy forward scan to see if a match exists at all.
    let mut at = 0;
    for &q in &query {
        at += lower[at..].iter().position(|&c| c == q)? + 1;
    }

    // Best alignment by dynamic programming: best[i][j] = best score matching
    // query[..=i] with query[i] placed at candidate position j.
    let n = chars.len();
    let m = query.len();
    let neg = i32::MIN / 2;
    let mut best = vec![vec![neg; n]; m];
    let mut from = vec![vec![usize::MAX; n]; m];
    for j in 0..n {
        if lower[j] != query[0] {
            continue;
        }
        let mut score = BASE_MATCH - GAP_PENALTY * j.min(20) as i32;
        score += position_bonus(&chars, j, name_start);
        best[0][j] = score;
    }
    for i in 1..m {
        for j in i..n {
            if lower[j] != query[i] {
                continue;
            }
            let mut top = neg;
            let mut top_k = usize::MAX;
            for (k, &previous) in best[i - 1].iter().enumerate().take(j).skip(i - 1) {
                if previous == neg {
                    continue;
                }
                let mut s = previous;
                if k + 1 == j {
                    s += CONSECUTIVE_BONUS;
                } else {
                    s -= GAP_PENALTY * (j - k - 1).min(10) as i32;
                }
                if s > top {
                    top = s;
                    top_k = k;
                }
            }
            if top != neg {
                best[i][j] = top + BASE_MATCH + position_bonus(&chars, j, name_start);
                from[i][j] = top_k;
            }
        }
    }
    let (mut end, mut score) = (usize::MAX, neg);
    for (j, &candidate) in best[m - 1].iter().enumerate() {
        if candidate > score {
            score = candidate;
            end = j;
        }
    }
    if end == usize::MAX {
        return None;
    }
    let mut indices = vec![0; m];
    let mut j = end;
    for i in (0..m).rev() {
        indices[i] = j;
        if i > 0 {
            j = from[i][j];
        }
    }
    Some(FuzzyMatch { score, indices })
}

fn position_bonus(chars: &[char], j: usize, name_start: usize) -> i32 {
    let mut bonus = 0;
    if j == 0 || is_separator(chars[j - 1]) || (chars[j].is_uppercase() && chars[j - 1].is_lowercase()) {
        bonus += WORD_START_BONUS;
    }
    if j >= name_start {
        bonus += FILE_NAME_BONUS;
    }
    bonus
}

/// Candidates that match `query`, best first; ties keep their input order.
pub fn rank<T>(query: &str, items: impl IntoIterator<Item = T>, text: impl Fn(&T) -> String) -> Vec<(T, FuzzyMatch)> {
    let mut hits: Vec<(T, FuzzyMatch)> = items
        .into_iter()
        .filter_map(|item| fuzzy_match(query, &text(&item)).map(|m| (item, m)))
        .collect();
    hits.sort_by_key(|hit| std::cmp::Reverse(hit.1.score));
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_query_matches_everything() {
        assert_eq!(fuzzy_match("", "anything").unwrap().score, 0);
    }

    #[test]
    fn every_query_char_must_appear_in_order() {
        assert!(fuzzy_match("abc", "a_b_c").is_some());
        assert!(fuzzy_match("abc", "acb").is_none());
        assert!(fuzzy_match("xyz", "abc").is_none());
    }

    #[test]
    fn matching_ignores_case_and_reports_indices() {
        let m = fuzzy_match("FB", "foo_bar").unwrap();
        assert_eq!(m.indices, vec![0, 4]);
    }

    #[test]
    fn consecutive_and_word_start_matches_rank_higher() {
        let tight = fuzzy_match("util", "src/util.rs").unwrap();
        let loose = fuzzy_match("util", "uxtxixl.rs").unwrap();
        assert!(tight.score > loose.score);
    }

    #[test]
    fn file_name_matches_beat_directory_matches() {
        let hits = rank("main", ["main/other.rs", "src/main.rs"], |s| s.to_string());
        assert_eq!(hits[0].0, "src/main.rs");
    }

    #[test]
    fn rank_drops_non_matches_and_orders_by_score() {
        let hits = rank("fit", ["Fit view", "Open folder", "Fill it"], |s| s.to_string());
        let names: Vec<_> = hits.iter().map(|h| h.0).collect();
        assert_eq!(names, vec!["Fit view", "Fill it"]);
    }
}
