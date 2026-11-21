//! 配置中心控制台(Nacos/Consul/Etcd 三类型共用):键列表(前缀过滤)/
//! 值查看编辑/保存/删除/新建。三类型经 backend_dispatch 共用同一后端。

use crate::app::KvTab;
use crate::table_view::TabAction;
use crate::{icons, theme};
use egui::RichText;

pub fn draw_kv_view(
    ctx: &egui::Context,
    tab_id: &str,
    k: &mut KvTab,
) -> Vec<TabAction> {
    let mut actions = Vec::new();
    let p = theme::c();

    egui::TopBottomPanel::top(format!("kv-top-{tab_id}"))
        .frame(
            egui::Frame::new()
                .fill(p.panel)
                .inner_margin(egui::Margin::symmetric(12, 8)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                theme::search_box(
                    ui,
                    &format!("kv-filter-{tab_id}"),
                    &mut k.filter,
                    ui_kit::t!("Filter keys…", "过滤键…"),
                    220.0,
                );
                let refresh = theme::button(
                    ui,
                    if k.loading {
                        ui_kit::t!("Loading…", "加载中…")
                    } else {
                        ui_kit::t!("Refresh", "刷新")
                    },
                    theme::BtnKind::Primary,
                );
                if refresh.clicked() && !k.loading {
                    actions.push(TabAction::KvLoad {
                        tab_id: tab_id.to_string(),
                    });
                }
                if let Some(err) = &k.error {
                    ui.label(RichText::new(icons::clip(err, 80)).size(11.5).color(p.red));
                }
            });
        });

    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(p.bg)
                .inner_margin(egui::Margin::symmetric(12, 8)),
        )
        .show(ctx, |ui| {
            if k.loading && k.entries.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.add_space(30.0);
                    ui.add(egui::Spinner::new().size(16.0).color(p.muted));
                });
                return;
            }
            // 过滤
            let shown: Vec<shujing_core::kv::KvEntry> = k
                .entries
                .iter()
                .filter(|e| k.filter.is_empty() || e.key.contains(&k.filter))
                .cloned()
                .collect();
            // 新建/编辑区
            if let Some(sel) = &k.selected.clone() {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(ui_kit::t!("Key", "键"))
                            .size(11.0)
                            .color(p.muted),
                    );
                    ui.label(
                        RichText::new(icons::clip(sel, 60))
                            .font(theme::mono(12.0))
                            .color(p.text),
                    );
                });
                ui.add_space(4.0);
                let mut value = k.edit_value.clone().unwrap_or_default();
                theme::form_field_multiline(
                    ui,
                    ui_kit::t!("Value (saved with Save)", "值(保存写入)"),
                    &format!("kv-value-{tab_id}"),
                    &mut value,
                    640.0,
                    8,
                );
                k.edit_value = Some(value);
                ui.horizontal(|ui| {
                    let save = theme::button(
                        ui,
                        if k.busy {
                            ui_kit::t!("Saving…", "保存中…")
                        } else {
                            ui_kit::t!("Save", "保存")
                        },
                        theme::BtnKind::Primary,
                    );
                    if save.clicked() && !k.busy {
                        actions.push(TabAction::KvSave {
                            tab_id: tab_id.to_string(),
                            key: sel.clone(),
                            value: k.edit_value.clone().unwrap_or_default(),
                        });
                    }
                    let dlabel = if k.drop_arm {
                        ui_kit::t!("Click again to delete", "再点一次确认删除")
                    } else {
                        ui_kit::t!("Delete Key", "删除键")
                    };
                    let db2 = theme::button(ui, dlabel, theme::BtnKind::Secondary);
                    if db2.clicked() && !k.busy {
                        if k.drop_arm {
                            actions.push(TabAction::KvDelete {
                                tab_id: tab_id.to_string(),
                                key: sel.clone(),
                            });
                            k.drop_arm = false;
                        } else {
                            k.drop_arm = true;
                        }
                    }
                    if !k.drop_arm {
                        db2.on_disabled_hover_text(ui_kit::t!(
                            "Destructive: removes this key from the config store",
                            "破坏性操作:从配置中心删除该键"
                        ));
                    }
                    ui.add_space(10.0);
                    if theme::button(ui, ui_kit::t!("Close", "收起"), theme::BtnKind::Secondary)
                        .clicked()
                    {
                        k.selected = None;
                        k.edit_value = None;
                        k.drop_arm = false;
                    }
                });
                ui.add_space(8.0);
                ui.separator();
            }
            // 新建键行
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(ui_kit::t!("New key", "新建键"))
                        .size(11.0)
                        .color(p.muted),
                );
                theme::field(ui, &format!("kv-new-{tab_id}"), &mut k.new_key, 240.0, false);
                let cb = theme::button(
                    ui,
                    if k.busy {
                        ui_kit::t!("…", "…")
                    } else {
                        ui_kit::t!("Create", "创建")
                    },
                    theme::BtnKind::Secondary,
                );
                if cb.clicked() && !k.busy && !k.new_key.trim().is_empty() {
                    actions.push(TabAction::KvSave {
                        tab_id: tab_id.to_string(),
                        key: k.new_key.trim().to_string(),
                        value: String::new(),
                    });
                    k.new_key.clear();
                }
            });
            ui.add_space(6.0);
            // 键列表
            ui.label(
                RichText::new(format!(
                    "{} {}",
                    shown.len(),
                    ui_kit::t!("keys", "个键")
                ))
                .size(11.0)
                .color(p.muted),
            );
            ui.add_space(2.0);
            egui::ScrollArea::vertical()
                .id_salt(format!("kv-keys-{tab_id}"))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for e in shown.iter().take(500) {
                        let selected = k.selected.as_deref() == Some(e.key.as_str());
                        if ui
                            .selectable_label(
                                selected,
                                RichText::new(&e.key).font(theme::mono(12.0)).color(p.text),
                            )
                            .clicked()
                        {
                            k.selected = Some(e.key.clone());
                            k.edit_value = Some(e.value.clone());
                            k.drop_arm = false;
                        }
                        if e.binary {
                            ui.label(
                                RichText::new("hex")
                                    .size(10.0)
                                    .color(DIFF_ORANGE),
                            );
                        } else {
                            ui.label(
                                RichText::new(format!("{} B", e.value.len()))
                                    .size(10.5)
                                    .color(p.muted),
                            );
                        }
                        ui.end_row();
                    }
                });
        });
    actions
}

const DIFF_ORANGE: egui::Color32 = egui::Color32::from_rgb(0xE0, 0x8A, 0x00);
