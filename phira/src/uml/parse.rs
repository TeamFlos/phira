use super::{lexer::Token, Assign, Collection, Element, Image, Text, Uml, Var};
use crate::icons::Icons;
use anyhow::Result;
use logos::Logos;
use macroquad::prelude::Rect;
use prpr::ext::{JoinToString, RectExt, SafeTexture};
use serde::{
    de::{value::MapDeserializer, DeserializeOwned, Visitor},
    Deserialize,
};
use serde_json::Number;
use std::{cell::Cell, collections::HashMap, fmt::Display, iter::Peekable, sync::Arc};
use tap::Tap;

macro_rules! bail {
    ($($t:tt)*) => {
        return Err(format!($($t)*))
    }
}

type Lexer<'a> = Peekable<logos::Lexer<'a, Token>>;

fn take(lexer: &mut Lexer, token: Token) -> Result<(), String> {
    if lexer.next().as_ref().map(|it| it.as_ref()) != Some(Ok(&token)) {
        bail!("expected {:?}", token);
    }
    Ok(())
}

fn take_config<T: DeserializeOwned>(lexer: &mut Lexer) -> Result<T, String> {
    let mut map: HashMap<String, serde_json::Value> = HashMap::new();
    if lexer.peek() == Some(&Ok(Token::LBrace)) {
        lexer.next();
        loop {
            let Some(Ok(Token::Ident(name))) = lexer.next() else {
                bail!("expected attribute name");
            };
            take(lexer, Token::Colon)?;
            let value = match lexer.peek() {
                Some(Ok(Token::Quoted(s))) => serde_json::Value::String(s.to_owned()).tap(|_| {
                    lexer.next();
                }),
                Some(Ok(Token::Number(num))) => serde_json::Value::Number(Number::from_f64(*num as _).unwrap()).tap(|_| {
                    lexer.next();
                }),
                Some(Ok(Token::Bool(val))) => serde_json::Value::Bool(*val).tap(|_| {
                    lexer.next();
                }),
                _ => serde_json::Value::String(take_expr(lexer)?.to_string()),
            };
            map.insert(name, value);
            match lexer.next().unwrap().unwrap() {
                Token::Comma => continue,
                Token::RBrace => break,
                x => bail!("expected brace or comma, got {x:?}"),
            }
        }
    }
    T::deserialize(MapDeserializer::new(map.into_iter())).map_err(|it| it.to_string())
}

fn take_text(lexer: &mut Lexer) -> Result<String, String> {
    let Some(Ok(Token::Text(s))) = lexer.next() else {
        bail!("expected text");
    };
    Ok(s)
}

pub type Expr = Box<RawExpr>;

macro_rules! bail {
    ($($t:tt)*) => {
        return Err(format!($($t)*))
    }
}

pub struct VarRef {
    id: String,
    resolved: Cell<Option<usize>>,
}

impl VarRef {
    pub fn new(id: String) -> Self {
        Self {
            id,
            resolved: Cell::new(None),
        }
    }

    pub fn id(&self, uml: &Uml) -> Result<usize> {
        Ok(if let Some(id) = self.resolved.get() {
            id
        } else {
            let Some(&id) = uml.var_map.get(&self.id) else {
                anyhow::bail!("variable `{}` not found", self.id);
            };
            self.resolved.set(Some(id));
            id
        })
    }
}

impl Display for VarRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.id.fmt(f)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
}

impl BinOp {
    pub fn precedence(&self) -> u8 {
        match self {
            Self::Mul | Self::Div => 1,
            Self::Add | Self::Sub => 2,
        }
    }
}

type Function = Box<dyn Fn(&[Var]) -> Result<Var>>;

pub enum RawExpr {
    Literal(f32),
    Rect([Expr; 4]),
    Var(VarRef),
    VarSub(VarRef, String),
    Func(&'static str, Function, Vec<Expr>),
    BinOp(Expr, Expr, BinOp),
}

impl Display for RawExpr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Literal(val) => val.fmt(f),
            Self::Rect([x, y, w, h]) => write!(f, "[{x}, {y}, {w}, {h}]"),
            Self::Var(rf) => rf.fmt(f),
            Self::VarSub(rf, field) => {
                write!(f, "{rf}.{field}")
            }
            Self::BinOp(x, y, op) => {
                write!(
                    f,
                    "({x} {} {y})",
                    match op {
                        BinOp::Add => '+',
                        BinOp::Sub => '-',
                        BinOp::Mul => '*',
                        BinOp::Div => '/',
                    }
                )
            }
            Self::Func(name, _, inner) => {
                write!(f, "{name}({})", inner.iter().map(|it| it.to_string()).join(", "))
            }
        }
    }
}
impl std::fmt::Debug for RawExpr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        <Self as Display>::fmt(self, f)
    }
}

impl RawExpr {
    pub fn eval(&self, uml: &Uml) -> Result<Var> {
        Ok(match self {
            Self::Literal(val) => Var::Float(*val),
            Self::Rect([x, y, w, h]) => {
                Var::Rect(Rect::new(x.eval(uml)?.float()?, y.eval(uml)?.float()?, w.eval(uml)?.float()?, h.eval(uml)?.float()?))
            }
            Self::Var(rf) => *uml.get_var(rf)?,
            Self::VarSub(rf, field) => {
                let r = uml.get_var(rf)?.rect()?;
                Var::Float(match field.as_str() {
                    "x" | "l" => r.x,
                    "y" | "t" => r.y,
                    "w" => r.w,
                    "h" => r.h,
                    "r" => r.right(),
                    "b" => r.bottom(),
                    _ => anyhow::bail!("unknown field: {field}"),
                })
            }
            Self::BinOp(x, y, op) => {
                let x = x.eval(uml)?;
                let y = y.eval(uml)?.float()?;
                match x {
                    Var::Rect(r) => match op {
                        BinOp::Add => Var::Rect(r.feather(y)),
                        BinOp::Sub => Var::Rect(r.feather(-y)),
                        x => anyhow::bail!("invalid op on rect and float: {x:?}"),
                    },
                    Var::Float(x) => Var::Float(match op {
                        BinOp::Add => x + y,
                        BinOp::Sub => x - y,
                        BinOp::Mul => x * y,
                        BinOp::Div => x / y,
                    }),
                }
            }
            Self::Func(_, func, inner) => {
                let vals = inner.iter().map(|it| it.eval(uml)).collect::<Result<Vec<_>>>()?;
                func(&vals)?
            }
        })
    }
}

fn expect<const N: usize>(s: &[Var]) -> Result<[Var; N]> {
    s.try_into().map_err(|_| anyhow::anyhow!("expected {N} arguments"))
}

fn non_empty(s: &[Var]) -> Result<&[Var]> {
    if s.is_empty() {
        anyhow::bail!("expected arguments");
    }
    Ok(s)
}

fn wrap(f: fn(f32) -> f32) -> Function {
    Box::new(move |args| {
        let [arg] = expect::<1>(args)?;
        Ok(Var::Float(f(arg.float()?)))
    })
}

fn wrap2(f: fn(f32, f32) -> f32) -> Function {
    Box::new(move |args| {
        let [x, y] = expect::<2>(args)?;
        Ok(Var::Float(f(x.float()?, y.float()?)))
    })
}

fn take_atom(lexer: &mut Lexer) -> Result<Expr, String> {
    Ok(match lexer.next().transpose().ok().flatten().ok_or_else(|| "expected atom".to_owned())? {
        Token::Ident(s) => match lexer.peek() {
            Some(&Ok(Token::Period)) => {
                lexer.next();
                let Some(Ok(Token::Ident(f))) = lexer.next() else {
                    bail!("expected field")
                };
                RawExpr::VarSub(VarRef::new(s), f).into()
            }
            Some(&Ok(Token::LBrace)) => {
                lexer.next();
                let (name, func) = match s.as_str() {
                    "sin" => ("sin", wrap(f32::sin)),
                    "cos" => ("cos", wrap(f32::cos)),
                    "tan" => ("tan", wrap(f32::tan)),
                    "abs" => ("abs", wrap(f32::abs)),
                    "exp" => ("exp", wrap(f32::exp)),
                    "atan2" => ("atan2", wrap2(f32::atan2)),
                    "ln" => ("ln", wrap(f32::ln)),
                    "sig" => ("sig", wrap(f32::signum)),
                    "max" => (
                        "max",
                        Box::new(|args: &[Var]| {
                            Ok(Var::Float(
                                non_empty(args)?
                                    .iter()
                                    .fold(Result::<f32>::Ok(f32::NEG_INFINITY), |mx, x| Ok(mx?.max(x.float()?)))
                                    .unwrap(),
                            ))
                        }) as Function,
                    ),
                    "min" => (
                        "min",
                        Box::new(|args: &[Var]| {
                            Ok(Var::Float(
                                non_empty(args)?
                                    .iter()
                                    .fold(Result::<f32>::Ok(f32::INFINITY), |mx, x| Ok(mx?.min(x.float()?)))
                                    .unwrap(),
                            ))
                        }) as Function,
                    ),
                    "clamp" => (
                        "clamp",
                        Box::new(|args: &[Var]| {
                            let [x, lo, hi] = expect::<3>(args)?;
                            Ok(Var::Float(x.float()?.clamp(lo.float()?, hi.float()?)))
                        }) as Function,
                    ),
                    _ => bail!("unknown function: {s}"),
                };
                let mut args = vec![take_expr(lexer)?];
                loop {
                    match lexer.next() {
                        Some(Ok(Token::Comma)) => {}
                        Some(Ok(Token::RBrace)) => break,
                        x => bail!("expected brace or comma, got {x:?}"),
                    }
                    args.push(take_expr(lexer)?);
                }
                RawExpr::Func(name, func, args).into()
            }
            _ => RawExpr::Var(VarRef::new(s)).into(),
        },
        Token::Number(val) => RawExpr::Literal(val).into(),
        Token::LBrace => {
            let res = take_expr(lexer)?;
            let Some(Ok(Token::RBrace)) = lexer.next() else {
                bail!("expected right brace")
            };
            res
        }
        Token::LBracket => {
            let mut one = || -> Result<Expr, String> {
                let res = take_expr(lexer)?;
                take(lexer, Token::Comma)?;
                Ok(res)
            };
            RawExpr::Rect([one()?, one()?, one()?, {
                let res = take_expr(lexer)?;
                take(lexer, Token::RBracket)?;
                res
            }])
            .into()
        }
        x => bail!("expected atom, got {x:?}"),
    })
}

fn take_op(lexer: &mut Lexer) -> Result<Option<BinOp>, String> {
    let Some(nxt) = lexer.peek() else { return Ok(None) };
    let res = match nxt.as_ref().unwrap() {
        Token::Add => BinOp::Add,
        Token::Sub => BinOp::Sub,
        Token::Mul => BinOp::Mul,
        Token::Div => BinOp::Div,
        _ => {
            return Ok(None);
        }
    };
    lexer.next();
    Ok(Some(res))
}

fn take_expr(lexer: &mut Lexer) -> Result<Expr, String> {
    let mut vals = vec![take_atom(lexer)?];
    let mut ops: Vec<BinOp> = Vec::new();
    fn apply(vals: &mut Vec<Expr>, op: BinOp) {
        let y = vals.pop().unwrap();
        let x = vals.pop().unwrap();
        vals.push(RawExpr::BinOp(x, y, op).into());
    }
    loop {
        let Some(op) = take_op(lexer)? else { break };
        let pred = op.precedence();
        while let Some(last) = ops.last() {
            if last.precedence() <= pred {
                apply(&mut vals, *last);
                ops.pop();
            } else {
                break;
            }
        }
        ops.push(op);
        vals.push(take_atom(lexer)?);
    }
    while let Some(op) = ops.pop() {
        apply(&mut vals, op);
    }
    if vals.len() != 1 {
        panic!("invalid expression");
    }
    Ok(vals.into_iter().next().unwrap())
}

pub fn parse_expr(s: &str) -> Result<Expr, String> {
    take_expr(&mut Token::lexer(s).peekable())
}

impl<'de> Deserialize<'de> for Expr {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de;

        struct ExprVisitor;
        impl<'de> Visitor<'de> for ExprVisitor {
            type Value = Expr;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("an expression")
            }

            fn visit_f32<E: de::Error>(self, value: f32) -> Result<Self::Value, E> {
                Ok(constant(value))
            }

            fn visit_f64<E: de::Error>(self, value: f64) -> Result<Self::Value, E> {
                Ok(constant(value as _))
            }

            fn visit_string<E: de::Error>(self, value: String) -> Result<Self::Value, E> {
                parse_expr(&value).map_err(E::custom)
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                parse_expr(v).map_err(E::custom)
            }
        }

        deserializer.deserialize_any(ExprVisitor)
    }
}

pub fn constant(val: f32) -> Expr {
    Box::new(RawExpr::Literal(val))
}

pub fn take_element(icons: &Arc<Icons>, rank_icons: &[SafeTexture; 8], lexer: &mut Lexer) -> Result<Option<Box<dyn Element>>, String> {
    let Some(nxt) = lexer.next() else { return Ok(None) };
    let Token::Ident(ty) = nxt? else {
        bail!("expected element");
    };
    Ok(Some(match ty.as_str() {
        "p" => Box::new(Text::new(take_config(lexer)?, take_text(lexer)?)),
        "img" => Box::new(Image::new(take_config(lexer)?)),
        "col" => Box::new(Collection::new(Arc::clone(icons), rank_icons.clone(), take_config(lexer)?)),
        "let" => {
            let Some(Ok(Token::Ident(id))) = lexer.next() else {
                bail!("expected variable name");
            };
            take(lexer, Token::Assign)?;
            Box::new(Assign::new(id, take_expr(lexer)?))
        }
        _ => bail!("unknown element type: {}", ty),
    }))
}

pub fn parse_uml(s: &str, icons: &Arc<Icons>, rank_icons: &[SafeTexture; 8]) -> Result<Uml, String> {
    let mut lexer = Token::lexer(s).peekable();
    let mut elements = Vec::new();
    while let Some(element) = take_element(icons, rank_icons, &mut lexer)? {
        elements.push(element);
    }
    Ok(Uml::new(elements))
}
