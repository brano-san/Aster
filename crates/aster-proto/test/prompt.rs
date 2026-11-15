use super::*;

#[test]
fn sudo_password_for_user() {
    assert!(is_password_prompt("[sudo] password for alice:"));
}

#[test]
fn trailing_spaces_and_case() {
    assert!(is_password_prompt("PaSsWoRd: \t"));
}

#[test]
fn russian_sudo() {
    assert!(is_password_prompt("[sudo] пароль для bob:"));
}

#[test]
fn german_password() {
    assert!(is_password_prompt("Passwort:"));
}

#[test]
fn french_password() {
    assert!(is_password_prompt("Mot de passe :"));
}

#[test]
fn fullwidth_colon() {
    assert!(is_password_prompt("密码："));
}

#[test]
fn sudo_authentication() {
    assert!(is_password_prompt("[sudo: authenticate] Password:"));
}

#[test]
fn rejects_passphrase() {
    assert!(!is_password_prompt(
        "Enter passphrase for key '/home/a/.ssh/id_ed25519':"
    ));
    assert!(!is_password_prompt("[sudo] Passphrase Password:"));
}

#[test]
fn rejects_answer_after_colon() {
    assert!(!is_password_prompt("password: hunter2"));
}

#[test]
fn rejects_shell_prompt_and_empty() {
    for text in ["$ ", "", "   ", "password", ":", "："] {
        assert!(!is_password_prompt(text));
    }
}

#[test]
fn all_localized_suffixes() {
    for word in [
        "password",
        "passwort",
        "mot de passe",
        "contraseña",
        "senha",
        "пароль",
        "hasło",
        "heslo",
        "lösenord",
        "adgangskode",
        "kata sandi",
        "parola",
        "wachtwoord",
        "salasana",
        "密码",
        "密碼",
        "パスワード",
        "암호",
        "비밀번호",
    ] {
        assert!(
            is_password_prompt(&format!("{}:", word.to_uppercase())),
            "{word}"
        );
    }
}

#[test]
fn patterns_require_following_text() {
    for text in [
        "Password for alice:",
        "ПАРОЛЬ ДЛЯ bob:",
        "[sudo] token:",
        "[sudo: authenticate] token:",
    ] {
        assert!(is_password_prompt(text));
    }
    for text in [
        "password for :",
        "пароль для :",
        "[sudo]:",
        "[sudo] :",
        "[sudo: authenticate]:",
    ] {
        assert!(!is_password_prompt(text));
    }
}

#[test]
fn only_last_output_line_is_checked() {
    assert!(is_password_prompt("passphrase rejected\nPassword:"));
    assert!(!is_password_prompt("Password:\nready:"));
    assert!(!is_password_prompt("Password:\n"));
}
