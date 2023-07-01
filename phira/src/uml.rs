mod lexer;
mod parse;

pub use parse::parse_uml;

use self::parse::constant;
use crate::{
    charts_view::{ChartDisplayItem, ChartsView},
    client::{recv_raw, Client, File},
    icons::Icons,
};
use anyhow::{bail, Result};
use image::DynamicImage;
use macroquad::prelude::*;
use parse::{Expr, VarRef};
use prpr::{
    ext::{semi_black, semi_white, SafeTexture, ScaleType},
    scene::NextScene,
    task::Task,
    ui::Ui,
};
use serde::Deserialize;
use std::{cell::RefCell, collections::HashMap, fmt::Debug, sync::Arc};
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
    fn touch(&self, _touch: &Touch, _uml: &Uml) -> Result<bool> {
        Ok(false)
    }
    fn render(&self, ui: &mut Ui, uml: &Uml, alpha: f32) -> Result<Var>;
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
    ax: f32,
    ay: f32,
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
            ax: 0.,
            ay: 0.,
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

    fn render(&self, ui: &mut Ui, uml: &Uml, alpha: f32) -> Result<Var> {
        let c = &self.config;
        let mut text = ui
            .text(&self.text)
            .pos(c.x.eval(uml)?.float()?, c.y.eval(uml)?.float()?)
            .anchor(c.ax, c.ay)
            .size(c.size.eval(uml)?.float()?)
            .color(Color { a: c.c.0.a * alpha, ..c.c.0 });
        if c.ml {
            text = text.multiline();
        }
        if let Some(w) = &c.mw {
            text = text.max_width(w.eval(uml)?.float()?);
        }
        if !c.bl {
            text = text.no_baseline();
        }
        Ok(Var::Rect(text.draw()))
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

    fn render(&self, ui: &mut Ui, uml: &Uml, alpha: f32) -> Result<Var> {
        let c = &self.config;
        let mut guard = self.task.borrow_mut();
        if let Some(task) = guard.as_mut() {
            if let Some(res) = task.take() {
                match res {
                    Ok(val) => *self.tex.borrow_mut() = Some(val.into()),
                    Err(err) => {
                        warn!("failed to load image ({}): {:?}", c.url.url, err);
                    }
                }
                drop(guard);
                *self.task.borrow_mut() = None;
            }
        }
        let r = c.r.eval(uml)?.rect()?;
        if let Some(tex) = self.tex.borrow().as_ref() {
            ui.fill_rect(r, (**tex, r, c.t, Color { a: c.c.0.a * alpha, ..c.c.0 }));
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
        Ok(Self(f32::deserialize(deserializer)?.round() as i32))
    }
}

fn default_row_num() -> I32 {
    I32(4)
}

fn default_chart_height() -> f32 {
    0.3
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
    rh: f32,
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
        charts_view.row_height = config.rh;
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

    fn touch(&self, touch: &Touch, uml: &Uml) -> Result<bool> {
        self.state.borrow_mut().charts_view.touch(touch, uml.t, uml.rt)
    }

    fn render(&self, ui: &mut Ui, uml: &Uml, alpha: f32) -> Result<Var> {
        let mut state = self.state.borrow_mut();
        if let Some(task) = &mut state.task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        warn!("failed to fetch collection: {:?}", err);
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

        state.charts_view.update(t)?;
        state.charts_view.render(ui, r, alpha, t);

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

    fn render(&self, _ui: &mut Ui, uml: &Uml, _alpha: f32) -> Result<Var> {
        self.value.eval(uml)
    }
}

#[derive(Clone, Copy)]
pub enum Var {
    Rect(Rect),
    Float(f32),
}

impl Var {
    pub fn float(self) -> Result<f32> {
        match self {
            Self::Rect(_) => bail!("expected float"),
            Self::Float(f) => Ok(f),
        }
    }

    pub fn rect(self) -> Result<Rect> {
        match self {
            Self::Float(_) => bail!("expected rect"),
            Self::Rect(r) => Ok(r),
        }
    }
}

pub struct Uml {
    elements: Vec<Box<dyn Element>>,

    vars: Vec<Var>,
    var_map: HashMap<String, usize>,

    t: f32,
    rt: f32,
}

impl Default for Uml {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

impl Uml {
    pub fn new(elements: Vec<Box<dyn Element>>) -> Self {
        Self {
            elements,

            vars: Vec::new(),
            var_map: HashMap::new(),

            t: 0.,
            rt: 0.,
        }
    }

    pub(crate) fn get_var(&self, rf: &VarRef) -> Result<&Var> {
        Ok(&self.vars[rf.id(self)?])
    }

    pub fn touch(&mut self, touch: &Touch, t: f32, rt: f32) -> Result<bool> {
        self.t = t;
        self.rt = rt;
        for element in &self.elements {
            if element.touch(touch, self)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn render(&mut self, ui: &mut Ui, t: f32, rt: f32, alpha: f32, vars: &[(&str, f32)]) -> Result<(f32, f32)> {
        let first_time = self.var_map.is_empty();
        self.vars.clear();
        for (name, val) in vars {
            if first_time {
                self.var_map.insert(name.to_string(), self.vars.len());
            }
            self.vars.push(Var::Float(*val));
        }
        let mut right = 0f32;
        let mut bottom = 0f32;
        self.t = t;
        self.rt = rt;
        for elem in &self.elements {
            let r = elem.render(ui, self, alpha)?;
            if let Var::Rect(r) = &r {
                right = right.max(r.right());
                bottom = bottom.max(r.bottom());
            }
            if let Some(id) = elem.id() {
                if first_time {
                    self.var_map.insert(id.to_owned(), self.vars.len());
                }
                self.vars.push(r);
            }
        }
        if let Some(Var::Float(w)) = self.var_map.get("$w").map(|it| &self.vars[*it]) {
            right = *w;
        }
        if let Some(Var::Float(h)) = self.var_map.get("$h").map(|it| &self.vars[*it]) {
            bottom = *h;
        }
        Ok((right, bottom))
    }

    pub fn render_top(&mut self, ui: &mut Ui, t: f32, rt: f32) -> Result<()> {
        self.t = t;
        self.rt = rt;
        for element in &self.elements {
            element.render_top(ui, self)?;
        }
        Ok(())
    }

    pub fn on_result(&self, t: f32, delete: bool) {
        for element in &self.elements {
            element.on_result(t, delete);
        }
    }

    pub fn next_scene(&self) -> Option<NextScene> {
        for element in &self.elements {
            if let Some(s) = element.next_scene() {
                return Some(s);
            }
        }
        None
    }
}
