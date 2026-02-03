prpr_l10n::tl_file!("multiplayer" mtl);

mod panel;
mod srv_resolver;

pub use panel::MPPanel;
pub use srv_resolver::resolve_server_address;
