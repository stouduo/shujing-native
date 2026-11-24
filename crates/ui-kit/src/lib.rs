//! ui-kit:iOS 风格深浅双主题的 egui 组件库,从数镜(数据库客户端)沉淀而来。
//!
//! 内容:
//! - [`theme`]:调色板(深/浅+6 强调色)、字号/行高标尺、按钮/输入框/下拉/复选框/
//!   分段控件/搜索框/弹窗套件/发丝线/文本样式,以及 `apply(ctx)` 一键换肤
//!   (含点击手抖容忍:位移 20px/按住 3s 内仍算点击)
//! - [`icons`]:painter 直绘图标(不依赖字体字形)+ 文本工具(截断/基线)
//! - [`scroll`]:滚动纪律(滚轮一步到位/按压冻结/悬浮细滚动条)——
//!   egui 默认滚轮惯性尾巴会让"滚完立刻点"的点击被静默取消
//!
//! 字体需宿主应用自行安装(中文字体/等宽字体等),本库只消费 FontId 族名。

pub mod i18n;
pub mod icons;
pub mod scroll;
pub mod theme;
pub mod widgets;

pub use theme::{apply, c, is_dark, set_dark};
