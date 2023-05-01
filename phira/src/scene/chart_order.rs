prpr::tl_file!("chart_order");

use crate::page::ChartItem;
use macroquad::prelude::*;
use prpr::{ext::SafeTexture, ui::RectButton};

#[derive(Clone, Copy)]
pub enum ChartOrder {
    Default,
    Name,
    Rating,
}

impl ChartOrder {
    pub fn names() -> Vec<String> {
        ORDER_LABELS.iter().map(|it| tl!(it).into_owned()).collect()
    }

    pub fn apply(&self, charts: &mut [ChartItem]) {
        self.apply_delegate(charts, |it| it)
    }

    pub fn apply_delegate<T>(&self, charts: &mut [T], f: impl Fn(&T) -> &ChartItem) {
        match self {
            Self::Default => {
                charts.reverse();
            }
            Self::Name => {
                charts.sort_by(|x, y| f(x).info.name.cmp(&f(y).info.name));
            }
            Self::Rating => {}
        }
    }
}

const ORDER_NUM: usize = 6;
const ORDER_LABELS: [&str; ORDER_NUM] = ["time", "rev-time", "name", "rev-name", "rating", "rev-rating"];
pub static ORDERS: [(ChartOrder, bool); ORDER_NUM] = [
    (ChartOrder::Default, false),
    (ChartOrder::Default, true),
    (ChartOrder::Name, false),
    (ChartOrder::Name, true),
    (ChartOrder::Rating, true),
    (ChartOrder::Rating, false),
];

pub struct ChartOrderBox {
    icon_play: SafeTexture,
    button: RectButton,
    index: usize,
}

impl ChartOrderBox {
    pub fn new(icon_play: SafeTexture) -> Self {
        Self {
            icon_play,
            button: RectButton::new(),
            index: 0,
        }
    }

    pub fn touch(&mut self, touch: &Touch) -> bool {
        if self.button.touch(touch) {
            self.index += 1;
            if self.index == ORDER_NUM {
                self.index = 0;
            }
            return true;
        }
        false
    }

    pub fn to_order(&self) -> &'static (ChartOrder, bool) {
        &ORDERS[self.index]
    }
}
