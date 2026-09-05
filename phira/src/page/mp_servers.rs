prpr_l10n::tl_file!("settings");

use super::{Page, SharedState};
use crate::{get_data, get_data_mut, popup::Popup, save_data, scene::confirm_dialog};
use anyhow::Result;
use inputbox::InputBox;
use macroquad::prelude::*;
use prpr::{
    ext::{semi_black, semi_white, RectExt},
    scene::{request_input, return_input, show_error, show_message, take_input, take_input_cancelled},
    ui::{DRectButton, Scroll, Ui},
};
use std::{
    borrow::Cow,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

const INPUT_ID: &str = "mp_server_input";
const ROW_HEIGHT: f32 = 0.14;
const ROW_GAP: f32 = 0.014;

fn validate_address(address: &str) -> Result<()> {
    let authority = address.parse::<http::uri::Authority>()?;
    if authority.port_u16().is_some_and(|port| port != 0) {
        Ok(())
    } else {
        anyhow::bail!("server address must include a nonzero port")
    }
}

struct ServerButtons {
    select: DRectButton,
    menu: DRectButton,
}

impl ServerButtons {
    fn new() -> Self {
        Self {
            select: DRectButton::new(),
            menu: DRectButton::new(),
        }
    }
}

enum PendingInput {
    AddName,
    AddAddress(String),
    Rename(usize),
    EditAddress(usize),
}

#[derive(Clone, Copy)]
enum ServerAction {
    Rename,
    EditAddress,
    Delete,
}

pub struct MpServerPage {
    scroll: Scroll,
    add_btn: DRectButton,
    server_btns: Vec<ServerButtons>,

    actions_menu: Popup,
    actions: Vec<ServerAction>,
    actions_server: Option<usize>,
    need_show_actions: bool,

    pending_input: Option<PendingInput>,
    delete_server: Option<usize>,
    should_delete: Arc<AtomicBool>,
}

impl MpServerPage {
    pub fn new() -> Self {
        Self {
            scroll: Scroll::new(),
            add_btn: DRectButton::new(),
            server_btns: Vec::new(),

            actions_menu: Popup::new().with_size(0.45),
            actions: Vec::new(),
            actions_server: None,
            need_show_actions: false,

            pending_input: None,
            delete_server: None,
            should_delete: Arc::default(),
        }
    }

    fn sync_server_buttons(&mut self) {
        let len = get_data().config.mp_servers.len();
        self.server_btns.truncate(len);
        while self.server_btns.len() < len {
            self.server_btns.push(ServerButtons::new());
        }
    }

    fn request_input(&mut self, pending: PendingInput, input: InputBox) {
        self.pending_input = Some(pending);
        request_input(INPUT_ID, input);
    }

    fn address_exists(address: &str, except: Option<usize>) -> bool {
        get_data()
            .config
            .mp_servers
            .iter()
            .enumerate()
            .any(|(index, server)| Some(index) != except && server.address == address)
    }

    fn handle_input(&mut self, pending: PendingInput, text: String) -> Result<()> {
        match pending {
            PendingInput::AddName => {
                let name = text.trim();
                if name.is_empty() {
                    show_message(tl!("mp-server-name-empty")).error();
                } else {
                    self.request_input(
                        PendingInput::AddAddress(name.to_owned()),
                        InputBox::new()
                            .title(tl!("mp-server-address-title"))
                            .prompt(tl!("mp-server-address-prompt")),
                    );
                }
            }
            PendingInput::AddAddress(name) => {
                let address = text.trim();
                if let Err(err) = validate_address(address) {
                    show_error(err.context(tl!("item-mp-addr-invalid")));
                } else if Self::address_exists(address, None) {
                    show_message(tl!("mp-server-address-duplicate")).error();
                } else {
                    get_data_mut().config.mp_servers.push(prpr::config::MpServer {
                        name,
                        address: address.to_owned(),
                    });
                    save_data()?;
                    show_message(tl!("mp-server-added")).ok();
                }
            }
            PendingInput::Rename(index) => {
                let name = text.trim();
                if name.is_empty() {
                    show_message(tl!("mp-server-name-empty")).error();
                } else if let Some(server) = get_data_mut().config.mp_servers.get_mut(index) {
                    server.name = name.to_owned();
                    save_data()?;
                    show_message(tl!("mp-server-name-updated")).ok();
                }
            }
            PendingInput::EditAddress(index) => {
                let address = text.trim();
                if let Err(err) = validate_address(address) {
                    show_error(err.context(tl!("item-mp-addr-invalid")));
                } else if Self::address_exists(address, Some(index)) {
                    show_message(tl!("mp-server-address-duplicate")).error();
                } else if let Some(old_address) = get_data().config.mp_servers.get(index).map(|server| server.address.clone()) {
                    let was_active = get_data().config.mp_address == old_address;
                    let server = &mut get_data_mut().config.mp_servers[index];
                    server.address = address.to_owned();
                    if was_active {
                        get_data_mut().config.mp_address = address.to_owned();
                    }
                    save_data()?;
                    show_message(tl!("mp-server-address-updated")).ok();
                }
            }
        }
        Ok(())
    }

    fn delete_server(&mut self, index: usize) -> Result<()> {
        let config = &mut get_data_mut().config;
        if index >= config.mp_servers.len() {
            return Ok(());
        }
        let was_active = config.mp_address == config.mp_servers[index].address;
        config.mp_servers.remove(index);
        if was_active {
            config.mp_address = config
                .mp_servers
                .get(index)
                .or_else(|| config.mp_servers.last())
                .map(|server| server.address.clone())
                .unwrap_or_default();
        }
        save_data()?;
        show_message(tl!("mp-server-deleted")).ok();
        Ok(())
    }

    fn open_actions(&mut self, index: usize) {
        self.actions = vec![ServerAction::Rename, ServerAction::EditAddress, ServerAction::Delete];
        self.actions_menu.set_selected(usize::MAX);
        self.actions_menu.set_options(
            self.actions
                .iter()
                .map(|action| match action {
                    ServerAction::Rename => tl!("mp-server-rename").into_owned(),
                    ServerAction::EditAddress => tl!("mp-server-edit-address").into_owned(),
                    ServerAction::Delete => tl!("mp-server-delete").into_owned(),
                })
                .collect(),
        );
        self.actions_server = Some(index);
        self.need_show_actions = true;
    }

    fn start_selected_action(&mut self) {
        let Some(index) = self.actions_server else {
            return;
        };
        let Some(action) = self.actions.get(self.actions_menu.selected()).copied() else {
            return;
        };
        let Some(server) = get_data().config.mp_servers.get(index) else {
            return;
        };

        match action {
            ServerAction::Rename => self.request_input(
                PendingInput::Rename(index),
                InputBox::new()
                    .title(tl!("mp-server-name-title"))
                    .prompt(tl!("mp-server-name-prompt"))
                    .default_text(&server.name),
            ),
            ServerAction::EditAddress => self.request_input(
                PendingInput::EditAddress(index),
                InputBox::new()
                    .title(tl!("mp-server-address-title"))
                    .prompt(tl!("mp-server-address-prompt"))
                    .default_text(&server.address),
            ),
            ServerAction::Delete => {
                self.delete_server = Some(index);
                confirm_dialog(tl!("mp-server-delete"), tl!("mp-server-delete-confirm"), Arc::clone(&self.should_delete));
            }
        }
    }
}

impl Page for MpServerPage {
    fn label(&self) -> Cow<'static, str> {
        tl!("mp-server-page")
    }

    fn touch(&mut self, touch: &Touch, s: &mut SharedState) -> Result<bool> {
        let t = s.t;
        if self.actions_menu.showing() {
            self.actions_menu.touch(touch, t);
            return Ok(true);
        }
        if self.scroll.touch(touch, t) {
            return Ok(true);
        }
        if self.add_btn.touch(touch, t) {
            self.request_input(PendingInput::AddName, InputBox::new().title(tl!("mp-server-name-title")).prompt(tl!("mp-server-name-prompt")));
            return Ok(true);
        }
        let mut actions_index = None;
        for (index, buttons) in self.server_btns.iter_mut().enumerate() {
            if buttons.menu.touch(touch, t) {
                actions_index = Some(index);
                break;
            }
            if buttons.select.touch(touch, t) {
                if let Some(server) = get_data().config.mp_servers.get(index) {
                    get_data_mut().config.mp_address = server.address.clone();
                    save_data()?;
                    show_message(tl!("mp-server-selected")).ok();
                }
                return Ok(true);
            }
        }
        if let Some(index) = actions_index {
            self.open_actions(index);
            return Ok(true);
        }
        Ok(false)
    }

    fn update(&mut self, s: &mut SharedState) -> Result<()> {
        self.scroll.update(s.t);
        self.actions_menu.update(s.t);

        if self.actions_menu.changed() {
            self.start_selected_action();
        }
        if let Some((id, text)) = take_input() {
            if id == INPUT_ID {
                if let Some(pending) = self.pending_input.take() {
                    self.handle_input(pending, text)?;
                }
            } else {
                return_input(id, text);
            }
        }
        if take_input_cancelled().as_deref() == Some(INPUT_ID) {
            self.pending_input = None;
        }
        if self.should_delete.fetch_and(false, Ordering::SeqCst) {
            if let Some(index) = self.delete_server.take() {
                self.delete_server(index)?;
            }
        }
        self.sync_server_buttons();
        Ok(())
    }

    fn render(&mut self, ui: &mut Ui, s: &mut SharedState) -> Result<()> {
        // The first frame can render before `update` has initialized these controls.
        self.sync_server_buttons();
        let t = s.t;
        let panel = ui.content_rect().feather(-0.04);
        s.render_fader(ui, |ui| {
            ui.fill_path(&panel.rounded(0.01), semi_black(0.45));
            ui.text(tl!("mp-server-page"))
                .pos(panel.x + 0.03, panel.y + 0.045)
                .anchor(0., 0.5)
                .no_baseline()
                .size(0.7)
                .draw();

            let add_rect = Rect::new(panel.right() - 0.14, panel.y + 0.015, 0.11, 0.06);
            self.add_btn.render_text(ui, add_rect, t, "+", 0.7, false);

            let list = Rect::new(panel.x + 0.025, panel.y + 0.095, panel.w - 0.05, panel.h - 0.12);
            self.scroll.size((list.w, list.h));
            ui.scope(|ui| {
                ui.dx(list.x);
                ui.dy(list.y);
                self.scroll.render(ui, |ui| {
                    let servers = &get_data().config.mp_servers;
                    if servers.is_empty() {
                        ui.text(tl!("mp-server-none"))
                            .pos(list.w / 2., 0.08)
                            .anchor(0.5, 0.5)
                            .no_baseline()
                            .size(0.45)
                            .color(semi_white(0.65))
                            .draw();
                        (list.w, list.h)
                    } else {
                        let active_address = get_data().config.mp_address.clone();
                        let mut y = 0.;
                        for (index, (server, buttons)) in servers.iter().zip(self.server_btns.iter_mut()).enumerate() {
                            let main = Rect::new(0., y, list.w - 0.105, ROW_HEIGHT);
                            let menu = Rect::new(main.right() + ROW_GAP, y, 0.091, ROW_HEIGHT);
                            let active = active_address == server.address;
                            let name = &server.name;
                            let address = &server.address;
                            buttons.select.render_shadow(ui, main, t, |ui, path| {
                                ui.fill_path(&path, if active { WHITE } else { semi_black(0.35) });
                                let color = if active { Color::new(0.3, 0.3, 0.3, 1.) } else { WHITE };
                                ui.text(name)
                                    .pos(main.x + 0.02, main.y + 0.045)
                                    .anchor(0., 0.5)
                                    .no_baseline()
                                    .max_width(main.w - 0.2)
                                    .size(0.46)
                                    .color(color)
                                    .draw();
                                ui.text(address)
                                    .pos(main.x + 0.02, main.y + 0.105)
                                    .anchor(0., 0.5)
                                    .no_baseline()
                                    .max_width(main.w - 0.04)
                                    .size(0.3)
                                    .color(if active { Color::new(0.3, 0.3, 0.3, 0.75) } else { semi_white(0.65) })
                                    .draw();
                                if active {
                                    ui.text(tl!("mp-server-current"))
                                        .pos(main.right() - 0.02, main.y + 0.045)
                                        .anchor(1., 0.5)
                                        .no_baseline()
                                        .size(0.27)
                                        .color(Color::new(0.3, 0.3, 0.3, 0.8))
                                        .draw();
                                }
                            });
                            buttons.menu.render_text(ui, menu, t, "...", 0.46, false);
                            if self.need_show_actions && self.actions_server == Some(index) {
                                self.actions_menu.set_auto_adjust(Some(ui.screen_rect().nonuniform_feather(-0.03, -0.05)));
                                self.actions_menu
                                    .show(ui, t, Rect::new(menu.right() - 0.45, menu.bottom() + 0.01, 0.45, 0.3));
                                self.need_show_actions = false;
                            }
                            y += ROW_HEIGHT + ROW_GAP;
                        }
                        (list.w, y)
                    }
                });
            });
        });
        self.actions_menu.render(ui, t, 1.);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::validate_address;

    #[test]
    fn multiplayer_server_address_requires_a_nonzero_port() {
        assert!(validate_address("mp2.phira.cn:12345").is_ok());
        assert!(validate_address("[::1]:12345").is_ok());
        assert!(validate_address("1").is_err());
        assert!(validate_address("example.com").is_err());
        assert!(validate_address("example.com:0").is_err());
        assert!(validate_address("example.com:port").is_err());
    }
}
