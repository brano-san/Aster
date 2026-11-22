mod algorithms;
pub use algorithms::catalog as algorithm_catalog;
pub mod client;
pub mod config;
pub mod console;
pub mod destination;
pub mod secrets;
pub mod sftp;
pub mod sftp_tree;
pub mod socks;
pub mod forward;
mod proxy;
pub(crate) mod x11;

#[cfg(test)]
#[path = "../test/server.rs"]
mod test_server;
