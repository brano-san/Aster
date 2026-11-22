use super::*;
use aster_proto::ssh::InteractivePrompt;

#[test]
fn interactive_panel_masks_password_and_resets_for_the_next_round() {
    let challenge = InteractiveChallenge { id: 2, name: "Sign in".into(), instructions: "Go to https://example.test/device before continuing".into(),
        prompts: vec![InteractivePrompt { label: "Name".into(), echo: true }, InteractivePrompt { label: "Password".into(), echo: false }], remember: Some(1) };
    let mut panel = Panel::new(challenge.clone());
    assert!(panel.rows.iter().any(|row| matches!(row, Row::Link(url) if url == "https://example.test/device")));
    panel.insert("ann");
    panel.step(false, 5);
    panel.insert("secret");
    assert_eq!(panel.display(0), "ann");
    assert_eq!(panel.display(1), "••••••");
    panel.remember = true;
    let answer = panel.answer(true);
    assert!(panel.pending && answer.remember);
    assert!(!format!("{answer:?}").contains("secret"));
    assert_eq!(answer.answers.unwrap(), ["ann", "secret"]);
    let mut next = Panel::new(InteractiveChallenge { id: 3, remember: None, ..challenge });
    assert!(!next.pending && !next.remember);
    assert!(next.display(0).is_empty());
    let cancel = next.answer(false);
    assert_eq!((cancel.id, cancel.answers), (3, None));
}
