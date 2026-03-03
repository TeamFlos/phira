prpr_l10n::tl_file!("chart_order");

use crate::page::ChartItem;

#[derive(Clone, Copy)]
pub enum ChartOrder {
    Default,
    Name,
    Difficulty,
    Rating,
}

impl ChartOrder {
    pub fn names() -> Vec<String> {
        ORDER_LABELS.iter().map(|it| tl!(*it).into_owned()).collect()
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

const ORDER_NUM: usize = 4;
const ORDER_LABELS: [&str; ORDER_NUM] = ["time", "rating", "name", "difficulty"];
pub static ORDERS: [ChartOrder; ORDER_NUM] = [ChartOrder::Default, ChartOrder::Rating, ChartOrder::Name, ChartOrder::Difficulty];
