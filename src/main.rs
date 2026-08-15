#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
fn main() {
    if let Err(error) = run_dog::windows::run() {
        eprintln!("RunDog failed to start: {error}");
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("RunDog is a Windows-only tray application.");
}
