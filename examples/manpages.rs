//! Writes heliosctl's manual pages to `man/` (or the directory given) from the command line's own
//! definitions, so they can't drift from `--help`: `cargo run --example manpages`.
//!
//! `heliosctl(1)` adds what `--help` can't hold: exit status, environment, files, security and
//! examples. Every subcommand gets its own page (`heliosctl-streams-start(1)`, ...).
//! `tests/manpages.rs` checks the checked-in pages match.

use std::io::Write;
use std::path::{Path, PathBuf};

use clap::CommandFactory;
use clap_mangen::Man;
use clap_mangen::roff::{Roff, bold, italic, roman};

/// The manual's name, as man(1) shows it in the header.
const MANUAL: &str = "Helios Manual";

fn main() -> std::io::Result<()> {
    let dir = std::env::args_os().nth(1).map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("man"),
        PathBuf::from,
    );
    std::fs::create_dir_all(&dir)?;
    for old in std::fs::read_dir(&dir)? {
        let old = old?.path();
        if old.extension().is_some_and(|ext| ext == "1") {
            std::fs::remove_file(old)?;
        }
    }
    for (name, page) in pages()? {
        std::fs::write(dir.join(name), page)?;
    }
    Ok(())
}

/// Every page: `(file name, roff)`, sorted by name.
///
/// # Errors
///
/// Only if writing to memory fails, which it doesn't.
pub fn pages() -> std::io::Result<Vec<(String, Vec<u8>)>> {
    let mut command = helios_cli::cli::Cli::command().disable_help_subcommand(true);
    command.build();
    let mut pages = Vec::new();
    collect(&command, &mut pages)?;
    pages.sort();
    Ok(pages)
}

fn collect(command: &clap::Command, pages: &mut Vec<(String, Vec<u8>)>) -> std::io::Result<()> {
    for sub in command.get_subcommands().filter(|sub| !sub.is_hide_set()) {
        collect(sub, pages)?;
    }
    // Every .TH argument must be non-empty: an empty one is dropped, shifting the rest.
    let version = env!("CARGO_PKG_VERSION");
    let man = Man::new(command.clone())
        .date(version)
        .source(format!("heliosctl {version}"))
        .manual(MANUAL);
    let name = man.get_filename();
    let mut page = Vec::new();
    if command.get_display_name() == Some("heliosctl") || command.get_name() == "heliosctl" {
        main_page(&man, &mut page)?;
    } else {
        man.render(&mut page)?;
        see_also(&mut page)?;
    }
    pages.push((name, page));
    Ok(())
}

fn main_page(man: &Man, w: &mut dyn Write) -> std::io::Result<()> {
    man.render_title(w)?;
    man.render_name_section(w)?;
    man.render_synopsis_section(w)?;
    man.render_description_section(w)?;
    man.render_options_section(w)?;
    man.render_subcommands_section(w)?;

    let mut roff = Roff::new();
    roff.control("SH", ["EXIT STATUS"]);
    for (code, meaning) in [
        ("0", "Done (with --wait: the device applied the change)."),
        (
            "1",
            "Anything else: a network error, an internal error, a response that doesn't parse.",
        ),
        (
            "2",
            "Invalid input: a usage error, ValidationException, InvalidDestination, or a --dry-run that failed its checks.",
        ),
        ("3", "Not found: DeviceNotFound, StreamNotFound."),
        (
            "4",
            "Refused: SettingUnavailable, WifiNetworkRefused, or (with --wait) the device rejected the change.",
        ),
        (
            "5",
            "Timed out: DeviceUnreachable, or --wait ran out of time (the device may still apply it).",
        ),
        ("6", "Not authorized: no AWS credentials, or HTTP 403."),
        (
            "7",
            "The device hasn't reported that yet: DeviceStateUnavailable.",
        ),
        ("8", "Throttled: RateLimited, after retries."),
    ] {
        roff.control("TP", [] as [&str; 0]);
        roff.text([bold(code)]);
        roff.text([roman(meaning)]);
    }
    roff.text([roman(
        "With --json, the error is one JSON document on standard error: {\"error\": {\"type\", \"message\", ...the error's members}}.",
    )]);

    roff.control("SH", ["ENVIRONMENT"]);
    for (name, meaning) in [
        (
            "HELIOS_CONTROL_URL",
            "The Control API's base URL, as --endpoint.",
        ),
        (
            "AWS_PROFILE",
            "The AWS profile, as --profile (default helios-operator).",
        ),
        (
            "AWS_REGION",
            "The AWS region, as --region (default: the profile's).",
        ),
        (
            "AWS_CONFIG_FILE, AWS_SHARED_CREDENTIALS_FILE",
            "Where the AWS SDK finds profiles and credentials.",
        ),
    ] {
        roff.control("TP", [] as [&str; 0]);
        roff.text([bold(name)]);
        roff.text([roman(meaning)]);
    }

    roff.control("SH", ["FILES"]);
    roff.control("TP", [] as [&str; 0]);
    roff.text([italic("~/.config/heliosctl/config.toml")]);
    roff.text([roman(
        "Settings, each used when its flag and environment variable aren't given: endpoint, profile and region. None of them is a secret.",
    )]);
    roff.control("TP", [] as [&str; 0]);
    roff.text([italic("~/.aws/config")]);
    roff.text([roman(
        "The AWS profiles. The default profile, helios-operator, should assume the helios-operator role. The Rust AWS SDK can't read aws login sessions: give such a profile a credential_process of aws configure export-credentials --profile <profile> --format process, and use it as helios-operator's source_profile.",
    )]);

    roff.control("SH", ["SECURITY"]);
    roff.text([roman(
        "heliosctl stores no credentials: requests are signed with SigV4 from the AWS credential chain. A Wi-Fi passphrase is read only from --passphrase-file (or - for standard input), and an SRT destination with a passphrase only from --url-file, never from an argument, so neither reaches shell history or the process list. Neither is ever printed: dry runs and output show ***.",
    )]);

    roff.control("SH", ["EXAMPLES"]);
    for (what, example) in [
        ("List devices:", "heliosctl devices list"),
        (
            "One device as JSON:",
            "heliosctl --json devices get t-01M80KAX049HNYQ4K9NYP8SNFH",
        ),
        (
            "Check a change without sending it:",
            "heliosctl streams update d-01M80KB9C9T0ZSPC64NZVY1QRA pocket --bitrate 3000 --dry-run",
        ),
        (
            "Start a stream and wait until it's applied:",
            "heliosctl streams start t-01M80KAX049HNYQ4K9NYP8SNFH pocket --wait",
        ),
        (
            "Add a Wi-Fi network, passphrase on standard input:",
            "heliosctl wifi set t-01M80KAX049HNYQ4K9NYP8SNFH --ssid Venue --passphrase-file - < passphrase.txt",
        ),
        (
            "An operation's input and output shapes:",
            "heliosctl schema SetWifiNetwork",
        ),
    ] {
        roff.text([roman(what)]);
        roff.control("RS", ["4"]);
        roff.text([bold(example)]);
        roff.control("RE", [] as [&str; 0]);
    }
    roff.to_writer(w)?;
    man.render_version_section(w)?;
    Ok(())
}

fn see_also(w: &mut dyn Write) -> std::io::Result<()> {
    let mut roff = Roff::new();
    roff.control("SH", ["SEE ALSO"]);
    roff.text([
        bold("heliosctl"),
        roman("(1), for exit status, environment, files and security."),
    ]);
    roff.to_writer(w)
}
