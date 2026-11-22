use super::editor::SshEditorAction;
use crate::app::{App, Effect};
use crate::i18n::t;
use crate::overlay::{Overlay, OverlayKind, Pick};
use crate::profile::effective;
use aster_proto::ssh::{PRIVATE_KEY_KIND, VAULT_IDENTITY_PREFIX};
use std::path::PathBuf;

impl App {
    pub(super) fn ssh_editor_action(&mut self, action: SshEditorAction) {
        match action {
            SshEditorAction::BrowseKey => return self.browse_ssh_keys(None, false),
            SshEditorAction::ImportKey | SshEditorAction::VaultKey if !self.vault.unlocked => {
                return self.ssh_editor_error(t("Unlock the vault to use private keys"));
            }
            SshEditorAction::ImportKey => return self.browse_ssh_keys(None, true),
            SshEditorAction::VaultKey => return self.browse_vault_keys(),
            _ => {}
        }
        let Some(editor) = self.settings.as_mut().and_then(|page| page.editor.as_mut()) else { return };
        let profile = match editor.profile_result() {
            Ok(profile) => effective(&profile, &self.config),
            Err(error) => { editor.error = Some(error); self.dirty = true; return; }
        };
        let Some(destination) = profile.ssh else { return };
        let value = if action == SshEditorAction::SetPassword {
            let Some(password) = editor.take_ssh_password() else {
                editor.error = Some(t("Enter the password to save it").into());
                self.dirty = true;
                return;
            };
            Some(password)
        } else { None };
        editor.error = None;
        self.ssh_editor_request = self.ssh_editor_request.wrapping_add(1);
        self.push(Effect::SshPassword { request: self.ssh_editor_request, destination, options: Box::new(profile.ssh_options.unwrap_or_default()), value });
        self.dirty = true;
    }

    pub fn ssh_password_updated(&mut self, request: u64, result: crate::ssh::PasswordUpdate) {
        use crate::ssh::PasswordUpdate;
        if request != self.ssh_editor_request { return; }
        let Some(page) = self.settings.as_mut().or(self.settings_stash.as_mut()) else { return };
        let Some(editor) = &mut page.editor else { return };
        match result {
            PasswordUpdate::Saved => page.notice = Some(t("SSH password saved").into()),
            PasswordUpdate::Forgotten => page.notice = Some(t("SSH password forgotten").into()),
            PasswordUpdate::MissingUser => editor.error = Some(t("Set a user before changing the saved password").into()),
            PasswordUpdate::Failed => editor.error = Some(t("Could not change the saved SSH password").into()),
        }
        self.dirty = true;
    }

    fn ssh_editor_error(&mut self, text: &str) {
        if let Some(editor) = self.settings.as_mut().or(self.settings_stash.as_mut()).and_then(|page| page.editor.as_mut()) {
            editor.error = Some(text.into());
        }
        self.dirty = true;
    }

    fn browse_vault_keys(&mut self) {
        self.ssh_editor_request = self.ssh_editor_request.wrapping_add(1);
        let picks = self.vault.entries.iter().filter(|entry| entry.kind == PRIVATE_KEY_KIND).map(|entry| {
            let id = entry.key.strip_prefix("key-file:").unwrap_or(&entry.key);
            Pick { label: entry.label.clone(), value: format!("{VAULT_IDENTITY_PREFIX}{id}"), ..Default::default() }
        }).collect();
        self.overlay = Some(Overlay::new(OverlayKind::SshKeys(self.ssh_editor_request), picks).opened(self.clock()));
        self.dirty = true;
    }

    fn browse_ssh_keys(&mut self, directory: Option<PathBuf>, import: bool) {
        self.ssh_editor_request = self.ssh_editor_request.wrapping_add(1);
        let request = self.ssh_editor_request;
        self.overlay = Some(Overlay::new(OverlayKind::SshKeys(request), Vec::new()).opened(self.clock()));
        self.push(Effect::BrowseSshKeys { request, directory, import });
        self.dirty = true;
    }

    pub fn ssh_key_files(&mut self, request: u64, import: bool, result: Option<(PathBuf, Vec<(String, bool)>)>) {
        let kind = OverlayKind::SshKeys(request);
        let Some(opened) = self.overlay.as_ref().filter(|overlay| overlay.kind == kind).map(|overlay| overlay.opened_at) else { return };
        if let Some((dir, files)) = result {
            let (directory_action, file_action) = if import { ("import-dir", "import-file") } else { ("dir", "file") };
            let mut picks = Vec::new();
            if let Some(parent) = dir.parent().and_then(|p| p.to_str()) {
                picks.push(Pick { label: t("Parent folder").into(), hint: parent.into(), value: format!("{directory_action}:{parent}"), ..Default::default() });
            }
            picks.extend(files.into_iter().filter_map(|(name, is_dir)| {
                let path = dir.join(&name).into_os_string().into_string().ok()?;
                let (action, label) = if is_dir { (directory_action, format!("{name}/")) } else { (file_action, name) };
                Some(Pick { label, hint: path.clone(), value: format!("{action}:{path}"), ..Default::default() })
            }));
            self.overlay = Some(Overlay::new(kind, picks).opened(opened));
        } else {
            self.overlay = None;
            self.ssh_editor_error(t("Could not read the key folder"));
        }
        self.dirty = true;
    }

    pub(crate) fn choose_ssh_key(&mut self, value: String) {
        let Some((action, path)) = value.split_once(':') else { return };
        match action {
            "dir" | "import-dir" => self.browse_ssh_keys(Some(PathBuf::from(path)), action == "import-dir"),
            "import-file" => {
                self.ssh_editor_request = self.ssh_editor_request.wrapping_add(1);
                self.push(Effect::ImportSshIdentity { request: self.ssh_editor_request, path: PathBuf::from(path) });
            }
            "file" => self.add_ssh_key(path.to_owned()),
            "vault" => self.add_ssh_key(value),
            _ => {}
        }
    }

    pub fn ssh_identity_imported(&mut self, request: u64, id: Option<String>) {
        if request != self.ssh_editor_request { return; }
        if let Some(id) = id {
            self.add_ssh_key(format!("{VAULT_IDENTITY_PREFIX}{id}"));
        } else {
            self.ssh_editor_error(t("Could not import a private key into the vault"));
        }
        self.dirty = true;
    }

    fn add_ssh_key(&mut self, path: String) {
        if let Some(editor) = self.settings.as_mut().or(self.settings_stash.as_mut()).and_then(|page| page.editor.as_mut()) {
            editor.add_ssh_identity(path);
        }
    }
}
