//! "Did you mean" helpers for writing diagnostic hints.
//!
//! When a name is not recognised (a component, a field, an asset), the
//! diagnostic should say which valid name was probably meant. These helpers
//! rank candidates deterministically: by similarity, then alphabetically.

/// The Levenshtein edit distance between two strings, counted in `char`s.
pub fn edit_distance(a: &str, b: &str) -> usize {
    let b_chars: Vec<char> = b.chars().collect();
    let mut previous: Vec<usize> = (0..=b_chars.len()).collect();
    let mut current: Vec<usize> = vec![0; b_chars.len() + 1];
    for (i, a_char) in a.chars().enumerate() {
        current[0] = i + 1;
        for (j, b_char) in b_chars.iter().enumerate() {
            let substitution = previous[j] + usize::from(a_char != *b_char);
            let deletion = previous[j + 1] + 1;
            let insertion = current[j] + 1;
            current[j + 1] = substitution.min(deletion).min(insertion);
        }
        core::mem::swap(&mut previous, &mut current);
    }
    previous[b_chars.len()]
}

/// How far a candidate may be from the input and still be suggested.
fn max_distance(input_len: usize) -> usize {
    (input_len / 3).max(1)
}

/// How well `candidate` matches `needle` (both lowercase); lower is better.
/// `None` means the candidate is too different to suggest.
fn score(needle: &str, candidate: &str) -> Option<usize> {
    if needle.is_empty() || candidate.is_empty() {
        return None;
    }
    let distance = edit_distance(needle, candidate);
    let needle_len = needle.chars().count();
    // "pos" for "position", or "player_position" for "position".
    let contained = needle_len >= 3 && (candidate.contains(needle) || needle.contains(candidate));
    if distance <= max_distance(needle_len) {
        Some(distance)
    } else if contained {
        Some(max_distance(needle_len) + 1)
    } else {
        None
    }
}

/// Ranks `candidates` by similarity to `input`, closest first.
///
/// Comparison ignores ASCII case. Candidates that are too different are
/// dropped. Ties are broken alphabetically, so the result is deterministic.
pub fn rank_matches<'a, I>(input: &str, candidates: I) -> Vec<&'a str>
where
    I: IntoIterator<Item = &'a str>,
{
    let needle = input.to_ascii_lowercase();
    let mut scored: Vec<(usize, &str)> = candidates
        .into_iter()
        .filter_map(|candidate| Some((score(&needle, &candidate.to_ascii_lowercase())?, candidate)))
        .collect();
    scored.sort_unstable();
    scored.dedup();
    scored.into_iter().map(|(_, candidate)| candidate).collect()
}

/// The candidate most similar to `input`, if any is similar enough.
pub fn closest_match<'a, I>(input: &str, candidates: I) -> Option<&'a str>
where
    I: IntoIterator<Item = &'a str>,
{
    rank_matches(input, candidates).into_iter().next()
}

/// The most valid names a hint lists before summarising the rest.
const MAX_LISTED_NAMES: usize = 8;

/// Writes the hint for an unknown name.
///
/// Suggests the closest valid name when there is one; otherwise lists the
/// valid names (alphabetically, at most eight) so the reader never has to
/// guess what is allowed.
pub fn unknown_name_hint(input: &str, valid: &[&str]) -> String {
    if let Some(closest) = closest_match(input, valid.iter().copied()) {
        return format!("Did you mean '{closest}'?");
    }
    if valid.is_empty() {
        return "There are no valid names here.".to_owned();
    }
    let mut names: Vec<&str> = valid.to_vec();
    names.sort_unstable();
    names.dedup();
    let listed: Vec<String> = names
        .iter()
        .take(MAX_LISTED_NAMES)
        .map(|name| format!("'{name}'"))
        .collect();
    let remaining = names.len().saturating_sub(MAX_LISTED_NAMES);
    if remaining == 0 {
        format!("Valid names: {}.", listed.join(", "))
    } else {
        format!("Valid names: {} (and {remaining} more).", listed.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_distance_of_known_pairs() {
        assert_eq!(edit_distance("", ""), 0);
        assert_eq!(edit_distance("abc", ""), 3);
        assert_eq!(edit_distance("", "abc"), 3);
        assert_eq!(edit_distance("kitten", "sitting"), 3);
        assert_eq!(edit_distance("translation", "translation"), 0);
        assert_eq!(edit_distance("translaton", "translation"), 1);
        assert_eq!(edit_distance("çay", "cay"), 1);
    }

    #[test]
    fn typos_are_matched_ignoring_case() {
        let fields = ["translation", "rotation", "scale"];
        assert_eq!(closest_match("translaton", fields), Some("translation"));
        assert_eq!(closest_match("Rotation", fields), Some("rotation"));
        assert_eq!(closest_match("scal", fields), Some("scale"));
    }

    #[test]
    fn unrelated_names_are_not_suggested() {
        let fields = ["translation", "rotation", "scale"];
        assert_eq!(closest_match("color", fields), None);
        assert_eq!(closest_match("", fields), None);
        assert!(rank_matches("xyz", fields).is_empty());
    }

    #[test]
    fn substrings_are_suggested_after_close_typos() {
        let names = ["player_position", "positon", "velocity"];
        assert_eq!(
            rank_matches("position", names),
            ["positon", "player_position"]
        );
        assert_eq!(closest_match("pos", ["position"]), Some("position"));
    }

    #[test]
    fn ranking_is_deterministic_and_deduplicated() {
        let forward = rank_matches("speed", ["speeb", "speed", "speex", "speeb"]);
        let backward = rank_matches("speed", ["speeb", "speex", "speed", "speeb"]);
        assert_eq!(forward, ["speed", "speeb", "speex"]);
        assert_eq!(forward, backward);
    }

    #[test]
    fn hint_suggests_or_lists_valid_names() {
        assert_eq!(
            unknown_name_hint("translaton", &["translation", "rotation", "scale"]),
            "Did you mean 'translation'?"
        );
        assert_eq!(
            unknown_name_hint("color", &["translation", "rotation", "scale"]),
            "Valid names: 'rotation', 'scale', 'translation'."
        );
        assert_eq!(
            unknown_name_hint("color", &[]),
            "There are no valid names here."
        );
        let many: Vec<String> = (0..11).map(|i| format!("field_{i:02}")).collect();
        let many_refs: Vec<&str> = many.iter().map(String::as_str).collect();
        assert_eq!(
            unknown_name_hint("zzz", &many_refs),
            "Valid names: 'field_00', 'field_01', 'field_02', 'field_03', 'field_04', 'field_05', 'field_06', 'field_07' (and 3 more)."
        );
    }
}
