//! 组件画廊:库内全部控件的离屏渲染自检(独立于宿主应用,不装字体,
//! 文案用 ASCII 避免 CJK 字形依赖)。存 PNG 到 /tmp/shujing_ui/kit_gallery.png。

use egui::{Align2, RichText, Sense};
use ui_kit::theme::{self, BtnKind};

#[test]
fn gallery_render() {
    let out_dir = std::path::PathBuf::from("/tmp/shujing_ui");
    let _ = std::fs::create_dir_all(&out_dir);
    let setup = std::cell::Cell::new(false);
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(980.0, 760.0))
        .with_pixels_per_point(2.0)
        .build(|ctx| {
            if !setup.get() {
                setup.set(true);
                theme::install_fallback_fonts(ctx);
                theme::set_dark(true);
                // 字体在下一帧生效:首帧只做安装(与宿主应用的 render 助手同款教训)
                return;
            }
            theme::apply(ctx);
            egui::CentralPanel::default().show(ctx, |ui| {
                let p = theme::c();
                ui.label(theme::strong("ui-kit component gallery", 15.0).color(p.text));

                ui.add_space(10.0);
                ui.label(RichText::new("Buttons").size(11.0).color(p.muted));
                ui.horizontal(|ui| {
                    let _ = theme::button(ui, "Primary", BtnKind::Primary);
                    let _ = theme::button(ui, "Success", BtnKind::Success);
                    let _ = theme::button(ui, "Danger", BtnKind::Danger);
                    let _ = theme::button(ui, "Secondary", BtnKind::Secondary);
                    let (mini, _) = theme::mini_text_button(ui, "mini", p.red);
                    let _ = mini;
                });

                ui.add_space(10.0);
                ui.label(RichText::new("Checkbox").size(11.0).color(p.muted));
                let mut on = true;
                let mut off = false;
                ui.horizontal(|ui| {
                    let _ = theme::checkbox(ui, &mut on, "checked");
                    let _ = theme::checkbox(ui, &mut off, "unchecked");
                });

                ui.add_space(10.0);
                ui.label(RichText::new("Select").size(11.0).color(p.muted));
                let sel = "Violet".to_string();
                if let Some(v) = theme::select(
                    ui,
                    "gal-sel",
                    &sel,
                    &[
                        ("Indigo".into(), "Indigo".into()),
                        ("Violet".into(), "Violet".into()),
                        ("Cyan".into(), "Cyan".into()),
                    ],
                    180.0,
                ) {
                    let _ = v;
                }

                ui.add_space(10.0);
                ui.label(RichText::new("Inputs").size(11.0).color(p.muted));
                let mut one = "single line".to_string();
                theme::form_field(ui, "Label", "gal-one", &mut one, 320.0, false);
                let mut multi = "l0\nl1\nl2\nl3".to_string();
                theme::form_field_multiline(ui, "Batch", "gal-multi", &mut multi, 320.0, 4);
                let mut q = "keyword".to_string();
                let _ = theme::search_box(ui, "gal-search", &mut q, "filter tables", 260.0);

                ui.add_space(10.0);
                ui.label(RichText::new("Chrome").size(11.0).color(p.muted));
                theme::hairline_bottom(ui);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("chip").size(11.0));
                    let _ = ui_kit::icons::chip(ui, "HASH", p.accent);
                    let (r, _) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), Sense::hover());
                    ui.painter().text(
                        r.center(),
                        Align2::CENTER_CENTER,
                        "+",
                        egui::FontId::proportional(16.0),
                        p.muted,
                    );
                });
                theme::hairline_top(ui);

                ui.add_space(10.0);
                ui.label(RichText::new("Modal kit (frame+title+footer)").size(11.0).color(p.muted));
                egui::Frame::new()
                    .fill(p.bg)
                    .corner_radius(egui::CornerRadius::same(12))
                    .show(ui, |ui| {
                        ui.set_width(360.0);
                        theme::modal_title(ui, "Dialog");
                        ui.label(RichText::new("body text").size(12.5).color(p.text));
                        theme::modal_footer(ui, |ui| {
                            let _ = theme::button(ui, "OK", BtnKind::Primary);
                            let _ = theme::button(ui, "Cancel", BtnKind::Secondary);
                        });
                    });

                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.label(RichText::new("accent swatches:").size(11.0).color(p.muted));
                    ui.horizontal(|ui| {
                        for i in 0..6 {
                            let (r, _) =
                                ui.allocate_exact_size(egui::vec2(18.0, 18.0), Sense::hover());
                            let col = if i % 2 == 0 {
                                theme::accent_color_dark(i)
                            } else {
                                theme::accent_color_light(i)
                            };
                            ui.painter()
                                .rect_filled(r, egui::CornerRadius::same(4), col);
                        }
                    });
                });
            });
        });
    harness.run_steps(4);
    let img = harness.render().expect("render");
    img.save(out_dir.join("kit_gallery.png")).expect("save");
}
