use crate::i18n::{t, tf};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum State {
    #[default]
    Unavailable,
    Available,
    Installed,
}

pub fn usage() -> &'static str {
    t("Usage: aster --install [--silent] | --uninstall [--silent]")
}

pub fn missing_environment_variable(name: &str) -> String {
    tf("Environment variable {name} is unavailable.", &[("name", name)])
}

pub fn confirmation(install: bool) -> &'static str {
    if install { t("Install Aster for this user?") } else { t("Uninstall Aster for this user?") }
}

pub fn delete_settings() -> &'static str {
    t("Also delete settings and other Aster data for this user?")
}

pub fn result(install: bool) -> &'static str {
    if install { t("Aster was installed for this user.") } else { t("Aster was unregistered. Its program files will be removed after exit.") }
}

pub fn failed(install: bool, error: &str) -> String {
    if install {
        tf("Could not install Aster: {error}", &[("error", error)])
    } else {
        tf("Could not uninstall Aster: {error}", &[("error", error)])
    }
}

pub fn integration_error() -> &'static str {
    t("Some Windows integration entries could not be removed.")
}

pub fn quit_timeout() -> &'static str {
    t("Aster did not exit within 10 seconds.")
}
