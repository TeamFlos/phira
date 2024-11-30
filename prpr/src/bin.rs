//! Binary serialization and deserialization for prpr data structures.
//! Currently:
//!   - [crate::core::Chart]
//!   - [crate::core::ChartSettings]
//!   - [crate::core::JudgeLine]
//!   - [crate::core::Note]
//!   - [crate::core::Object]
//!   - [crate::core::CtrlObject]
//!   - [crate::core::Anim]
//!   - [crate::core::Keyframe]
//!   - [macroquad::prelude::Color]

use crate::{
    core::{
        Anim, AnimVector, BezierTween, BpmList, Chart, ChartExtra, ChartSettings, ClampedTween, CtrlObject, JudgeLine, JudgeLineCache, JudgeLineKind,
        Keyframe, Note, NoteKind, Object, StaticTween, Tweenable, UIElement,
    },
    judge::{HitSound, JudgeStatus},
    parse::process_lines,
};
use anyhow::{bail, Result};
use byteorder::{LittleEndian as LE, ReadBytesExt, WriteBytesExt};
use macroquad::{prelude::Color, texture::Texture2D};
use std::{
    cell::RefCell,
    collections::HashMap,
    io::{Read, Write},
    ops::Deref,
    rc::Rc,
};

pub trait BinaryData: Sized {
    fn read_binary<R: Read>(r: &mut BinaryReader<R>) -> Result<Self>;
    fn write_binary<W: Write>(&self, w: &mut BinaryWriter<W>) -> Result<()>;
}

pub struct BinaryReader<R: Read>(pub R, u32);

impl<R: Read> BinaryReader<R> {
    pub fn new(reader: R) -> Self {
        Self(reader, 0)
    }

    pub fn reset_time(&mut self) {
        self.1 = 0;
    }

    pub fn time(&mut self) -> Result<f32> {
        self.1 += self.uleb()? as u32;
        Ok(self.1 as f32 / 1000.)
    }

    pub fn array<T: BinaryData>(&mut self) -> Result<Vec<T>> {
        (0..self.uleb()?).map(|_| self.read()).collect()
    }

    pub fn read<T: BinaryData>(&mut self) -> Result<T> {
        T::read_binary(self)
    }

    pub fn uleb(&mut self) -> Result<u64> {
        let mut result = 0;
        let mut shift = 0;
        loop {
            let byte = self.read::<u8>()?;
            result |= ((byte & 0x7f) as u64) << shift;
            if byte & 0x80 == 0 {
                break Ok(result);
            }
            shift += 7;
        }
    }
}

pub struct BinaryWriter<W: Write>(pub W, u32);

impl<W: Write> BinaryWriter<W> {
    pub fn new(writer: W) -> Self {
        Self(writer, 0)
    }

    pub fn reset_time(&mut self) {
        self.1 = 0;
    }

    pub fn time(&mut self, v: f32) -> Result<()> {
        let v = (v * 1000.).round() as u32;
        assert!(v >= self.1);
        self.uleb((v - self.1) as _)?;
        self.1 = v;
        Ok(())
    }

    pub fn array<T: BinaryData>(&mut self, v: &[T]) -> Result<()> {
        self.uleb(v.len() as _)?;
        for element in v {
            element.write_binary(self)?;
        }
        Ok(())
    }

    #[inline]
    pub fn write<T: BinaryData>(&mut self, v: &T) -> Result<()> {
        v.write_binary(self)
    }

    #[inline]
    pub fn write_val<T: BinaryData>(&mut self, v: T) -> Result<()> {
        v.write_binary(self)
    }

    pub fn uleb(&mut self, mut v: u64) -> Result<()> {
        loop {
            let mut byte = (v & 0x7f) as u8;
            v >>= 7;
            if v != 0 {
                byte |= 0x80;
            }
            self.write_val(byte)?;
            if v == 0 {
                break Ok(());
            }
        }
    }
}

impl BinaryData for u8 {
    fn read_binary<R: Read>(r: &mut BinaryReader<R>) -> Result<Self> {
        Ok(r.0.read_u8()?)
    }

    fn write_binary<W: Write>(&self, w: &mut BinaryWriter<W>) -> Result<()> {
        Ok(w.0.write_u8(*self)?)
    }
}

impl BinaryData for i32 {
    fn read_binary<R: Read>(r: &mut BinaryReader<R>) -> Result<Self> {
        Ok(r.0.read_i32::<LE>()?)
    }

    fn write_binary<W: Write>(&self, w: &mut BinaryWriter<W>) -> Result<()> {
        Ok(w.0.write_i32::<LE>(*self)?)
    }
}

impl BinaryData for bool {
    fn read_binary<R: Read>(r: &mut BinaryReader<R>) -> Result<Self> {
        Ok(r.0.read_u8()? == 1)
    }

    fn write_binary<W: Write>(&self, w: &mut BinaryWriter<W>) -> Result<()> {
        Ok(w.0.write_u8(if *self { 1 } else { 0 })?)
    }
}

impl BinaryData for f32 {
    fn read_binary<R: Read>(r: &mut BinaryReader<R>) -> Result<Self> {
        Ok(r.0.read_f32::<LE>()?)
    }

    fn write_binary<W: Write>(&self, w: &mut BinaryWriter<W>) -> Result<()> {
        Ok(w.0.write_f32::<LE>(*self)?)
    }
}

impl BinaryData for String {
    fn read_binary<R: Read>(r: &mut BinaryReader<R>) -> Result<Self> {
        Ok(String::from_utf8(r.array()?)?)
    }

    fn write_binary<W: Write>(&self, w: &mut BinaryWriter<W>) -> Result<()> {
        w.array(self.as_bytes())
    }
}

impl BinaryData for Color {
    fn read_binary<R: Read>(r: &mut BinaryReader<R>) -> Result<Self> {
        Ok(Self::from_rgba(r.read()?, r.read()?, r.read()?, r.read()?))
    }

    fn write_binary<W: Write>(&self, w: &mut BinaryWriter<W>) -> Result<()> {
        w.write_val((self.r * 256.) as u8)?;
        w.write_val((self.g * 256.) as u8)?;
        w.write_val((self.b * 256.) as u8)?;
        w.write_val((self.a * 256.) as u8)?;
        Ok(())
    }
}

// IMPLEMENTATIONS

impl<T: BinaryData> BinaryData for Keyframe<T> {
    fn read_binary<R: Read>(r: &mut BinaryReader<R>) -> Result<Self> {
        Ok(Self {
            time: r.time()?,
            value: r.read()?,
            tween: {
                let b = r.read::<u8>()?;
                match b & 0xC0 {
                    0 => StaticTween::get_rc(b),
                    0x80 => Rc::new(ClampedTween::new(b & 0x7f, r.read()?..r.read()?)),
                    0xC0 => Rc::new(BezierTween::new((r.read()?, r.read()?), (r.read()?, r.read()?))),
                    _ => panic!("invalid tween"),
                }
            },
        })
    }

    fn write_binary<W: Write>(&self, w: &mut BinaryWriter<W>) -> Result<()> {
        w.time(self.time)?;
        w.write(&self.value)?;
        let tween = self.tween.as_any();
        if let Some(t) = tween.downcast_ref::<StaticTween>() {
            w.write_val(t.0)?;
        } else if let Some(t) = tween.downcast_ref::<ClampedTween>() {
            w.write_val(0x80 | t.0)?;
            w.write_val(t.1.start)?;
            w.write_val(t.1.end)?;
        } else if let Some(t) = tween.downcast_ref::<BezierTween>() {
            w.write_val(0xC0)?;
            w.write_val(t.p1.0)?;
            w.write_val(t.p1.1)?;
            w.write_val(t.p2.0)?;
            w.write_val(t.p2.1)?;
        }
        Ok(())
    }
}

fn read_opt<R: Read, T: BinaryData + Tweenable>(r: &mut BinaryReader<R>) -> Result<Option<Box<Anim<T>>>> {
    Ok(match r.read::<u8>()? {
        0 => None,
        x => {
            let mut res = if x == 1 {
                Anim::default()
            } else {
                r.reset_time();
                Anim::new(r.array()?)
            };
            res.next = read_opt(r)?;
            Some(Box::new(res))
        }
    })
}

impl<T: BinaryData + Tweenable> BinaryData for Anim<T> {
    fn read_binary<R: Read>(r: &mut BinaryReader<R>) -> Result<Self> {
        Ok(*read_opt(r)?.unwrap())
    }

    fn write_binary<W: Write>(&self, w: &mut BinaryWriter<W>) -> Result<()> {
        let mut cur = self;
        loop {
            if cur.keyframes.is_empty() {
                w.write_val(1_u8)?;
            } else {
                w.write_val(2_u8)?;
                w.uleb(cur.keyframes.len() as _)?;
                w.reset_time();
                for kf in cur.keyframes.iter() {
                    kf.write_binary(w)?;
                }
            }
            if let Some(next) = &cur.next {
                cur = next;
            } else {
                w.write_val(0_u8)?;
                break Ok(());
            }
        }
    }
}

impl BinaryData for Object {
    fn read_binary<R: Read>(r: &mut BinaryReader<R>) -> Result<Self> {
        Ok(Self {
            alpha: r.read()?,
            scale: AnimVector(r.read()?, r.read()?),
            rotation: r.read()?,
            translation: AnimVector(r.read()?, r.read()?),
        })
    }

    fn write_binary<W: Write>(&self, w: &mut BinaryWriter<W>) -> Result<()> {
        w.write(&self.alpha)?;
        w.write(&self.scale.0)?;
        w.write(&self.scale.1)?;
        w.write(&self.rotation)?;
        w.write(&self.translation.0)?;
        w.write(&self.translation.1)?;
        Ok(())
    }
}

impl BinaryData for CtrlObject {
    fn read_binary<R: Read>(r: &mut BinaryReader<R>) -> Result<Self> {
        assert_eq!(r.read::<u8>()?, 8);
        Ok(Self {
            alpha: r.read()?,
            size: r.read()?,
            pos: r.read()?,
            y: r.read()?,
        })
    }

    fn write_binary<W: Write>(&self, w: &mut BinaryWriter<W>) -> Result<()> {
        w.write_val(8_u8)?;
        w.write(&self.alpha)?;
        w.write(&self.size)?;
        w.write(&self.pos)?;
        w.write(&self.y)?;
        Ok(())
    }
}

impl BinaryData for Note {
    fn read_binary<R: Read>(r: &mut BinaryReader<R>) -> Result<Self> {
        let object = r.read()?;
        let kind = match r.read::<u8>()? {
            0 => NoteKind::Click,
            1 => NoteKind::Hold {
                end_time: r.read()?,
                end_height: r.read()?,
            },
            2 => NoteKind::Flick,
            3 => NoteKind::Drag,
            _ => bail!("invalid note kind"),
        };
        let hitsound = HitSound::default_from_kind(&kind);
        Ok(Self {
            object,
            kind,
            hitsound,
            time: r.time()?,
            height: r.read()?,
            speed: if r.read()? { r.read::<f32>()? } else { 1. },
            above: r.read()?,
            multiple_hint: false,
            fake: r.read()?,
            judge: JudgeStatus::NotJudged,
        })
    }

    fn write_binary<W: Write>(&self, w: &mut BinaryWriter<W>) -> Result<()> {
        w.write(&self.object)?;
        match self.kind {
            NoteKind::Click => {
                w.write_val(0_u8)?;
            }
            NoteKind::Hold { end_time, end_height } => {
                w.write_val(1_u8)?;
                w.write_val(end_time)?;
                w.write_val(end_height)?;
            }
            NoteKind::Flick => w.write_val(2_u8)?,
            NoteKind::Drag => w.write_val(3_u8)?,
        }
        w.time(self.time)?;
        w.write_val(self.height)?;
        if self.speed == 1.0 {
            w.write_val(false)?;
        } else {
            w.write_val(true)?;
            w.write_val(self.speed)?;
        }
        w.write_val(self.above)?;
        w.write_val(self.fake)?;
        Ok(())
    }
}

impl BinaryData for JudgeLine {
    fn read_binary<R: Read>(r: &mut BinaryReader<R>) -> Result<Self> {
        r.reset_time();
        let object = r.read()?;
        let kind = match r.read::<u8>()? {
            0 => JudgeLineKind::Normal,
            1 => JudgeLineKind::Texture(Texture2D::empty().into(), r.read()?),
            2 => JudgeLineKind::Text(r.read()?),
            3 => JudgeLineKind::Paint(r.read()?, RefCell::default()),
            4 => unimplemented!(),
            _ => bail!("invalid judge line kind"),
        };
        let height = r.read()?;
        let mut notes = r.array()?;
        let color = r.read()?;
        let parent = match r.uleb()? {
            0 => None,
            x => Some(x as usize - 1),
        };
        let show_below = r.read()?;
        let cache = JudgeLineCache::new(&mut notes);
        let attach_ui = UIElement::from_u8(r.read()?);
        let ctrl_obj = RefCell::new(r.read()?);
        let incline = r.read()?;
        let z_index = r.read()?;
        Ok(Self {
            object,
            kind,
            height,
            notes,
            color,
            parent,
            show_below,

            attach_ui,
            ctrl_obj,
            incline,
            z_index,

            cache,
        })
    }

    fn write_binary<W: Write>(&self, w: &mut BinaryWriter<W>) -> Result<()> {
        w.write(&self.object)?;
        match &self.kind {
            JudgeLineKind::Normal => w.write_val(0_u8)?,
            JudgeLineKind::Texture(_, path) => {
                w.write_val(1_u8)?;
                w.write(path)?;
            }
            JudgeLineKind::Text(text) => {
                w.write_val(2_u8)?;
                w.write(text)?;
            }
            JudgeLineKind::Paint(events, _) => {
                w.write_val(3_u8)?;
                w.write(events)?;
            }
            JudgeLineKind::TextureGif(..) => {
                bail!("gif texture binary not supported");
            }
        }
        w.write(&self.height)?;
        w.array(&self.notes)?;
        w.write(&self.color)?;
        w.uleb(match self.parent {
            None => 0,
            Some(index) => index as u64 + 1,
        })?;
        w.write_val(self.show_below)?;
        w.write_val(self.attach_ui.map_or(0, |it| it as u8))?;
        w.write(self.ctrl_obj.borrow().deref())?;
        w.write(&self.incline)?;
        w.write(&self.z_index)?;
        Ok(())
    }
}

impl BinaryData for ChartSettings {
    fn read_binary<R: Read>(r: &mut BinaryReader<R>) -> Result<Self> {
        Ok(Self {
            pe_alpha_extension: r.read::<u8>()? == 1,
            hold_partial_cover: r.read::<u8>()? == 1,
        })
    }

    fn write_binary<W: Write>(&self, w: &mut BinaryWriter<W>) -> Result<()> {
        w.write_val(self.pe_alpha_extension as u8)?;
        w.write_val(self.hold_partial_cover as u8)?;
        Ok(())
    }
}

impl BinaryData for Chart {
    fn read_binary<R: Read>(r: &mut BinaryReader<R>) -> Result<Self> {
        let offset = r.read()?;
        let mut lines = r.array()?;
        process_lines(&mut lines);
        let settings = r.read()?;
        Ok(Chart::new(offset, lines, BpmList::new(vec![(0., 60.)]), settings, ChartExtra::default(), HashMap::new()))
    }

    fn write_binary<W: Write>(&self, w: &mut BinaryWriter<W>) -> Result<()> {
        w.write_val(self.offset)?;
        w.array(&self.lines)?;
        w.write(&self.settings)?;
        Ok(())
    }
}
