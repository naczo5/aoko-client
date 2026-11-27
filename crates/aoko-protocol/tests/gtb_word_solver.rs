//! Port of `Aoko.Tests/GtbWordSolverTests.cs`.
//!
//! The managed suite rewrites `Data/gtb_wordlist.js` on disk and reflects over
//! private statics to reset the cache. Here each case builds its own solver over
//! an in-memory word list, which is the same thing without the I/O.

use aoko_protocol::gtb::{extract_mask, load_custom_words, parse_wordlist_js, GtbWordSolver};

fn solver() -> GtbWordSolver {
    GtbWordSolver::from_words(["cat", "cot", "cut", "dog"])
}

#[test]
fn solve_extracts_mask_and_matches_expected_words() {
    let mut solver = solver();
    let (mask, matches) = solver.solve("The theme is C_T (1)", 50);

    assert_eq!(mask, "c_t");
    assert_eq!(matches, ["cat", "cot", "cut"]);
}

#[test]
fn solve_respects_max_results_limit() {
    let mut solver = solver();
    let (mask, matches) = solver.solve("Theme is C_T", 2);

    assert_eq!(mask, "c_t");
    assert_eq!(matches.len(), 2);
}

#[test]
fn max_results_zero_still_returns_one_match() {
    // The managed host clamps with `Math.Max(1, maxResults)`; a zero limit must not
    // mean "unlimited".
    let mut solver = solver();
    let (_, matches) = solver.solve("Theme is C_T", 0);

    assert_eq!(matches.len(), 1);
}

#[test]
fn try_learn_solved_word_adds_new_word_for_future_solves() {
    let mut solver = GtbWordSolver::from_words(["cot", "cut", "dog"]);

    let (_, initial) = solver.solve("The theme is C_T", 50);
    assert_eq!(initial, ["cot", "cut"]);
    assert!(!initial.iter().any(|word| word == "cat"));

    assert!(solver.try_learn_solved_word("The theme is cat"));

    let (_, after_learn) = solver.solve("The theme is C_T", 50);
    assert!(after_learn.iter().any(|word| word == "cat"));
}

#[test]
fn repeated_mask_is_memoised_and_returns_the_same_slice() {
    let mut solver = solver();

    let (mask, matches) = solver.solve("Theme is C_T", 50);
    let mask = mask.to_owned();
    let matches = matches.to_vec();

    let (again_mask, again) = solver.solve("Theme is C_T", 50);

    assert_eq!(mask, again_mask);
    assert_eq!(matches, again);
}

#[test]
fn learn_rejects_words_that_do_not_fit_the_last_mask() {
    let mut solver = solver();
    solver.solve("Theme is C_T", 50);

    assert!(!solver.try_learn_solved_word("The theme is dog"));
    assert!(!solver.try_learn_solved_word("The theme is "));
    assert!(!solver.try_learn_solved_word("no solved word here"));
}

#[test]
fn learn_rejects_a_word_that_is_already_known() {
    let mut solver = solver();
    solver.solve("Theme is C_T", 50);

    assert!(!solver.try_learn_solved_word("The theme is cat"));
}

#[test]
fn learn_before_any_solve_is_a_no_op() {
    let mut solver = solver();

    assert!(!solver.try_learn_solved_word("The theme is cat"));
    assert_eq!(solver.len(), 4);
}

#[test]
fn parse_wordlist_js_reads_the_words_data_array() {
    let words = parse_wordlist_js(r#"const wordsData = ["cat", "cot", "cut", "dog"];"#);

    assert_eq!(words, ["cat", "cot", "cut", "dog"]);
}

#[test]
fn parse_wordlist_js_falls_back_to_array_data() {
    let words = parse_wordlist_js(r#"var arrayData = ["apple", "banana"];"#);

    assert_eq!(words, ["apple", "banana"]);
}

#[test]
fn parse_wordlist_js_drops_words_shorter_than_three_characters() {
    let words = parse_wordlist_js(r#"const wordsData = ["ab", "abc", "abcd"];"#);

    assert_eq!(words, ["abc", "abcd"]);
}

#[test]
fn parse_wordlist_js_lowercases_and_deduplicates() {
    let words = parse_wordlist_js(r#"const wordsData = ["Cat", "cat", "DOG"];"#);

    assert_eq!(words, ["cat", "dog"]);
}

#[test]
fn solve_on_a_line_without_a_mask_returns_nothing() {
    let mut solver = solver();
    let (mask, matches) = solver.solve("no theme yet", 50);

    assert_eq!(mask, "");
    assert!(matches.is_empty());
}

#[test]
fn custom_words_extend_the_index() {
    let mut solver = solver();
    solver.extend_with_custom_words("zebra\n\nquokka\nZEBRA");

    assert_eq!(solver.len(), 6, "blank lines and duplicates are dropped");
}

#[test]
fn custom_words_have_no_comment_syntax() {
    // `LoadCustomWords` in the managed host takes every non-blank line as a word —
    // it never strips comments — and `NormalizeWord` then deletes everything that
    // is not a letter or a space. So `# comment` becomes the word `comment`. A `#`
    // skip would silently reshape existing user word lists, hence the pin.
    let words = load_custom_words("# comment\nzebra\n");

    assert_eq!(words, ["comment", "zebra"]);
}

/// The mask pipeline is now a chain of stages that write into reusable buffers instead
/// of returning fresh strings. These pin the stages that made the rewrite worth doing —
/// colour codes, brackets, counters, ratios, the colon fallback and the tokenised form —
/// so a future "cleanup" of a stage has to face what it is for. Every expectation here
/// is what the C# `GtbWordSolver` produced, not what the new buffers happen to give.
#[test]
fn extract_mask_normalises_the_shapes_the_bridge_sends() {
    // `§x` colour-code pairs are two characters each: the code and the one it colours.
    assert_eq!(
        extract_mask("\u{00A7}cThe theme \u{00A7}fis C_T (1)"),
        "c_t"
    );
    // A bracketed prefix is replaced rather than stripped.
    assert_eq!(extract_mask("[Aoko] The theme is C_T"), "c_t");
    // The trailing `(12)` counter goes before the mask is taken.
    assert_eq!(extract_mask("The theme is C_T (12)"), "c_t");
    // Every line is a candidate: the first with a mask wins, CRLF included.
    assert_eq!(extract_mask("line one\r\nThe theme is D_G"), "d_g");
}

#[test]
fn extract_mask_falls_back_to_the_last_colon() {
    // No `theme ... is` guard, so everything after the last colon is the candidate.
    assert_eq!(extract_mask("x:y:z_w"), "z_w");
}

#[test]
fn extract_mask_keeps_each_segment_that_carries_an_underscore() {
    // The token is a run of space-separated segments, each with an underscore.
    assert_eq!(extract_mask("a_b c_d e_f"), "a_b c_d e_f");
}

#[test]
fn extract_mask_decodes_the_tokenised_form() {
    // Four lone tokens is the spaced form the challenge list uses, so the single spaces
    // between them are dropped and the characters are joined: `____`.
    assert_eq!(extract_mask("3/5 _ _ _ _"), "____");
    // Three lone tokens is not that form, so the spaces stay and only the segment that
    // carries the underscore is the mask. Pinned because it looks like a bug and is the
    // managed behaviour.
    assert_eq!(extract_mask("The theme is C _ T"), "_");
}
