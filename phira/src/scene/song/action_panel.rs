use macroquad::{input::Touch, math::Rect};
use prpr::{ext::semi_black, time::TimeManager, ui::{DRectButton, Ui}};
use tracing::debug;

pub struct ActionPanel {
    buttonl: DRectButton,
    buttonr: DRectButton,
    last_height: f32,
    last_width: f32,
}


impl ActionPanel {
    pub fn new() -> Self {
        Self {
            buttonl: DRectButton::new(),
            buttonr: DRectButton::new(),
            last_height:0.2,
            last_width:0.
        }
    }
    pub fn render(&mut self,ui: &mut Ui,width: f32, t:f32,) -> (f32, f32) {
        let mut sy = 0.02;
        let h_pad = 0.01;
        let w_pad = 0.01;
        ui.scope(|ui| {
            ui.dy(sy);
            let y_top = sy-h_pad;
            ui.fill_rect(Rect::new(-2.0*w_pad, 0., self.last_width + 5.0*w_pad, self.last_height), semi_black(0.5));
            macro_rules! dy {
                ($dy:expr) => {{
                    let dy = $dy;
                    sy += dy;
                    ui.dy(dy);
                }};
            }
            macro_rules! gain_w {
                ($w:expr) => {
                    let w = $w;
                    self.last_width = f32::max(self.last_width, w);
                };
            }
            dy!(h_pad);
            let r = ui.text("Stable stat:").size(0.7).draw();
            ui.dx(r.w + w_pad);
            let w2 = ui.text("test text").size(0.7).draw().w;
            ui.dx(-r.w-w_pad);
            gain_w!(r.w + w_pad + w2);
            dy!(r.h + h_pad);
            self.buttonl.render_text(ui, Rect::new(0., 0., 0.25, 0.075), t, "Test1", 0.6, true);
            ui.dx(0.25 + w_pad);
            self.buttonr.render_text(ui, Rect::new(0., 0., 0.25, 0.075), t, "Test", 0.6, true);
            dy!(0.075);
            gain_w!(0.5);
        });
        self.last_height = sy + h_pad;
        (self.last_width, sy)
    }
    pub fn touch(&mut self,touch: &Touch, t:f32) {
        if self.buttonl.touch(touch, t) {
            debug!("touched test button")
        }
        if self.buttonr.touch(touch, t) {
            debug!("touched test button")
        }
    }
    pub fn update(&mut self, tm: &mut TimeManager) {
        
    }
}