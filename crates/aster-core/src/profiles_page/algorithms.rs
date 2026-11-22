use super::{EditorField, EditorTab, Field, ProfileEditor};
use crate::i18n::t;
use aster_proto::ssh::{AlgorithmCatalog, AlgorithmOptions, Algorithms};

#[derive(Clone, Copy)]
enum Category {
    Cipher,
    Kex,
    Mac,
    HostKey,
    Compression,
}

impl Category {
    fn from_value(value: &str) -> Self {
        match value {
            "kex" => Self::Kex, "mac" => Self::Mac, "host-key" => Self::HostKey,
            "compression" => Self::Compression, _ => Self::Cipher,
        }
    }

    fn names(self, algorithms: &Algorithms) -> &[String] {
        match self {
            Self::Cipher => &algorithms.cipher, Self::Kex => &algorithms.kex, Self::Mac => &algorithms.mac,
            Self::HostKey => &algorithms.host_key, Self::Compression => &algorithms.compression,
        }
    }

    fn field(self, options: &mut AlgorithmOptions) -> &mut Option<Vec<String>> {
        match self {
            Self::Cipher => &mut options.cipher, Self::Kex => &mut options.kex, Self::Mac => &mut options.mac,
            Self::HostKey => &mut options.host_key, Self::Compression => &mut options.compression,
        }
    }
}

pub(super) fn tab() -> EditorTab {
    let choices = [("cipher", t("Ciphers")), ("kex", t("Key exchange")), ("mac", t("HMAC")),
        ("host-key", t("Host keys")), ("compression", t("Compression"))]
        .into_iter().map(|(id, label)| (id.into(), label.into())).collect();
    EditorTab::new("Algorithms", vec![Field::AlgorithmCategory.choice("cipher", choices, false)])
}

impl EditorField {
    pub(crate) fn checked(&self) -> Option<bool> {
        (self.id == Field::AlgorithmCheck).then(|| self.value == "true")
    }
}

impl ProfileEditor {
    pub(in crate::profiles_page) fn set_algorithm_catalog(&mut self, catalog: AlgorithmCatalog) {
        if self.kind != "ssh" { return; }
        self.algorithm_catalog = catalog;
        self.populate_algorithm_fields();
    }

    pub(super) fn populate_algorithm_fields(&mut self) {
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.fields.first().is_some_and(|field| field.id == Field::AlgorithmCategory)) else { return };
        let category = Category::from_value(&tab.fields[0].value);
        let defaults = category.names(&self.algorithm_catalog.defaults);
        let selected = category.field(&mut self.algorithm_options).as_deref().filter(|names| !names.is_empty()).unwrap_or(defaults);
        let mut names = selected.to_vec();
        for name in defaults.iter().chain(category.names(&self.algorithm_catalog.supported)) {
            if !names.contains(name) { names.push(name.clone()); }
        }
        tab.fields.truncate(1);
        tab.fields.extend(names.into_iter().map(|label| {
            let value = selected.contains(&label).to_string();
            EditorField { label, ..Field::AlgorithmCheck.choice(value, Vec::new(), false) }
        }));
    }

    pub(super) fn toggle_algorithm(&mut self) {
        let fields = &mut self.tabs[self.tab].fields;
        let checked = fields[self.focus].checked() == Some(true);
        if checked && fields.iter().filter(|field| field.checked() == Some(true)).count() == 1 { return; }
        fields[self.focus].value = (!checked).to_string();
        let category = Category::from_value(&fields[0].value);
        *category.field(&mut self.algorithm_options) = Some(fields.iter().filter(|field| field.checked() == Some(true)).map(|field| field.label.clone()).collect());
    }

    pub(super) fn ssh_algorithm_options(&self) -> Option<AlgorithmOptions> {
        let configured = self.source.ssh_options.as_ref().is_some_and(|s| s.algorithms.is_some());
        (configured || self.algorithm_options != AlgorithmOptions::default()).then(|| self.algorithm_options.clone())
    }
}
