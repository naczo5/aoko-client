//! Command-line parsing.
//!
//! `MIGRATION_ARCHITECTURE.md` §10 makes `cargo run -p aoko-app -- --capture-screenshots
//! <out_dir>` a verification command, so the flag is part of the contract rather than
//! a developer convenience. Parsing is a pure function over an argument list: no
//! `std::env::args` inside, so it is unit-tested without spawning a process.
//!
//! Deliberately no clap: this binary takes three flags and adding a parser dependency
//! for them would need a workspace entry that only the workspace owner may add.

use std::fmt;
use std::path::PathBuf;

/// What the process was asked to do.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Args {
    /// `--capture-screenshots <out_dir>`: write one PNG per registered screen and exit.
    pub capture_screenshots: Option<PathBuf>,
    /// `--theme <name>`: override the starting palette. Unknown names fall back to
    /// `Slate`, matching `Theme::by_name`.
    pub theme: Option<String>,
    /// `--help` was asked for.
    pub help: bool,
    /// `--version` was asked for.
    pub version: bool,
}

/// A parse failure. Every variant carries enough to print without a second lookup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArgError {
    /// A flag that needs a value was last on the line.
    MissingValue(&'static str),
    /// An argument that is not a flag and not a value.
    Unexpected(String),
}

impl fmt::Display for ArgError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingValue(flag) => write!(formatter, "{flag} needs a value"),
            Self::Unexpected(argument) => write!(formatter, "unexpected argument: {argument}"),
        }
    }
}

impl std::error::Error for ArgError {}

/// The usage line, printed for `--help` and on any parse failure.
pub const USAGE: &str = "\
aoko — the Aoko host

USAGE:
    aoko [--theme <name>]
    aoko --capture-screenshots <out_dir> [--theme <name>]

OPTIONS:
    --capture-screenshots <out_dir>  Render every registered screen to
                                     <out_dir>/<screen>.png and exit. Requires a
                                     build with the `capture` feature.
    --theme <name>                   Start with this palette. Unknown names fall
                                     back to Slate.
    -h, --help                       Print this message.
    -V, --version                    Print the version.";

/// Parses arguments, excluding the program name.
pub fn parse<I, S>(arguments: I) -> Result<Args, ArgError>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut args = Args::default();
    let mut arguments = arguments.into_iter().map(Into::into);

    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "-h" | "--help" => args.help = true,
            "-V" | "--version" => args.version = true,
            "--capture-screenshots" => {
                let value = arguments
                    .next()
                    .ok_or(ArgError::MissingValue("--capture-screenshots"))?;
                args.capture_screenshots = Some(PathBuf::from(value));
            }
            "--theme" => {
                let value = arguments.next().ok_or(ArgError::MissingValue("--theme"))?;
                args.theme = Some(value);
            }
            other => return Err(ArgError::Unexpected(other.to_owned())),
        }
    }

    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_arguments_is_a_normal_launch() {
        let args = parse(Vec::<String>::new()).unwrap();
        assert_eq!(args, Args::default());
        assert!(args.capture_screenshots.is_none());
    }

    #[test]
    fn the_capture_command_from_the_architecture_doc_parses() {
        // The exact verification command in `MIGRATION_ARCHITECTURE.md` §10.
        let args = parse(["--capture-screenshots", "/tmp/screens"]).unwrap();
        assert_eq!(
            args.capture_screenshots,
            Some(PathBuf::from("/tmp/screens"))
        );
        assert_eq!(args.theme, None);
    }

    #[test]
    fn theme_accepts_a_multi_word_palette_name_as_one_argument() {
        let args = parse(["--theme", "Lime Water"]).unwrap();
        assert_eq!(args.theme.as_deref(), Some("Lime Water"));
    }

    #[test]
    fn flags_combine_in_any_order() {
        let args = parse(["--theme", "Steel", "--capture-screenshots", "out"]).unwrap();
        assert_eq!(args.theme.as_deref(), Some("Steel"));
        assert_eq!(args.capture_screenshots, Some(PathBuf::from("out")));
    }

    #[test]
    fn a_missing_value_is_reported_with_the_flag_name() {
        assert_eq!(
            parse(["--capture-screenshots"]),
            Err(ArgError::MissingValue("--capture-screenshots"))
        );
        assert_eq!(parse(["--theme"]), Err(ArgError::MissingValue("--theme")));
        assert_eq!(
            ArgError::MissingValue("--theme").to_string(),
            "--theme needs a value"
        );
    }

    #[test]
    fn an_unknown_argument_fails_rather_than_being_ignored() {
        // Silently ignoring a typo would run the app when the caller wanted a capture.
        assert_eq!(
            parse(["--captur-screenshots", "out"]),
            Err(ArgError::Unexpected("--captur-screenshots".to_owned()))
        );
        assert_eq!(
            parse(["stray"]),
            Err(ArgError::Unexpected("stray".to_owned()))
        );
    }

    #[test]
    fn help_and_version_are_recognised_in_both_spellings() {
        assert!(parse(["-h"]).unwrap().help);
        assert!(parse(["--help"]).unwrap().help);
        assert!(parse(["-V"]).unwrap().version);
        assert!(parse(["--version"]).unwrap().version);
    }

    #[test]
    fn usage_mentions_the_capture_flag_once() {
        assert_eq!(USAGE.matches("--capture-screenshots").count(), 2);
        assert!(USAGE.contains("capture` feature"));
    }
}
