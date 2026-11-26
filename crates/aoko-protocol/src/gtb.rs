//! GTB (Guess The Build) word matching, ported from `Aoko/Core/GtbWordSolver.cs`.
//!
//! The managed solver is a static with a lazily loaded wordlist and a one-entry
//! answer cache. This port owns the same behaviour but exposes it as a value: the
//! caller constructs it once with the shipped wordlist, and every query is
//! allocation-free until it produces a result.
//!
//! The wordlist is pre-indexed by character length at construction, so a query
//! scans only candidates that can possibly fit the mask instead of the whole
//! list. `solve` memoises the last mask *and* the line it came from, which is what
//! makes the steady-state per-frame call allocation-free: the bridge repeats the
//! same action-bar line on every frame, so an unchanged line is answered from the
//! memo before the normalisation pipeline runs. A memo on the mask alone is not
//! enough, because extracting the mask is what allocates.

use std::collections::HashSet;

/// Shortest word the solver will keep.
const MINIMUM_WORD_LENGTH: usize = 3;

/// One parsed GTB wordlist plus its length index.
#[derive(Debug, Default)]
pub struct GtbWordSolver {
    words: Vec<String>,
    /// Indices into `words`, bucketed by character length.
    by_length: Vec<Vec<u32>>,
    known: HashSet<String>,
    /// The action-bar line the memo below was built from.
    last_action_bar: String,
    last_mask: String,
    last_matches: Vec<String>,
}

impl GtbWordSolver {
    /// Builds a solver over an already-normalised word list.
    #[must_use]
    pub fn from_words<I, S>(words: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut solver = Self::default();
        for word in words {
            solver.insert_word(&word.as_ref().to_lowercase());
        }
        solver
    }

    /// Parses the shipped `gtb_wordlist.js` and builds the index.
    ///
    /// The asset lives in `Aoko/Data/`; `aoko-app` ships it and hands the text
    /// here, so this crate keeps no file-system dependency.
    #[must_use]
    pub fn from_wordlist_js(text: &str) -> Self {
        Self::from_words(parse_wordlist_js(text))
    }

    /// Adds the extra words a user learned in an earlier session.
    pub fn extend_with_custom_words(&mut self, text: &str) {
        for word in load_custom_words(text) {
            self.insert_word(&word);
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.words.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    /// Solves a GTB action-bar line.
    ///
    /// Returns the extracted mask and up to `max_results` matching words in
    /// wordlist order. An unchanged line returns the memoised answer without
    /// running the mask pipeline, and a repeated mask returns it after one pass.
    pub fn solve(&mut self, action_bar_text: &str, max_results: usize) -> (&str, &[String]) {
        // The steady state: the bridge re-sends the same line, so this comparison is
        // the whole cost of a frame that changes nothing. It reads the memo only
        // while it is valid, which is why `try_learn_solved_word` clears it.
        if !self.last_mask.is_empty() && action_bar_text == self.last_action_bar {
            return (&self.last_mask, &self.last_matches);
        }

        let mask = extract_mask(action_bar_text);
        if mask.trim().is_empty() {
            return ("", &[]);
        }

        if mask == self.last_mask {
            self.remember_action_bar(action_bar_text);
            return (&self.last_mask, &self.last_matches);
        }

        let limit = max_results.max(1);
        let mut matches: Vec<String> = Vec::new();

        if let Some(candidates) = self.by_length.get(mask.len()) {
            for index in candidates {
                let word = match self.words.get(*index as usize) {
                    Some(word) => word,
                    None => continue,
                };
                if word_matches_mask(word, &mask) {
                    matches.push(word.clone());
                    if matches.len() == limit {
                        break;
                    }
                }
            }
        }

        self.remember_action_bar(action_bar_text);
        self.last_mask = mask;
        self.last_matches = matches;
        (&self.last_mask, &self.last_matches)
    }

    /// Records the line the current memo answers, reusing the buffer so a line that
    /// is no longer than the longest seen costs no allocation.
    fn remember_action_bar(&mut self, action_bar_text: &str) {
        self.last_action_bar.clear();
        self.last_action_bar.push_str(action_bar_text);
    }

    /// Learns the word that just completed a round, so the next mask that can
    /// only mean that word resolves immediately.
    ///
    /// Returns `false` when the line carries no solved word, when the word does
    /// not fit the last mask, or when it is already known.
    pub fn try_learn_solved_word(&mut self, action_bar_text: &str) -> bool {
        let solved_word = extract_solved_word(action_bar_text);
        if solved_word.trim().is_empty() {
            return false;
        }

        if self.last_mask.is_empty()
            || solved_word.len() != self.last_mask.len()
            || !word_matches_mask(&solved_word, &self.last_mask)
        {
            return false;
        }

        if self.known.contains(&solved_word) {
            return false;
        }

        self.insert_word(&solved_word);
        // The answered mask is now wrong: a line that repeated it must be solved
        // again, so both halves of the memo go.
        self.last_action_bar.clear();
        self.last_mask.clear();
        self.last_matches.clear();
        true
    }

    fn insert_word(&mut self, word: &str) {
        if word.is_empty() || !self.known.insert(word.to_owned()) {
            return;
        }

        let index = self.words.len() as u32;
        self.words.push(word.to_owned());

        let length = word.chars().count();
        if self.by_length.len() <= length {
            self.by_length.resize_with(length + 1, Vec::new);
        }
        self.by_length[length].push(index);
    }
}

/// Extracts the quoted words from a `gtb_wordlist.js` body.
///
/// Mirrors the managed loader: prefer `wordsData`, fall back to `arrayData`, then
/// to the first `[` .. last `]`. Every quoted literal inside that slice is
/// normalised, and words shorter than three characters are dropped.
#[must_use]
pub fn parse_wordlist_js(text: &str) -> Vec<String> {
    let lowered = text.to_lowercase();

    let mut start = None;
    let mut end = None;

    if let Some(named) = lowered.find("wordsdata") {
        if let Some(open) = text[named..].find('[') {
            let open = named + open;
            if let Some(close) = text[open..].find("];") {
                start = Some(open);
                end = Some(open + close);
            }
        }
    }

    if start.is_none() {
        if let Some(fallback) = lowered.find("arraydata") {
            if let Some(open) = text[fallback..].find('[') {
                let open = fallback + open;
                if let Some(close) = text[open..].find("];") {
                    start = Some(open);
                    end = Some(open + close);
                }
            }
        }
    }

    let body = match (start, end) {
        (Some(start), Some(end)) if end > start => &text[start..end],
        _ => {
            // Last resort: the widest bracket pair in the document.
            let Some(open) = text.find('[') else {
                return Vec::new();
            };
            let Some(close) = text.rfind(']') else {
                return Vec::new();
            };
            if close <= open {
                return Vec::new();
            }
            &text[open..close]
        }
    };

    let mut words = Vec::new();
    let mut seen = HashSet::new();
    for raw in quoted_strings(body) {
        let word = normalize_word(&raw.replace("\\\"", "\""));
        if word.is_empty() {
            continue;
        }
        if seen.insert(word.clone()) {
            words.push(word);
        }
    }

    words
}

/// Parses a custom wordlist: one word per line, blank lines ignored.
#[must_use]
pub fn load_custom_words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut seen = HashSet::new();
    for line in text.lines() {
        let word = normalize_word(line);
        if word.is_empty() {
            continue;
        }
        if seen.insert(word.clone()) {
            words.push(word);
        }
    }
    words
}

/// Every double-quoted literal in `body`, honouring backslash escapes.
fn quoted_strings(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = body.char_indices().peekable();

    while let Some((index, character)) = chars.next() {
        if character != '"' {
            continue;
        }

        let content_start = index + character.len_utf8();
        let mut content_end = content_start;
        let mut escaped = false;
        for (offset, inner) in body[content_start..].char_indices() {
            if escaped {
                escaped = false;
                continue;
            }
            match inner {
                '\\' => escaped = true,
                '"' => {
                    content_end = content_start + offset;
                    break;
                }
                _ => {}
            }
        }

        if content_end >= content_start {
            out.push(body[content_start..content_end].to_owned());
        }
        // Skip past the closing quote so the caller's iterator stays in sync.
        while let Some((position, _)) = chars.peek() {
            if *position < content_end + 1 {
                chars.next();
            } else {
                break;
            }
        }
    }

    out
}

/// Extracts the `c_t` shape from an action-bar line.
#[must_use]
pub fn extract_mask(action_bar_text: &str) -> String {
    for line in split_candidate_lines(action_bar_text) {
        if let Some(mask) = extract_mask_from_line(&line) {
            return mask;
        }
    }
    String::new()
}

/// Extracts the word that completed a round, from an action-bar line.
#[must_use]
pub fn extract_solved_word(action_bar_text: &str) -> String {
    for line in split_candidate_lines(action_bar_text) {
        let solved = extract_solved_word_from_line(&line);
        if !solved.trim().is_empty() {
            return solved;
        }
    }
    String::new()
}

/// Strips colour codes, splits on line breaks, trims, and drops empty lines.
fn split_candidate_lines(text: &str) -> Vec<String> {
    let cleaned = remove_section_codes(text).trim().to_owned();
    if cleaned.trim().is_empty() {
        return Vec::new();
    }

    let mut lines: Vec<String> = Vec::new();
    for line in cleaned.split(['\r', '\n']) {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            lines.push(trimmed.to_owned());
        }
    }

    if lines.is_empty() {
        lines.push(cleaned);
    }

    lines
}

/// Removes `§x` colour-code pairs, one character each.
fn remove_section_codes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut skip_next = false;
    for character in text.chars() {
        if skip_next {
            skip_next = false;
            continue;
        }
        if character == '\u{00A7}' {
            skip_next = true;
            continue;
        }
        out.push(character);
    }
    out
}

fn extract_mask_from_line(input: &str) -> Option<String> {
    let mut s = input.trim().to_owned();
    if !s.contains('_') {
        return None;
    }

    s = replace_brackets(&s);

    s = match theme_is_capture(&s, true) {
        Some(theme) => theme,
        None => match s.rfind(':') {
            Some(colon) if colon + 1 < s.len() => s[colon + 1..].to_owned(),
            _ => s,
        },
    };

    s = strip_trailing_counter(&s);
    s = replace_number_ratios(&s);
    s = keep_only(&s, |c| c.is_ascii_alphabetic() || c == '_' || c == ' ');
    s = collapse_spaces(&s);
    s = decode_spaced_mask_if_needed(&s);
    s = collapse_spaces(&s).to_lowercase();

    if !s.contains('_') {
        return None;
    }

    first_mask_token(&s)
}

fn extract_solved_word_from_line(input: &str) -> String {
    let mut s = input.trim().to_owned();
    if s.contains('_') {
        return String::new();
    }

    s = replace_brackets(&s);

    s = match theme_is_capture(&s, false) {
        Some(theme) => theme,
        None => match s.rfind(':') {
            Some(colon) if colon + 1 < s.len() => s[colon + 1..].to_owned(),
            _ => s,
        },
    };

    s = strip_trailing_counter(&s);
    s = replace_number_ratios(&s);
    s = keep_only(&s, |c| c.is_ascii_alphabetic() || c == ' ');
    s = collapse_spaces(&s);

    normalize_word(&s)
}

/// `[A-Za-z ]` only. `with_underscore` widens the class for the mask path.
fn theme_is_capture(input: &str, with_underscore: bool) -> Option<String> {
    let lowered = input.to_lowercase();
    let bytes = lowered.as_bytes();
    let needle = b"theme";

    let mut search_from = 0;
    while let Some(found) = lowered[search_from..].find("theme") {
        let at = search_from + found;
        let boundary_before = at == 0 || !is_word_byte(bytes[at - 1]);
        let after_theme = at + needle.len();
        if !boundary_before || after_theme > bytes.len() {
            search_from = at + needle.len();
            continue;
        }

        let mut cursor = after_theme;
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if !lowered[cursor..].starts_with("is") {
            search_from = at + needle.len();
            continue;
        }
        cursor += 2;
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }

        let capture: String = input[cursor..]
            .chars()
            .take_while(|c: &char| {
                c.is_ascii_alphabetic() || *c == ' ' || (with_underscore && *c == '_')
            })
            .collect();

        return Some(capture);
    }

    None
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Replaces each `[...]` run with a single space.
fn replace_brackets(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut inside = false;

    for character in input.chars() {
        match character {
            '[' => inside = true,
            ']' => {
                if inside {
                    out.push(' ');
                    inside = false;
                } else {
                    out.push(character);
                }
            }
            _ if inside => {}
            _ => out.push(character),
        }
    }

    out
}

/// Removes a trailing `(123)` counter, with the whitespace around it.
fn strip_trailing_counter(input: &str) -> String {
    let trimmed = input.trim_end();
    let Some(close) = trimmed.strip_suffix(')') else {
        return input.to_owned();
    };
    let Some(open) = close.rfind('(') else {
        return input.to_owned();
    };
    let digits = &close[open + 1..];
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return input.to_owned();
    }

    trimmed[..open].trim_end().to_owned()
}

/// Replaces each `\b\d+/\d+\b` run with a single space.
fn replace_number_ratios(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::with_capacity(input.len());
    let mut index = 0;

    while index < chars.len() {
        let starts_run =
            (index == 0 || !is_word_char(chars[index - 1])) && chars[index].is_ascii_digit();
        if !starts_run {
            out.push(chars[index]);
            index += 1;
            continue;
        }

        let mut cursor = index;
        while cursor < chars.len() && chars[cursor].is_ascii_digit() {
            cursor += 1;
        }
        let numerator_end = cursor;

        if cursor < chars.len() && chars[cursor] == '/' {
            cursor += 1;
            let denominator_start = cursor;
            while cursor < chars.len() && chars[cursor].is_ascii_digit() {
                cursor += 1;
            }
            let has_denominator = cursor > denominator_start;
            let boundary_after = cursor >= chars.len() || !is_word_char(chars[cursor]);
            if has_denominator && boundary_after {
                out.push(' ');
                index = cursor;
                continue;
            }
        }

        out.extend(&chars[index..numerator_end]);
        index = numerator_end;
    }

    out
}

fn is_word_char(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

fn keep_only(input: &str, keep: impl Fn(char) -> bool) -> String {
    let mut out = String::with_capacity(input.len());
    for character in input.chars() {
        if keep(character) {
            out.push(character);
        } else {
            out.push(' ');
        }
    }
    out
}

/// Collapses runs of whitespace to one space and trims.
fn collapse_spaces(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut pending_space = false;

    for character in input.chars() {
        if character.is_whitespace() {
            pending_space = true;
            continue;
        }
        if pending_space && !out.is_empty() {
            out.push(' ');
        }
        pending_space = false;
        out.push(character);
    }

    out
}

/// A tokenised mask such as `c _ t` (single characters separated by spaces)
/// becomes `c_t`; two or more spaces are treated as a word break.
fn decode_spaced_mask_if_needed(input: &str) -> String {
    if input.trim().is_empty() {
        return input.to_owned();
    }

    let tokens: Vec<&str> = input.split_whitespace().collect();
    let looks_tokenised = tokens.len() >= 4
        && tokens.iter().all(|token| {
            let mut chars = token.chars();
            let (Some(first), None) = (chars.next(), chars.next()) else {
                return false;
            };
            first == '_' || first.is_alphabetic()
        });
    if !looks_tokenised {
        return input.to_owned();
    }

    let chars: Vec<char> = input.chars().collect();
    let mut out: Vec<char> = Vec::with_capacity(input.len());
    let mut index = 0;

    while index < chars.len() {
        let character = chars[index];
        if character == '_' || character.is_alphabetic() {
            out.push(character.to_lowercase().next().unwrap_or(character));

            let mut cursor = index + 1;
            let mut spaces = 0;
            while cursor < chars.len() && chars[cursor] == ' ' {
                spaces += 1;
                cursor += 1;
            }

            if spaces >= 2 && cursor < chars.len() && out.last() != Some(&' ') {
                out.push(' ');
            }
            index = cursor;
            continue;
        }

        index += 1;
    }

    out.into_iter().collect()
}

/// The first `[a-z_]*_[a-z_]*(?: [a-z_]*_[a-z_]*)*` run in a filtered string.
///
/// Because the input is already restricted to `[a-z_ ]` and collapsed, the
/// leftmost match always starts at the first space-separated segment that
/// contains an underscore and extends while each following segment does too.
fn first_mask_token(input: &str) -> Option<String> {
    let segments: Vec<&str> = input.split(' ').filter(|s| !s.is_empty()).collect();
    let start = segments.iter().position(|segment| segment.contains('_'))?;

    let mut end = start;
    while end + 1 < segments.len() && segments[end + 1].contains('_') {
        end += 1;
    }

    let mask = segments[start..=end].join(" ");
    if mask.contains('_') {
        Some(mask)
    } else {
        None
    }
}

/// `[A-Za-z ]` only, collapsed, lowercased; empty when shorter than three.
#[must_use]
pub fn normalize_word(value: &str) -> String {
    if value.trim().is_empty() {
        return String::new();
    }

    let filtered = keep_only(value, |c| c.is_ascii_alphabetic() || c == ' ');
    let normalized = collapse_spaces(&filtered).to_lowercase();
    if normalized.chars().count() >= MINIMUM_WORD_LENGTH {
        normalized
    } else {
        String::new()
    }
}

/// Mask comparison: `_` matches anything, spaces must agree, everything else is
/// byte equality.
#[must_use]
pub fn word_matches_mask(word: &str, mask: &str) -> bool {
    if mask.contains(' ') != word.contains(' ') {
        return false;
    }

    let word_bytes = word.as_bytes();
    let mask_bytes = mask.as_bytes();
    if word_bytes.len() != mask_bytes.len() {
        return false;
    }

    for (word_byte, mask_byte) in word_bytes.iter().zip(mask_bytes.iter()) {
        match mask_byte {
            b'_' => {}
            b' ' => {
                if *word_byte != b' ' {
                    return false;
                }
            }
            other => {
                if word_byte != other {
                    return false;
                }
            }
        }
    }

    true
}
