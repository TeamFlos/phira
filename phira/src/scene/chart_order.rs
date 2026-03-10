prpr_l10n::tl_file!("chart_order");

use std::borrow::Cow;

use crate::page::ChartItem;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ChartOrder {
    Default,
    Name,
    Difficulty,
    Rating,
}

impl ChartOrder {
    pub fn label(&self) -> Cow<'static, str> {
        match self {
            Self::Default => tl!("time"),
            Self::Name => tl!("name"),
            Self::Difficulty => tl!("difficulty"),
            Self::Rating => tl!("rating"),
        }
    }

    pub fn apply<T>(&self, charts: &mut [T], f: impl Fn(&T) -> &ChartItem) {
        match self {
            Self::Default => {}
            Self::Name => {
                charts.sort_by(|x, y| f(x).info.name.cmp(&f(y).info.name));
            }
            Self::Difficulty => {
                charts.sort_by(|x, y| {
                    f(x).info
                        .difficulty
                        .partial_cmp(&f(y).info.difficulty)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
            }
            Self::Rating => {}
        }
    }
}
