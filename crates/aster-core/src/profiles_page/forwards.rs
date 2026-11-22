use super::{enum_value, EditorField, EditorTab, Field, Profile, ProfileEditor};
use crate::i18n::{t, tf};
use aster_proto::ssh_forward::{ForwardKind, ForwardRule};

const FIELDS: usize = 4;

impl Field {
    pub(super) fn is_forward(self) -> bool {
        matches!(self, Self::ForwardType | Self::ForwardListen | Self::ForwardDestination | Self::ForwardDescription)
    }
}

pub(super) fn tab(profile: &Profile) -> EditorTab {
    let rules = profile.ssh_options.as_ref().and_then(|s| s.forwards.as_deref()).unwrap_or_default();
    let mut fields: Vec<_> = rules.iter().flat_map(form).collect();
    fields.push(Field::AddForward.choice("", Vec::new(), false));
    EditorTab::new("Ports", fields)
}

fn form(rule: &ForwardRule) -> [EditorField; FIELDS] {
    let choices = [("local", t("Local forwarding")), ("remote", t("Remote forwarding")), ("dynamic", t("Dynamic forwarding"))]
        .into_iter().map(|(key, label)| (key.into(), label.into())).collect();
    [
        Field::ForwardType.choice(rule.kind.as_str(), choices, false),
        Field::ForwardListen.text(&rule.listen),
        Field::ForwardDestination.choice(rule.destination.as_deref().unwrap_or_default(), Vec::new(), rule.kind != ForwardKind::Dynamic),
        Field::ForwardDescription.text(&rule.description),
    ]
}

impl ProfileEditor {
    pub(super) fn add_forward(&mut self) {
        let fields = &mut self.tabs[self.tab].fields;
        let rule = ForwardRule { kind: ForwardKind::Local, listen: "127.0.0.1:".into(), destination: None, description: String::new() };
        fields.splice(self.focus..self.focus, form(&rule));
    }

    pub(super) fn remove_forward(&mut self) {
        let first = self.focus - self.focus % FIELDS;
        self.tabs[self.tab].fields.drain(first..first + FIELDS);
        self.focus = first;
    }

    pub(super) fn ssh_forwards(&self) -> Result<Option<Vec<ForwardRule>>, String> {
        let fields: Vec<_> = self.fields().filter(|field| field.id.is_forward()).collect();
        let mut rules = Vec::new();
        for (index, fields) in fields.chunks_exact(FIELDS).enumerate() {
            let kind = enum_value(&fields[0].value).unwrap_or_default();
            let destination = (kind != ForwardKind::Dynamic).then(|| fields[2].value.trim().to_owned());
            let rule = ForwardRule { kind, listen: fields[1].value.trim().into(), destination, description: fields[3].value.trim().into() };
            rule.resolve().map_err(|_| tf("Check the addresses of port forward {number}", &[("number", &(index + 1).to_string())]))?;
            rules.push(rule);
        }
        let originally_set = self.source.ssh_options.as_ref().is_some_and(|s| s.forwards.is_some());
        Ok((originally_set || !rules.is_empty()).then_some(rules))
    }
}
