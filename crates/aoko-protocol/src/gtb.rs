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
//!
//! The pipeline itself allocates nothing either: each of its stages used to return a
//! fresh `String`, and now writes into a buffer the solver keeps. That is what makes a
//! line the solver has never seen cost only its output, and the buffers are sized at
//! construction because a scratch that grows on first use is the allocation it exists
//! to remove.

use std::collections::HashSet;

/// Shortest word the solver will keep.
const MINIMUM_WORD_LENGTH: usize = 3;

/// One parsed GTB wordlist plus its length index.
#[derive(Debug)]
pub struct GtbWordSolver {
    words: Vec<String>,
    /// Indices into `words`, bucketed by character length.
    by_length: Vec<Vec<u32>>,
    known: HashSet<String>,
    /// The action-bar line the memo below was built from: with `last_mask` it is the
    /// memo key.
    last_action_bar: String,
    last_mask: String,
    last_matches: Vec<String>,
    /// Normalisation buffers, reused by every line the memo cannot answer.
    scratch: MaskScratch,
}

impl Default for GtbWordSolver {
    /// Sizes the mask scratch and the memo's own strings.
    ///
    /// Construction is the one moment where allocating is free, so it is where every
    /// buffer a query could need is paid for once. This is why a cold line costs only
    /// its output: a scratch that grew on first use would hand the pipeline back the
    /// allocations it was built to remove.
    fn default() -> Self {
        Self {
            words: Vec::new(),
            by_length: Vec::new(),
            known: HashSet::new(),
            last_action_bar: String::with_capacity(SCRATCH_CAPACITY),
            last_mask: String::with_capacity(SCRATCH_CAPACITY),
            last_matches: Vec::new(),
            scratch: MaskScratch::default(),
        }
    }
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
        // the whole cost of a frame that changes nothing. The memo key is the pair
        // (line, mask it normalises to); keying it on the mask alone is what this
        // guard replaced, and it cannot come back, because reaching the mask means
        // running the whole normalisation pipeline, which is the work a repeated line
        // must not repeat. It reads the memo only while it is valid, which is why
        // `try_learn_solved_word` clears it.
        if !self.last_mask.is_empty() && action_bar_text == self.last_action_bar {
            return (&self.last_mask, &self.last_matches);
        }

        // The pipeline writes into the scratch and leaves the mask there, so a line
        // that is not the memo's costs the normalisation work and nothing else. The
        // mask is copied into the memo rather than written there directly: an early
        // return has to leave `last_mask` answering the line it was built from.
        if !extract_mask_into(&mut self.scratch, action_bar_text) {
            return ("", &[]);
        }

        if self.last_mask == self.scratch.mask {
            self.remember_action_bar(action_bar_text);
            return (&self.last_mask, &self.last_matches);
        }

        let limit = max_results.max(1);
        let mut matches: Vec<String> = Vec::new();

        if let Some(candidates) = self.by_length.get(self.scratch.mask.len()) {
            for index in candidates {
                let word = match self.words.get(*index as usize) {
                    Some(word) => word,
                    None => continue,
                };
                if word_matches_mask(word, &self.scratch.mask) {
                    matches.push(word.clone());
                    if matches.len() == limit {
                        break;
                    }
                }
            }
        }

        self.remember_action_bar(action_bar_text);
        self.last_mask.clear();
        self.last_mask.push_str(&self.scratch.mask);
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
///
/// This builds the buffers for one call. A caller that extracts masks in a loop should
/// hold a [`GtbWordSolver`] instead, whose scratch is paid for once at construction.
#[must_use]
pub fn extract_mask(action_bar_text: &str) -> String {
    let mut scratch = MaskScratch::default();
    if extract_mask_into(&mut scratch, action_bar_text) {
        scratch.mask.clone()
    } else {
        String::new()
    }
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
    remove_section_codes_into(text, &mut out);
    out
}

/// [`remove_section_codes`], into a buffer the caller keeps.
fn remove_section_codes_into(text: &str, out: &mut String) {
    out.clear();
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
}

/// Room for one action-bar line, and for one character per byte of it.
///
/// A colour-coded action bar with a counter and a couple of lines fits comfortably. A
/// longer line grows whichever buffer it overflows exactly once and pays that growth,
/// which is the one allocation the steady state cannot absorb.
const SCRATCH_CAPACITY: usize = 256;

/// The normalisation buffers, kept across lines.
///
/// Every stage of the mask pipeline writes into one of these instead of returning a
/// fresh `String`. Sizing them is the whole point: `Default` pre-allocates, so the
/// first line the solver sees costs the same as the thousandth.
#[derive(Debug)]
struct MaskScratch {
    /// The action-bar text with `§x` colour codes removed. The candidate lines are
    /// slices of it.
    cleaned: String,
    /// Ping-pong buffers: a stage reads one and writes the other, so no stage ever
    /// overwrites the text it is reading.
    current: String,
    next: String,
    /// Lowercased copy of the current stage, for the `theme ... is` guard.
    lowered: String,
    /// Indexed access to the current stage, for the two stages that need it.
    chars: Vec<char>,
    /// What the spaced-mask decoder builds before it becomes text again.
    decoded: Vec<char>,
    /// The extracted mask, valid while the call that filled it returned `true`.
    mask: String,
}

impl Default for MaskScratch {
    fn default() -> Self {
        Self {
            cleaned: String::with_capacity(SCRATCH_CAPACITY),
            current: String::with_capacity(SCRATCH_CAPACITY),
            next: String::with_capacity(SCRATCH_CAPACITY),
            lowered: String::with_capacity(SCRATCH_CAPACITY),
            chars: Vec::with_capacity(SCRATCH_CAPACITY),
            decoded: Vec::with_capacity(SCRATCH_CAPACITY),
            mask: String::with_capacity(SCRATCH_CAPACITY),
        }
    }
}

/// The same buffers, split apart so a stage can read one and write another.
struct MaskBuffers<'a> {
    current: &'a mut String,
    next: &'a mut String,
    lowered: &'a mut String,
    chars: &'a mut Vec<char>,
    decoded: &'a mut Vec<char>,
    mask: &'a mut String,
}

/// The mask pipeline, writing every intermediate into `scratch`.
///
/// Returns `true` with the mask left in `scratch.mask`, or `false` when the text
/// carries no `c_t` shape. This is what [`extract_mask`] wraps for callers that want an
/// owned string, and what [`GtbWordSolver::solve`] calls directly.
fn extract_mask_into(scratch: &mut MaskScratch, text: &str) -> bool {
    let MaskScratch {
        cleaned,
        current,
        next,
        lowered,
        chars,
        decoded,
        mask,
    } = scratch;

    cleaned.clear();
    remove_section_codes_into(text, cleaned);
    mask.clear();

    let source: &str = cleaned.as_str();
    if source.trim().is_empty() {
        return false;
    }

    let mut buffers = MaskBuffers {
        current,
        next,
        lowered,
        chars,
        decoded,
        mask,
    };

    let mut saw_line = false;
    for line in source.split(['\r', '\n']) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        saw_line = true;
        if extract_mask_from_line_into(line, &mut buffers) {
            return true;
        }
    }

    // `split_candidate_lines` falls back to the whole cleaned text when no line
    // survived the trim. The empty check above already rules that out, but the
    // fallback stays so this and the owned splitter cannot drift apart.
    !saw_line && extract_mask_from_line_into(source.trim(), &mut buffers)
}

/// One candidate line through the mask pipeline, into `buffers`.
fn extract_mask_from_line_into(input: &str, buffers: &mut MaskBuffers<'_>) -> bool {
    if !input.contains('_') {
        return false;
    }

    replace_brackets_into(input, &mut *buffers.current);

    // The `theme ... is` guard, or else everything after the last colon.
    if !theme_is_capture_into(
        buffers.current.as_str(),
        true,
        &mut *buffers.lowered,
        &mut *buffers.next,
    ) {
        let current = buffers.current.as_str();
        let tail = current.rfind(':').filter(|colon| colon + 1 < current.len());
        buffers.next.clear();
        match tail {
            Some(colon) => buffers.next.push_str(&current[colon + 1..]),
            None => buffers.next.push_str(current),
        }
    }

    strip_trailing_counter_into(buffers.next.as_str(), &mut *buffers.current);
    replace_number_ratios_into(
        buffers.current.as_str(),
        &mut *buffers.next,
        &mut *buffers.chars,
    );
    keep_only_into(buffers.next.as_str(), &mut *buffers.current, |character| {
        character.is_ascii_alphabetic() || character == '_' || character == ' '
    });
    collapse_spaces_into(buffers.current.as_str(), &mut *buffers.next);
    decode_spaced_mask_into(
        buffers.next.as_str(),
        &mut *buffers.current,
        &mut *buffers.chars,
        &mut *buffers.decoded,
    );
    collapse_spaces_into(buffers.current.as_str(), &mut *buffers.next);
    lowercase_into(buffers.next.as_str(), &mut *buffers.current);

    if !buffers.current.contains('_') {
        return false;
    }

    first_mask_token_into(buffers.current.as_str(), &mut *buffers.mask)
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
    let mut lowered = String::with_capacity(input.len());
    let mut capture = String::with_capacity(input.len());
    if theme_is_capture_into(input, with_underscore, &mut lowered, &mut capture) {
        Some(capture)
    } else {
        None
    }
}

/// [`theme_is_capture`], with both buffers handed in.
///
/// Returns whether the guard matched, leaving the captured suffix in `capture` when it
/// did and leaving `capture` alone when it did not.
fn theme_is_capture_into(
    input: &str,
    with_underscore: bool,
    lowered: &mut String,
    capture: &mut String,
) -> bool {
    lowercase_into(input, lowered);
    let bytes = lowered.as_bytes();
    let needle = b"theme";

    let mut search_from = 0;
    while let Some(found) = lowered.as_str()[search_from..].find("theme") {
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
        if !lowered.as_str()[cursor..].starts_with("is") {
            search_from = at + needle.len();
            continue;
        }
        cursor += 2;
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }

        capture.clear();
        for character in input[cursor..].chars() {
            if character.is_ascii_alphabetic()
                || character == ' '
                || (with_underscore && character == '_')
            {
                capture.push(character);
            } else {
                break;
            }
        }

        return true;
    }

    false
}

/// `str::to_lowercase` without the allocation.
///
/// The standard library's only departure from per-character mapping is the final-sigma
/// rule, and a sigma is two bytes in either form and never ASCII, so no byte offset
/// `theme_is_capture_into` compares can move.
fn lowercase_into(input: &str, out: &mut String) {
    out.clear();
    for character in input.chars() {
        for mapped in character.to_lowercase() {
            out.push(mapped);
        }
    }
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Replaces each `[...]` run with a single space.
fn replace_brackets(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    replace_brackets_into(input, &mut out);
    out
}

/// [`replace_brackets`], into a buffer the caller keeps.
fn replace_brackets_into(input: &str, out: &mut String) {
    out.clear();
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
}

/// Removes a trailing `(123)` counter, with the whitespace around it.
fn strip_trailing_counter(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    strip_trailing_counter_into(input, &mut out);
    out
}

/// [`strip_trailing_counter`], into a buffer the caller keeps.
fn strip_trailing_counter_into(input: &str, out: &mut String) {
    out.clear();
    let trimmed = input.trim_end();
    if let Some(close) = trimmed.strip_suffix(')') {
        if let Some(open) = close.rfind('(') {
            let digits = &close[open + 1..];
            if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
                out.push_str(trimmed[..open].trim_end());
                return;
            }
        }
    }
    out.push_str(input);
}

/// Replaces each `\b\d+/\d+\b` run with a single space.
fn replace_number_ratios(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = Vec::with_capacity(input.len());
    replace_number_ratios_into(input, &mut out, &mut chars);
    out
}

/// [`replace_number_ratios`], into buffers the caller keeps.
fn replace_number_ratios_into(input: &str, out: &mut String, chars: &mut Vec<char>) {
    chars.clear();
    chars.extend(input.chars());
    out.clear();

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
}

fn is_word_char(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

fn keep_only(input: &str, keep: impl Fn(char) -> bool) -> String {
    let mut out = String::with_capacity(input.len());
    keep_only_into(input, &mut out, keep);
    out
}

/// [`keep_only`], into a buffer the caller keeps.
fn keep_only_into(input: &str, out: &mut String, keep: impl Fn(char) -> bool) {
    out.clear();
    for character in input.chars() {
        if keep(character) {
            out.push(character);
        } else {
            out.push(' ');
        }
    }
}

/// Collapses runs of whitespace to one space and trims.
fn collapse_spaces(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    collapse_spaces_into(input, &mut out);
    out
}

/// [`collapse_spaces`], into a buffer the caller keeps.
fn collapse_spaces_into(input: &str, out: &mut String) {
    out.clear();
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
}

/// A tokenised mask such as `c _ t` (single characters separated by spaces)
/// becomes `c_t`; two or more spaces are treated as a word break.
fn decode_spaced_mask_into(
    input: &str,
    out: &mut String,
    chars: &mut Vec<char>,
    decoded: &mut Vec<char>,
) {
    out.clear();
    if input.trim().is_empty() {
        out.push_str(input);
        return;
    }

    if input.split_whitespace().count() < 4
        || !input.split_whitespace().all(is_lone_token_character)
    {
        out.push_str(input);
        return;
    }

    chars.clear();
    chars.extend(input.chars());
    decoded.clear();

    let mut index = 0;

    while index < chars.len() {
        let character = chars[index];
        if character == '_' || character.is_alphabetic() {
            decoded.push(character.to_lowercase().next().unwrap_or(character));

            let mut cursor = index + 1;
            let mut spaces = 0;
            while cursor < chars.len() && chars[cursor] == ' ' {
                spaces += 1;
                cursor += 1;
            }

            if spaces >= 2 && cursor < chars.len() && decoded.last() != Some(&' ') {
                decoded.push(' ');
            }
            index = cursor;
            continue;
        }

        index += 1;
    }

    for character in decoded.iter() {
        out.push(*character);
    }
}

/// One token of a tokenised mask: a single `_` or letter.
fn is_lone_token_character(token: &str) -> bool {
    let mut chars = token.chars();
    let (Some(first), None) = (chars.next(), chars.next()) else {
        return false;
    };
    first == '_' || first.is_alphabetic()
}

/// The first `[a-z_]*_[a-z_]*(?: [a-z_]*_[a-z_]*)*` run in a filtered string.
///
/// Because the input is already restricted to `[a-z_ ]` and collapsed, the
/// leftmost match always starts at the first space-separated segment that
/// contains an underscore and extends while each following segment does too.
fn first_mask_token_into(input: &str, out: &mut String) -> bool {
    out.clear();

    let mut start = None;
    let mut end = 0;
    for (index, segment) in input
        .split(' ')
        .filter(|segment| !segment.is_empty())
        .enumerate()
    {
        if segment.contains('_') {
            if start.is_none() {
                start = Some(index);
            }
            end = index;
        } else if start.is_some() {
            break;
        }
    }

    let Some(start) = start else {
        return false;
    };

    for (index, segment) in input
        .split(' ')
        .filter(|segment| !segment.is_empty())
        .enumerate()
    {
        if index < start {
            continue;
        }
        if index > end {
            break;
        }
        if index > start {
            out.push(' ');
        }
        out.push_str(segment);
    }

    if out.contains('_') {
        true
    } else {
        out.clear();
        false
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
