//! The checked-in manual pages (`man/`) match the command line: regenerate them with
//! `cargo run --example manpages` after changing a command, flag or help text.

#[path = "../examples/manpages.rs"]
#[allow(dead_code, reason = "the example's main isn't used here")]
mod manpages;

use std::path::Path;

#[test]
fn the_manual_pages_are_up_to_date() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("man");
    let expected = manpages::pages().unwrap();
    let mut stale = Vec::new();
    for (name, page) in &expected {
        if std::fs::read(dir.join(name)).ok().as_deref() != Some(page.as_slice()) {
            stale.push(name.clone());
        }
    }
    let mut checked_in: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".1"))
        .collect();
    checked_in.sort();
    let names: Vec<String> = expected.iter().map(|(name, _)| name.clone()).collect();
    assert!(
        stale.is_empty() && checked_in == names,
        "man/ is out of date (stale: {stale:?}; checked in: {checked_in:?}): run `cargo run \
         --example manpages`"
    );
}

#[test]
fn every_command_has_a_page_and_the_main_page_its_sections() {
    let pages = manpages::pages().unwrap();
    let names: Vec<&str> = pages.iter().map(|(name, _)| name.as_str()).collect();
    for name in [
        "heliosctl.1",
        "heliosctl-devices-list.1",
        "heliosctl-streams-start.1",
        "heliosctl-streams-set-destination.1",
        "heliosctl-wifi-set.1",
        "heliosctl-schema.1",
    ] {
        assert!(names.contains(&name), "{name}");
    }
    let main = String::from_utf8(
        pages[names.iter().position(|n| *n == "heliosctl.1").unwrap()]
            .1
            .clone(),
    )
    .unwrap();
    for section in [
        "EXIT STATUS",
        "ENVIRONMENT",
        "FILES",
        "SECURITY",
        "EXAMPLES",
    ] {
        assert!(
            main.contains(&format!(".SH \"{section}\""))
                || main.contains(&format!(".SH {section}")),
            "{section}"
        );
    }
    let sub = String::from_utf8(
        pages[names
            .iter()
            .position(|n| *n == "heliosctl-wifi-set.1")
            .unwrap()]
        .1
        .clone(),
    )
    .unwrap();
    assert!(sub.contains("SEE ALSO"));
    assert!(
        sub.contains(".TH heliosctl-wifi-set 1 "),
        "the title line keeps its arguments"
    );
}
