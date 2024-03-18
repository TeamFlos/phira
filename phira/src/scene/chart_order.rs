prpr::tl_file!("chart_order");

use crate::page::ChartItem;

#[derive(Clone, Copy)]
pub enum ChartOrder {
    Default,
    Name,
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
            Self::Rating => {}
        }
    }
}

const ORDER_NUM: usize = 6;
const ORDER_LABELS: [&str; ORDER_NUM] = ["time", "rev-time", "rating", "rev-rating", "name", "rev-name"];
pub static ORDERS: [(ChartOrder, bool); ORDER_NUM] = [
    (ChartOrder::Default, false),
    (ChartOrder::Default, true),
    (ChartOrder::Rating, true),
    (ChartOrder::Rating, false),
    (ChartOrder::Name, false),
    (ChartOrder::Name, true),
];
