pub fn is_password_prompt(line: &str) -> bool {
    let line = line
        .rsplit('\n')
        .next()
        .unwrap_or("")
        .trim_end()
        .to_lowercase();
    let Some(text) = line.strip_suffix([':', '：']) else {
        return false;
    };
    if text.contains("passphrase") {
        return false;
    }
    let text = text.trim_end();
    let suffixes = [
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
    ];
    suffixes.iter().any(|suffix| text.ends_with(suffix))
        || [
            "password for ",
            "пароль для ",
            "[sudo]",
            "[sudo: authenticate]",
        ]
        .iter()
        .any(|pattern| {
            text.split_once(pattern)
                .is_some_and(|(_, after)| !after.trim().is_empty())
        })
}

#[cfg(test)]
#[path = "../test/prompt.rs"]
mod tests;
