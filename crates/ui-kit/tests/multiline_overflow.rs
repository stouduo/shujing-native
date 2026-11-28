//! 多行输入框的溢出纪律:内容超过预设行数时,文字必须留在框里(框内滚动),
//! 不能画到外框之外,也不能把下方的控件挤走。
//!
//! egui 多行 TextEdit 的高度是 max(内容高度, desired_rows 行高)——desired_rows
//! 是最小行数,文本框会随内容无限撑大。外层预留框只有 rows 行高,内容一多就越过
//! 框画到外面(数镜里"文字压到下面的按钮上"的一个来源)。

use std::cell::RefCell;
use ui_kit::theme;

/// 预设 3 行的框高(行高 17 × 3 + 上下内边距 12)
const BOX_H: f32 = 3.0 * 17.0 + 12.0;
/// 框的上下内边距
const PAD_Y: f32 = 6.0;

#[derive(Default, Clone)]
struct Measured {
    box_top: f32,
    /// 框下方那个按钮的上缘
    next_top: f32,
    /// 文本编辑区的底
    text_bottom: f32,
}

/// 画一个预设 3 行的框,内容 `lines` 行,下面紧跟一个按钮。
/// 返回 (按钮上缘-框顶, 框内第一行文字的亮像素数, 框外亮像素数)
fn measure(kind: &str, lines: usize) -> (f32, u32, u32) {
    let text = RefCell::new(
        (0..lines)
            .map(|i| format!("line number {i}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    let m: std::rc::Rc<RefCell<Option<Measured>>> = Default::default();
    let inner = m.clone();
    let setup = std::cell::Cell::new(false);
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(520.0, 600.0))
        .with_pixels_per_point(2.0)
        .build_state(
            move |ctx, text: &mut RefCell<String>| {
                if !setup.get() {
                    setup.set(true);
                    theme::install_fallback_fonts(ctx);
                    theme::set_dark(true);
                    return;
                }
                theme::apply(ctx);
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.add_space(10.0);
                    let box_top = ui.cursor().min.y;
                    // 带标签的多行框:文本区在标签列之后;等宽框:从左边距起
                    let text = text.get_mut();
                    let resp = match kind {
                        "form" => theme::form_field_multiline(ui, "Label", "t", text, 300.0, 3),
                        _ => theme::code_box(ui, "t", text, 300.0, 3, ""),
                    };
                    let text_bottom = resp.rect.bottom();
                    let b = theme::button(ui, "Next", theme::BtnKind::Secondary);
                    *inner.borrow_mut() = Some(Measured {
                        box_top,
                        next_top: b.rect.top(),
                        text_bottom,
                    });
                });
            },
            text,
        );
    harness.run_steps(4);
    let m = m.borrow().clone().expect("measured");
    let img = harness.render().expect("render");
    // 顺手落盘,便于人工核对
    let out = std::path::PathBuf::from(format!("/tmp/shujing_ui/kit_overflow_{kind}_{lines}.png"));
    let _ = std::fs::create_dir_all(out.parent().unwrap());
    let _ = img.save(&out);
    let gap = m.next_top - m.box_top;
    // 框底不直接量:用按钮上缘反推(form 布局框底=按钮上缘-14,等宽=-6),
    // 避免对 form_row 内部结构的坐标推算
    let box_bottom = m.next_top - if kind == "form" { 14.0 } else { 6.0 };
    // 文字列范围:form 的文字从标签列后(~132)起,code 的从框左缘(~20)起,
    // 两种布局都要覆盖到
    let x0 = 30.0;
    let x1 = 290.0;
    let bright_in = |y0: f32, y1: f32| -> u32 {
        let mut n = 0u32;
        for y in (y0 * 2.0) as u32..(y1 * 2.0) as u32 {
            for x in (x0 * 2.0) as u32..(x1 * 2.0) as u32 {
                let [r, g, b, _a] = img.get_pixel(x, y).0;
                if r > 90 || g > 90 || b > 90 {
                    n += 1;
                }
            }
        }
        n
    };
    // 框外亮像素:框底以下到按钮上缘之前(避开 1px 边框线与按钮自身)
    let outside = bright_in(box_bottom + 2.5, m.next_top - 2.0);
    // 框内亮像素:整个框内区(避开上下 8px 边框带)——修复不能把内容藏没
    let inside = bright_in(m.box_top + 8.0, m.box_top + BOX_H - 8.0);
    (gap, inside, outside)
}

/// 框与下方按钮的间隔:form=框底+form_row 底隙 8+行距 6;code=框底+行距 6。
/// 精确到 ±1:间隙被吃掉(双重布局回退)或被撑大(框高溢出)都要红
fn assert_not_pushed(kind: &str, gap: f32) {
    let want = BOX_H + if kind == "form" { 14.0 } else { 6.0 };
    assert!(
        (want - 0.5..=want + 0.5).contains(&gap),
        "{kind}: 框与下方控件的间隔 {gap} 不在预期 {want}±0.5 内(被挤走或被撑大)"
    );
}

/// 内容远超预设行数:文字留在框里,下面的控件不被挤走
#[test]
fn multiline_boxes_do_not_overflow() {
    for kind in ["form", "code"] {
        let (gap, inside, outside) = measure(kind, 12);
        assert!(inside > 20, "{kind}: 框内没有显示内容");
        assert_not_pushed(kind, gap);
        assert_eq!(outside, 0, "{kind}: 框外画出了文字");
    }
}

/// 内容不足预设行数:框仍是预设高度,下方控件位置不变(修复不能改坏这一半)
#[test]
fn multiline_boxes_fit_short_content() {
    for kind in ["form", "code"] {
        let (gap, inside, outside) = measure(kind, 1);
        assert!(inside > 20, "{kind}: 框内没有显示内容");
        assert_not_pushed(kind, gap);
        assert_eq!(outside, 0, "{kind}: 框外画出了文字");
    }
}
