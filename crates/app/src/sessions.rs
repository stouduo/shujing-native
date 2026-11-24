//! 服务器会话面板:MySQL SHOW FULL PROCESSLIST / PG pg_stat_activity 的
//! 网格展示 + 终止查询/终止连接(armed 二次确认)。仅 MySQL/PG 支持的连接
//! 在侧栏右键菜单出现入口。

use crate::app::SessionsTab;
use crate::table_view::TabAction;
use crate::{icons, theme};
use egui::RichText;

pub fn draw_sessions_view(
    ctx: &egui::Context,
    tab_id: &str,
    s: &mut SessionsTab,
) -> Vec<TabAction> {
    let mut actions = Vec::new();
    let p = theme::c();
    let is_mysql = s.db_type == shujing_core::DbType::MySql;

    egui::TopBottomPanel::top(format!("sessions-top-{tab_id}"))
        .frame(
            egui::Frame::new()
                .fill(p.panel)
                .inner_margin(egui::Margin::symmetric(12, 8)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(if is_mysql {
                        ui_kit::t!("MySQL Processlist", "MySQL 进程列表")
                    } else {
                        ui_kit::t!("pg_stat_activity", "pg_stat_activity")
                    })
                    .size(theme::fs_px(11.0))
                    .color(p.muted),
                );
                let refresh = theme::button(
                    ui,
                    if s.loading {
                        ui_kit::t!("Loading…", "刷新中…")
                    } else {
                        ui_kit::t!("Refresh", "刷新")
                    },
                    theme::BtnKind::Primary,
                );
                if refresh.clicked() && !s.loading {
                    actions.push(TabAction::SessionsRefresh {
                        tab_id: tab_id.to_string(),
                    });
                }
                ui.label(
                    RichText::new(format!("{} {}", s.rows.len(), ui_kit::t!("sessions", "个会话")))
                        .size(theme::fs_px(11.0))
                        .color(p.muted),
                );
            });
            if let Some(err) = &s.error {
                ui.label(RichText::new(icons::clip(err, 120)).size(theme::fs_px(11.5)).color(p.red));
            }
        });

    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(p.bg)
                .inner_margin(egui::Margin::symmetric(12, 8)),
        )
        .show(ctx, |ui| {
            if s.loading && s.rows.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.add_space(30.0);
                    ui.add(egui::Spinner::new().size(theme::fs_px(16.0)).color(p.muted));
                });
                return;
            }
            if s.rows.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.add_space(30.0);
                    ui.label(
                        RichText::new(ui_kit::t!(
                            "No other sessions",
                            "没有其他活动会话"
                        ))
                        .size(theme::fs_px(12.0))
                        .color(p.muted),
                    );
                });
                return;
            }
            ui.push_id(format!("sessions-grid-{tab_id}"), |ui| {
                egui::ScrollArea::vertical()
                    .id_salt(format!("sessions-scroll-{tab_id}"))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        egui::Grid::new(format!("sessions-g-{tab_id}"))
                            .num_columns(8)
                            .spacing([10.0, 4.0])
                            .show(ui, |ui| {
                                for h in [
                                    ui_kit::t!("ID", "ID"),
                                    ui_kit::t!("User", "用户"),
                                    ui_kit::t!("Host", "主机"),
                                    ui_kit::t!("Database", "库"),
                                    ui_kit::t!("Command/State", "命令/状态"),
                                    ui_kit::t!("Time(s)", "耗时(秒)"),
                                    ui_kit::t!("Query", "SQL"),
                                    "",
                                ] {
                                    ui.label(RichText::new(h).size(theme::fs_px(10.5)).color(p.muted));
                                }
                                ui.end_row();
                                for row in s.rows.clone() {
                                    ui.label(RichText::new(&row.id).size(theme::fs_px(11.5)).color(p.text));
                                    ui.label(RichText::new(icons::clip(&row.user, 16)).size(theme::fs_px(11.5)).color(p.text));
                                    ui.label(RichText::new(icons::clip(&row.host, 22)).font(theme::mono(11.0)).color(p.text2));
                                    ui.label(
                                        RichText::new(row.db.as_deref().unwrap_or("—"))
                                            .size(theme::fs_px(11.5))
                                            .color(p.text2),
                                    );
                                    ui.label(
                                        RichText::new(icons::clip(
                                            &if row.command.is_empty() { row.state.clone() } else { row.command },
                                            14,
                                        ))
                                        .size(theme::fs_px(11.5))
                                        .color(p.text2),
                                    );
                                    ui.label(
                                        RichText::new(row.time_secs.to_string())
                                            .size(theme::fs_px(11.5))
                                            .color(if row.time_secs > 60 { theme::c().orange } else { p.text }),
                                    );
                                    ui.label(
                                        RichText::new(icons::clip(&row.query, 44))
                                            .font(theme::mono(11.0))
                                            .color(p.text2)
                                            .strong(),
                                    );
                                    // 操作列:终止查询 / 终止连接(armed 二次确认)
                                    let armed = s.armed.as_deref() == Some(row.id.as_str());
                                    let qlabel = if armed {
                                        ui_kit::t!("Sure?", "确认?")
                                    } else {
                                        ui_kit::t!("Kill Query", "终止查询")
                                    };
                                    if theme::button(ui, qlabel, theme::BtnKind::Secondary).clicked()
                                        && !s.loading
                                    {
                                        if armed {
                                            actions.push(TabAction::SessionKill {
                                                tab_id: tab_id.to_string(),
                                                sid: row.id.clone(),
                                                query_only: true,
                                            });
                                            s.armed = None;
                                        } else {
                                            s.armed = Some(row.id.clone());
                                        }
                                    }
                                    ui.end_row();
                                }
                            });
                    });
            });
            ui.add_space(4.0);
            ui.label(
                RichText::new(ui_kit::t!(
                    "Kill Query interrupts the running statement only; the connection stays alive. Full terminate is not exposed here to avoid accidental drops",
                    "终止查询仅中断当前执行的语句,连接保留;为避免误杀,整机断连不在此面板提供"
                ))
                .size(theme::fs_px(10.5))
                .color(p.muted),
            );
        });
    actions
}

