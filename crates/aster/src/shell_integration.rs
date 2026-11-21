use aster_platform::{Platform, PlatformError};
use aster_core::jump_list::JumpList;
use crossbeam_channel::{unbounded, Sender};

enum Request {
    JumpList(JumpList),
    UrlScheme(bool),
    Shortcut { change: Option<bool>, complete: Box<dyn FnOnce(Result<bool, PlatformError>) + Send> },
    Explorer { open: &'static str, paste: &'static str, enabled: bool },
}

pub struct ShellIntegration(Sender<Request>);

impl ShellIntegration {
    pub fn start<P: Platform>() -> std::io::Result<Self> {
        let (tx, rx) = unbounded();
        std::thread::Builder::new().name("aster-shell-integration".into()).spawn(move || {
            for request in rx {
                match request {
                    Request::UrlScheme(enabled) => {
                        if !P::set_url_scheme(enabled) {
                            tracing::warn!("could not update the Aster URL scheme");
                        }
                    }
                    Request::JumpList(list) => {
                        if let Err(error) = P::set_jump_list(&list) {
                            tracing::warn!("jump list: {error}");
                        }
                    }
                    Request::Shortcut { change, complete } => complete(P::start_menu_shortcut(change)),
                    Request::Explorer { open, paste, enabled } => {
                        let opened = P::set_explorer_menu(open, enabled);
                        let pasted = P::set_explorer_paste_menu(paste, enabled);
                        if !opened || !pasted {
                            tracing::warn!("could not update the Explorer menu");
                        }
                    }
                }
            }
        })?;
        Ok(Self(tx))
    }

    pub fn url_scheme(&self, enabled: bool) {
        let _ = self.0.send(Request::UrlScheme(enabled));
    }

    pub fn jump_list(&self, list: JumpList) {
        let _ = self.0.send(Request::JumpList(list));
    }

    pub fn shortcut(&self, change: Option<bool>, complete: impl FnOnce(Result<bool, PlatformError>) + Send + 'static) {
        let _ = self.0.send(Request::Shortcut { change, complete: Box::new(complete) });
    }

    pub fn explorer(&self, open: &'static str, paste: &'static str, enabled: bool) {
        let _ = self.0.send(Request::Explorer { open, paste, enabled });
    }
}
