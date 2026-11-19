# Milestones

## Done

- 建立 Rust binary crate
- 使用 `asdfqwer` 选择区域，使用 `uio` 点击
- 实现 4、8、水平和垂直分割
- 实现像素余数分配和 Backspace 历史回退
- 为核心状态机和配置解析添加测试
- 记录 Wayland 协议边界和不做屏幕检测的约束
- 建立 layer-shell overlay、键盘焦点和 `wl_shm` 软件绘制
- 接入 `xkbcommon` 按键解析和 `wlr-virtual-pointer` 移动与点击
- 提供 `SUPER + F` 的已安装 `binger` toggle 绑定示例
- 在单屏 Hyprland、1 倍缩放环境验证 overlay、键盘选择、Backspace、Escape 和虚拟指针流程
- 用单屏 Hyprland 的 GTK 测试窗口验证左键、右键和中键的按下与释放
- 验证八格、四格和小选区的离屏渲染，以及 2 倍缩放的像素尺寸
- 用模拟 Wayland 合成器验证第二屏的指针绑定、逻辑坐标、回退、取消和覆盖层关闭后的点击顺序

## Next

1. 在 Hyprland 多屏会话中验证覆盖层和点击是否落在同一块显示器，包括 HiDPI 和混合缩放。

## Deliberate exclusions

- OpenCV 和屏幕截图
- OCR、窗口识别和目标检测
- tile、floating、split 等额外模式
- 复杂配置系统和插件接口

