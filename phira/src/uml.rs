mod lexer;
mod parse;

use anyhow::Result;
use image::DynamicImage;
use macroquad::prelude::*;
use parse::{parse_uml, Expr, VarRef};
use prpr::{
    ext::{semi_black, semi_white, SafeTexture, ScaleType},
    task::Task,
    ui::Ui,
};
use serde::Deserialize;
use std::{cell::RefCell, collections::HashMap, fmt::Debug, str::FromStr};

use crate::client::File;

use self::parse::constant;

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
                if let Some(d) = s.strip_prefix("#") {
                    let int = u32::from_str_radix(d, 16).map_err(D::Error::custom)?;
                    let mut v = int.to_be_bytes();
                    if d.len() == 6 {
                        v[0] = 0xff;
                    }
                    Color::from_rgba(v[1], v[2], v[3], v[0])
                } else if let Some(d) = s.strip_prefix("w") {
                    semi_white(d.parse().map_err(D::Error::custom)?)
                } else if let Some(d) = s.strip_prefix("b") {
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
    fn render(&self, ui: &mut Ui, uml: &Uml, alpha: f32) -> Result<Var>;
}

#[derive(Debug, Deserialize)]
#[serde(default)]
struct BaseConfig {
    id: Option<String>,
    x: Expr,
    y: Expr,
}

impl Default for BaseConfig {
    fn default() -> Self {
        Self {
            id: None,
            x: constant(0.),
            y: constant(0.),
        }
    }
}

impl BaseConfig {
    pub fn pos(&self, uml: &Uml) -> Result<(f32, f32)> {
        Ok((self.x.eval(uml)?, self.y.eval(uml)?))
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TextConfig {
    #[serde(flatten)]
    base: BaseConfig,
    size: Expr,
    ax: f32,
    ay: f32,
    ml: bool,
    mw: Option<f32>,
    bl: bool,
    c: WrappedColor,
}

impl Default for TextConfig {
    fn default() -> Self {
        Self {
            base: BaseConfig::default(),
            size: constant(1.0),
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
        self.config.base.id.as_deref()
    }

    fn render(&self, ui: &mut Ui, uml: &Uml, alpha: f32) -> Result<Var> {
        let c = &self.config;
        let (x, y) = c.base.pos(uml)?;
        let mut text = ui
            .text(&self.text)
            .size(c.size.eval(uml)?)
            .pos(x, y)
            .anchor(c.ax, c.ay)
            .color(Color { a: c.c.0.a * alpha, ..c.c.0 });
        if c.ml {
            text = text.multiline();
        }
        if let Some(w) = c.mw {
            text = text.max_width(w);
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
    #[serde(flatten)]
    base: BaseConfig,
    url: File,
    w: Expr,
    h: Expr,
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
        self.config.base.id.as_deref()
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
        let (x, y) = c.base.pos(uml)?;
        let r = Rect::new(x, y, c.w.eval(uml)?, c.h.eval(uml)?);
        if let Some(tex) = self.tex.borrow().as_ref() {
            ui.fill_rect(r, (**tex, r, c.t, Color { a: c.c.0.a * alpha, ..c.c.0 }));
        }
        Ok(Var::Rect(r))
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
        Ok(Var::Float(self.value.eval(uml)?))
    }
}

pub enum Var {
    Rect(Rect),
    Float(f32),
}

pub struct Uml {
    elements: Vec<Box<dyn Element>>,

    vars: Vec<Var>,
    var_map: HashMap<String, usize>,
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
        }
    }

    pub(crate) fn get_var(&self, rf: &VarRef) -> Result<&Var> {
        Ok(&self.vars[rf.id(self)?])
    }

    pub fn render(&mut self, ui: &mut Ui, alpha: f32, vars: &[(&str, f32)]) -> Result<(f32, f32)> {
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
}

impl FromStr for Uml {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_uml(s)
    }
}
