//! Release-hardening docs must stay current and secret-free.

const ARCH: &str = include_str!("../docs/ARCHITECTURE.md");
const REPRO: &str = include_str!("../docs/REPRODUCIBILITY.md");
const SIGNING: &str = include_str!("../CODE_SIGNING.md");
const VERIFY: &str = include_str!("../.github/workflows/verify.yml");
const RELEASE: &str = include_str!("../.github/workflows/release.yml");

#[test]
fn architecture_is_current_and_not_plan() {
    assert!(ARCH.contains("PLAN.md"));
    assert!(ARCH.contains("not the spec"));
    assert!(ARCH.contains("%LOCALAPPDATA%\\SystemExe\\RunDog\\usage"));
    assert!(ARCH.contains("HKCU\\Software\\SystemExe\\RunDog"));
    assert!(!ARCH.contains("sk-"));
    assert!(!ARCH.contains("Bearer "));
}

#[test]
fn reproducibility_does_not_heavy_ci() {
    assert!(REPRO.contains("Cargo.lock"));
    assert!(REPRO.contains("rust-version"));
    assert!(REPRO.contains("Inno Setup"));
    assert!(REPRO.contains("cargo-deny"));
    assert!(REPRO.contains("deferred") || REPRO.contains("not in CI"));
    assert!(
        !VERIFY.contains("cargo-deny") && !RELEASE.contains("cargo-deny"),
        "do not add cargo-deny to CI in this phase"
    );
    assert!(
        !VERIFY.contains("verify-authenticode") && !RELEASE.contains("verify-authenticode"),
        "Authenticode gate must stay off CI until SignPath signs releases"
    );
}

#[test]
fn signing_doc_states_post_signpath_verify() {
    assert!(
        SIGNING.contains("Get-AuthenticodeSignature")
            || REPRO.contains("Get-AuthenticodeSignature")
    );
    assert!(SIGNING.contains("pending SignPath") || SIGNING.contains("pending"));
}
