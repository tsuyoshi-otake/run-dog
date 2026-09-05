//! Application-defined window messages for the hidden tray HWND.
//!
//! These are `WM_APP` (`0x8000`) plus a named slot. IDs are the single source
//! of truth; uniqueness is checked at compile time and in tests. Do not assign
//! a new constant by incrementing a neighbor in another file.

/// `WM_APP`. User messages for the unowned message window start here.
pub const WM_APP: u32 = 0x8000;

pub const TRAY_CALLBACK_MESSAGE: u32 = WM_APP + 1;
/// Deferred tray context menu. Posted from the shell callback so the opening
/// right-button release cannot activate a menu item underneath the cursor.
pub const WM_SHOW_TRAY_MENU: u32 = WM_APP + 2;
pub const USAGE_READY_MESSAGE: u32 = WM_APP + 3;
/// Posted when an update check worker reaches a terminal menu state.
pub const UPDATE_CHECK_DONE_MESSAGE: u32 = WM_APP + 4;
/// Reserved after a verified installer has been handed to ShellExecute.
/// Historically collided with `WM_SHOW_TRAY_MENU` when both used `WM_APP + 2`.
pub const UPDATE_REQUEST_EXIT_MESSAGE: u32 = WM_APP + 5;

const APP_MESSAGE_IDS: &[u32] = &[
    TRAY_CALLBACK_MESSAGE,
    WM_SHOW_TRAY_MENU,
    USAGE_READY_MESSAGE,
    UPDATE_CHECK_DONE_MESSAGE,
    UPDATE_REQUEST_EXIT_MESSAGE,
];

const fn ids_are_unique(ids: &[u32]) -> bool {
    let mut i = 0;
    while i < ids.len() {
        let mut j = i + 1;
        while j < ids.len() {
            if ids[i] == ids[j] {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

const _: () = assert!(
    ids_are_unique(APP_MESSAGE_IDS),
    "application window message IDs must be unique"
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn component_app_window_messages_are_unique_and_named() {
        let named = [
            ("TRAY_CALLBACK_MESSAGE", TRAY_CALLBACK_MESSAGE),
            ("WM_SHOW_TRAY_MENU", WM_SHOW_TRAY_MENU),
            ("USAGE_READY_MESSAGE", USAGE_READY_MESSAGE),
            ("UPDATE_CHECK_DONE_MESSAGE", UPDATE_CHECK_DONE_MESSAGE),
            ("UPDATE_REQUEST_EXIT_MESSAGE", UPDATE_REQUEST_EXIT_MESSAGE),
        ];
        assert_eq!(named.len(), APP_MESSAGE_IDS.len());
        for (i, (_, id)) in named.iter().enumerate() {
            assert_eq!(*id, APP_MESSAGE_IDS[i]);
            assert!(*id >= WM_APP);
            assert!(*id < WM_APP + 0x4000);
        }
        let mut sorted = APP_MESSAGE_IDS.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), APP_MESSAGE_IDS.len());
        assert_ne!(WM_SHOW_TRAY_MENU, UPDATE_REQUEST_EXIT_MESSAGE);
    }

    #[test]
    fn component_uniqueness_oracle_rejects_duplicate_ids() {
        assert!(!ids_are_unique(&[0x8002, 0x8002]));
        assert!(ids_are_unique(&[0x8001, 0x8002, 0x8003]));
    }
}
