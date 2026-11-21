use super::*;

#[test]
fn decodes_the_two_supported_routes_as_utf8() {
    assert_eq!(parse("aster://profile?profileName=%D0%A0%D0%B0%D0%B1%D0%BE%D1%82%D0%B0%20%26%20C++"), Some(UrlCommand::Profile("Работа & C++".into())));
    assert_eq!(parse("ASTER://open?path=C%3a%5cWork%20Notes%5c%23draft"), Some(UrlCommand::Open(r"C:\Work Notes\#draft".into())));
}

#[test]
fn rejects_unknown_routes_parameters_fragments_and_malformed_escapes() {
    for invalid in [
        "https://profile?profileName=X", "aster:profile?profileName=X",
        "aster://run?cmd=X", "aster://profile/path?profileName=X",
        "aster://profile?name=X", "aster://open?profileName=X",
        "aster://profile?profileName=X&profileName=Y", "aster://open?path=X#Y",
        "aster://open?path=", "aster://open?path=%20", "aster://open?path=a b",
        "aster://open?path=%0A", "aster://open?path=%0", "aster://open?path=%2z",
        "aster://open?path=%ff", "aster://open?path=%00", "aster://open?path=%",
    ] {
        assert_eq!(parse(invalid), None, "{invalid}");
    }
}
