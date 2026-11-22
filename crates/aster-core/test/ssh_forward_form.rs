use super::*;

#[test]
fn forward_form_validates_endpoints_and_ignores_hidden_destination() {
    let mut form = ForwardForm::new(1);
    let edit = TextEditor::new;
    assert_eq!(form.fields[0].text(), "127.0.0.1");
    assert!(form.rule().is_err());
    form.fields[1] = edit("65536");
    assert!(form.rule().is_err());
    form.fields[0] = edit("::1");
    form.fields[1] = edit("0");
    form.fields[2] = edit("bad host");
    form.fields[3] = edit("123");
    assert!(form.rule().is_err());
    form.fields[2] = edit("[::1]");
    form.fields[3] = edit("0");
    assert!(form.rule().is_err());
    form.fields[3] = edit("123");
    assert_eq!(form.rule().unwrap().1, Forward::Local {
        bind: "::1".into(), port: 0, host: "::1".into(), host_port: 123,
    });
    form.kind = ForwardKind::Dynamic;
    form.fields[3] = edit("");
    let (rule, forward) = form.rule().unwrap();
    assert_eq!(rule.destination, None);
    assert_eq!(forward, Forward::Dynamic { bind: "::1".into(), port: 0 });
    assert!(!form.save);
}

#[test]
fn forward_form_routes_commands_and_saves_only_when_checked() {
    use crate::config::Config;
    use crate::profile::Profile;
    use aster_proto::ssh_forward::ForwardStatus;
    let profile = Profile { id: "custom:ssh".into(), ssh: Some("ann@host".into()), ..Default::default() };
    let config = Config { profiles: vec![profile.clone()], ..Default::default() };
    let mut app = App::new(config, Vec::new(), true, None, std::sync::Arc::new(|| {}));
    app.open_tab(profile);
    let pane = app.focused_pane().unwrap();
    app.set_ssh_forwards(pane, Vec::new());
    app.take_effects();

    for checked in [false, true] {
        app.choose_ssh_forward(pane, "add");
        let form = app.ssh_forward_form.as_mut().unwrap();
        form.kind = ForwardKind::Dynamic;
        form.save = checked;
        form.fields[1] = TextEditor::new("0");
        app.submit_forward();
        let effects = app.take_effects();
        assert!(effects.iter().any(|effect| matches!(effect, Effect::SshForward {
            pane: target, command: Command::AddForward(Forward::Dynamic { bind, port: 0 }),
        } if *target == pane && bind == "127.0.0.1")));
        assert_eq!(effects.iter().any(|effect| matches!(effect, Effect::SaveConfig(_))), checked);
    }
    let config = Config::parse(&app.config.to_text()).unwrap();
    let saved = config.profiles[0].ssh_options.as_ref().unwrap();
    assert!(saved.inherit_forwards);
    let forward = Forward::Dynamic { bind: "127.0.0.1".into(), port: 0 };
    assert_eq!(saved.forwards.as_ref().unwrap()[0].resolve().unwrap(), forward);

    app.set_ssh_forwards(pane, vec![ForwardStatus { index: 7, forward, bound: Ok(49152), stop_error: None }]);
    app.choose_ssh_forward(pane, "0");
    assert!(app.take_effects().is_empty());
    app.choose_ssh_forward(pane, "7");
    assert!(matches!(app.take_effects().as_slice(), [Effect::SshForward { pane: target, command: Command::StopForward(7) }] if *target == pane));
}
