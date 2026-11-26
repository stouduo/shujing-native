//! 共用结果网格:egui_extras TableBuilder(粘性表头/列宽可调/行虚拟化/横向滚动)。
//! 查询结果、执行计划、Parquet 预览、Kafka 消息共用;视觉与表数据页同一套:
//! 两行列头(名称 + 类型小字)、数字右对齐、列间分隔线、行间靠斑马纹。
//! 交互也与表数据页同一套(键盘/粘贴/焦点锁的辅助函数放在这里,两处共用):
//! 点/拖/⇧ 矩形选区,方向键/Tab/翻页移动,⌘C 复制、⌘A 全选,右键复制菜单,
//! 双击/回车查看完整内容;可编辑结果集另有回车编辑、⌘V 粘贴、Delete 置 NULL。

use crate::theme;
use egui::{Color32, Sense};
use egui_extras::{Column, TableBuilder};
use std::cell::Cell;

// ---------------------------------------------------------------- 键盘/剪贴板辅助(与表数据页共用)

/// 键盘移动网格选中格的方式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GridMotion {
    Rows(isize),
    Cols(isize),
    /// Tab:逐格右移(负 = 左移),行尾折到下一行行首、行首折到上一行行尾
    Tab(isize),
    /// PageUp/PageDown:移动一屏的行数
    Page(isize),
    FirstCol,
    LastCol,
    FirstRow,
    LastRow,
}

impl GridMotion {
    /// 翻页/首尾跳转:落点离原格很远,滚入视野时居中而不是贴边
    pub(crate) fn is_jump(self) -> bool {
        matches!(
            self,
            GridMotion::Page(_) | GridMotion::FirstRow | GridMotion::LastRow
        )
    }
}

/// 按键 → (移动方式, 是否按着 Shift 扩展选区)。⌘+方向键 = 跳到行/列的首尾。
/// 返回 None = 不是导航键,或带 ⌥/⌃(留给别的用途)
pub(crate) fn key_motion(key: egui::Key, m: egui::Modifiers) -> Option<(GridMotion, bool)> {
    if m.alt || m.ctrl {
        return None;
    }
    let extend = m.shift;
    let (dc, dr) = match key {
        egui::Key::ArrowLeft => (-1, 0),
        egui::Key::ArrowRight => (1, 0),
        egui::Key::ArrowUp => (0, -1),
        egui::Key::ArrowDown => (0, 1),
        egui::Key::Tab => return Some((GridMotion::Tab(if extend { -1 } else { 1 }), false)),
        egui::Key::PageUp => return Some((GridMotion::Page(-1), extend)),
        egui::Key::PageDown => return Some((GridMotion::Page(1), extend)),
        egui::Key::Home if !m.command => return Some((GridMotion::FirstCol, extend)),
        egui::Key::End if !m.command => return Some((GridMotion::LastCol, extend)),
        _ => return None,
    };
    Some((
        if m.command {
            match (dc, dr) {
                (-1, _) => GridMotion::FirstCol,
                (1, _) => GridMotion::LastCol,
                (_, -1) => GridMotion::FirstRow,
                _ => GridMotion::LastRow,
            }
        } else if dr != 0 {
            GridMotion::Rows(dr)
        } else {
            GridMotion::Cols(dc)
        },
        extend,
    ))
}

/// 从当前格出发按移动方式落到的新格(越界夹取;Tab 在行内折返)。
/// `bounds` = (行数, 可见列数);`page_rows` = 一屏能放下的行数(翻页用)
pub(crate) fn move_cell(
    cur: (usize, usize),
    motion: GridMotion,
    bounds: (usize, usize),
    page_rows: usize,
) -> (usize, usize) {
    let (rows, cols) = (bounds.0.max(1), bounds.1.max(1));
    let (r, c) = cur;
    let clamp = |v: isize, max: usize| v.clamp(0, max as isize - 1) as usize;
    match motion {
        GridMotion::Rows(d) => (clamp(r as isize + d, rows), c),
        GridMotion::Cols(d) => (r, clamp(c as isize + d, cols)),
        GridMotion::Tab(d) => {
            let nc = c as isize + d;
            if nc < 0 {
                (r.saturating_sub(1), cols - 1)
            } else if nc as usize >= cols {
                ((r + 1).min(rows - 1), 0)
            } else {
                (r, nc as usize)
            }
        }
        GridMotion::Page(d) => (clamp(r as isize + d * page_rows.max(1) as isize, rows), c),
        GridMotion::FirstCol => (r, 0),
        GridMotion::LastCol => (r, cols - 1),
        GridMotion::FirstRow => (0, c),
        GridMotion::LastRow => (rows - 1, c),
    }
}

/// 剪贴板文本 → 行 × 列(换行分行、制表符分列;去掉首尾空行与行尾 \r)
pub(crate) fn parse_clipboard(text: &str) -> Vec<Vec<String>> {
    text.trim_matches('\n')
        .split('\n')
        .map(|line| {
            line.trim_end_matches('\r')
                .split('\t')
                .map(str::to_string)
                .collect()
        })
        .collect()
}

/// 选区两端归一化为(左上,右下)
pub(crate) fn norm_range(r: ((usize, usize), (usize, usize))) -> ((usize, usize), (usize, usize)) {
    let ((r1, c1), (r2, c2)) = r;
    ((r1.min(r2), c1.min(c2)), (r1.max(r2), c1.max(c2)))
}

/// 选区浸染色(强调色低透明)
pub(crate) fn range_tint(accent: Color32) -> Color32 {
    Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 30)
}

/// 网格持焦点时,这些键留给网格(不给 egui 做焦点遍历)
pub(crate) const GRID_FOCUS_KEYS: egui::EventFilter = egui::EventFilter {
    tab: true,
    horizontal_arrows: true,
    vertical_arrows: true,
    escape: true,
};

/// 单元格编辑框持焦点时,Tab 留给网格(提交后移到下一格)
pub(crate) const EDIT_FOCUS_KEYS: egui::EventFilter = egui::EventFilter {
    tab: true,
    horizontal_arrows: true,
    vertical_arrows: true,
    escape: false,
};

/// 单元格双击判定(两个网格共用):egui 自带的双击只看间隔不看位置——快速连点两个不同
/// 的格,第二下也会被算成双击。这里要求两次点的是同一格,且落在系统双击间隔内。
/// 每次点击都调一次(它会记下这一下);按住拖过了别的格之后用 [`reset_double_click`] 清掉,
/// 免得"拖选完马上点回起点"被当成双击
pub(crate) fn cell_double_clicked(ui: &egui::Ui, key: egui::Id, cell: (usize, usize)) -> bool {
    let now = ui.input(|i| i.time);
    let delay = ui.ctx().options(|o| o.input_options.max_double_click_delay);
    let hit = matches!(
        ui.memory(|m| m.data.get_temp::<((usize, usize), f64)>(key)),
        Some((prev, t)) if prev == cell && now - t < delay
    );
    ui.memory_mut(|m| m.data.insert_temp(key, (cell, now)));
    hit
}

pub(crate) fn reset_double_click(ctx: &egui::Context, key: egui::Id) {
    ctx.memory_mut(|m| m.data.remove::<((usize, usize), f64)>(key));
}

/// 单元格的交互感度:可点(右键菜单)但不参与 Tab 焦点遍历——网格自己有一个焦点位
fn cell_sense() -> Sense {
    Sense::click() - Sense::focusable_noninteractive()
}

// ---------------------------------------------------------------- 选中态与命中

/// 结果网格的选中态(按网格 id 存 egui 临时态,各调用点不必各自加字段)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GridSel {
    /// (锚点, 当前格),未归一化:锚点是拖选/Shift 扩展时不动的那一端
    pub range: ((usize, usize), (usize, usize)),
    /// 建立选区时的结果集形状(行数, 列数):重新查询后形状变了,旧选区作废
    shape: (usize, usize),
}

impl GridSel {
    fn new(anchor: (usize, usize), cur: (usize, usize), shape: (usize, usize)) -> Self {
        Self {
            range: (anchor, cur),
            shape,
        }
    }

    /// (左上, 右下)
    pub(crate) fn norm(self) -> ((usize, usize), (usize, usize)) {
        norm_range(self.range)
    }

    /// 是否跨了多格(行或列方向)
    pub(crate) fn spanning(self) -> bool {
        let (a, b) = self.norm();
        a != b
    }

    fn contains(self, r: usize, c: usize) -> bool {
        let ((r1, c1), (r2, c2)) = self.norm();
        r >= r1 && r <= r2 && c >= c1 && c <= c2
    }
}

fn sel_id(id_salt: &str) -> egui::Id {
    egui::Id::new(("grid-sel", id_salt))
}

fn focus_id(id_salt: &str) -> egui::Id {
    egui::Id::new(("grid-focus", id_salt))
}

fn reveal_id(id_salt: &str) -> egui::Id {
    egui::Id::new(("grid-reveal", id_salt))
}

fn view_id(id_salt: &str) -> egui::Id {
    egui::Id::new(("grid-view-cell", id_salt))
}

fn load_sel(ctx: &egui::Context, id_salt: &str, shape: (usize, usize)) -> Option<GridSel> {
    ctx.memory(|m| m.data.get_temp::<GridSel>(sel_id(id_salt)))
        .filter(|s| s.shape == shape)
}

fn store_sel(ctx: &egui::Context, id_salt: &str, sel: Option<GridSel>) {
    ctx.memory_mut(|m| match sel {
        Some(s) => m.data.insert_temp(sel_id(id_salt), s),
        None => {
            m.data.remove::<GridSel>(sel_id(id_salt));
        }
    });
}

/// 结果网格当前的选区,归一化为 (左上, 右下)(调用方显示"已选 R×C"之类的提示用);
/// 形状不符 = 没有选区
pub fn selection(
    ctx: &egui::Context,
    id_salt: &str,
    shape: (usize, usize),
) -> Option<((usize, usize), (usize, usize))> {
    load_sel(ctx, id_salt, shape).map(GridSel::norm)
}

/// 读上一帧横向偏移(按网格 id 存 egui 临时态;静态会被同帧渲染的优化计划网格互相覆盖)
fn load_prev_hscroll(ctx: &egui::Context, id_salt: &str) -> f32 {
    ctx.data_mut(|d| {
        d.get_temp::<f32>(egui::Id::new(("grid-hscroll", id_salt)))
            .unwrap_or(0.0)
    })
}

#[cfg(test)]
fn ctx_dbg_store_geom(ctx: &egui::Context, id_salt: &str, ws: &[f32], left: f32, body_top: f32) {
    ctx.data_mut(|d| {
        d.insert_temp(
            egui::Id::new(("grid-geom", id_salt)),
            (ws.to_vec(), left, body_top),
        )
    });
}

fn store_hscroll(ctx: &egui::Context, id_salt: &str, x: f32) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(("grid-hscroll", id_salt)), x));
}

/// 冻结覆盖层需要的渲染几何(每帧由网格填充;选区高亮与指针命中共用)
#[derive(Default)]
struct GridGeom {
    header_top: Option<f32>,
    /// 本帧实际渲染的 (行号, 行带顶部真实屏幕 y)。
    /// 行虚拟化首行可能带裁剪(滚动偏移非行高整数倍),y 必须逐行记录不可推算
    vis_rows: Vec<(usize, f32)>,
}

fn row_at(pt: egui::Pos2, geom: &GridGeom) -> Option<usize> {
    geom.vis_rows
        .iter()
        .find(|(_, y)| pt.y >= *y && pt.y < y + theme::row_h())
        .map(|(ri, _)| *ri)
}

/// 指针落在行号列的哪一行(点行号 = 选中整行)
fn rowno_at(pt: egui::Pos2, geom: &GridGeom, left: f32) -> Option<usize> {
    (pt.x >= left && pt.x < left + theme::ROWNO_W)
        .then(|| row_at(pt, geom))
        .flatten()
}

/// 指针落在哪个数据格:(行, 列)。行号列、表头、表尾空白、占位列都返回 None。
/// `widths` = 表格实际列宽 [行号列, 各数据列…, 占位列];`frozen_n`/`offset_x` 描述
/// 横向滚动与冻结覆盖层(滚动后前 N 列钉在左侧,命中优先于底层滚走的同位置格);
/// `left` = 网格左缘
fn cell_at(
    pt: egui::Pos2,
    geom: &GridGeom,
    widths: &[f32],
    frozen_n: usize,
    offset_x: f32,
    left: f32,
) -> Option<(usize, usize)> {
    let row = row_at(pt, geom)?;
    let w = |i: usize| widths.get(i).copied().unwrap_or(0.0);
    // 首尾是行号列与占位列
    let n_cols = widths.len().saturating_sub(2);
    if pt.x < left + w(0) {
        return None;
    }
    let frozen_n = frozen_n.min(n_cols);
    // 冻结覆盖层只在滚动之后出现;没滚动时冻结列与普通列无异
    let overlay = frozen_n > 0 && offset_x > 0.5;
    if overlay {
        // 覆盖层钉在最上层:先查钉住的列(不随滚动移动)
        for ci in 0..frozen_n {
            let x0 = left + w(0) + (1..=ci).map(w).sum::<f32>();
            if pt.x >= x0 && pt.x < x0 + w(1 + ci) {
                return Some((row, ci));
            }
        }
        if pt.x < left + w(0) + (1..=frozen_n).map(w).sum::<f32>() {
            return None;
        }
    }
    // 滚动区:换算到内容坐标逐列找(覆盖层出现后,前 N 列已被它盖住,不算滚动区)
    let start = if overlay { frozen_n } else { 0 };
    let cx = pt.x - left - w(0) + offset_x;
    let mut acc = 0.0;
    for ci in 0..n_cols {
        acc += w(1 + ci);
        if cx < acc {
            return (ci >= start).then_some((row, ci));
        }
    }
    None
}

/// 诊断:网格上一帧的几何 (列宽, 左缘, 数据区顶)。kittest 按真实布局算点击坐标,
/// 不写死魔法数字(窗口宽度变了列宽会变)
#[cfg(test)]
pub(crate) fn debug_geom(ctx: &egui::Context, id_salt: &str) -> Option<(Vec<f32>, f32, f32)> {
    ctx.data(|d| d.get_temp::<(Vec<f32>, f32, f32)>(egui::Id::new(("grid-geom", id_salt))))
}

/// 诊断:结果网格当前的横向滚动偏移(kittest 行为测试读取)
#[cfg(test)]
pub(crate) fn debug_hscroll(ctx: &egui::Context, id_salt: &str) -> f32 {
    load_prev_hscroll(ctx, id_salt)
}

/// 往矩形选区的每一格写同一个值(批量填充 / 置 NULL 的空串)
pub(crate) fn fill_cells(
    ((r1, c1), (r2, c2)): ((usize, usize), (usize, usize)),
    val: &str,
) -> Vec<(usize, usize, String)> {
    (r1..=r2)
        .flat_map(|r| (c1..=c2).map(move |c| (r, c)))
        .map(|(r, c)| (r, c, val.to_string()))
        .collect()
}

/// 把剪贴板块贴到以 (r0, c0) 为左上角的位置:逐格给出目标坐标,行/列越界的部分丢弃。
/// 返回 (要写入的格, 实际贴到的右下角)
pub(crate) fn paste_cells(
    shape: (usize, usize),
    (r0, c0): (usize, usize),
    block: &[Vec<String>],
) -> (Vec<(usize, usize, String)>, Option<(usize, usize)>) {
    let mut out = Vec::new();
    let mut last = None;
    for (dr, line) in block.iter().enumerate() {
        let rr = r0 + dr;
        if rr >= shape.0 {
            break;
        }
        for (dc, val) in line.iter().enumerate() {
            let cc = c0 + dc;
            if cc >= shape.1 {
                break;
            }
            out.push((rr, cc, val.clone()));
            last = Some((rr, cc));
        }
    }
    (out, last)
}

/// 选区 → TSV。NULL 写成 "NULL";格内的制表符/换行压成空格(贴进 Excel 不串列)。
/// `headers` = 首行带列名
pub(crate) fn selection_tsv(
    columns: &[String],
    rows: &[Vec<Option<String>>],
    ((r1, c1), (r2, c2)): ((usize, usize), (usize, usize)),
    headers: bool,
) -> String {
    let mut lines = Vec::new();
    if headers {
        let last = c2.min(columns.len().saturating_sub(1));
        lines.push(columns[c1..=last].join("\t"));
    }
    for row in rows.iter().take(r2.saturating_add(1)).skip(r1) {
        let cells: Vec<String> = row
            .iter()
            .take(c2.saturating_add(1))
            .skip(c1)
            .map(|v| {
                v.as_deref()
                    .unwrap_or("NULL")
                    .replace('\t', " ")
                    .replace('\n', " ")
            })
            .collect();
        lines.push(cells.join("\t"));
    }
    lines.join("\n")
}

fn copy_selection(
    ctx: &egui::Context,
    columns: &[String],
    rows: &[Vec<Option<String>>],
    sel: GridSel,
) {
    let norm = sel.norm();
    if sel.spanning() {
        // 矩形选区:复制 行×列 TSV
        ctx.copy_text(selection_tsv(columns, rows, norm, false));
    } else if let Some(v) = rows.get(norm.0 .0).and_then(|row| row.get(norm.0 .1)) {
        // 单格:原样复制(NULL 写成 "NULL" 文本)
        ctx.copy_text(v.clone().unwrap_or_else(|| "NULL".into()));
    }
}

/// 单元格显示值:有待保存改动取改动(包括改成 NULL),否则取原始值
fn shown_value<'a>(
    changes: &'a [(usize, String, Option<String>)],
    v: &'a Option<String>,
    ri: usize,
    col: &str,
) -> &'a Option<String> {
    match changes.iter().find(|(r, cn, _)| *r == ri && cn == col) {
        Some((_, _, nv)) => nv,
        None => v,
    }
}

fn has_change(changes: &[(usize, String, Option<String>)], ri: usize, col: &str) -> bool {
    changes.iter().any(|(r, c, _)| *r == ri && c == col)
}

/// 单元格右键菜单(只读与可编辑网格共用)。右键落在多格选区内时,复制作用于整个选区
#[allow(clippy::too_many_arguments)]
fn cell_menu(
    ui: &mut egui::Ui,
    columns: &[String],
    rows: &[Vec<Option<String>>],
    sel: Option<GridSel>,
    ri: usize,
    ci: usize,
    shown: &Option<String>,
    editable: bool,
    out: &GridOut,
) {
    let ctx = ui.ctx().clone();
    let mut act = |ui: &mut egui::Ui| {
        ui.close_menu();
    };
    if sel.is_some_and(|s| s.spanning() && s.contains(ri, ci)) {
        let norm = sel.map_or(((ri, ci), (ri, ci)), GridSel::norm);
        if ui
            .button(ui_kit::t!("Copy Selection", "复制选区"))
            .clicked()
        {
            ctx.copy_text(selection_tsv(columns, rows, norm, false));
            act(ui);
        }
        if ui
            .button(ui_kit::t!(
                "Copy Selection with Headers",
                "复制选区(带列名)"
            ))
            .clicked()
        {
            ctx.copy_text(selection_tsv(columns, rows, norm, true));
            act(ui);
        }
    } else if ui.button(ui_kit::t!("Copy Cell", "复制单元格")).clicked() {
        // NULL 格复制成 "NULL" 文本(与 ⌘C 一致)
        ctx.copy_text(shown.clone().unwrap_or_else(|| "NULL".into()));
        act(ui);
    }
    if ui
        .button(ui_kit::t!("Copy Row (TSV)", "复制整行 (TSV)"))
        .clicked()
    {
        let last = columns.len().saturating_sub(1);
        ctx.copy_text(selection_tsv(columns, rows, ((ri, 0), (ri, last)), false));
        act(ui);
    }
    if ui
        .button(ui_kit::t!("View Full Content…", "查看完整内容…"))
        .clicked()
    {
        out.view.set(Some((
            columns.get(ci).cloned().unwrap_or_default(),
            shown.clone().unwrap_or_default(),
        )));
        act(ui);
    }
    if editable {
        ui.separator();
        if ui.button(ui_kit::t!("Edit Cell", "编辑此格")).clicked() {
            out.start_edit
                .set(Some((ri, ci, shown.clone().unwrap_or_default())));
            act(ui);
        }
    }
}

// ---------------------------------------------------------------- 编辑框与查看弹窗

/// 编辑框怎么结束的:是否提交,以及提交后往哪边走(回车向下、Tab 向右;批量填充后留在原处)
struct EditEnd {
    commit: bool,
    motion: Option<GridMotion>,
}

/// 单元格编辑框(底层网格与冻结覆盖层共用):出现即抢焦点、把 Tab 锁给网格;
/// Esc 取消,回车/Tab 提交并移到下一格,失焦提交
fn cell_editor(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    draft: &mut String,
    spanning: bool,
    end: &Cell<Option<EditEnd>>,
) {
    let inner = rect.shrink2(egui::vec2(4.0, 2.0));
    let mut eui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    let resp = eui.add(
        egui::TextEdit::singleline(draft)
            .font(egui::FontId::proportional(theme::fs_data()))
            .frame(false)
            .desired_width(inner.width()),
    );
    // 双击出现编辑框后立即抢焦点,否则打字仍进原焦点控件
    resp.request_focus();
    // Tab 留给网格:提交后移到下一格,而不是被 egui 拿去遍历焦点
    ui.memory_mut(|mem| mem.set_focus_lock_filter(resp.id, EDIT_FOCUS_KEYS));
    let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
    let enter = ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.command);
    let (tab, shift) = ui.input(|i| (i.key_pressed(egui::Key::Tab), i.modifiers.shift));
    if esc {
        end.set(Some(EditEnd {
            commit: false,
            motion: None,
        }));
    } else if enter || tab {
        end.set(Some(EditEnd {
            commit: true,
            motion: (!spanning).then_some(if enter {
                GridMotion::Rows(1)
            } else {
                GridMotion::Tab(if shift { -1 } else { 1 })
            }),
        }));
    } else if resp.lost_focus() {
        end.set(Some(EditEnd {
            commit: true,
            motion: None,
        }));
    }
}

/// "查看完整内容"弹窗的状态(按网格 id 存临时态)
#[derive(Clone)]
struct ViewState {
    col: String,
    text: String,
}

/// "查看完整内容"弹窗:长文本被列宽截断时用(只读网格双击/回车,或右键菜单打开)
fn draw_view_modal(ctx: &egui::Context, id_salt: &str) {
    let id = view_id(id_salt);
    let Some(v) = ctx.memory(|m| m.data.get_temp::<ViewState>(id)) else {
        return;
    };
    let p = theme::c();
    let mut close = false;
    let mut copy_it = false;
    let title = ui_kit::t!("Full Content", "完整内容").to_string();
    let modal = egui::Modal::new(id)
        .frame(theme::modal_frame())
        .backdrop_color(p.overlay)
        .show(ctx, |ui| {
            ui.set_width(540.0);
            theme::modal_title(ui, &format!("{} · {title}", v.col));
            egui::ScrollArea::vertical()
                .max_height(300.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&v.text)
                                .font(theme::mono(12.0))
                                .color(p.cell_text),
                        )
                        .wrap()
                        .selectable(true),
                    );
                });
            theme::modal_footer(ui, |ui| {
                if theme::button(ui, ui_kit::t!("Copy", "复制"), theme::BtnKind::Primary).clicked()
                {
                    copy_it = true;
                }
                ui.add_space(6.0);
                if theme::button(ui, ui_kit::t!("Close", "关闭"), theme::BtnKind::Secondary)
                    .clicked()
                {
                    close = true;
                }
            });
        });
    if copy_it {
        ctx.copy_text(v.text.clone());
    }
    if close || modal.should_close() {
        ctx.memory_mut(|m| {
            m.data.remove::<ViewState>(id);
        });
    }
}

// ---------------------------------------------------------------- 绘制

/// 可编辑结果集的编辑态(与调用方共享;只读网格传 None)
struct EditFields<'a> {
    changes: &'a [(usize, String, Option<String>)],
    editing: &'a mut Option<(usize, usize)>,
    draft: &'a mut String,
}

/// 一帧之内各处绘制(底层网格/冻结覆盖层)往外交互结果的通道
#[derive(Default)]
struct GridOut {
    start_edit: Cell<Option<(usize, usize, String)>>,
    /// 编辑框提交的那一格(多格选区的左上角 = 批量填充的起点)
    editor_commit: Cell<Option<(usize, usize, String)>>,
    /// 请求打开"查看完整内容":(列名, 文本)
    view: Cell<Option<(String, String)>>,
    /// 编辑提交后的去向(回车向下 / Tab 向右)
    after_edit: Cell<Option<GridMotion>>,
    /// 编辑框刚结束:把键盘焦点还给网格
    refocus: Cell<bool>,
}

static NONE_CELL: Option<String> = None;

/// 画结果网格。`esc_to` = 网格持焦时按 Esc 把焦点交给谁(查询页交给 SQL 编辑器;
/// None = 只放开焦点)。返回 (请求开始编辑的格, 要写入的格);只读网格两者皆空
#[allow(clippy::too_many_arguments)]
fn draw_grid(
    ui: &mut egui::Ui,
    id_salt: &str,
    columns: &[String],
    column_types: &[String],
    rows: &[Vec<Option<String>>],
    frozen: &mut usize,
    esc_to: Option<egui::Id>,
    mut edits: Option<EditFields<'_>>,
) -> (Option<(usize, usize, String)>, Vec<(usize, usize, String)>) {
    let p = theme::c();
    let old_spacing = ui.spacing().item_spacing;
    ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
    let col_widths = theme::fitted_widths_typed(rows, columns, column_types, ui.available_width());
    let numeric = theme::numeric_flags(columns, column_types, rows);
    let desired_w = theme::ROWNO_W + col_widths.iter().sum::<f32>();
    let table_left = ui.max_rect().left();
    let table_right = ui.max_rect().right();
    let outer = ui.available_rect_before_wrap();
    let frozen_n = (*frozen).min(columns.len());
    let shape = (rows.len(), columns.len());
    let mut sel = load_sel(ui.ctx(), id_salt, shape);
    // 网格的键盘焦点位:点选单元格后焦点归网格,方向键/Tab/Esc 由网格处理
    let grid_id = focus_id(id_salt);
    let grid_focused = ui.memory(|m| m.has_focus(grid_id));
    // 上一帧键盘移到的格:(行, 列, 是否居中滚入)
    let reveal = ui.memory(|m| m.data.get_temp::<(usize, usize, bool)>(reveal_id(id_salt)));
    let mut geom = GridGeom::default();
    let actual_widths = Cell::new(Vec::<f32>::new());
    let prev_hscroll = load_prev_hscroll(ui.ctx(), id_salt);
    // 编辑行滚出视口的帧末取消:行虚拟化下编辑格不渲染,Esc/Enter 处理执行不到,
    // editing 悬死 → 焦点门全局失效且 Esc 救不回
    let editing_at_start = edits.as_ref().is_some_and(|e| e.editing.is_some());
    let editing_rendered = Cell::new(false);
    let out = GridOut::default();
    let mut commits: Vec<(usize, usize, String)> = Vec::new();
    // 右键菜单会拿走焦点,关掉时却不还(Esc / 点到别处 / 点了菜单项都一样),键盘导航就此失效。
    // 这里逐帧记"本网格的菜单开着吗",从开到关的那一帧把焦点交还网格。
    // 菜单只能从指针所在处打开:平时只问指针下那一格,上一帧有菜单时才逐格问
    let menu_key = egui::Id::new(("grid-menu-open", id_salt));
    let menu_open_last = ui
        .memory(|m| m.data.get_temp::<bool>(menu_key))
        .unwrap_or(false);
    let pointer_here = ui
        .input(|i| i.pointer.latest_pos())
        .is_some_and(|pt| outer.contains(pt));
    let menu_open = Cell::new(false);

    // 结果网格和表数据页共用同一套横向滚动条策略:常驻、实体、高可见度
    ui.spacing_mut().scroll.floating = false;
    ui.spacing_mut().scroll.dormant_handle_opacity = 0.9;
    ui.spacing_mut().scroll.dormant_background_opacity = 0.15;
    ui.spacing_mut().scroll.interact_handle_opacity = 1.0;
    ui.spacing_mut().scroll.active_handle_opacity = 1.0;
    ui.spacing_mut().scroll.bar_width = 12.0;
    ui.spacing_mut().scroll.bar_inner_margin = 2.0;
    // 网格的键盘焦点位:一块只可获焦点的区域(不拦点击)。持焦时把 Tab/方向键/Esc
    // 锁给网格,否则 egui 会拿它们在控件间遍历焦点
    let grid_resp = ui.interact(outer, grid_id, Sense::focusable_noninteractive());
    if grid_resp.has_focus() {
        ui.memory_mut(|mem| mem.set_focus_lock_filter(grid_id, GRID_FOCUS_KEYS));
    }
    let hout = egui::ScrollArea::horizontal()
        .id_salt(format!("{id_salt}-hscroll"))
        // 内建拖拽滚动与矩形拖选冲突:按住数据格拖动是选区,平移走滚轮/滚动条
        .drag_to_scroll(false)
        .auto_shrink([false, false])
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width().max(desired_w));
            // body 闭包内外层 ui 被 builder 借用,先 clone painter 供空行延伸
            let body_painter = ui.painter().clone();
            let mut tb = TableBuilder::new(ui)
                .id_salt(id_salt)
                .striped(false)
                .sense(Sense::hover())
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .vscroll(true)
                .drag_to_scroll(false)
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .column(Column::exact(theme::ROWNO_W));
            // initial(而非 exact):exact 把 width_range 钉死在 w..=w,拖拽调宽无效
            for (i, _c) in columns.iter().enumerate() {
                tb = tb.column(
                    Column::initial(col_widths[i])
                        .resizable(true)
                        .clip(true)
                        .at_least(40.0),
                );
            }
            tb = tb.column(Column::remainder().at_least(0.0));
            // 行是虚拟化的(滚出视口的不渲染),键盘移到的行靠表格自己的"滚到第 N 行"进视野:
            // 逐格移动刚好露出即可,翻页/首尾跳转居中
            let tb = match reveal {
                Some((row, _, center)) => {
                    tb.scroll_to_row(row, center.then_some(egui::Align::Center))
                }
                None => tb,
            };
            tb.header(theme::data_header_h(), |mut header| {
                header.col(|ui| {
                    let rect = ui.max_rect();
                    if geom.header_top.is_none() {
                        geom.header_top = Some(rect.top());
                    }
                    crate::theme::paint_rowno_head(ui.painter(), rect);
                });
                for (vw, c) in columns.iter().enumerate() {
                    header.col(|ui| {
                        let rect = ui.max_rect();
                        theme::paint_grid_head(
                            ui.painter(),
                            rect,
                            c,
                            theme::type_at(column_types, vw),
                        );
                        let resp = ui.allocate_rect(rect, Sense::click());
                        grid_header_freeze_menu(&resp, vw, columns.len(), frozen, grid_id);
                    });
                }
                header.col(|ui| {
                    // 末尾占位列:延续表头底色与底线
                    let rect = ui.max_rect();
                    ui.painter().rect_filled(rect, 0, p.header);
                    theme::hline(ui.painter(), rect, p.border);
                });
            })
            .body(|body| {
                // rows() 消费 body:列宽、数据区矩形都先取出来
                let ws = body.widths().to_vec();
                actual_widths.set(ws.clone());
                let brect = body.max_rect();
                // 行悬停:每格用自身矩形判定(跨行累积并工会点亮悬停行下方全部行)
                let hovers = |ui: &egui::Ui, rect: egui::Rect| {
                    ui.input(|i| i.pointer.latest_pos())
                        .map(|pt| {
                            pt.y >= rect.top()
                                && pt.y < rect.bottom()
                                && pt.x >= table_left
                                && pt.x <= table_right
                        })
                        .unwrap_or(false)
                };
                body.rows(theme::row_h(), rows.len(), |mut row| {
                    let ri = row.index();
                    let row_y = Cell::new(f32::NAN);
                    let rowdata = &rows[ri];
                    row.col(|ui| {
                        let rect = ui.max_rect();
                        row_y.set(rect.top());
                        theme::paint_grid_row_bg(ui.painter(), rect, ri, hovers(ui, rect), false);
                        theme::paint_grid_rowno(ui.painter(), rect, ri + 1);
                    });
                    for (ci, col) in columns.iter().enumerate() {
                        // 冻结列已横向滚出视口:编辑框改由覆盖层持有,底层画普通格防双 TextEdit。
                        // 用函数开头采样的 prev 值:与覆盖层编辑框归属判定严格同源
                        let frozen_scrolled = frozen_n > 0 && ci < frozen_n && prev_hscroll > 0.5;
                        let editing_here =
                            edits.as_mut().is_some_and(|e| *e.editing == Some((ri, ci)))
                                && !frozen_scrolled;
                        row.col(|ui| {
                            let rect = ui.max_rect();
                            let painter = ui.painter().with_clip_rect(rect);
                            let changed = edits
                                .as_ref()
                                .is_some_and(|e| has_change(e.changes, ri, col));
                            theme::paint_grid_row_bg(&painter, rect, ri, hovers(ui, rect), changed);
                            if editing_here {
                                editing_rendered.set(true);
                                let e = edits.as_mut().unwrap();
                                let end = Cell::new(None);
                                cell_editor(
                                    ui,
                                    rect,
                                    e.draft,
                                    sel.is_some_and(|s| s.spanning()),
                                    &end,
                                );
                                if let Some(x) = end.take() {
                                    if x.commit {
                                        out.editor_commit.set(Some((ri, ci, e.draft.clone())));
                                    }
                                    if x.motion.is_some() {
                                        out.after_edit.set(x.motion);
                                    }
                                    *e.editing = None;
                                    out.refocus.set(true);
                                }
                            } else {
                                let v: &Option<String> = rowdata.get(ci).unwrap_or(&NONE_CELL);
                                let shown = edits
                                    .as_ref()
                                    .map_or(v, |e| shown_value(e.changes, v, ri, col));
                                theme::paint_grid_cell_text(
                                    &painter,
                                    rect,
                                    col,
                                    shown,
                                    changed,
                                    numeric[ci],
                                );
                                let resp = ui.allocate_rect(rect, cell_sense());
                                let _ = resp.clone().context_menu(|ui| {
                                    cell_menu(
                                        ui,
                                        columns,
                                        rows,
                                        sel,
                                        ri,
                                        ci,
                                        shown,
                                        edits.is_some(),
                                        &out,
                                    );
                                });
                                if menu_open_last || (pointer_here && resp.contains_pointer()) {
                                    if resp.context_menu_opened() {
                                        menu_open.set(true);
                                    }
                                }
                            }
                        });
                    }
                    // 末尾占位列:延续行底色
                    row.col(|ui| {
                        let rect = ui.max_rect();
                        theme::paint_grid_row_bg(ui.painter(), rect, ri, hovers(ui, rect), false);
                    });
                    geom.vis_rows.push((ri, row_y.get()));
                });

                // 空白区网格延续(与数据表格页同款):斑马纹 + 列分隔线,竖线位置与数据列右缘对齐
                {
                    let mut y = brect.top() + rows.len() as f32 * theme::row_h();
                    let mut empty_ri = rows.len();
                    while y + theme::row_h() <= brect.bottom() + 0.5 {
                        let band = egui::Rect::from_min_max(
                            egui::pos2(brect.left(), y),
                            egui::pos2(brect.right(), y + theme::row_h()),
                        );
                        if empty_ri % 2 == 1 {
                            body_painter.rect_filled(band, 0, p.stripe);
                        }
                        let mut ex = band.left();
                        // 末位是 remainder 占位列,没有右缘
                        for w in ws.iter().take(ws.len().saturating_sub(1)) {
                            ex += *w;
                            if ex >= band.right() - 1.0 {
                                break;
                            }
                            body_painter.line_segment(
                                [
                                    egui::pos2(ex - 0.5, band.top()),
                                    egui::pos2(ex - 0.5, band.bottom()),
                                ],
                                egui::Stroke::new(1.0, p.row_line),
                            );
                        }
                        y += theme::row_h();
                        empty_ri += 1;
                    }
                }
            });
        });

    // ------------------------------------------------ 表格画完之后:滚入视野、指针选区、键盘、复制
    let ws = actual_widths.take();
    let inner = hout.inner_rect;
    let offset_x = hout.state.offset.x;
    let header_bottom = geom
        .header_top
        .map_or(inner.top(), |t| t + theme::data_header_h());
    #[cfg(test)]
    ctx_dbg_store_geom(ui.ctx(), id_salt, &ws, inner.left(), header_bottom);
    // 一屏能放下的行数(翻页步长)
    let page_rows =
        (((inner.height() - theme::data_header_h()) / theme::row_h()).max(0.0) as usize).max(1);
    // 键盘移到的列滚入横向视野。表格自己的滚动区会把横向目标一并取走,外层这层
    // 拿不到,只能直接改它的偏移。冻结列始终钉在可见区,不用管
    let mut hscroll_x = offset_x;
    if let Some((_, c, _)) = reveal {
        ui.memory_mut(|m| m.data.remove::<(usize, usize, bool)>(reveal_id(id_salt)));
        if c >= frozen_n && !ws.is_empty() {
            let w = |i: usize| ws.get(i).copied().unwrap_or(0.0);
            let left = ws.iter().take(1 + c).sum::<f32>();
            let right = left + w(1 + c);
            // 冻结覆盖层盖住的那一段
            let pinned: f32 = if offset_x > 0.5 {
                ws.iter().take(1 + frozen_n).skip(1).sum()
            } else {
                0.0
            };
            let view_w = inner.width();
            hscroll_x = if left < offset_x + pinned {
                (left - pinned).max(0.0)
            } else if right > offset_x + view_w - 2.0 {
                right - view_w + 2.0
            } else {
                offset_x
            }
            .max(0.0);
        }
        if hscroll_x != offset_x {
            let mut st = hout.state;
            st.offset.x = hscroll_x;
            st.store(ui.ctx(), hout.id);
        }
    }

    // 指针:点选 / 拖选矩形 / ⇧ 扩展 / 右键改选(行虚拟化 + 冻结覆盖层,命中用几何反推)
    let pointer = ui.input(|i| i.pointer.latest_pos());
    // 按压点必须在本网格层(右键菜单/弹窗在其上层,几何命中需手动避让)
    let on_layer =
        pointer.is_some_and(|pt| ui.memory(|m| m.layer_id_at(pt) == Some(ui.layer_id())));
    let (p_pressed, p_down, r_pressed) = ui.input(|i| {
        (
            i.pointer.primary_pressed(),
            i.pointer.primary_down(),
            i.pointer.secondary_pressed(),
        )
    });
    let origin = ui.input(|i| i.pointer.press_origin());
    let shift = ui.input(|i| i.modifiers.shift);
    let hit_rowno = |pt: egui::Pos2| rowno_at(pt, &geom, inner.left());
    let hit_cell = |pt: egui::Pos2| cell_at(pt, &geom, &ws, frozen_n, offset_x, inner.left());
    let last_col = columns.len().saturating_sub(1);
    // 点选/导航了单元格:键盘焦点归网格
    let mut focus_req = false;
    // 双击(同一格、间隔内):只读 = 查看完整内容,可编辑 = 进入编辑。
    // 不用 egui 自带的双击判定——它只看间隔不看位置,快速连点两个格也会弹出弹窗
    let dbl_key = egui::Id::new(("grid-dbl", id_salt));
    let mut dbl_cell: Option<(usize, usize)> = None;
    if p_pressed && on_layer {
        if let Some(c) = pointer.and_then(hit_cell) {
            if cell_double_clicked(ui, dbl_key, c) {
                dbl_cell = Some(c);
            }
        }
    }
    if on_layer {
        if p_pressed {
            if let Some(pt) = pointer {
                if let Some(ri) = hit_rowno(pt) {
                    // 点行号 = 选中整行
                    if !columns.is_empty() {
                        sel = Some(GridSel::new((ri, 0), (ri, last_col), shape));
                        focus_req = true;
                    }
                } else if let Some(c) = hit_cell(pt) {
                    let a = if shift {
                        sel.map(|s| s.range.0).unwrap_or(c)
                    } else {
                        c
                    };
                    sel = Some(GridSel::new(a, c, shape));
                    focus_req = true;
                }
            }
        } else if p_down {
            // 拖选:起点在数据格/行号上才扩展选区(起点在表尾空白的不算)
            if let (Some(o), Some(pt)) = (origin, pointer) {
                let (a_rowno, a_cell) = (hit_rowno(o), hit_cell(o));
                let (b_rowno, b_cell) = (hit_rowno(pt), hit_cell(pt));
                if let Some(a) = a_rowno {
                    if let Some(ri) = b_rowno.or_else(|| b_cell.map(|c| c.0)) {
                        if !columns.is_empty() {
                            sel = Some(GridSel::new((a, 0), (ri, last_col), shape));
                        }
                    }
                } else if let (Some(a), Some(c)) = (a_cell, b_cell) {
                    let anchor = sel.map(|s| s.range.0).unwrap_or(a);
                    sel = Some(GridSel::new(anchor, c, shape));
                    if c != a {
                        // 拖选经过了别的格:起点那一下不再算双击的前半程
                        reset_double_click(ui.ctx(), dbl_key);
                    }
                }
            }
        }
        // 右键落在多格选区内:保留选区(菜单作用于整个选区);否则改选该格
        if r_pressed {
            if let Some((ri, ci)) = pointer.and_then(hit_cell) {
                if !sel.is_some_and(|s| s.spanning() && s.contains(ri, ci)) {
                    sel = Some(GridSel::new((ri, ci), (ri, ci), shape));
                }
                focus_req = true;
            }
        }
    }

    // 右键菜单刚关掉:焦点交还网格
    let menu_open_now = menu_open.get();
    if menu_open_now != menu_open_last {
        ui.memory_mut(|m| m.data.insert_temp(menu_key, menu_open_now));
    }
    if menu_open_last && !menu_open_now {
        focus_req = true;
    }

    if let Some((ri, ci)) = dbl_cell {
        if let (Some(row), Some(col)) = (rows.get(ri), columns.get(ci)) {
            let orig = row.get(ci);
            // 草稿/正文取现值(优先待保存改动):取原始值会让 Enter 静默撤销改动
            let text = edits
                .as_ref()
                .and_then(|e| orig.map(|v| shown_value(e.changes, v, ri, col)))
                .or(orig)
                .cloned()
                .flatten()
                .unwrap_or_default();
            if edits.is_some() {
                out.start_edit.set(Some((ri, ci, text)));
            } else {
                out.view.set(Some((col.clone(), text)));
            }
        }
    }

    // ---- 键盘(输入框没持焦、没在编辑时):方向键/Tab/Home/End/翻页/⌘+方向 移动选中格,
    // 按着 Shift 则以原锚点扩展矩形选区;⌘A 全选、⌘C 复制;回车 = 编辑(可编辑)或
    // 查看完整内容;Delete 置 NULL、⌘V 粘贴(可编辑);Esc 放开网格焦点
    let edit_ended = out.refocus.get();
    // 有输入框持焦点时让给它;网格自己的焦点位不算(否则网格持焦时打字/⌘C 全失效)
    let field_focused = ui.memory(|m| m.focused().is_some_and(|id| id != grid_id));
    let editing_now = edits.as_ref().is_some_and(|e| e.editing.is_some());
    let editable = edits.is_some();
    let mut moved: Option<(usize, usize, bool)> = None;
    let mut edit_req: Option<(usize, usize)> = None;
    let mut open_view: Option<(usize, usize)> = None;
    // 直接打字进入编辑:(首字符, 编辑的格)
    let mut typed: Option<(char, (usize, usize))> = None;
    let mut paste: Option<String> = None;
    let mut null_fill = false;
    let mut release = false;
    if !edit_ended && !field_focused && !editing_now {
        // 注:ui.input 拿的是 context 的写锁,它的闭包里不能再读 memory/data(同线程自己锁死自己)。
        // 焦点状态先在这里取好
        let nothing_focused = ui.memory(|m| m.focused().is_none());
        let cmd = |key: egui::Key| ui.input(|i| i.modifiers.command && i.key_pressed(key));
        if !rows.is_empty() && !columns.is_empty() && cmd(egui::Key::A) {
            sel = Some(GridSel::new((0, 0), (rows.len() - 1, last_col), shape));
        }
        // 单格原样 / 矩形选区 TSV
        if cmd(egui::Key::C) {
            if let Some(s) = sel {
                copy_selection(ui.ctx(), columns, rows, s);
            }
        }
        if !rows.is_empty() && !columns.is_empty() {
            let cur = sel.map(|s| s.range.1);
            ui.input(|i| {
                for e in i.events.iter() {
                    let egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } = e
                    else {
                        match e {
                            // ⌘V 把剪贴板块贴到选区左上角
                            egui::Event::Paste(t) if editable => paste = Some(t.clone()),
                            // 无焦点输入时直接打字 = 从选区左上角开始编辑,首字符就是打的内容
                            egui::Event::Text(t) if editable => {
                                if let Some(ch) = t.chars().find(|c| !c.is_control()) {
                                    typed = sel.map(|s| (ch, s.norm().0));
                                }
                            }
                            _ => {}
                        }
                        continue;
                    };
                    let plain =
                        !modifiers.command && !modifiers.shift && !modifiers.alt && !modifiers.ctrl;
                    if let Some((motion, extend)) = key_motion(*key, *modifiers) {
                        // 还没有选中格时第一次按导航键:落到左上角
                        let cur = cur.unwrap_or((0, 0));
                        let next = if sel.is_some() {
                            move_cell(cur, motion, shape, page_rows)
                        } else {
                            cur
                        };
                        let anchor = if extend {
                            sel.map(|s| s.range.0).unwrap_or(cur)
                        } else {
                            next
                        };
                        sel = Some(GridSel::new(anchor, next, shape));
                        moved = Some((next.0, next.1, motion.is_jump()));
                        // 导航键一旦被网格接手,焦点也归网格(后续 Tab 不被 egui 拿去遍历)
                        focus_req = true;
                    } else if plain && (*key == egui::Key::Enter || *key == egui::Key::F2) {
                        if editable {
                            edit_req = cur;
                        } else {
                            open_view = cur;
                        }
                    } else if plain
                        && (*key == egui::Key::Delete || *key == egui::Key::Backspace)
                        && editable
                        && cur.is_some()
                    {
                        null_fill = true;
                    } else if plain
                        && *key == egui::Key::Escape
                        // 本网格的右键菜单开着:这个 Esc 是关菜单的,不算网格自己的 Esc
                        && !menu_open_last
                        && (grid_focused || nothing_focused)
                    {
                        release = true;
                    }
                }
            });
        }
    }
    // 编辑提交后的去向(回车向下 / Tab 向右):落到新格并滚入视野。
    // 此时输入事件里还留着刚按的回车/Tab/Esc,本帧不能再当导航键处理(edit_ended 已挡住)
    if let Some(motion) = out.after_edit.take() {
        if let Some(c) = sel.map(|s| s.range.1) {
            let next = move_cell(c, motion, shape, page_rows);
            sel = Some(GridSel::new(next, next, shape));
            moved = Some((next.0, next.1, motion.is_jump()));
        }
    }
    // 绘制途中(双击/右键菜单)已经请求了编辑的,键盘这条不再覆盖
    let mut start_edit = out.start_edit.take();
    if start_edit.is_none() {
        if let Some((r, c)) = edit_req {
            if let (Some(row), Some(col)) = (rows.get(r), columns.get(c)) {
                let orig = row.get(c);
                // 草稿取现值(优先待保存改动):取原始值会让回车静默撤销改动
                let text = edits
                    .as_ref()
                    .and_then(|e| orig.map(|v| shown_value(e.changes, v, r, col)))
                    .or(orig)
                    .cloned()
                    .flatten()
                    .unwrap_or_default();
                start_edit = Some((r, c, text));
            }
        } else if let Some((ch, (r, c))) = typed {
            start_edit = Some((r, c, ch.to_string()));
        }
    }
    // 可编辑:粘贴块 / 选区批量置 NULL / 编辑框提交到多格选区 = 填充整个选区。
    // 只是暂存改动,保存前不会碰数据库
    if editable {
        if let Some(text) = paste {
            let block = parse_clipboard(&text);
            // 锚点:选区左上角
            if let Some((r0, c0)) = sel.map(|s| s.norm().0) {
                let single = block.len() == 1 && block[0].len() == 1;
                if single && sel.is_some_and(|s| s.spanning()) {
                    // 单值贴进多格选区 = 填充整个选区(走和"打字后回车"同一条路)
                    commits.extend(fill_cells(
                        sel.map_or(((r0, c0), (r0, c0)), GridSel::norm),
                        &block[0][0],
                    ));
                } else {
                    let (cells, last) = paste_cells(shape, (r0, c0), &block);
                    if let Some(last) = last {
                        sel = Some(GridSel::new((r0, c0), last, shape));
                    }
                    commits.extend(cells);
                }
            }
        }
        if null_fill {
            if let Some(norm) = sel.map(GridSel::norm) {
                commits.extend(fill_cells(norm, ""));
            }
        }
        if let Some((ri, ci, val)) = out.editor_commit.take() {
            match sel.filter(|s| s.spanning() && s.norm().0 == (ri, ci)) {
                Some(s) => commits.extend(fill_cells(s.norm(), &val)),
                None => commits.push((ri, ci, val)),
            }
        }
    }

    if let Some(req) = moved {
        ui.memory_mut(|m| m.data.insert_temp(reveal_id(id_salt), req));
    }
    store_sel(ui.ctx(), id_salt, sel);
    if focus_req || edit_ended {
        ui.memory_mut(|mem| mem.request_focus(grid_id));
    }
    if release {
        ui.memory_mut(|mem| mem.surrender_focus(grid_id));
        if let Some(t) = esc_to {
            ui.memory_mut(|mem| mem.request_focus(t));
        }
    }
    // 选区高亮(滚动区):冻结覆盖层之前画,被钉住的那一段由覆盖层盖掉
    if let Some(s) = sel {
        let w0 = ws.first().copied().unwrap_or(theme::ROWNO_W);
        let pinned: f32 = if frozen_n > 0 && offset_x > 0.5 {
            ws.iter().take(1 + frozen_n).skip(1).sum()
        } else {
            0.0
        };
        let clip = egui::Rect::from_min_max(
            egui::pos2(inner.left() + w0 + pinned, header_bottom),
            egui::pos2(inner.right(), inner.bottom()),
        );
        paint_selection(
            ui,
            s,
            &geom,
            &ws,
            frozen_n,
            offset_x,
            inner.left(),
            clip,
            false,
            edits.as_ref().and_then(|e| *e.editing),
        );
    }
    store_hscroll(ui.ctx(), id_salt, hscroll_x);
    // 冻结列覆盖层:横向滚动时把行号+前 N 列钉在视口左侧重绘
    draw_frozen_grid_overlay(
        ui,
        id_salt,
        columns,
        column_types,
        &numeric,
        rows,
        frozen,
        hscroll_x,
        prev_hscroll,
        &geom,
        &ws,
        inner,
        edits.as_mut(),
        &editing_rendered,
        sel,
        &out,
        grid_id,
        &menu_open,
        menu_open_last,
        pointer_here,
    );
    if editing_at_start
        && edits.as_ref().is_some_and(|e| e.editing.is_some())
        && !editing_rendered.get()
    {
        if let Some(e) = edits.as_mut() {
            *e.editing = None;
        }
    }
    // "查看完整内容"
    if let Some((col, text)) = out.view.take() {
        ui.memory_mut(|m| {
            m.data
                .insert_temp(view_id(id_salt), ViewState { col, text })
        });
    }
    if let Some((r, c)) = open_view {
        if let Some(v) = rows.get(r).and_then(|row| row.get(c)) {
            ui.memory_mut(|m| {
                m.data.insert_temp(
                    view_id(id_salt),
                    ViewState {
                        col: columns[c].clone(),
                        text: v.clone().unwrap_or_default(),
                    },
                )
            });
        }
    }
    draw_view_modal(ui.ctx(), id_salt);
    ui.spacing_mut().item_spacing = old_spacing;
    (start_edit, commits)
}

/// 渲染只读结果集(带行号列)。`column_types` 与 `columns` 等长,拿不到类型的列传空串。
/// `esc_to` = 网格持焦时按 Esc 把焦点交给谁(None = 只放开焦点)
#[allow(clippy::too_many_arguments)]
pub fn result_rows(
    ui: &mut egui::Ui,
    id_salt: &str,
    columns: &[String],
    column_types: &[String],
    rows: &[Vec<Option<String>>],
    frozen: &mut usize,
    esc_to: Option<egui::Id>,
) {
    let _ = draw_grid(
        ui,
        id_salt,
        columns,
        column_types,
        rows,
        frozen,
        esc_to,
        None,
    );
}

/// 可编辑结果集(查询回写):双击/回车进入编辑,改动以 (行号,列名,新值) 记在 changes。
/// 返回 (请求开始编辑的格, 要写入的格);开始编辑由调用方置 draft,写入由调用方记 changes
#[allow(clippy::too_many_arguments)]
pub fn result_rows_editable(
    ui: &mut egui::Ui,
    id_salt: &str,
    columns: &[String],
    column_types: &[String],
    rows: &[Vec<Option<String>>],
    changes: &[(usize, String, Option<String>)],
    editing: &mut Option<(usize, usize)>,
    draft: &mut String,
    frozen: &mut usize,
    esc_to: Option<egui::Id>,
) -> (Option<(usize, usize, String)>, Vec<(usize, usize, String)>) {
    draw_grid(
        ui,
        id_salt,
        columns,
        column_types,
        rows,
        frozen,
        esc_to,
        Some(EditFields {
            changes,
            editing,
            draft,
        }),
    )
}

/// 选区高亮:浸染 + 四边描线(与表数据页同一套样式)。冻结覆盖层把前 N 列钉在左侧时,
/// 滚动区与冻结区各画各的(`frozen_side`),中间被盖住的那段不画
#[allow(clippy::too_many_arguments)]
fn paint_selection(
    ui: &egui::Ui,
    sel: GridSel,
    geom: &GridGeom,
    widths: &[f32],
    frozen_n: usize,
    offset_x: f32,
    left: f32,
    clip: egui::Rect,
    frozen_side: bool,
    editing: Option<(usize, usize)>,
) {
    let p = theme::c();
    let ((r1, c1), (r2, c2)) = sel.norm();
    let overlay = frozen_n > 0 && offset_x > 0.5;
    let (ca, cb) = if frozen_side {
        (c1, c2.min(frozen_n.saturating_sub(1)))
    } else {
        (c1.max(if overlay { frozen_n } else { 0 }), c2)
    };
    if ca > cb || !clip.is_positive() {
        return;
    }
    let w = |i: usize| widths.get(i).copied().unwrap_or(0.0);
    let x = |ci: usize| {
        left + w(0) + (1..=ci).map(w).sum::<f32>() - if frozen_side { 0.0 } else { offset_x }
    };
    let (x0, x1) = (x(ca), x(cb) + w(1 + cb));
    if x1 <= x0 {
        return;
    }
    let tint = range_tint(p.accent);
    let stroke = egui::Stroke::new(1.5, p.accent);
    for &(ri, y) in &geom.vis_rows {
        // 正在编辑的那一格不浸染(编辑框自己有描边)
        if ri < r1 || ri > r2 || (ca == cb && editing == Some((ri, ca))) {
            continue;
        }
        let full = egui::Rect::from_min_max(egui::pos2(x0, y), egui::pos2(x1, y + theme::row_h()));
        let rect = full.intersect(clip);
        if !rect.is_positive() {
            continue;
        }
        let painter = ui.painter().with_clip_rect(rect);
        painter.rect_filled(full, 0, tint);
        // 描线画在选区真实的边上:滚出视口的那一侧不画,免得在视口边缘冒出一条假边
        let (lx, rx) = (full.left().max(rect.left()), full.right().min(rect.right()));
        if ri == r1 {
            painter.line_segment(
                [egui::pos2(lx, full.top()), egui::pos2(rx, full.top())],
                stroke,
            );
        }
        if ri == r2 {
            painter.line_segment(
                [egui::pos2(lx, full.bottom()), egui::pos2(rx, full.bottom())],
                stroke,
            );
        }
        if full.left() >= clip.left() {
            painter.line_segment(
                [egui::pos2(x0, rect.top()), egui::pos2(x0, rect.bottom())],
                stroke,
            );
        }
        if full.right() <= clip.right() {
            painter.line_segment(
                [egui::pos2(x1, rect.top()), egui::pos2(x1, rect.bottom())],
                stroke,
            );
        }
    }
}

/// 列头右键:冻结到此列 / 取消冻结列(只读与可编辑结果网格共用)
fn grid_header_freeze_menu(
    resp: &egui::Response,
    vw: usize,
    vis_len: usize,
    frozen: &mut usize,
    focus: egui::Id,
) {
    let mut act: Option<u8> = None;
    let _ = resp.clone().context_menu(|ui| {
        // 菜单按钮拿走的焦点交还网格
        let mut act_once = |ui: &mut egui::Ui| {
            ui.memory_mut(|mem| mem.request_focus(focus));
            ui.close_menu();
        };
        if vw + 1 < vis_len
            && ui
                .button(ui_kit::t!("Freeze to This Column", "冻结到此列"))
                .clicked()
        {
            act = Some(0);
            act_once(ui);
        }
        if *frozen > 0
            && ui
                .button(ui_kit::t!("Unfreeze Columns", "取消冻结列"))
                .clicked()
        {
            act = Some(1);
            act_once(ui);
        }
    });
    if let Some(a) = act {
        *frozen = if a == 0 { vw + 1 } else { 0 };
    }
}

/// 查询结果网格的冻结覆盖层:横向滚动时把行号+前 N 列钉在视口左侧重绘。
/// 表头、数据格与底层同一套绘制;交互(选中/右键菜单/双击/编辑框)也一致
#[allow(clippy::too_many_arguments)]
fn draw_frozen_grid_overlay(
    ui: &mut egui::Ui,
    id_salt: &str,
    columns: &[String],
    column_types: &[String],
    numeric: &[bool],
    rows: &[Vec<Option<String>>],
    frozen: &mut usize,
    offset_x: f32,
    prev_hscroll: f32,
    geom: &GridGeom,
    widths: &[f32],
    inner: egui::Rect,
    mut edits: Option<&mut EditFields<'_>>,
    editing_rendered: &Cell<bool>,
    sel: Option<GridSel>,
    out: &GridOut,
    focus: egui::Id,
    menu_open: &Cell<bool>,
    menu_open_last: bool,
    pointer_here: bool,
) {
    let frozen_n = (*frozen).min(columns.len());
    if frozen_n == 0 || offset_x <= 0.5 {
        return;
    }
    let frozen_ws: Vec<f32> = (0..frozen_n)
        .map(|v| widths.get(1 + v).copied().unwrap_or(120.0))
        .collect();
    let p = theme::c();
    let left = inner.left();
    let top = geom.header_top.unwrap_or(inner.top());
    // 到横向滚动条上缘为止:盖住滚动条的左段就拖不了了
    let bottom = inner.bottom();
    let region_w = theme::ROWNO_W + frozen_ws.iter().sum::<f32>();
    if region_w < 1.0 || !(bottom > top) {
        return;
    }
    let region =
        egui::Rect::from_min_size(egui::pos2(left, top), egui::vec2(region_w, bottom - top));
    let painter = ui.painter().with_clip_rect(region);
    painter.rect_filled(region, 0, p.bg);
    let pointer = ui.input(|i| i.pointer.latest_pos());

    // 钉住的表头
    crate::theme::paint_rowno_head(
        &painter,
        egui::Rect::from_min_size(
            egui::pos2(left, top),
            egui::vec2(theme::ROWNO_W, theme::data_header_h()),
        ),
    );
    let mut hx = left + theme::ROWNO_W;
    for (vw, c) in columns.iter().enumerate().take(frozen_n) {
        let w = frozen_ws[vw];
        let rect =
            egui::Rect::from_min_size(egui::pos2(hx, top), egui::vec2(w, theme::data_header_h()));
        theme::paint_grid_head(&painter, rect, c, theme::type_at(column_types, vw));
        let resp = ui.interact(rect, egui::Id::new(("qfz-h", id_salt, vw)), Sense::click());
        grid_header_freeze_menu(&resp, vw, columns.len(), frozen, focus);
        hx += w;
    }

    // 钉住的数据行(y 用渲染时记录的真实行带;滚动偏移非行高整数倍时不可推算)
    for &(ri, y) in geom.vis_rows.iter() {
        if ri >= rows.len() || y.is_nan() {
            continue;
        }
        if y >= bottom {
            break;
        }
        let rowdata = &rows[ri];
        let hovered = pointer
            .map(|pt| {
                pt.y >= y && pt.y < y + theme::row_h() && pt.x >= left && pt.x < left + region_w
            })
            .unwrap_or(false);
        theme::paint_grid_row_bg(
            &painter,
            egui::Rect::from_min_size(egui::pos2(left, y), egui::vec2(region_w, theme::row_h())),
            ri,
            hovered,
            false,
        );
        theme::paint_grid_rowno(
            &painter,
            egui::Rect::from_min_size(
                egui::pos2(left, y),
                egui::vec2(theme::ROWNO_W, theme::row_h()),
            ),
            ri + 1,
        );
        let mut x = left + theme::ROWNO_W;
        for (ci, col) in columns.iter().enumerate().take(frozen_n) {
            let w = frozen_ws[ci];
            let rect = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, theme::row_h()));
            let v: &Option<String> = rowdata.get(ci).unwrap_or(&NONE_CELL);
            let cp = painter.with_clip_rect(rect);
            let changed = edits
                .as_ref()
                .is_some_and(|e| has_change(e.changes, ri, col));
            theme::paint_grid_row_bg(&cp, rect, ri, hovered, changed);
            // 编辑框归属:横向滚动时冻结列的编辑框由覆盖层持有(与底层互斥)
            let editing_here =
                edits.as_mut().is_some_and(|e| *e.editing == Some((ri, ci))) && prev_hscroll > 0.5;
            if editing_here {
                editing_rendered.set(true);
                let e = edits.as_mut().unwrap();
                let end = Cell::new(None);
                cell_editor(ui, rect, e.draft, sel.is_some_and(|s| s.spanning()), &end);
                if let Some(x) = end.take() {
                    if x.commit {
                        out.editor_commit.set(Some((ri, ci, e.draft.clone())));
                    }
                    if x.motion.is_some() {
                        out.after_edit.set(x.motion);
                    }
                    *e.editing = None;
                    out.refocus.set(true);
                }
            } else {
                let shown = edits
                    .as_ref()
                    .map_or(v, |e| shown_value(e.changes, v, ri, col));
                let resp = ui.interact(
                    rect,
                    egui::Id::new(("qfz-c", id_salt, ri, ci)),
                    cell_sense(),
                );
                let _ = resp.clone().context_menu(|ui| {
                    cell_menu(
                        ui,
                        columns,
                        rows,
                        sel,
                        ri,
                        ci,
                        shown,
                        edits.is_some(),
                        out,
                    );
                });
                if menu_open_last || (pointer_here && resp.contains_pointer()) {
                    if resp.context_menu_opened() {
                        menu_open.set(true);
                    }
                }
                theme::paint_grid_cell_text(
                    &cp,
                    rect,
                    col,
                    shown,
                    changed,
                    numeric.get(ci).copied().unwrap_or(false),
                );
            }
            x += w;
        }
    }
    // 与滚动区分界的右缘竖线
    painter.line_segment(
        [
            egui::pos2(left + region_w - 0.5, top),
            egui::pos2(left + region_w - 0.5, bottom),
        ],
        egui::Stroke::new(2.0, p.border),
    );
    // 选区高亮(冻结区的列)
    if let Some(s) = sel {
        let clip = egui::Rect::from_min_max(
            egui::pos2(left + theme::ROWNO_W, top + theme::data_header_h()),
            egui::pos2(left + region_w, bottom),
        );
        paint_selection(
            ui,
            s,
            geom,
            widths,
            frozen_n,
            offset_x,
            left,
            clip,
            true,
            edits.as_ref().and_then(|e| *e.editing),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(spec: &[&[Option<&str>]]) -> Vec<Vec<Option<String>>> {
        spec.iter()
            .map(|r| r.iter().map(|v| v.map(|s| s.to_string())).collect())
            .collect()
    }

    /// PG 的简单查询协议、执行计划、Parquet 预览都没有类型:类型数组为空也不能越界,
    /// 数字列退回按值预判
    #[test]
    fn grid_without_column_types_still_works() {
        let cols = ["id", "name", "zip", "amount"].map(String::from);
        let types: [String; 0] = [];
        let data = rows(&[
            &[Some("1"), Some("张三"), Some("010000"), Some("12.50")],
            &[Some("2"), None, Some("100000"), Some("-3")],
        ]);
        let flags = theme::numeric_flags(&cols, &types, &data);
        assert_eq!(flags, [true, false, false, true]);
        // 列数与类型数不一致时以列数为准
        let short = ["bigint".to_string()];
        assert_eq!(
            theme::numeric_flags(&cols, &short, &data),
            [true, false, false, true]
        );
        let w = theme::fitted_widths_typed(&data, &cols, &types, 800.0);
        assert_eq!(w.len(), cols.len());
        assert!(w.iter().all(|x| *x >= 84.0), "{w:?}");
    }

    /// 有类型时只看类型:存成文本的数字串(varchar)不右对齐
    #[test]
    fn numeric_flags_follow_types_when_present() {
        let cols = ["a", "b", "c"].map(String::from);
        let types = ["varchar", "int unsigned", "decimal"].map(String::from);
        let data = rows(&[&[Some("123"), Some("5"), Some("1.5")]]);
        assert_eq!(
            theme::numeric_flags(&cols, &types, &data),
            [false, true, true]
        );
    }

    /// 按值预判的边界:前导零 / 超长数字串 / 非数字 / 全 NULL / 没有行 都不算数字列
    #[test]
    fn value_inference_rejects_identifiers_and_empty_columns() {
        let cols = ["a", "b", "c", "d", "e"].map(String::from);
        let types: [String; 0] = [];
        let data = rows(&[
            &[
                Some("0123"),
                Some("6222020200112345678"),
                Some("12a"),
                None,
                Some("1"),
            ],
            &[
                Some("456"),
                Some("6222030300987654321"),
                None,
                None,
                Some("2.5"),
            ],
        ]);
        assert_eq!(
            theme::numeric_flags(&cols, &types, &data),
            [false, false, false, false, true]
        );
        // 一行数据都没有:无从判断,全部不右对齐
        assert_eq!(theme::numeric_flags(&cols, &types, &[]), [false; 5]);
        assert!(
            theme::looks_numeric("-3.5e2")
                && theme::looks_numeric("0.5")
                && theme::looks_numeric("0")
                && theme::looks_numeric("123456789012345")
        );
        for (v, why) in [
            ("", "空"),
            ("NaN", "不是十进制数字"),
            ("inf", "不是十进制数字"),
            ("1,000", "千分位"),
            ("1234567890123456", "16 位"),
            ("6222020200112345678", "卡号"),
        ] {
            assert!(!theme::looks_numeric(v), "{why}: {v:?}");
        }
    }

    #[test]
    fn key_motion_maps_navigation_keys() {
        use egui::{Key, Modifiers};
        let m = Modifiers::NONE;
        assert_eq!(
            key_motion(Key::ArrowDown, m),
            Some((GridMotion::Rows(1), false))
        );
        assert_eq!(
            key_motion(Key::ArrowLeft, m),
            Some((GridMotion::Cols(-1), false))
        );
        // Shift = 扩展选区;Shift+Tab = 反向移动(不是扩展)
        let shift = Modifiers {
            shift: true,
            ..Default::default()
        };
        assert_eq!(
            key_motion(Key::ArrowRight, shift),
            Some((GridMotion::Cols(1), true))
        );
        assert_eq!(key_motion(Key::Tab, m), Some((GridMotion::Tab(1), false)));
        assert_eq!(
            key_motion(Key::Tab, shift),
            Some((GridMotion::Tab(-1), false))
        );
        assert_eq!(
            key_motion(Key::Home, m),
            Some((GridMotion::FirstCol, false))
        );
        assert_eq!(key_motion(Key::End, m), Some((GridMotion::LastCol, false)));
        assert_eq!(
            key_motion(Key::PageUp, m),
            Some((GridMotion::Page(-1), false))
        );
        // ⌘+方向 = 跳到行/列首尾
        let cmd = Modifiers {
            command: true,
            ..Default::default()
        };
        assert_eq!(
            key_motion(Key::ArrowDown, cmd),
            Some((GridMotion::LastRow, false))
        );
        assert_eq!(
            key_motion(Key::ArrowLeft, cmd),
            Some((GridMotion::FirstCol, false))
        );
        // ⌘+Home/End 不是导航;回车/删除/字母都不是(⌥/⌃ 组合留给别的用途)
        assert_eq!(key_motion(Key::Home, cmd), None);
        assert_eq!(key_motion(Key::Enter, m), None);
        assert_eq!(key_motion(Key::Delete, m), None);
        assert_eq!(
            key_motion(
                Key::ArrowUp,
                Modifiers {
                    alt: true,
                    ..Default::default()
                }
            ),
            None
        );
    }

    #[test]
    fn move_cell_clamps_and_tab_wraps() {
        let bounds = (8, 6);
        assert_eq!(move_cell((0, 0), GridMotion::Rows(-1), bounds, 10), (0, 0));
        assert_eq!(move_cell((7, 5), GridMotion::Cols(1), bounds, 10), (7, 5));
        // Tab:行尾折到下一行行首;最后一格停在原地;行首折到上一行行尾
        assert_eq!(move_cell((2, 5), GridMotion::Tab(1), bounds, 10), (3, 0));
        assert_eq!(move_cell((7, 5), GridMotion::Tab(1), bounds, 10), (7, 0));
        assert_eq!(move_cell((2, 0), GridMotion::Tab(-1), bounds, 10), (1, 5));
        // 翻页按一屏行数走,越界夹取
        assert_eq!(move_cell((1, 1), GridMotion::Page(1), bounds, 3), (4, 1));
        assert_eq!(move_cell((6, 1), GridMotion::Page(1), bounds, 3), (7, 1));
        assert_eq!(move_cell((6, 1), GridMotion::Page(-1), bounds, 3), (3, 1));
        assert_eq!(move_cell((6, 2), GridMotion::LastCol, bounds, 3), (6, 5));
        assert_eq!(move_cell((6, 2), GridMotion::FirstRow, bounds, 3), (0, 2));
    }

    #[test]
    fn clipboard_parses_rows_and_cells() {
        assert_eq!(
            parse_clipboard("a\tb\nc\td\r\n"),
            vec![vec!["a", "b"], vec!["c", "d"]]
        );
        assert_eq!(parse_clipboard("单格"), vec![vec!["单格"]]);
        // 空单元格保留(贴进去 = 置 NULL),不塌成少一列
        assert_eq!(parse_clipboard("\tb\n\n"), vec![vec!["", "b"]]);
    }

    /// 指针 → 单元格:行号列/表头/表尾/占位列都不算;横向滚动按内容坐标换算;
    /// 冻结覆盖层只在滚动后出现,钉住的列优先于底层滚到同一位置的列
    #[test]
    fn cell_at_maps_pointer_to_cell() {
        // 行高/表头高随密度变:持渲染锁,免得并行的紧凑密度测试中途把它们改掉
        let _guard = crate::render_test::RENDER_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // 行号列 48,三列各 100,占位列 300;网格左缘 200 → 三列占 [248,348) [348,448) [448,548)
        let widths = [48.0, 100.0, 100.0, 100.0, 300.0];
        let top = 300.0;
        let row0 = top + theme::data_header_h() + theme::row_h() / 2.0;
        let geom = GridGeom {
            header_top: Some(top),
            vis_rows: (0..5)
                .map(|i| (i, top + theme::data_header_h() + i as f32 * theme::row_h()))
                .collect(),
        };
        let at = |x: f32, frozen: usize, off: f32| {
            cell_at(egui::pos2(x, row0), &geom, &widths, frozen, off, 200.0)
        };
        // 没滚动:列边界落在右缘(左闭右开)
        assert_eq!(at(230.0, 0, 0.0), None, "行号列");
        assert_eq!(at(248.0, 0, 0.0), Some((0, 0)));
        assert_eq!(at(347.9, 0, 0.0), Some((0, 0)));
        assert_eq!(at(348.0, 0, 0.0), Some((0, 1)));
        assert_eq!(at(448.0, 0, 0.0), Some((0, 2)));
        assert_eq!(at(548.0, 0, 0.0), None, "占位列");
        assert_eq!(
            cell_at(egui::pos2(300.0, top + 10.0), &geom, &widths, 0, 0.0, 200.0),
            None,
            "表头"
        );
        assert_eq!(
            cell_at(
                egui::pos2(300.0, row0 + 6.0 * theme::row_h()),
                &geom,
                &widths,
                0,
                0.0,
                200.0
            ),
            None,
            "表尾空白"
        );
        assert_eq!(
            cell_at(
                egui::pos2(300.0, row0 + 4.0 * theme::row_h()),
                &geom,
                &widths,
                0,
                0.0,
                200.0
            ),
            Some((4, 0)),
            "最后一行"
        );
        // 横向滚了 60:内容左移,第 0 列只剩 [188,288) 可点
        assert_eq!(at(287.0, 0, 60.0), Some((0, 0)));
        assert_eq!(at(288.0, 0, 60.0), Some((0, 1)));
        assert_eq!(at(388.0, 0, 60.0), Some((0, 2)));
        // 冻结 1 列且已滚动:钉住的第 0 列盖在 [248,348),压住底层滚到这里的第 0/1 列
        assert_eq!(at(250.0, 1, 60.0), Some((0, 0)), "冻结列命中");
        assert_eq!(at(347.0, 1, 60.0), Some((0, 0)));
        assert_eq!(at(348.0, 1, 60.0), Some((0, 1)), "冻结区右侧是滚动区");
        assert_eq!(at(388.0, 1, 60.0), Some((0, 2)));
        // 还没滚动:覆盖层没出现,冻结列与普通列无异
        assert_eq!(at(250.0, 1, 0.0), Some((0, 0)));
        assert_eq!(at(350.0, 1, 0.0), Some((0, 1)));
        // 行号列(整行选择的命中)
        assert_eq!(rowno_at(egui::pos2(230.0, row0), &geom, 200.0), Some(0));
        assert_eq!(rowno_at(egui::pos2(250.0, row0), &geom, 200.0), None);
    }

    /// 复制选区:NULL 写成 "NULL",格内的制表符/换行压成空格(贴进 Excel 不串列);
    /// 带列名时首行是列名;越界的右下角被夹取
    #[test]
    fn selection_tsv_flattens_tabs_and_nulls() {
        let cols = ["id", "name", "note"].map(String::from);
        let data = rows(&[
            &[Some("1"), Some("a\tb"), Some("x\ny")],
            &[Some("2"), None, Some("z")],
            &[Some("3"), Some("c"), None],
        ]);
        assert_eq!(
            selection_tsv(&cols, &data, ((0, 0), (1, 2)), false),
            "1\ta b\tx y\n2\tNULL\tz"
        );
        assert_eq!(
            selection_tsv(&cols, &data, ((0, 0), (1, 2)), true),
            "id\tname\tnote\n1\ta b\tx y\n2\tNULL\tz"
        );
        // 只要第 2 列(从 0 数)
        assert_eq!(
            selection_tsv(&cols, &data, ((0, 1), (2, 1)), true),
            "name\na b\nNULL\nc"
        );
        assert_eq!(selection_tsv(&cols, &data, ((2, 2), (2, 2)), false), "NULL");
        // 右下角超出结果集:按实际有的行列输出
        assert_eq!(selection_tsv(&cols, &data, ((2, 2), (9, 9)), false), "NULL");
        assert_eq!(
            selection_tsv(&cols, &data, ((0, 2), (9, 9)), true),
            "note\nx y\nz\nNULL"
        );
    }

    /// 单元格显示值:待保存改动优先于原始值——包括"改成 NULL"(此前改成 NULL 后
    /// 网格里仍显示旧值,只有底色提示,看着像没生效)
    #[test]
    fn shown_value_prefers_pending_change_including_null() {
        let cols = ["id", "name"].map(String::from);
        let data = rows(&[
            &[Some("1"), Some("甲")],
            &[Some("2"), Some("乙")],
            &[Some("3"), None],
        ]);
        let changes = vec![
            (0usize, "name".to_string(), None),
            (1usize, "name".to_string(), Some("丙".to_string())),
            (2usize, "name".to_string(), Some("丁".to_string())),
        ];
        assert_eq!(shown_value(&changes, &data[0][1], 0, "name"), &None);
        assert_eq!(
            shown_value(&changes, &data[1][1], 1, "name").as_deref(),
            Some("丙")
        );
        // 本来就是 NULL、被改成有值
        assert_eq!(
            shown_value(&changes, &data[2][1], 2, "name").as_deref(),
            Some("丁")
        );
        // 没有改动的格取原始值
        assert_eq!(
            shown_value(&changes, &data[0][0], 0, "id").as_deref(),
            Some("1")
        );
    }

    #[test]
    fn paste_and_fill_produce_per_cell_writes() {
        let blk = |v: &[&[&str]]| -> Vec<Vec<String>> {
            v.iter()
                .map(|line| line.iter().map(|s| s.to_string()).collect())
                .collect()
        };
        // 2×3 的块贴到 3×3 结果集的 (2,1):越界的行、列丢弃
        let (cells, last) =
            paste_cells((3, 3), (2, 1), &blk(&[&["a", "b", "c"], &["d", "e", "f"]]));
        assert_eq!(cells, vec![(2, 1, "a".into()), (2, 2, "b".into())]);
        assert_eq!(last, Some((2, 2)));
        let (cells, last) = paste_cells((3, 3), (0, 0), &blk(&[&["a", "b"], &["c", "d"]]));
        assert_eq!(cells.len(), 4);
        assert_eq!(last, Some((1, 1)));
        // 空块什么都不写
        assert_eq!(paste_cells((3, 3), (0, 0), &[]).1, None);
        // 填充:选区里每一格一条
        let cells = fill_cells(((0, 1), (2, 2)), "");
        assert_eq!(cells.len(), 6);
        assert!(cells.iter().all(|(_, _, v)| v.is_empty()));
        assert!(cells.contains(&(1, 2, String::new())));
    }

    /// 单元格可点(右键菜单/双击)但不参与 Tab 焦点遍历:焦点位是网格自己那一个
    #[test]
    fn cells_are_clickable_but_not_focusable() {
        assert!(cell_sense().senses_click());
        assert!(!cell_sense().is_focusable());
        assert!(!cell_sense().senses_drag());
    }

    /// 选中态按网格 id 存取;结果集形状变了(重新查询)旧选区作废
    #[test]
    fn selection_is_stored_per_grid_and_invalidated_by_shape() {
        let ctx = egui::Context::default();
        store_sel(&ctx, "g", Some(GridSel::new((1, 2), (0, 1), (5, 3))));
        assert_eq!(selection(&ctx, "g", (5, 3)), Some(((0, 1), (1, 2))));
        assert_eq!(selection(&ctx, "g", (5, 4)), None, "列数变了");
        assert_eq!(selection(&ctx, "g", (6, 3)), None, "行数变了");
        assert_eq!(selection(&ctx, "other", (5, 3)), None, "按网格 id 隔离");
        store_sel(&ctx, "g", None);
        assert_eq!(selection(&ctx, "g", (5, 3)), None);
    }
}
