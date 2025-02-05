use prpr::ui::Ui;

pub struct ActionPanel {
    
}


pub fn render_action_panel(ui: &mut Ui, width: f32) -> (f32, f32) {
    let mut sy = 0.02;
    ui.scope(|ui| {
        let s = 0.01;
        ui.dx(0.01);
        ui.dy(sy);
        macro_rules! dy {
            ($dy:expr) => {{
                let dy = $dy;
                sy += dy;
                ui.dy(dy);
            }};
        }
        dy!(0.01);
        let r = ui.text("1111").size(0.9).draw();
        dy!(r.h + 0.04);
        let r = ui.text("1111").size(0.9).draw();
        dy!(r.h + 0.04);
        let r = ui.text("1111").size(0.9).draw();
        dy!(r.h + 0.04);
        let r = ui.text("1111").size(0.9).draw();
        dy!(r.h + 0.04);
        let r = ui.text("1111").size(0.9).draw();
        dy!(r.h + 0.04);
        let r = ui.text("1111").size(0.9).draw();
        dy!(r.h + 0.04);
        let r = ui.text("1111").size(0.9).draw();
        dy!(r.h + 0.04);
        let r = ui.text("1111").size(0.9).draw();
        dy!(r.h + 0.04);
        let r = ui.text("1111").size(0.9).draw();
        dy!(r.h + 0.04);
        let r = ui.text("1111").size(0.9).draw();
        dy!(r.h + 0.04);
        let r = ui.text("1111").size(0.9).draw();
        dy!(r.h + 0.04);
        let r = ui.text("1111").size(0.9).draw();
        dy!(r.h + 0.04);
        let rt = 0.22;
        ui.dx(rt);
    });
    (width, dbg!(sy))
}