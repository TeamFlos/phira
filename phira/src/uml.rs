mod lexer;
mod parse;

pub use parse::parse_uml;

use self::parse::{constant, ButtonState, TopLevel};
use crate::{
    charts_view::{ChartDisplayItem, ChartsView},
    client::{recv_raw, Client, File},
    icons::Icons,
};
use anyhow::{anyhow, bail, Result};
use image::DynamicImage;
use macroquad::prelude::*;
use nalgebra::Vector2;
use parse::Expr;
use prpr::{
    core::Matrix,
    ext::{semi_black, semi_white, RectExt, SafeTexture, ScaleType},
    scene::NextScene,
    task::Task,
    ui::{RectButton, Ui},
};
use serde::Deserialize;
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    fmt::Debug,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
};
use tracing::warn;

#[derive(Debug)]
struct WrappedColor(Color);
impl Default for WrappedColor {
    fn default() -> Self {
        Self(WHITE)
    }
}

impl<'de> Deserialize<'de> for WrappedColor {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;
        let s = String::deserialize(deserializer)?;
        Ok(WrappedColor(match s.as_str() {
            "white" => WHITE,
            "black" => BLACK,
            "red" => RED,
            "blue" => BLUE,
            "yellow" => YELLOW,
            "green" => GREEN,
            "gray" => GRAY,
            _ => {
                if let Some(d) = s.strip_prefix('#') {
                    let int = u32::from_str_radix(d, 16).map_err(D::Error::custom)?;
                    let mut v = int.to_be_bytes();
                    if d.len() == 6 {
                        v[0] = 0xff;
                    }
                    Color::from_rgba(v[1], v[2], v[3], v[0])
                } else if let Some(d) = s.strip_prefix('w') {
                    semi_white(d.parse().map_err(D::Error::custom)?)
                } else if let Some(d) = s.strip_prefix('b') {
                    semi_black(d.parse().map_err(D::Error::custom)?)
                } else {
                    return Err(D::Error::custom(format!("invalid color: {s}")));
                }
            }
        }))
    }
}

pub trait Element {
    fn id(&self) -> Option<&str>;
    fn on_result(&self, _t: f32, _delete: bool) {}
    fn touch(&self, _touch: &Touch, _uml: &Uml, _action: &mut Option<String>) -> Result<bool> {
        Ok(false)
    }
    fn render(&self, ui: &mut Ui, uml: &Uml) -> Result<Var>;
    fn render_top(&self, _ui: &mut Ui, _uml: &Uml) -> Result<()> {
        Ok(())
    }
    fn next_scene(&self) -> Option<NextScene> {
        None
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TextConfig {
    id: Option<String>,
    size: Expr,
    x: Expr,
    y: Expr,
    ax: Expr,
    ay: Expr,
    ml: bool,
    mw: Option<Expr>,
    bl: bool,
    c: WrappedColor,
}

impl Default for TextConfig {
    fn default() -> Self {
        Self {
            id: None,
            size: constant(1.0),
            x: constant(0.),
            y: constant(0.),
            ax: constant(0.),
            ay: constant(0.),
            ml: false,
            mw: None,
            bl: true,
            c: WrappedColor::default(),
        }
    }
}

#[derive(Debug)]
pub struct Text {
    config: TextConfig,
    text: String,
}

impl Text {
    pub fn new(config: TextConfig, text: String) -> Self {
        Self { config, text }
    }
}

impl Element for Text {
    fn id(&self) -> Option<&str> {
        self.config.id.as_deref()
    }

    fn render(&self, ui: &mut Ui, uml: &Uml) -> Result<Var> {
        let c = &self.config;
        let mut text = ui
            .text(&self.text)
            .pos(c.x.eval(uml)?.float()?, c.y.eval(uml)?.float()?)
            .anchor(c.ax.eval(uml)?.float()?, c.ay.eval(uml)?.float()?)
            .size(c.size.eval(uml)?.float()?)
            .color(c.c.0);
        if c.ml {
            text = text.multiline();
        }
        if let Some(w) = &c.mw {
            text = text.max_width(w.eval(uml)?.float()?);
        }
        if !c.bl {
            text = text.no_baseline();
        }
        Ok(Var::Rect(match c.id.as_deref() {
            Some("pgr") => prpr::core::PGR_FONT.with(|it| text.draw_with_font(it.borrow_mut().as_mut())),
            Some("bold") => prpr::core::BOLD_FONT.with(|it| text.draw_with_font(it.borrow_mut().as_mut())),
            _ => text.draw(),
        }))
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageConfig {
    #[serde(default)]
    id: Option<String>,
    url: File,
    r: Expr,
    #[serde(default)]
    c: WrappedColor,
    #[serde(default)]
    t: ScaleType,
}

pub struct Image {
    config: ImageConfig,
    task: RefCell<Option<Task<Result<DynamicImage>>>>,
    tex: RefCell<Option<SafeTexture>>,
}

impl Image {
    pub fn new(config: ImageConfig) -> Self {
        let url = config.url.clone();
        Self {
            config,
            task: RefCell::new(Some(Task::new(async move { url.load_image().await }))),
            tex: RefCell::new(None),
        }
    }
}

impl Element for Image {
    fn id(&self) -> Option<&str> {
        self.config.id.as_deref()
    }

    fn render(&self, ui: &mut Ui, uml: &Uml) -> Result<Var> {
        let c = &self.config;
        let mut guard = self.task.borrow_mut();
        if let Some(task) = guard.as_mut() {
            if let Some(res) = task.take() {
                match res {
                    Ok(val) => *self.tex.borrow_mut() = Some(val.into()),
                    Err(err) => {
                        warn!(url = c.url.url, ?err, "failed to load image");
                    }
                }
                drop(guard);
                *self.task.borrow_mut() = None;
            }
        }
        let r = c.r.eval(uml)?.rect()?;
        if let Some(tex) = self.tex.borrow().as_ref() {
            ui.fill_rect(r, (**tex, r, c.t, c.c.0));
        }
        Ok(Var::Rect(r))
    }
}

#[derive(Debug, Clone, Copy)]
struct I32(i32);
impl<'de> Deserialize<'de> for I32 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;
        Ok(Self(String::deserialize(deserializer)?.parse().map_err(D::Error::custom)?))
    }
}

fn default_row_num() -> I32 {
    I32(4)
}

fn default_chart_height() -> Expr {
    constant(0.3)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionConfig {
    #[serde(default)]
    id: Option<String>,
    cid: I32,
    #[serde(default = "default_row_num")]
    rn: I32,
    #[serde(default = "default_chart_height")]
    rh: Expr,
    r: Expr,
}

struct CollectionState {
    task: Option<Task<Result<crate::client::Collection>>>,
    charts_view: ChartsView,
}

pub struct Collection {
    config: CollectionConfig,
    state: RefCell<CollectionState>,
}

impl Collection {
    pub fn new(icons: Arc<Icons>, rank_icons: [SafeTexture; 8], config: CollectionConfig) -> Self {
        let cid = config.cid;
        let mut charts_view = ChartsView::new(icons, rank_icons);
        charts_view.row_num = config.rn.0 as _;
        Self {
            config,
            state: RefCell::new(CollectionState {
                task: Some(Task::new(async move { Ok(recv_raw(Client::get(format!("/collection/{}", cid.0))).await?.json().await?) })),
                charts_view,
            }),
        }
    }
}

impl Element for Collection {
    fn id(&self) -> Option<&str> {
        self.config.id.as_deref()
    }

    fn on_result(&self, t: f32, delete: bool) {
        self.state.borrow_mut().charts_view.on_result(t, delete)
    }

    fn touch(&self, touch: &Touch, uml: &Uml, _action: &mut Option<String>) -> Result<bool> {
        self.state.borrow_mut().charts_view.touch(touch, uml.t, uml.rt)
    }

    fn render(&self, ui: &mut Ui, uml: &Uml) -> Result<Var> {
        let mut state = self.state.borrow_mut();
        if let Some(task) = &mut state.task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        warn!(?err, "failed to fetch collection");
                    }
                    Ok(col) => {
                        state
                            .charts_view
                            .set(uml.t, col.charts.iter().map(ChartDisplayItem::from_remote).collect());
                    }
                }
                state.task = None;
            }
        }

        let c = &self.config;
        let t = uml.t;
        let r = c.r.eval(uml)?.rect()?;

        state.charts_view.row_height = self.config.rh.eval(uml)?.float()?;
        state.charts_view.update(t)?;
        state.charts_view.render(ui, r, t);

        Ok(Var::Float(0.))
    }

    fn render_top(&self, ui: &mut Ui, uml: &Uml) -> Result<()> {
        self.state.borrow_mut().charts_view.render_top(ui, uml.t);
        Ok(())
    }

    fn next_scene(&self) -> Option<NextScene> {
        self.state.borrow_mut().charts_view.next_scene()
    }
}

fn default_radius() -> Expr {
    constant(0.)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RectConfig {
    #[serde(default)]
    id: Option<String>,
    r: Expr,
    #[serde(default)]
    c: WrappedColor,
    #[serde(default = "default_radius")]
    rad: Expr,
}

pub struct RectElement {
    config: RectConfig,
}

impl RectElement {
    pub fn new(config: RectConfig) -> Self {
        Self { config }
    }
}

impl Element for RectElement {
    fn id(&self) -> Option<&str> {
        self.config.id.as_deref()
    }

    fn render(&self, ui: &mut Ui, uml: &Uml) -> Result<Var> {
        let c = &self.config;
        let r = c.r.eval(uml)?.rect()?;
        let rad = c.rad.eval(uml)?.float()?;
        if rad > 1e-5 {
            ui.fill_path(&r.rounded(rad), c.c.0);
        } else {
            ui.fill_rect(r, c.c.0);
        }
        Ok(Var::Rect(r))
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ButtonConfig {
    #[serde(default)]
    id: Option<String>,
    r: Expr,
    action: Option<String>,
}

pub struct ButtonElement {
    config: ButtonConfig,
    btn: RefCell<RectButton>,
    last_touched: Cell<f32>,
    count: AtomicU32,
}

impl ButtonElement {
    pub fn new(config: ButtonConfig) -> Self {
        Self {
            config,
            btn: RefCell::default(),
            last_touched: Cell::new(-1.),
            count: AtomicU32::new(0),
        }
    }
}

impl Element for ButtonElement {
    fn id(&self) -> Option<&str> {
        self.config.id.as_deref()
    }

    fn touch(&self, touch: &Touch, uml: &Uml, action: &mut Option<String>) -> Result<bool> {
        if self.btn.borrow_mut().touch(touch) {
            *action = self.config.action.clone();
            self.last_touched.set(uml.t);
            self.count.fetch_add(1, Ordering::SeqCst);
            return Ok(true);
        }
        Ok(false)
    }

    fn render(&self, ui: &mut Ui, uml: &Uml) -> Result<Var> {
        let r = self.config.r.eval(uml)?.rect()?;
        let mut btn = self.btn.borrow_mut();
        btn.set(ui, r);
        Ok(Var::ButtonState(ButtonState {
            last: self.last_touched.get(),
            cnt: self.count.load(Ordering::SeqCst),
            touching: btn.touching(),
        }))
    }
}

pub struct Assign {
    id: String,
    value: Expr,
}

impl Assign {
    pub fn new(id: String, value: Expr) -> Self {
        Self { id, value }
    }
}

impl Element for Assign {
    fn id(&self) -> Option<&str> {
        Some(&self.id)
    }

    fn render(&self, _ui: &mut Ui, uml: &Uml) -> Result<Var> {
        self.value.eval(uml)
    }
}

fn default_zero() -> Expr {
    constant(0.)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RotationConfig {
    #[serde(default = "default_zero")]
    angle: Expr,
    #[serde(default = "default_zero")]
    cx: Expr,
    #[serde(default = "default_zero")]
    cy: Expr,
}
pub struct Rotation {
    config: RotationConfig,
}
impl Rotation {
    pub fn new(config: RotationConfig) -> Self {
        Self { config }
    }
}
impl Element for Rotation {
    fn id(&self) -> Option<&str> {
        None
    }

    fn render(&self, ui: &mut Ui, uml: &Uml) -> Result<Var> {
        let angle = self.config.angle.eval(uml)?.float()?;
        let cx = self.config.cx.eval(uml)?.float()?;
        let cy = self.config.cy.eval(uml)?.float()?;
        let ct = Vector2::new(cx, cy);
        let mat = Matrix::new_translation(&ct) * Matrix::new_rotation(angle);
        let mat = mat.prepend_translation(&-ct);
        uml.push(ui, StackLayer::Mat(mat));
        Ok(Var::default())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationConfig {
    #[serde(default = "default_zero")]
    dx: Expr,
    #[serde(default = "default_zero")]
    dy: Expr,
}
pub struct Translation {
    config: TranslationConfig,
}
impl Translation {
    pub fn new(config: TranslationConfig) -> Self {
        Self { config }
    }
}
impl Element for Translation {
    fn id(&self) -> Option<&str> {
        None
    }

    fn render(&self, ui: &mut Ui, uml: &Uml) -> Result<Var> {
        let dx = self.config.dx.eval(uml)?.float()?;
        let dy = self.config.dy.eval(uml)?.float()?;
        uml.push(ui, StackLayer::Mat(Matrix::new_translation(&Vector2::new(dx, dy))));
        Ok(Var::default())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlphaConfig {
    #[serde(default = "default_zero")]
    a: Expr,
}
pub struct Alpha {
    config: AlphaConfig,
}
impl Alpha {
    pub fn new(config: AlphaConfig) -> Self {
        Self { config }
    }
}
impl Element for Alpha {
    fn id(&self) -> Option<&str> {
        None
    }

    fn render(&self, ui: &mut Ui, uml: &Uml) -> Result<Var> {
        let alpha = self.config.a.eval(uml)?.float()?;
        uml.push(ui, StackLayer::Alpha(alpha));
        Ok(Var::default())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatConfig {
    #[serde(default = "default_zero")]
    x00: Expr,
    #[serde(default = "default_zero")]
    x01: Expr,
    #[serde(default = "default_zero")]
    x02: Expr,
    #[serde(default = "default_zero")]
    x03: Expr,
    #[serde(default = "default_zero")]
    x10: Expr,
    #[serde(default = "default_zero")]
    x11: Expr,
    #[serde(default = "default_zero")]
    x12: Expr,
    #[serde(default = "default_zero")]
    x13: Expr,
    #[serde(default = "default_zero")]
    x20: Expr,
    #[serde(default = "default_zero")]
    x21: Expr,
    #[serde(default = "default_zero")]
    x22: Expr,
    #[serde(default = "default_zero")]
    x23: Expr,
    #[serde(default = "default_zero")]
    x30: Expr,
    #[serde(default = "default_zero")]
    x31: Expr,
    #[serde(default = "default_zero")]
    x32: Expr,
    #[serde(default = "default_zero")]
    x33: Expr,
}
pub struct Mat {
    config: MatConfig,
}
impl Mat {
    pub fn new(config: MatConfig) -> Self {
        Self { config }
    }
}
impl Element for Mat {
    fn id(&self) -> Option<&str> {
        None
    }

    fn render(&self, ui: &mut Ui, uml: &Uml) -> Result<Var> {
        let x00 = self.config.x00.eval(uml)?.float()?;
        let x01 = self.config.x01.eval(uml)?.float()?;
        let x02 = self.config.x02.eval(uml)?.float()?;
        let x03 = self.config.x03.eval(uml)?.float()?;
        let x10 = self.config.x10.eval(uml)?.float()?;
        let x11 = self.config.x11.eval(uml)?.float()?;
        let x12 = self.config.x12.eval(uml)?.float()?;
        let x13 = self.config.x13.eval(uml)?.float()?;
        let x20 = self.config.x20.eval(uml)?.float()?;
        let x21 = self.config.x21.eval(uml)?.float()?;
        let x22 = self.config.x22.eval(uml)?.float()?;
        let x23 = self.config.x23.eval(uml)?.float()?;
        let x30 = self.config.x30.eval(uml)?.float()?;
        let x31 = self.config.x31.eval(uml)?.float()?;
        let x32 = self.config.x32.eval(uml)?.float()?;
        let x33 = self.config.x33.eval(uml)?.float()?;
        let mat = Matrix::from_column_slice(&[x00, x10, x20, x30, x01, x11, x21, x31, x02, x12, x22, x32, x03, x13, x23, x33]);
        uml.push(ui, StackLayer::Mat(mat));
        Ok(Var::default())
    }
}

pub struct Pop;
impl Element for Pop {
    fn id(&self) -> Option<&str> {
        None
    }

    fn render(&self, ui: &mut Ui, uml: &Uml) -> Result<Var> {
        uml.pop(ui);
        Ok(Var::default())
    }
}

#[derive(Clone, Copy)]
pub enum Var {
    Rect(Rect),
    ButtonState(ButtonState),
    Float(f32),
}
impl Default for Var {
    fn default() -> Self {
        Self::Float(0.)
    }
}

impl Var {
    pub fn float(self) -> Result<f32> {
        match self {
            Self::Float(f) => Ok(f),
            _ => bail!("expected float"),
        }
    }

    pub fn rect(self) -> Result<Rect> {
        match self {
            Self::Rect(r) => Ok(r),
            _ => bail!("expected rect"),
        }
    }
}

enum StackLayer {
    Mat(Matrix),
    Alpha(f32),
}
pub struct Uml {
    elements: Vec<TopLevel>,

    var_map: HashMap<String, Var>,
    persistent_vars: Vec<String>,

    stack: RefCell<Vec<StackLayer>>,

    t: f32,
    rt: f32,

    first_time: bool,
}

impl Default for Uml {
    fn default() -> Self {
        Self::new(Vec::new(), &[]).unwrap()
    }
}

impl Uml {
    pub fn new(elements: Vec<TopLevel>, global_defs: &[(String, Expr)]) -> Result<Self> {
        let mut res = Self {
            elements,

            var_map: HashMap::new(),
            persistent_vars: Vec::new(),

            stack: RefCell::new(Vec::new()),

            t: 0.,
            rt: 0.,

            first_time: true,
        };
        res.init(global_defs);
        Ok(res)
    }

    fn init(&mut self, global_defs: &[(String, Expr)]) {
        for (name, initial) in global_defs {
            self.var_map.insert(name.clone(), initial.eval(self).unwrap());
            self.persistent_vars.push(name.clone());
        }
    }

    fn push(&self, ui: &mut Ui, layer: StackLayer) {
        match layer {
            StackLayer::Mat(mat) => {
                self.stack.borrow_mut().push(StackLayer::Mat(ui.transform));
                ui.transform *= mat;
            }
            StackLayer::Alpha(alpha) => {
                self.stack.borrow_mut().push(StackLayer::Alpha(ui.alpha));
                ui.alpha *= alpha;
            }
        }
    }
    fn pop(&self, ui: &mut Ui) {
        match self.stack.borrow_mut().pop() {
            Some(StackLayer::Mat(mat)) => ui.transform = mat,
            Some(StackLayer::Alpha(alpha)) => ui.alpha = alpha,
            None => {}
        }
    }

    pub(crate) fn get_var(&self, id: &str) -> Result<&Var> {
        self.var_map.get(id).ok_or_else(|| anyhow!("variable not found: {id}"))
    }

    pub fn touch(&mut self, touch: &Touch, t: f32, rt: f32, action: &mut Option<String>) -> Result<bool> {
        self.t = t;
        self.rt = rt;
        for el in &self.elements {
            if let TopLevel::Element(el) = el {
                if el.touch(touch, self, action)? {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    pub fn render(&mut self, ui: &mut Ui, t: f32, rt: f32, vars: &[(&str, f32)]) -> Result<(f32, f32)> {
        self.var_map = std::mem::take(&mut self.var_map)
            .into_iter()
            .filter(|(key, _)| self.persistent_vars.contains(key))
            .collect::<HashMap<_, _>>();
        for (name, value) in vars.iter().copied().chain(std::iter::once(("version", 2.))) {
            self.var_map.insert(name.to_owned(), Var::Float(value));
        }

        #[derive(Clone, Copy, PartialEq, Eq)]
        enum IfState {
            IfPassed,
            IfFailed,
            Nopped,
        }

        let mut ifs = vec![IfState::IfPassed];

        let mut right = 0f32;
        let mut bottom = 0f32;
        self.t = t;
        self.rt = rt;
        ui.scope::<Result<()>>(|ui| {
            for el in &self.elements {
                match el {
                    TopLevel::Element(el) => {
                        if let Some(IfState::IfPassed) = ifs.last() {
                            let r = el.render(ui, self)?;
                            if let Var::Rect(r) = &r {
                                right = right.max(r.right());
                                bottom = bottom.max(r.bottom());
                            }
                            if let Some(id) = el.id() {
                                self.var_map.insert(id.to_owned(), r);
                            }
                        }
                    }
                    TopLevel::If(cond) => {
                        if let Some(IfState::IfPassed) = ifs.last() {
                            ifs.push(if cond.eval(self)?.float()? > 0. {
                                IfState::IfPassed
                            } else {
                                IfState::IfFailed
                            });
                        }
                    }
                    TopLevel::Else => {
                        if let Some(IfState::IfFailed) = ifs.last() {
                            *ifs.last_mut().unwrap() = IfState::IfPassed;
                        } else {
                            *ifs.last_mut().unwrap() = IfState::Nopped;
                        }
                    }
                    TopLevel::ElseIf(cond) => {
                        if let Some(IfState::IfFailed) = ifs.last() {
                            *ifs.last_mut().unwrap() = if cond.eval(self)?.float()? > 0. {
                                IfState::IfPassed
                            } else {
                                IfState::IfFailed
                            };
                        } else {
                            *ifs.last_mut().unwrap() = IfState::Nopped;
                        }
                    }
                    TopLevel::EndIf => {
                        ifs.pop();
                    }
                    TopLevel::GlobalDef(..) => {}
                }
            }

            Ok(())
        })?;
        if let Some(Var::Float(w)) = self.var_map.get("$w") {
            right = *w;
        }
        if let Some(Var::Float(h)) = self.var_map.get("$h") {
            bottom = *h;
        }
        self.first_time = false;

        Ok((right, bottom))
    }

    pub fn render_top(&mut self, ui: &mut Ui, t: f32, rt: f32) -> Result<()> {
        self.t = t;
        self.rt = rt;
        for el in &self.elements {
            if let TopLevel::Element(el) = el {
                el.render_top(ui, self)?;
            }
        }
        Ok(())
    }

    pub fn on_result(&self, t: f32, delete: bool) {
        for el in &self.elements {
            if let TopLevel::Element(el) = el {
                el.on_result(t, delete);
            }
        }
    }

    pub fn next_scene(&self) -> Option<NextScene> {
        for el in &self.elements {
            if let TopLevel::Element(el) = el {
                if let Some(next) = el.next_scene() {
                    return Some(next);
                }
            }
        }
        None
    }
}
