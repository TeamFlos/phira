prpr_l10n::tl_file!("login");

use crate::{
    client::{Client, LoginParams, User, UserManager, API_URL},
    get_data_mut,
    icons::Icons,
    page::Fader,
    save_data,
    scene::{check_read_tos_and_policy, dispatch_tos_task, JUST_ACCEPTED_TOS},
};
use anyhow::Result;
use inputbox::{InputBox, InputMode};
use macroquad::prelude::*;
use once_cell::sync::Lazy;
#[cfg(feature = "aa")]
use prpr::ext::ScaleType;
use prpr::{
    core::BOLD_FONT,
    ext::{open_url, semi_black, semi_white, RectExt},
    scene::{request_input, return_input, show_error, show_message, take_input},
    task::Task,
    ui::{button_hit, DRectButton, Dialog, RectButton, Ui},
};
use regex::Regex;
use std::{borrow::Cow, future::Future, sync::atomic::Ordering, sync::Arc};

#[cfg(feature = "aa")]
use crate::{client::HykbLoginOutcome, obtain_hykb_credential};
#[cfg(feature = "aa")]
use anyhow::bail;
#[cfg(feature = "aa")]
use std::sync::Mutex;

/// The user's choice in the HYKB "register or claim" dialog.
#[cfg(feature = "aa")]
#[derive(Clone, Copy)]
enum HykbChoice {
    Register,
    Claim,
}

/// Result of the initial HYKB login step.
#[cfg(feature = "aa")]
enum HykbStep {
    /// The HYKB account was already bound; carries the fetched user.
    LoggedIn(Box<User>),
    /// The account is new; carries the short-lived token for register/claim.
    NeedChoice(String),
}

static EMAIL_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"\A[a-z0-9!#$%&'*+/=?^_‘{|}~-]+(?:\.[a-z0-9!#$%&'*+/=?^_‘{|}~-]+)*@(?:[a-z0-9](?:[a-z0-9-]*[a-z0-9])?\.)+[a-z0-9](?:[a-z0-9-]*[a-z0-9])?\z",
    )
    .unwrap()
});

fn validate_username(username: &str) -> Option<Cow<'static, str>> {
    if !(4..=12).contains(&username.chars().count()) {
        return Some(tl!("name-length-req"));
    }
    if username.chars().any(|it| it != '_' && it != '-' && !it.is_alphanumeric()) {
        return Some(tl!("name-has-illegal-char"));
    }
    None
}

pub struct Login {
    #[cfg(feature = "aa")]
    icons: Arc<Icons>,

    fader: Fader,
    show: bool,

    /// The method-choice panel ("email vs HYKB"), shown before the form when
    /// HYKB is available.
    #[cfg(feature = "aa")]
    picker_fader: Fader,
    #[cfg(feature = "aa")]
    picker_show: bool,
    #[cfg(feature = "aa")]
    btn_method_email: DRectButton,
    #[cfg(feature = "aa")]
    btn_method_hykb: DRectButton,

    input_email: DRectButton,
    input_pwd: DRectButton,
    input_reg_email: DRectButton,
    input_reg_name: DRectButton,
    input_reg_pwd: DRectButton,

    btn_to_reg: DRectButton,
    btn_to_login: DRectButton,
    btn_reg: DRectButton,
    btn_login: DRectButton,
    btn_forget_pwd: RectButton,

    t_email: String,
    t_pwd: String,
    t_reg_email: String,
    t_reg_name: String,
    t_reg_pwd: String,

    start_time: f32,
    in_reg: bool,

    task: Option<(&'static str, Task<Result<Option<User>>>)>,
    after_accept_tos: Option<NextAction>,
    /// HYKB login phase 1 (verify uid/token), distinct from `task` which handles
    /// the register/claim follow-up that resolves to a `User`.
    #[cfg(feature = "aa")]
    hykb_task: Option<Task<Result<HykbStep>>>,
    /// Pending HYKB token awaiting the user's register/claim choice.
    #[cfg(feature = "aa")]
    hykb_pending_token: Option<String>,
    /// Choice written by the register/claim dialog listener.
    #[cfg(feature = "aa")]
    hykb_choice: Arc<Mutex<Option<HykbChoice>>>,
}

enum NextAction {
    Login,
    Register,
    #[cfg(feature = "aa")]
    Hykb,
}

impl Login {
    const TIME: f32 = 0.7;

    pub fn new(icons: Arc<Icons>) -> Self {
        #[cfg(not(feature = "aa"))]
        let _ = icons;
        Self {
            #[cfg(feature = "aa")]
            icons,

            fader: Fader::new().with_distance(-0.4).with_time(0.5),
            show: false,

            #[cfg(feature = "aa")]
            picker_fader: Fader::new().with_distance(-0.4).with_time(0.5),
            #[cfg(feature = "aa")]
            picker_show: false,
            #[cfg(feature = "aa")]
            btn_method_email: DRectButton::new().with_radius(0.012).with_elevation(0.004),
            #[cfg(feature = "aa")]
            btn_method_hykb: DRectButton::new().with_radius(0.012).with_elevation(0.004),

            input_email: DRectButton::new().with_delta(-0.002),
            input_pwd: DRectButton::new().with_delta(-0.002),
            input_reg_email: DRectButton::new().with_delta(-0.002),
            input_reg_name: DRectButton::new().with_delta(-0.002),
            input_reg_pwd: DRectButton::new().with_delta(-0.002),

            btn_to_reg: DRectButton::new(),
            btn_to_login: DRectButton::new(),
            btn_reg: DRectButton::new(),
            btn_login: DRectButton::new(),
            btn_forget_pwd: RectButton::new(),

            t_email: String::new(),
            t_pwd: String::new(),
            t_reg_email: String::new(),
            t_reg_name: String::new(),
            t_reg_pwd: String::new(),

            start_time: f32::NAN,
            in_reg: false,

            task: None,
            after_accept_tos: None,
            #[cfg(feature = "aa")]
            hykb_task: None,
            #[cfg(feature = "aa")]
            hykb_pending_token: None,
            #[cfg(feature = "aa")]
            hykb_choice: Arc::new(Mutex::new(None)),
        }
    }

    #[inline]
    fn start(&mut self, desc: &'static str, future: impl Future<Output = Result<Option<User>>> + Send + 'static) {
        self.task = Some((desc, Task::new(future)));
    }

    pub fn enter(&mut self, t: f32) {
        // With HYKB available, show the method-choice panel before any form;
        // otherwise go straight to the email form.
        #[cfg(feature = "aa")]
        {
            self.picker_show = true;
            self.picker_fader.sub(t);
        }
        #[cfg(not(feature = "aa"))]
        self.show_form(t);
    }

    /// Reveal the email login/register form.
    fn show_form(&mut self, t: f32) {
        self.fader.sub(t);
    }

    /// Dismiss the method-choice panel.
    #[cfg(feature = "aa")]
    fn dismiss_picker(&mut self, t: f32) {
        self.picker_show = false;
        self.picker_fader.back(t);
    }

    pub fn dismiss(&mut self, t: f32) {
        self.show = false;
        self.fader.back(t);
        // Drop any pending claim so a later plain login isn't treated as a claim.
        #[cfg(feature = "aa")]
        {
            self.hykb_pending_token = None;
        }
    }

    fn register(&mut self) -> Option<Cow<'static, str>> {
        let email = self.t_reg_email.clone();
        let name = self.t_reg_name.clone();
        let pwd = self.t_reg_pwd.clone();
        if let Some(error) = validate_username(&name) {
            show_message(error).error();
        }
        if !EMAIL_REGEX.is_match(&email) {
            return Some(tl!("illegal-email"));
        }
        if !(8..=32).contains(&pwd.len()) {
            return Some(tl!("pwd-length-req"));
        }
        self.start("register", async move {
            Client::register(&email, &name, &pwd).await?;
            Ok(None)
        });
        None
    }

    /// Kick off the native HYKB login: obtain credentials, then call `/login/hykb`.
    #[cfg(feature = "aa")]
    fn start_hykb_login(&mut self) {
        self.hykb_task = Some(Task::new(async move {
            let cred = obtain_hykb_credential().await?;
            if cred.code != 0 {
                bail!(tl!("hykb-login-cancelled"));
            }
            match Client::login_hykb(cred.uid, &cred.access_token, &cred.nick).await? {
                HykbLoginOutcome::LoggedIn => Ok(HykbStep::LoggedIn(Box::new(Client::get_me().await?))),
                HykbLoginOutcome::NeedChoice { hykb_token } => Ok(HykbStep::NeedChoice(hykb_token)),
            }
        }));
    }

    /// Show the "register a new account / claim an existing one" dialog after a
    /// first-time HYKB login. The chosen action is recorded in `hykb_choice`.
    #[cfg(feature = "aa")]
    fn show_hykb_choice(&self) {
        let choice = Arc::clone(&self.hykb_choice);
        Dialog::plain(tl!("hykb-choice-title"), tl!("hykb-choice-sub"))
            .buttons(vec![tl!("hykb-choice-register").to_string(), tl!("hykb-choice-claim").to_string()])
            .listener(move |_, pos| {
                match pos {
                    -1 => {
                        return true;
                    }
                    0 => *choice.lock().unwrap() = Some(HykbChoice::Register),
                    1 => *choice.lock().unwrap() = Some(HykbChoice::Claim),
                    _ => {}
                }
                false
            })
            .show();
    }

    pub fn touch(&mut self, touch: &Touch, t: f32) -> bool {
        if self.fader.transiting() || self.task.is_some() || !self.start_time.is_nan() {
            return true;
        }
        #[cfg(feature = "aa")]
        if self.hykb_task.is_some() {
            return true;
        }
        // The method-choice panel sits on top of (and gates) the form.
        #[cfg(feature = "aa")]
        if self.picker_show {
            if self.picker_fader.transiting() {
                return true;
            }
            if !Self::picker_rect().contains(touch.position) && touch.phase == TouchPhase::Started {
                self.dismiss_picker(t);
                return true;
            }
            if self.btn_method_email.touch(touch, t) {
                self.dismiss_picker(t);
                self.show_form(t);
                return true;
            }
            if self.btn_method_hykb.touch(touch, t) {
                self.dismiss_picker(t);
                if !check_read_tos_and_policy(true, true) {
                    self.after_accept_tos = Some(NextAction::Hykb);
                } else {
                    self.start_hykb_login();
                }
                return true;
            }
            return true;
        }
        if self.show {
            if !Ui::dialog_rect().contains(touch.position) && touch.phase == TouchPhase::Started {
                self.dismiss(t);
                return true;
            }
            if self.input_email.touch(touch, t) {
                request_input("email", InputBox::new().default_text(&self.t_email));
                return true;
            }
            if self.input_pwd.touch(touch, t) {
                request_input("pwd", InputBox::new().default_text(&self.t_pwd).mode(InputMode::Password));
                return true;
            }
            if self.input_reg_email.touch(touch, t) {
                request_input("reg_email", InputBox::new().default_text(&self.t_reg_email));
                return true;
            }
            if self.input_reg_name.touch(touch, t) {
                request_input("reg_name", InputBox::new().default_text(&self.t_reg_name));
                return true;
            }
            if self.input_reg_pwd.touch(touch, t) {
                request_input("reg_pwd", InputBox::new().default_text(&self.t_reg_pwd).mode(InputMode::Password));
                return true;
            }
            if self.btn_to_reg.touch(touch, t) || self.btn_to_login.touch(touch, t) {
                self.start_time = t;
                return true;
            }
            if self.btn_reg.touch(touch, t) {
                if !check_read_tos_and_policy(true, true) {
                    self.after_accept_tos = Some(NextAction::Register);
                    return true;
                }
                if let Some(error) = self.register() {
                    show_message(error).error();
                }
                return true;
            }
            if self.btn_login.touch(touch, t) {
                // A pending HYKB claim already accepted TOS in the picker; only
                // gate a plain email login on the TOS check.
                #[cfg(feature = "aa")]
                let pending_claim = self.hykb_pending_token.is_some();
                #[cfg(not(feature = "aa"))]
                let pending_claim = false;
                if !pending_claim && !check_read_tos_and_policy(true, true) {
                    self.after_accept_tos = Some(NextAction::Login);
                    return true;
                }
                self.start_login();
                return true;
            }
            if self.btn_forget_pwd.touch(touch) {
                button_hit();
                let _ = open_url(&format!("{API_URL}/reset-password"));
            }
            return true;
        }
        false
    }

    /// Submit the email login form. When a HYKB token is pending (the user chose
    /// to claim an existing account), this claims it with the entered email and
    /// password instead of a plain password login.
    fn start_login(&mut self) {
        #[cfg(feature = "aa")]
        if let Some(token) = self.hykb_pending_token.clone() {
            let email = self.t_email.clone();
            let pwd = self.t_pwd.clone();
            if !EMAIL_REGEX.is_match(&email) || pwd.is_empty() {
                show_message(tl!("hykb-claim-need-cred")).error();
                return;
            }
            self.hykb_pending_token = None;
            self.start("hykb-login", async move {
                Client::login_hykb_claim(&token, &email, &pwd).await?;
                Ok(Some(Client::get_me().await?))
            });
            return;
        }
        let email = self.t_email.clone();
        let pwd = self.t_pwd.clone();
        self.start("login", async move {
            Client::login(LoginParams::Password {
                email: &email,
                password: &pwd,
            })
            .await?;
            Ok(Some(Client::get_me().await?))
        });
    }

    pub fn update(&mut self, t: f32) -> Result<()> {
        if let Some(done) = self.fader.done(t) {
            self.show = !done;
        }
        #[cfg(feature = "aa")]
        if let Some(done) = self.picker_fader.done(t) {
            self.picker_show = !done;
        }
        dispatch_tos_task();
        if let Some((id, text)) = take_input() {
            'tmp: {
                let tmp = match id.as_str() {
                    "email" => &mut self.t_email,
                    "pwd" => &mut self.t_pwd,
                    "reg_email" => &mut self.t_reg_email,
                    "reg_name" => &mut self.t_reg_name,
                    "reg_pwd" => &mut self.t_reg_pwd,
                    _ => {
                        return_input(id, text);
                        break 'tmp;
                    }
                };
                *tmp = text;
            }
        }
        if JUST_ACCEPTED_TOS.fetch_and(false, Ordering::Relaxed) {
            match self.after_accept_tos {
                Some(NextAction::Login) => {
                    self.start_login();
                }
                Some(NextAction::Register) => {
                    if let Some(error) = self.register() {
                        show_message(error).error();
                    }
                }
                #[cfg(feature = "aa")]
                Some(NextAction::Hykb) => {
                    self.start_hykb_login();
                }
                None => (),
            }
            self.after_accept_tos = None;
        }
        if let Some((action, task)) = &mut self.task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => show_error(err.context(tl!("action-failed", "action" => *action))),
                    Ok(user) => {
                        if let Some(user) = user {
                            UserManager::request(user.id);
                            get_data_mut().me = Some(user);
                            save_data()?;
                        }
                        self.t_pwd.clear();
                        show_message(tl!("action-success", "action" => *action)).ok();
                        if *action == "register" {
                            Dialog::simple(tl!("email-sent")).show();
                            self.t_reg_email.clear();
                            self.t_reg_name.clear();
                            self.t_reg_pwd.clear();
                            self.start_time = t;
                        }
                        if *action == "login" || *action == "hykb-login" {
                            self.dismiss(t);
                        }
                    }
                }
                self.task = None;
            }
        }
        #[cfg(feature = "aa")]
        self.update_hykb(t)?;
        Ok(())
    }

    /// Drive the HYKB login phases: the initial verify task, the register/claim
    /// choice dialog, and the follow-up that resolves to a logged-in user.
    #[cfg(feature = "aa")]
    fn update_hykb(&mut self, t: f32) -> Result<()> {
        if let Some(task) = &mut self.hykb_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => show_error(err.context(tl!("action-failed", "action" => "hykb-login"))),
                    Ok(HykbStep::LoggedIn(user)) => {
                        UserManager::request(user.id);
                        get_data_mut().me = Some(*user);
                        save_data()?;
                        show_message(tl!("action-success", "action" => "hykb-login")).ok();
                        self.dismiss(t);
                    }
                    Ok(HykbStep::NeedChoice(token)) => {
                        self.hykb_pending_token = Some(token);
                        self.show_hykb_choice();
                    }
                }
                self.hykb_task = None;
            }
        }
        // The choice dialog records its result here; pick it up and start the
        // matching follow-up request, reusing `task` so the success path is shared.
        let choice = self.hykb_choice.lock().unwrap().take();
        if let Some(choice) = choice {
            if let Some(token) = self.hykb_pending_token.take() {
                match choice {
                    HykbChoice::Register => {
                        self.start("hykb-login", async move {
                            Client::login_hykb_register(&token).await?;
                            Ok(Some(Client::get_me().await?))
                        });
                    }
                    HykbChoice::Claim => {
                        // Reveal the email form so the user can enter the
                        // credentials of the account they want to claim. The
                        // pending token is kept; the login button submits the
                        // claim while it is set.
                        self.hykb_pending_token = Some(token);
                        self.show_form(t);
                        show_message(tl!("hykb-claim-need-cred")).ok();
                    }
                }
            }
        }
        Ok(())
    }

    pub fn render(&mut self, ui: &mut Ui, t: f32) {
        self.fader.reset();
        if self.show || self.fader.transiting() {
            let p = if self.show { 1. } else { -self.fader.progress(t) };
            ui.fill_rect(ui.screen_rect(), semi_black(p * 0.7));
            self.fader.for_sub(|f| {
                f.render(ui, t, |ui| {
                    let mut wr = Ui::dialog_rect();
                    wr.y -= 0.03;
                    wr.h += 0.06;
                    ui.fill_path(&wr.rounded(0.01), ui.background());
                    ui.scissor(wr, |ui| {
                        let p = (if self.start_time.is_nan() {
                            if self.in_reg {
                                0.
                            } else {
                                -1.
                            }
                        } else {
                            let p = ((t - self.start_time) / Self::TIME).clamp(0., 1.);
                            let p = 1. - (1. - p).powi(3);
                            let res = if self.in_reg { -p } else { p - 1. };
                            if p >= 1. {
                                self.in_reg = !self.in_reg;
                                self.start_time = f32::NAN;
                            }
                            res
                        }) * wr.h;
                        ui.dy(p);

                        let r = ui.text(tl!("register")).pos(wr.x + 0.045, wr.y + 0.037).size(1.1).draw_using(&BOLD_FONT);
                        let pad = 0.035;
                        let mut r = Rect::new(wr.x + pad, r.bottom() + 0.05, wr.w - pad * 2., 0.1);
                        self.input_reg_email.render_input(ui, r, t, &self.t_reg_email, tl!("email"), 0.62);
                        r.y += r.h + 0.02;
                        self.input_reg_name.render_input(ui, r, t, &self.t_reg_name, tl!("username"), 0.62);
                        r.y += r.h + 0.02;
                        self.input_reg_pwd
                            .render_input(ui, r, t, "*".repeat(self.t_reg_pwd.len()), tl!("password"), 0.62);
                        let h = 0.09;
                        let pad = 0.05;
                        let mut r = Rect::new(wr.x + pad, wr.bottom() - h - 0.04, (wr.w - pad) / 2. - pad, h);
                        self.btn_to_login.render_text(ui, r, t, tl!("back-login"), 0.66, false);
                        r.x += r.w + pad;
                        self.btn_reg.render_text(ui, r, t, tl!("register"), 0.66, false);

                        ui.dy(wr.h);
                        let r = ui.text(tl!("login")).pos(wr.x + 0.045, wr.y + 0.037).size(1.1).draw_using(&BOLD_FONT);
                        let r = ui
                            .text(tl!("login-sub"))
                            .pos(r.x + 0.006, r.bottom() + 0.032)
                            .size(0.4)
                            .color(semi_white(0.6))
                            .max_width(wr.w - 0.05)
                            .multiline()
                            .draw();
                        let pad = 0.037;
                        let mut r = Rect::new(wr.x + pad, r.bottom() + 0.06, wr.w - pad * 2., 0.1);
                        self.input_email.render_input(ui, r, t, &self.t_email, tl!("email"), 0.62);
                        r.y += r.h + 0.04;
                        self.input_pwd.render_input(ui, r, t, "*".repeat(self.t_pwd.len()), tl!("password"), 0.62);

                        let r = ui
                            .text(tl!("forget-password"))
                            .pos(r.right() - 0.02, r.y + r.h + 0.02)
                            .anchor(1., 0.)
                            .size(0.4)
                            .color(semi_white(0.6))
                            .draw();
                        self.btn_forget_pwd.set(ui, r.feather(0.02));

                        let h = 0.09;
                        let pad = 0.05;
                        let mut r = Rect::new(wr.x + pad, wr.bottom() - h - 0.04, (wr.w - pad) / 2. - pad, h);
                        self.btn_to_reg.render_text(ui, r, t, tl!("register"), 0.66, false);
                        r.x += r.w + pad;
                        self.btn_login.render_text(ui, r, t, tl!("login"), 0.66, false);
                    });
                });
            });
        }
        if self.task.is_some() {
            ui.full_loading_simple(t);
        }
        #[cfg(feature = "aa")]
        self.render_picker(ui, t);
        #[cfg(feature = "aa")]
        if self.hykb_task.is_some() {
            ui.full_loading_simple(t);
        }
    }

    /// The bounding rect of the method-choice panel.
    #[cfg(feature = "aa")]
    fn picker_rect() -> Rect {
        let hw = 0.4;
        let hh = 0.25;
        Rect::new(-hw, -hh, hw * 2., hh * 2.)
    }

    /// Render the method-choice panel: a title and two vertical, styled buttons
    /// (email login and the green HYKB login with its logo).
    #[cfg(feature = "aa")]
    fn render_picker(&mut self, ui: &mut Ui, t: f32) {
        if !self.picker_show && !self.picker_fader.transiting() {
            return;
        }
        self.picker_fader.reset();
        let p = if self.picker_show { 1. } else { -self.picker_fader.progress(t) };
        ui.fill_rect(ui.screen_rect(), semi_black(p * 0.7));
        self.picker_fader.for_sub(|f| {
            f.render(ui, t, |ui| {
                let wr = Self::picker_rect();
                ui.fill_path(&wr.rounded(0.02), ui.background());

                let pad = 0.045;
                ui.text(tl!("login-method-title"))
                    .pos(wr.x + pad, wr.y + 0.037)
                    .size(1.1)
                    .draw_using(&BOLD_FONT);

                let bh = 0.13;
                let gap = 0.028;
                let bw = wr.w - pad * 2.;
                // HYKB login — brand green with the popcorn logo.
                let r = Rect::new(wr.x + pad, wr.bottom() - bh * 2. - gap - 0.05, bw, bh);
                let green = Color::from_rgba(0x5f, 0xb8, 0x78, 255);
                self.btn_method_hykb.render_shadow(ui, r, t, |ui, path| {
                    ui.fill_path(&path, green);
                    let ir = Rect::new(r.x + 0.03, r.center().y - 0.045, 0.09, 0.09);
                    ui.fill_rect(ir, (*self.icons.hykb, ir, ScaleType::Fit));
                    ui.text(tl!("login-method-hykb"))
                        .pos(ir.right() + 0.03, r.center().y)
                        .anchor(0., 0.5)
                        .no_baseline()
                        .size(0.6)
                        .color(WHITE)
                        .draw();
                });
                // Email login — neutral dark with the envelope icon.
                let r = Rect::new(wr.x + pad, r.bottom() + gap, bw, bh);
                self.btn_method_email.render_shadow(ui, r, t, |ui, path| {
                    ui.fill_path(&path, semi_black(0.4));
                    let ir = Rect::new(r.x + 0.03, r.center().y - 0.04, 0.08, 0.08);
                    ui.fill_rect(ir, (*self.icons.msg, ir, ScaleType::Fit, semi_white(0.9)));
                    ui.text(tl!("login-method-email"))
                        .pos(ir.right() + 0.035, r.center().y)
                        .anchor(0., 0.5)
                        .no_baseline()
                        .size(0.6)
                        .color(WHITE)
                        .draw();
                });
            });
        });
    }
}
