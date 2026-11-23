//! The agent skill (`skills/heliosctl/`) follows the Agent Skills format that Claude and ChatGPT
//! read, and names only commands and flags heliosctl has, so it can't drift from the CLI.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use clap::{CommandFactory, Parser};
use helios_cli::cli::Cli;

fn skill_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("skills/heliosctl")
}

/// Every Markdown file of the skill, with its text.
fn files() -> Vec<(PathBuf, String)> {
    let mut paths = vec![skill_dir().join("SKILL.md")];
    for entry in std::fs::read_dir(skill_dir().join("references")).unwrap() {
        paths.push(entry.unwrap().path());
    }
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let text = std::fs::read_to_string(&path).unwrap();
            (path, text)
        })
        .collect()
}

/// The frontmatter's `key: value` lines, and the body after it.
fn frontmatter(text: &str) -> (Vec<(String, String)>, String) {
    let rest = text
        .strip_prefix("---\n")
        .expect("starts with a --- frontmatter block");
    let (head, body) = rest.split_once("\n---\n").expect("a closing ---");
    let fields = head
        .lines()
        .map(|line| {
            let (key, value) = line.split_once(':').expect("key: value");
            (key.trim().to_owned(), value.trim().to_owned())
        })
        .collect();
    (fields, body.to_owned())
}

#[test]
fn follows_the_agent_skills_format() {
    let text = std::fs::read_to_string(skill_dir().join("SKILL.md")).unwrap();
    let (fields, body) = frontmatter(&text);
    let keys: Vec<&str> = fields.iter().map(|(key, _)| key.as_str()).collect();
    assert_eq!(
        keys,
        ["name", "description"],
        "only the fields both Claude and ChatGPT document"
    );
    let name = &fields[0].1;
    assert_eq!(name, "heliosctl", "matches the directory");
    assert!(
        (1..=64).contains(&name.len())
            && name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            && !name.starts_with('-')
            && !name.ends_with('-')
            && !name.contains("--"),
        "{name}"
    );
    let description = &fields[1].1;
    assert!(
        (1..=1024).contains(&description.len()),
        "{} characters",
        description.len()
    );
    assert!(description.contains("Use when"), "says when to use it");
    assert!(body.lines().count() < 500, "the spec's limit for SKILL.md");
}

#[test]
fn links_point_at_files_in_the_skill() {
    for (path, text) in files() {
        for (_, rest) in text.match_indices("](").map(|(i, _)| ((), &text[i + 2..])) {
            let target = &rest[..rest.find(')').unwrap()];
            if target.starts_with("http") {
                continue;
            }
            let base = path.parent().unwrap();
            assert!(base.join(target).exists(), "{}: {target}", path.display());
        }
    }
}

/// Every long flag in heliosctl's command tree.
fn known_flags() -> BTreeSet<String> {
    fn walk(command: &clap::Command, flags: &mut BTreeSet<String>) {
        for arg in command.get_arguments() {
            if let Some(long) = arg.get_long() {
                flags.insert(format!("--{long}"));
            }
        }
        for sub in command.get_subcommands() {
            walk(sub, flags);
        }
    }
    let mut flags = BTreeSet::from(["--help".to_owned(), "--version".to_owned()]);
    walk(&Cli::command(), &mut flags);
    flags
}

/// The `heliosctl …` invocations in backticks.
fn invocations(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("`heliosctl") {
        let after = &rest[start + 1..];
        let end = after.find('`').expect("a closing backtick");
        found.push(
            after[..end]
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" "),
        );
        rest = &after[end + 1..];
    }
    found
}

/// A placeholder's stand-in, so the example parses.
fn stand_in(word: &str) -> String {
    match word {
        "<device>" => "t-01M80KAX049HNYQ4K9NYP8SNFH".into(),
        "<stream>" => "pocket".into(),
        "<token>" => "change-1".into(),
        "<ssid>" => "Venue".into(),
        "<Operation>" => "UpdateStream".into(),
        other => other.into(),
    }
}

#[test]
fn every_command_example_parses() {
    let mut checked = 0;
    for (path, text) in files() {
        for invocation in invocations(&text) {
            // Usage summaries ([optional], a|b, …) are checked by the flag test below.
            if invocation.contains(['[', '|', '…']) || invocation.contains("...") {
                continue;
            }
            let words: Vec<String> = invocation.split(' ').map(stand_in).collect();
            // Prose mentions of the tool itself, and examples with a generic placeholder.
            if words.len() < 2 || words.iter().any(|word| word.starts_with('<')) {
                continue;
            }
            if let Err(error) = Cli::try_parse_from(&words) {
                // clap reports --help and --version as "errors" that print and exit 0.
                use clap::error::ErrorKind;
                assert!(
                    matches!(
                        error.kind(),
                        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
                    ),
                    "{}: `{invocation}` doesn't parse:\n{error}",
                    path.display()
                );
            }
            checked += 1;
        }
    }
    assert!(checked >= 10, "only {checked} examples checked");
}

/// Flags the skill names to say they don't exist.
const ABSENT: [&str; 1] = [
    // "There is no `--passphrase` flag": passphrases come from files or stdin only.
    "--passphrase",
];

#[test]
fn every_flag_named_exists() {
    let known = known_flags();
    for flag in ABSENT {
        assert!(!known.contains(flag), "{flag} exists now: update the skill");
    }
    for (path, text) in files() {
        for word in text.split(|c: char| c.is_whitespace() || "`(),:;".contains(c)) {
            let Some(flag) = word.strip_prefix("--") else {
                continue;
            };
            let flag: String = flag
                .chars()
                .take_while(|c| c.is_ascii_lowercase() || *c == '-')
                .collect();
            // Frontmatter fences and table rules (`---`) aren't flags.
            if !flag.starts_with(|c: char| c.is_ascii_lowercase()) {
                continue;
            }
            let flag = format!("--{flag}");
            if ABSENT.contains(&flag.as_str()) {
                continue;
            }
            assert!(
                known.contains(&flag),
                "{}: {flag} isn't a heliosctl flag",
                path.display()
            );
        }
    }
}

#[test]
fn every_subcommand_named_exists() {
    let cli = Cli::command();
    for (path, text) in files() {
        for invocation in invocations(&text) {
            let words: Vec<&str> = invocation
                .split(' ')
                .skip(1)
                .filter(|word| !word.starts_with('-'))
                .collect();
            let Some(first) = words.first().filter(|word| !word.starts_with('<')) else {
                continue;
            };
            let Some(group) = cli.find_subcommand(first) else {
                panic!("{}: no command {first}", path.display());
            };
            if group.has_subcommands()
                && let Some(second) = words.get(1)
                && !second.starts_with('<')
                && !second.contains('…')
            {
                assert!(
                    group.find_subcommand(second).is_some(),
                    "{}: no command {first} {second}",
                    path.display()
                );
            }
        }
    }
}
