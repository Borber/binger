# Upstream reference

行为参考来自 [`moverest/wl-kbptr`](https://github.com/moverest/wl-kbptr)。本项目只提取固定的 bisect 和 click 路径，重新实现 Rust 后端，不引入上游的截图检测功能。

上游实现确认了这些协议边界。

- `wlr-layer-shell` 提供全屏 overlay 和键盘交互
- `wl_keyboard` 接收 compositor 转发的按键
- `wlr-virtual-pointer` 发出绝对指针移动和鼠标按钮事件
- `xdg-output` 提供 output 的逻辑尺寸

上游项目的 `modes=bisect` 使用前八个 home-row 字符选择子区域，后三个字符映射左键、右键和中键。本项目默认配置为 `asdfqweruio`。

上游仓库采用 GPL-3.0-only，并且部分协议源码带有单独的 NTP 许可证。binger 的许可证和发布方式留待项目所有者决定。

