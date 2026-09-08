//! Static uninstall contract. Does not run Inno or a live uninstall.

const ISS: &str = include_str!("../installer/RunDog.iss");
const CONTRACT: &str = include_str!("../docs/UNINSTALL.md");

const FORBIDDEN_ISS_NEEDLES: &[&str] = &[
    ".claude",
    ".codex",
    "claude_config",
    "codex_home",
    "credentials.json",
    "auth.json",
    "userprofile",
    "anthropic",
    "openai",
    "chatgpt",
];

#[test]
fn inno_script_never_mentions_provider_homes_or_credentials() {
    let lower = ISS.to_ascii_lowercase();
    for needle in FORBIDDEN_ISS_NEEDLES {
        assert!(
            !lower.contains(needle),
            "installer/RunDog.iss must not mention {needle}"
        );
    }
}

#[test]
fn inno_script_deletes_rundog_owned_paths_only() {
    assert!(
        ISS.contains("[UninstallDelete]"),
        "uninstall must declare RunDog cache deletion"
    );
    assert!(
        ISS.contains("{localappdata}\\RunDog"),
        "uninstall must delete the current RunDog usage subtree"
    );
    assert!(
        ISS.contains("{localappdata}\\SystemExe\\RunDog"),
        "uninstall must delete the legacy usage and update subtree"
    );
    assert!(
        ISS.contains("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
        "uninstall must clear the startup Run value"
    );
    assert!(
        ISS.contains("Software\\SystemExe\\RunDog"),
        "uninstall must clear HKCU settings"
    );
    assert!(
        ISS.contains("CurUninstallStepChanged"),
        "uninstall must hook Inno uninstall for registry cleanup"
    );
    assert!(
        ISS.contains("RegDeleteValue"),
        "startup Run value must be deleted explicitly"
    );
    assert!(
        ISS.contains("RegDeleteKeyIncludingSubkeys"),
        "settings key must be deleted including subkeys"
    );
}

#[test]
fn contract_document_states_delete_preserve_and_not_run() {
    assert!(CONTRACT.contains("DELETE"));
    assert!(CONTRACT.contains("PRESERVE"));
    assert!(CONTRACT.contains("NOT RUN"));
    assert!(CONTRACT.contains(".claude"));
    assert!(CONTRACT.contains(".codex"));
    assert!(CONTRACT.contains("%LOCALAPPDATA%\\RunDog\\usage"));
    assert!(CONTRACT.contains("%LOCALAPPDATA%\\SystemExe\\RunDog\\usage"));
    assert!(CONTRACT.contains("%LOCALAPPDATA%\\SystemExe\\RunDog\\updates"));
    assert!(CONTRACT.contains("Launch at startup"));
}
