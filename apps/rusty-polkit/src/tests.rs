use rusty_services::{PolkitPromptRequest, SecretText};
use rusty_ui::{InputModifiers, MemoryClipboard, TextInputKey, UiFontEngine};

use super::dialog::{DialogAction, PolkitDialogState};

fn sample_request() -> PolkitPromptRequest {
    PolkitPromptRequest {
        cookie: "test-cookie-9988".to_owned(),
        action_id: "org.freedesktop.policykit.exec".to_owned(),
        message: "Аутентификация требуется для запуска терминала от root.".to_owned(),
        icon_name: "dialog-password".to_owned(),
        username: "admin".to_owned(),
        uid: 1000,
        caller_pid: Some(4242),
        prompt_label: "Password:".to_owned(),
        echo_on: false,
    }
}

#[test]
fn test_dialog_initial_state_and_layout() {
    let mut dlg = PolkitDialogState::new(sample_request());
    assert_eq!(dlg.cookie, "test-cookie-9988");
    assert_eq!(dlg.username, "admin");
    assert_eq!(dlg.uid, 1000);
    assert!(!dlg.is_error);
    assert!(!dlg.checking);
    assert_eq!(dlg.tries, 0);

    dlg.update_layout(420, 240);
    assert!(dlg.pass_field.rect.width > 200);
    assert!(dlg.submit_rect.x > dlg.cancel_rect.x);
    assert_eq!(dlg.submit_rect.y, dlg.cancel_rect.y);
}

#[test]
fn test_dialog_key_input_and_submission() {
    let mut dlg = PolkitDialogState::new(sample_request());
    let mut fonts = UiFontEngine::new();
    let mut clip = MemoryClipboard::default();
    let mods = InputModifiers::default();

    // Empty field + Enter does not submit
    assert_eq!(
        dlg.on_key(TextInputKey::Enter, mods, &mut fonts, &mut clip),
        None
    );

    // Type characters "s3cr3t"
    for ch in "s3cr3t".chars() {
        let _ = dlg.on_key(TextInputKey::Char(ch), mods, &mut fonts, &mut clip);
    }
    assert_eq!(dlg.pass_field.buffer.as_str(), "s3cr3t");

    // Enter submits password
    let action = dlg.on_key(TextInputKey::Enter, mods, &mut fonts, &mut clip);
    assert_eq!(
        action,
        Some(DialogAction::Submit(SecretText::from_slice("s3cr3t")))
    );

    // Escape cancels
    let cancel_act = dlg.on_key(TextInputKey::Escape, mods, &mut fonts, &mut clip);
    assert_eq!(cancel_act, Some(DialogAction::Cancel));
}

#[test]
fn test_dialog_error_and_retry_clears_input() {
    let mut dlg = PolkitDialogState::new(sample_request());
    let mut fonts = UiFontEngine::new();
    let mut clip = MemoryClipboard::default();
    let mods = InputModifiers::default();

    // Type wrong password
    for ch in "wrong".chars() {
        let _ = dlg.on_key(TextInputKey::Char(ch), mods, &mut fonts, &mut clip);
    }
    assert_eq!(dlg.pass_field.buffer.as_str(), "wrong");

    // Helper sends error
    dlg.set_error("Неверный пароль. Попробуйте снова.");
    assert!(dlg.is_error);
    assert_eq!(dlg.tries, 1);
    assert_eq!(dlg.pass_field.buffer.as_str(), "");
    assert_eq!(
        dlg.status_msg.as_deref(),
        Some("Неверный пароль. Попробуйте снова.")
    );

    // Typing new password resets error state
    let _ = dlg.on_key(TextInputKey::Char('a'), mods, &mut fonts, &mut clip);
    assert!(!dlg.is_error);
    assert_eq!(dlg.status_msg, None);
}

#[test]
fn test_pointer_click_cancel_and_submit() {
    let mut dlg = PolkitDialogState::new(sample_request());
    dlg.update_layout(420, 240);

    // Click outside buttons returns None
    assert_eq!(dlg.on_pointer_click(10, 10), None);

    // Click Cancel returns Cancel
    let cx = dlg.cancel_rect.x + 5;
    let cy = dlg.cancel_rect.y + 5;
    assert_eq!(dlg.on_pointer_click(cx, cy), Some(DialogAction::Cancel));

    // Click Submit when empty returns None
    let sx = dlg.submit_rect.x + 5;
    let sy = dlg.submit_rect.y + 5;
    assert_eq!(dlg.on_pointer_click(sx, sy), None);

    // Type text and click Submit
    dlg.pass_field.buffer.set_text("adminpass");
    assert_eq!(
        dlg.on_pointer_click(sx, sy),
        Some(DialogAction::Submit(SecretText::from_slice("adminpass")))
    );
}

#[test]
fn test_demo_mode_dialog() {
    let dlg = PolkitDialogState::demo();
    assert_ne!(dlg.cookie, "");
    assert_ne!(dlg.username, "");
    assert!(dlg.uid >= 1000 || dlg.uid == 0);
}
