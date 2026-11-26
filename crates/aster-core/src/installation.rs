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

pub fn delete_settings() -> &'static str {
    t("Also delete Aster settings and user data")
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

pub fn invalid_directory() -> &'static str {
    t("Choose an installation folder on a local drive.")
}

pub fn existing_installation() -> &'static str {
    t("Aster is already installed in another folder. Run Setup again to use that folder.")
}

pub fn portable_directory() -> &'static str {
    t("Choose a folder outside the portable copy or the Aster settings folder.")
}

pub fn not_installed() -> &'static str {
    t("Aster is not installed for this user.")
}

pub fn welcome(version: &str, installed: bool) -> String {
    if installed {
        tf("Continue to install Aster {version} in the existing installation folder.", &[("version", version)])
    } else {
        tf("This wizard installs Aster {version} for your Windows account.", &[("version", version)])
    }
}

#[derive(Clone, Copy)]
pub enum WizardLabel {
    Setup, Uninstall, Welcome, Options, ChooseOptions, UpgradeOptions, Folder, Browse, StartMenu, Desktop,
    KeepData, Installing, Removing, ReplacingFiles, RemovingIntegration, Installed, Uninstalled,
    Launch, Error, Back, Next, Install, Remove, Finish, Cancel, Retry,
}

impl WizardLabel {
    pub fn text(self) -> &'static str {
        match self {
            Self::Setup => t("Aster Setup"),
            Self::Uninstall => t("Remove Aster"),
            Self::Welcome => t("Install Aster"),
            Self::Options => t("Installation options"),
            Self::ChooseOptions => t("Choose the installation folder and the shortcuts you want to create."),
            Self::UpgradeOptions => t("Setup will use the existing installation folder. You can change the shortcuts."),
            Self::Folder => t("Installation folder"),
            Self::Browse => t("Browse…"),
            Self::StartMenu => t("Create a Start menu shortcut"),
            Self::Desktop => t("Create a desktop shortcut"),
            Self::KeepData => t("Your profiles and settings will be kept unless you select the option below."),
            Self::Installing => t("Installing Aster…"),
            Self::Removing => t("Removing Aster…"),
            Self::ReplacingFiles => t("Aster may close while Setup replaces its files."),
            Self::RemovingIntegration => t("Removing Aster shortcuts and Windows integration."),
            Self::Installed => t("Installation complete"),
            Self::Uninstalled => t("Uninstall complete"),
            Self::Launch => t("Launch Aster"),
            Self::Error => t("Setup could not finish"),
            Self::Back => t("Back"),
            Self::Next => t("Next"),
            Self::Install => t("Install"),
            Self::Remove => t("Uninstall"),
            Self::Finish => t("Finish"),
            Self::Cancel => t("Cancel"),
            Self::Retry => t("Try again"),
        }
    }
}
