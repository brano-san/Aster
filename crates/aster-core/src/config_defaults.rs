use crate::config::{Config, Terminal};
use toml::{Table, Value};

pub fn apply_os_defaults(terminal: &mut Terminal) {
    if cfg!(windows) {
        terminal.font = "Cascadia Mono".into();
    }
}

pub fn overrides(config: &Config) -> Result<Table, toml::ser::Error> {
    let mut settings = Table::try_from(config)?;
    let defaults = Table::try_from(Config::default())?;
    without_defaults(&mut settings, &defaults);
    Ok(settings)
}

fn without_defaults(settings: &mut Table, defaults: &Table) {
    settings.retain(|key, setting| {
        let Some(default) = defaults.get(key) else { return true };
        match (setting, default) {
            (Value::Table(settings), Value::Table(defaults)) => {
                without_defaults(settings, defaults);
                !settings.is_empty()
            }
            (setting, default) => setting != default,
        }
    });
}
