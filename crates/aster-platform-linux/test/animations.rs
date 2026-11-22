use super::parse;

#[test]
fn parses_plain_and_portal_booleans() {
    for (input, expected) in [
        ("true\n", true),
        ("false\n", false),
        ("(<true>,)\n", true),
        (" (<<false>>, ) \n", false),
    ] {
        assert_eq!(parse(input), Some(expected), "{input:?}");
    }
}

#[test]
fn rejects_invalid_and_non_boolean_values() {
    for input in ["", "not true", "true false", "'false'", "(<'true'>,)", "(<false>, <true>)", "(<false>,", "(<true>)", "<false>>"] {
        assert_eq!(parse(input), None, "{input:?}");
    }
}
