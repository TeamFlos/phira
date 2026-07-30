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
#[cfg(feature = "hykb")]
use prpr::ext::ScaleType;
use prpr::{
    core::BOLD_FONT,
    ext::{open_url, semi_black, semi_white, RectExt},
    scene::{request_input, return_input, show_error, show_message, take_input},
    task::Task,
    ui::{button_hit, DRectButton, Dialog, RectButton, Ui},
};
use regex::Regex;
use std::{future::Future, sync::atomic::Ordering, sync::Arc};

const USERNAME_LEN_MIN: usize = 2;
const USERNAME_LEN_MAX: usize = 14;

const PWD_LEN_MIN: usize = 8;
const PWD_LEN_MAX: usize = 32;

#[cfg(feature = "hykb")]
use crate::{client::HykbLoginOutcome, obtain_hykb_credential};
#[cfg(feature = "hykb")]
use prpr::scene::take_input_cancelled;
#[cfg(feature = "hykb")]
use std::sync::Mutex;

/// The user's choice in the HYKB "register or claim" dialog.
#[cfg(feature = "hykb")]
#[derive(Clone, Copy)]
enum HykbChoice {
    Register,
    Claim,
    /// The player dismissed the dialog without choosing; back out to the picker.
    Cancel,
}

/// Result of the initial HYKB login step.
#[cfg(feature = "hykb")]
enum HykbStep {
    /// The HYKB account was already bound; carries the fetched user.
    LoggedIn(Box<User>),
    /// The account is new; carries the short-lived token for register/claim and
    /// the HYKB nickname used to prefill the username input.
    NeedChoice { hykb_token: String, nick: String },
}

static EMAIL_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"\A[a-z0-9!#$%&'*+/=?^_‘{|}~-]+(?:\.[a-z0-9!#$%&'*+/=?^_‘{|}~-]+)*@(?:[a-z0-9](?:[a-z0-9-]*[a-z0-9])?\.)+[a-z0-9](?:[a-z0-9-]*[a-z0-9])?\z",
    )
    .unwrap()
});

fn validate_username(username: &str) -> Option<String> {
    if !(USERNAME_LEN_MIN..=USERNAME_LEN_MAX).contains(&username.chars().count()) {
        return Some(tl!("name-length-req", "min" => USERNAME_LEN_MIN, "max" => USERNAME_LEN_MAX));
    }
    if username.chars().any(|it| it != '_' && it != '-' && !it.is_alphanumeric()) {
        return Some(tl!("name-has-illegal-char").into_owned());
    }
    None
}

pub struct Login {
    #[cfg(feature = "hykb")]
    icons: Arc<Icons>,

    fader: Fader,
    show: bool,

    /// In HYKB builds an account is mandatory: while set, the panel is kept
    /// open and cannot be dismissed by tapping outside — only a successful
    /// login clears it.
    #[cfg(feature = "hykb")]
    forced: bool,

    /// The method-choice panel ("email vs HYKB"), shown before the form when
    /// HYKB is available.
    #[cfg(feature = "hykb")]
    picker_fader: Fader,
    #[cfg(feature = "hykb")]
    picker_show: bool,
    #[cfg(feature = "hykb")]
    btn_method_email: DRectButton,
    #[cfg(feature = "hykb")]
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
    #[cfg(feature = "hykb")]
    hykb_task: Option<Task<Result<HykbStep>>>,
    /// Pending HYKB token awaiting the user's register/claim choice.
    #[cfg(feature = "hykb")]
    hykb_pending_token: Option<String>,
    /// HYKB token kept while the player types the username for their new account.
    #[cfg(feature = "hykb")]
    hykb_reg_token: Option<String>,
    /// HYKB nickname, used only to prefill the username input for a new account.
    #[cfg(feature = "hykb")]
    hykb_nick: Option<String>,
    /// Choice written by the register/claim dialog listener.
    #[cfg(feature = "hykb")]
    hykb_choice: Arc<Mutex<Option<HykbChoice>>>,
    /// Choice written by the "bind HYKB to continue" dialog shown after an email
    /// login to an account not yet bound to HYKB: `Some(true)` = bind now,
    /// `Some(false)` = cancel (and log the half-finished session back out).
    #[cfg(feature = "hykb")]
    email_bind_choice: Arc<Mutex<Option<bool>>>,

    /// The in-app "choose your username" panel shown for a new HYKB account,
    /// in place of popping the native InputBox directly. The InputBox only
    /// appears when the player taps the input slot inside this panel.
    #[cfg(feature = "hykb")]
    reg_name_fader: Fader,
    #[cfg(feature = "hykb")]
    reg_name_show: bool,
    #[cfg(feature = "hykb")]
    input_hykb_name: DRectButton,
    #[cfg(feature = "hykb")]
    btn_hykb_name_confirm: DRectButton,
    #[cfg(feature = "hykb")]
    t_hykb_name: String,
}

enum NextAction {
    Login,
    Register,
    #[cfg(feature = "hykb")]
    Hykb,
}

impl Login {
    const TIME: f32 = 0.7;

    pub fn new(icons: Arc<Icons>) -> Self {
        #[cfg(not(feature = "hykb"))]
        let _ = icons;
        Self {
            #[cfg(feature = "hykb")]
            icons,

            fader: Fader::new().with_distance(-0.4).with_time(0.5),
            show: false,

            #[cfg(feature = "hykb")]
            forced: false,

            #[cfg(feature = "hykb")]
            picker_fader: Fader::new().with_distance(-0.4).with_time(0.5),
            #[cfg(feature = "hykb")]
            picker_show: false,
            #[cfg(feature = "hykb")]
            btn_method_email: DRectButton::new().with_radius(0.012).with_elevation(0.004),
            #[cfg(feature = "hykb")]
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
            #[cfg(feature = "hykb")]
            hykb_task: None,
            #[cfg(feature = "hykb")]
            hykb_pending_token: None,
            #[cfg(feature = "hykb")]
            hykb_reg_token: None,
            #[cfg(feature = "hykb")]
            hykb_nick: None,
            #[cfg(feature = "hykb")]
            hykb_choice: Arc::new(Mutex::new(None)),
            #[cfg(feature = "hykb")]
            email_bind_choice: Arc::new(Mutex::new(None)),

            #[cfg(feature = "hykb")]
            reg_name_fader: Fader::new().with_distance(-0.4).with_time(0.5),
            #[cfg(feature = "hykb")]
            reg_name_show: false,
            #[cfg(feature = "hykb")]
            input_hykb_name: DRectButton::new().with_delta(-0.002),
            #[cfg(feature = "hykb")]
            btn_hykb_name_confirm: DRectButton::new().with_radius(0.012).with_elevation(0.004),
            #[cfg(feature = "hykb")]
            t_hykb_name: String::new(),
        }
    }

    #[inline]
    fn start(&mut self, desc: &'static str, future: impl Future<Output = Result<Option<User>>> + Send + 'static) {
        self.task = Some((desc, Task::new(future)));
    }

    pub fn enter(&mut self, t: f32) {
        // With HYKB available, show the method-choice panel before any form;
        // otherwise go straight to the email form.
        #[cfg(feature = "hykb")]
        self.show_picker(t);
        #[cfg(not(feature = "hykb"))]
        self.show_form(t);
    }

    /// Whether any part of the login flow is currently on screen or in flight
    /// (the picker, the form, a running task, or an in-between HYKB step such as
    /// the register/claim choice dialog or the username input). Used to keep
    /// `force()` from re-showing the picker over a sub-flow.
    #[cfg(feature = "hykb")]
    fn is_active(&self) -> bool {
        self.show
            || self.picker_show
            || self.fader.transiting()
            || self.picker_fader.transiting()
            || self.reg_name_show
            || self.reg_name_fader.transiting()
            || self.task.is_some()
            || self.hykb_task.is_some()
            || self.hykb_pending_token.is_some()
            || self.hykb_reg_token.is_some()
            || !self.start_time.is_nan()
    }

    /// Force the login flow open and keep it non-dismissible until the player
    /// logs in. HYKB builds call this whenever the home page is shown while
    /// signed out (fresh launch, or after a manual logout). It is a no-op while
    /// the flow is already active, so it can safely be polled every frame.
    #[cfg(feature = "hykb")]
    pub fn force(&mut self, t: f32) {
        if self.is_active() {
            return;
        }
        self.forced = true;
        self.enter(t);
    }

    /// Reveal the method-choice panel ("email vs HYKB").
    #[cfg(feature = "hykb")]
    fn show_picker(&mut self, t: f32) {
        self.picker_show = true;
        self.picker_fader.sub(t);
    }

    /// Reveal the email login/register form.
    fn show_form(&mut self, t: f32) {
        self.fader.sub(t);
    }

    /// Dismiss the method-choice panel.
    #[cfg(feature = "hykb")]
    fn dismiss_picker(&mut self, t: f32) {
        self.picker_show = false;
        self.picker_fader.back(t);
    }

    pub fn dismiss(&mut self, t: f32) {
        self.show = false;
        self.fader.back(t);
        // Drop any pending claim so a later plain login isn't treated as a claim.
        #[cfg(feature = "hykb")]
        {
            self.forced = false;
            self.hykb_pending_token = None;
            self.hykb_reg_token = None;
            self.hykb_nick = None;
            self.reg_name_show = false;
            self.t_hykb_name.clear();
        }
    }

    fn register(&mut self) -> Option<String> {
        let email = self.t_reg_email.clone();
        let name = self.t_reg_name.clone();
        let pwd = self.t_reg_pwd.clone();
        if let Some(error) = validate_username(&name) {
            return Some(error);
        }
        if !EMAIL_REGEX.is_match(&email) {
            return Some(tl!("illegal-email").into_owned());
        }
        if !(8..=32).contains(&pwd.len()) {
            return Some(tl!("pwd-length-req", "min" => PWD_LEN_MIN, "max" => PWD_LEN_MAX));
        }
        self.start("register", async move {
            Client::register(&email, &name, &pwd).await?;
            Ok(None)
        });
        None
    }

    /// Kick off the native HYKB login: obtain credentials, then call `/login/hykb`.
    #[cfg(feature = "hykb")]
    fn start_hykb_login(&mut self) {
        self.hykb_task = Some(Task::new(async move {
            let cred = obtain_hykb_credential().await?.ok_or_err()?;
            match Client::login_hykb(cred.uid, &cred.access_token).await? {
                HykbLoginOutcome::LoggedIn => Ok(HykbStep::LoggedIn(Box::new(Client::get_me().await?))),
                HykbLoginOutcome::NeedChoice { hykb_token } => Ok(HykbStep::NeedChoice { hykb_token, nick: cred.nick }),
            }
        }));
    }

    /// Show the "register a new account / claim an existing one" dialog after a
    /// first-time HYKB login. The chosen action is recorded in `hykb_choice`.
    #[cfg(feature = "hykb")]
    fn show_hykb_choice(&self) {
        let choice = Arc::clone(&self.hykb_choice);
        Dialog::plain(tl!("hykb-choice-title"), tl!("hykb-choice-sub"))
            .buttons(vec![tl!("hykb-choice-register").to_string(), tl!("hykb-choice-claim").to_string()])
            .listener(move |_, pos| {
                match pos {
                    // Outside click / dismiss: treat as backing out to the picker.
                    -1 => *choice.lock().unwrap() = Some(HykbChoice::Cancel),
                    0 => *choice.lock().unwrap() = Some(HykbChoice::Register),
                    1 => *choice.lock().unwrap() = Some(HykbChoice::Claim),
                    _ => {}
                }
                false
            })
            .show();
    }

    /// Show the "this account isn't bound to HYKB yet" dialog after an email
    /// login, offering to bind HYKB (or cancel). The choice is recorded in
    /// `email_bind_choice` and acted on in `update_hykb`.
    #[cfg(feature = "hykb")]
    fn show_email_bind(&self) {
        let choice = Arc::clone(&self.email_bind_choice);
        Dialog::plain(tl!("hykb-bind-required-title"), tl!("hykb-bind-required"))
            .buttons(vec![crate::ttl!("cancel").into_owned(), tl!("hykb-bind-required-confirm").into_owned()])
            .listener(move |_, pos| {
                match pos {
                    // Cancel button or outside click: back out and log out.
                    0 | -1 => *choice.lock().unwrap() = Some(false),
                    1 => *choice.lock().unwrap() = Some(true),
                    _ => {}
                }
                false
            })
            .show();
    }

    /// Reveal the in-app "choose your username" panel for a new HYKB account.
    #[cfg(feature = "hykb")]
    fn show_reg_name(&mut self, t: f32) {
        self.reg_name_show = true;
        self.reg_name_fader.sub(t);
    }

    /// Dismiss the username panel.
    #[cfg(feature = "hykb")]
    fn dismiss_reg_name(&mut self, t: f32) {
        self.reg_name_show = false;
        self.reg_name_fader.back(t);
    }

    /// Validate the username the player chose and create their HYKB-bound account.
    /// Called from the username panel's confirm button; the panel stays visible on
    /// an invalid name so it can be fixed.
    #[cfg(feature = "hykb")]
    fn submit_hykb_register(&mut self, name: String) {
        if let Some(error) = validate_username(&name) {
            show_message(error).error();
            return;
        }
        let Some(token) = self.hykb_reg_token.take() else {
            return;
        };
        self.start("hykb-login", async move {
            Client::login_hykb_register(&token, &name).await?;
            Ok(Some(Client::get_me().await?))
        });
    }

    pub fn touch(&mut self, touch: &Touch, t: f32) -> bool {
        if self.fader.transiting() || self.task.is_some() || !self.start_time.is_nan() {
            return true;
        }
        #[cfg(feature = "hykb")]
        if self.hykb_task.is_some() {
            return true;
        }
        // The "choose your username" panel for a new HYKB account.
        #[cfg(feature = "hykb")]
        if self.reg_name_show {
            if self.reg_name_fader.transiting() {
                return true;
            }
            if !Self::reg_name_rect().contains(touch.position) && touch.phase == TouchPhase::Started {
                // Backing out returns to the register/claim choice dialog (the
                // previous level), keeping the token so the choice can be remade.
                self.dismiss_reg_name(t);
                if let Some(token) = self.hykb_reg_token.take() {
                    self.hykb_pending_token = Some(token);
                    self.show_hykb_choice();
                }
                return true;
            }
            if self.input_hykb_name.touch(touch, t) {
                request_input(
                    "hykb_reg_name",
                    InputBox::new()
                        .title(tl!("username"))
                        .prompt(tl!("hykb-reg-name-prompt", "min" => USERNAME_LEN_MIN, "max" => USERNAME_LEN_MAX))
                        .default_text(&self.t_hykb_name),
                );
                return true;
            }
            if self.btn_hykb_name_confirm.touch(touch, t) {
                if let Some(error) = validate_username(&self.t_hykb_name) {
                    show_message(error).error();
                } else {
                    self.dismiss_reg_name(t);
                    self.submit_hykb_register(self.t_hykb_name.clone());
                }
                return true;
            }
            return true;
        }
        // The method-choice panel sits on top of (and gates) the form.
        #[cfg(feature = "hykb")]
        if self.picker_show {
            if self.picker_fader.transiting() {
                return true;
            }
            if !Self::picker_rect().contains(touch.position) && touch.phase == TouchPhase::Started {
                // When login is mandatory, swallow the touch but keep the panel.
                if !self.forced {
                    self.dismiss_picker(t);
                }
                return true;
            }
            if self.btn_method_email.touch(touch, t) {
                self.dismiss_picker(t);
                self.show_form(t);
                return true;
            }
            if self.btn_method_hykb.touch(touch, t) {
                if !check_read_tos_and_policy(true, true) {
                    // Keep the picker up behind the TOS dialog: if the player
                    // denies (which never fires JUST_ACCEPTED_TOS), they simply
                    // stay on the picker rather than being stranded on a blank,
                    // forced home. The picker is dismissed once TOS is accepted.
                    self.after_accept_tos = Some(NextAction::Hykb);
                } else {
                    self.dismiss_picker(t);
                    self.start_hykb_login();
                }
                return true;
            }
            return true;
        }
        if self.show {
            if !Ui::dialog_rect().contains(touch.position) && touch.phase == TouchPhase::Started {
                // In the HYKB claim flow, backing out of the credential form
                // returns to the register/claim choice dialog (the previous
                // level) instead of closing the login entirely. Keep the
                // pending token so the choice can be made again.
                #[cfg(feature = "hykb")]
                if self.hykb_pending_token.is_some() {
                    self.show = false;
                    self.fader.back(t);
                    self.show_hykb_choice();
                    return true;
                }
                // When login is mandatory, the flow can't be dismissed, but the
                // player may still back out of the email form to the method
                // picker (rather than being stranded on the form).
                #[cfg(feature = "hykb")]
                if self.forced {
                    self.show = false;
                    self.fader.back(t);
                    self.show_picker(t);
                    return true;
                }
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
                #[cfg(feature = "hykb")]
                let pending_claim = self.hykb_pending_token.is_some();
                #[cfg(not(feature = "hykb"))]
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
        #[cfg(feature = "hykb")]
        if let Some(token) = self.hykb_pending_token.clone() {
            let email = self.t_email.clone();
            let pwd = self.t_pwd.clone();
            if !EMAIL_REGEX.is_match(&email) {
                show_message(tl!("illegal-email")).error();
                return;
            }
            // Keep the pending token: on success `dismiss` clears it, but on a
            // failed claim it must survive so backing out returns to the
            // register/claim dialog (and a retry still claims) rather than
            // dropping all the way back to the method picker.
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
            // Fetch without the HYKB guard: an unbound account is handled by the
            // "bind HYKB to continue" dialog rather than an immediate logout.
            #[cfg(feature = "hykb")]
            let me = Client::get_me_unchecked().await?;
            #[cfg(not(feature = "hykb"))]
            let me = Client::get_me().await?;
            // An account already bound to a HYKB uid must still have the native
            // SDK signed in (same requirement as the app-restart session restore
            // in `page/home.rs`): restore it silently and tear the session down if
            // that login fails/cancels. The signed-in HYKB account no longer has
            // to match the bound `hykb_uid` — any successful HYKB login is
            // accepted.
            #[cfg(feature = "hykb")]
            if me.hykb_uid.is_some() {
                crate::obtain_hykb_credential_silent().await?.ok_or_err()?;
            }
            Ok(Some(me))
        });
    }

    pub fn update(&mut self, t: f32) -> Result<()> {
        if let Some(done) = self.fader.done(t) {
            self.show = !done;
        }
        #[cfg(feature = "hykb")]
        if let Some(done) = self.picker_fader.done(t) {
            self.picker_show = !done;
        }
        #[cfg(feature = "hykb")]
        if let Some(done) = self.reg_name_fader.done(t) {
            self.reg_name_show = !done;
        }
        dispatch_tos_task();
        if let Some((id, text)) = take_input() {
            'tmp: {
                // The HYKB register username feeds the in-app panel's input slot
                // rather than being stored into one of the email-form fields.
                #[cfg(feature = "hykb")]
                if id == "hykb_reg_name" {
                    self.t_hykb_name = text;
                    break 'tmp;
                }
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
        // Cancelling the username InputBox simply returns to the in-app username
        // panel (still shown); consume the event so it doesn't leak to others.
        #[cfg(feature = "hykb")]
        if let Some(id) = take_input_cancelled() {
            let _ = id;
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
                #[cfg(feature = "hykb")]
                Some(NextAction::Hykb) => {
                    // The picker was kept visible through the TOS gate; drop it
                    // now that the player accepted and we're proceeding.
                    self.dismiss_picker(t);
                    self.start_hykb_login();
                }
                None => (),
            }
            self.after_accept_tos = None;
        }
        #[cfg(feature = "hykb")]
        let mut email_needs_bind = false;
        if let Some((action, task)) = &mut self.task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => show_error(err.context(tl!("action-failed", "action" => *action))),
                    Ok(user) => {
                        // In HYKB builds a plain email login is only permitted for
                        // accounts already bound to a HYKB account; others are sent
                        // through the "bind HYKB to continue" dialog below.
                        #[cfg(feature = "hykb")]
                        let needs_bind = *action == "login" && user.as_ref().is_some_and(|u| u.hykb_uid.is_none());
                        #[cfg(not(feature = "hykb"))]
                        let needs_bind = false;
                        if needs_bind {
                            self.t_pwd.clear();
                            #[cfg(feature = "hykb")]
                            {
                                email_needs_bind = true;
                            }
                        } else {
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
                }
                self.task = None;
            }
        }
        // The email login resolved to an account not yet bound to HYKB: prompt the
        // player to bind it (deferred to here so it doesn't overlap the `self.task`
        // borrow above).
        #[cfg(feature = "hykb")]
        if email_needs_bind {
            self.show_email_bind();
        }
        #[cfg(feature = "hykb")]
        self.update_hykb(t)?;
        Ok(())
    }

    /// Drive the HYKB login phases: the initial verify task, the register/claim
    /// choice dialog, and the follow-up that resolves to a logged-in user.
    #[cfg(feature = "hykb")]
    fn update_hykb(&mut self, t: f32) -> Result<()> {
        // The "bind HYKB to continue" dialog (shown after an email login to an
        // unbound account) recorded the player's decision.
        let email_bind = self.email_bind_choice.lock().unwrap().take();
        if let Some(bind) = email_bind {
            if bind {
                // Bind the freshly-authenticated session to a HYKB account, then
                // finish as a normal login. Reuse the "hykb-login" action so the
                // success path (set user, dismiss) is shared.
                self.start("hykb-login", async move {
                    let cred = obtain_hykb_credential().await?.ok_or_err()?;
                    Client::bind_hykb(cred.uid, &cred.access_token).await?;
                    Ok(Some(Client::get_me().await?))
                });
            } else {
                // Cancelled: the email login already obtained tokens, so drop them
                // and return to the method picker rather than leaving an unbound,
                // half-logged-in session.
                get_data_mut().me = None;
                get_data_mut().tokens = None;
                save_data()?;
                crate::sync_data();
                self.show = false;
                self.fader.back(t);
                self.show_picker(t);
            }
        }
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
                    Ok(HykbStep::NeedChoice { hykb_token, nick }) => {
                        self.hykb_pending_token = Some(hykb_token);
                        self.hykb_nick = Some(nick);
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
            match choice {
                HykbChoice::Register => {
                    if let Some(token) = self.hykb_pending_token.take() {
                        // Let the player choose their own username before creating
                        // the account via the in-app panel (prefilled with their
                        // HYKB nickname). The token is kept until the name is submitted.
                        self.hykb_reg_token = Some(token);
                        self.t_hykb_name = self.hykb_nick.clone().unwrap_or_default();
                        self.show_reg_name(t);
                    }
                }
                HykbChoice::Claim => {
                    // Keep the pending token; reveal the email form so the user can
                    // enter the credentials of the account they want to claim. The
                    // login button submits the claim while the token is set.
                    if self.hykb_pending_token.is_some() {
                        self.show_form(t);
                    }
                }
                HykbChoice::Cancel => {
                    // Backed out of register/claim: drop the pending identity and
                    // return to the method-choice panel.
                    self.hykb_pending_token = None;
                    self.hykb_nick = None;
                    self.show_picker(t);
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
                        // Under the anti-addiction build, self-service email
                        // registration is disabled: only offer login.
                        #[cfg(feature = "hykb")]
                        {
                            let r = Rect::new(wr.x + pad, wr.bottom() - h - 0.04, wr.w - pad * 2., h);
                            self.btn_login.render_text(ui, r, t, tl!("login"), 0.66, false);
                        }
                        #[cfg(not(feature = "hykb"))]
                        {
                            let mut r = Rect::new(wr.x + pad, wr.bottom() - h - 0.04, (wr.w - pad) / 2. - pad, h);
                            self.btn_to_reg.render_text(ui, r, t, tl!("register"), 0.66, false);
                            r.x += r.w + pad;
                            self.btn_login.render_text(ui, r, t, tl!("login"), 0.66, false);
                        }
                    });
                });
            });
        }
        if self.task.is_some() {
            ui.full_loading_simple(t);
        }
        #[cfg(feature = "hykb")]
        self.render_picker(ui, t);
        #[cfg(feature = "hykb")]
        self.render_reg_name(ui, t);
        #[cfg(feature = "hykb")]
        if self.hykb_task.is_some() {
            ui.full_loading_simple(t);
        }
    }

    /// The bounding rect of the method-choice panel.
    #[cfg(feature = "hykb")]
    fn picker_rect() -> Rect {
        let hw = 0.4;
        let hh = 0.25;
        Rect::new(-hw, -hh, hw * 2., hh * 2.)
    }

    /// Render the method-choice panel: a title and two vertical, styled buttons
    /// (email login and the green HYKB login with its logo).
    #[cfg(feature = "hykb")]
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
                        .pos(ir.right() + 0.03, r.center().y - 0.016)
                        .anchor(0., 0.5)
                        .no_baseline()
                        .size(0.6)
                        .color(WHITE)
                        .draw();
                    ui.text(tl!("login-method-recommended"))
                        .pos(ir.right() + 0.03, r.center().y + 0.024)
                        .anchor(0., 0.5)
                        .no_baseline()
                        .size(0.45)
                        .color(Color::from_hex_rgb(0xffc107))
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

    /// The bounding rect of the "choose your username" panel.
    #[cfg(feature = "hykb")]
    fn reg_name_rect() -> Rect {
        let hw = 0.4;
        let hh = 0.24;
        Rect::new(-hw, -hh, hw * 2., hh * 2.)
    }

    /// Render the "choose your username" panel: a title, a hint line, a tappable
    /// input slot (which opens the native InputBox) and a confirm button.
    #[cfg(feature = "hykb")]
    fn render_reg_name(&mut self, ui: &mut Ui, t: f32) {
        if !self.reg_name_show && !self.reg_name_fader.transiting() {
            return;
        }
        self.reg_name_fader.reset();
        let p = if self.reg_name_show { 1. } else { -self.reg_name_fader.progress(t) };
        ui.fill_rect(ui.screen_rect(), semi_black(p * 0.7));
        self.reg_name_fader.for_sub(|f| {
            f.render(ui, t, |ui| {
                let wr = Self::reg_name_rect();
                ui.fill_path(&wr.rounded(0.02), ui.background());

                let pad = 0.045;
                let r = ui.text(tl!("username")).pos(wr.x + pad, wr.y + 0.037).size(1.1).draw_using(&BOLD_FONT);
                let r = ui
                    .text(tl!("hykb-reg-name-prompt", "min" => USERNAME_LEN_MIN, "max" => USERNAME_LEN_MAX))
                    .pos(wr.x + pad + 0.006, r.bottom() + 0.028)
                    .size(0.4)
                    .color(semi_white(0.6))
                    .max_width(wr.w - pad * 2.)
                    .multiline()
                    .draw();

                let r = Rect::new(wr.x + pad, r.bottom() + 0.04, wr.w - pad * 2., 0.1);
                self.input_hykb_name.render_input(ui, r, t, &self.t_hykb_name, tl!("username"), 0.62);

                let h = 0.09;
                let bpad = 0.05;
                let r = Rect::new(wr.x + bpad, wr.bottom() - h - 0.04, wr.w - bpad * 2., h);
                self.btn_hykb_name_confirm.render_text(ui, r, t, tl!("hykb-reg-name-confirm"), 0.66, true);
            });
        });
    }
}
