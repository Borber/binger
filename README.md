# binger

`binger` 是一个面向 wlroots Wayland 合成器的键盘鼠标选择器。它只保留当前工作流中需要的固定路径。

- 在全屏透明覆盖层上进行二分选择
- `asdfqwer` 选择区域。八格布局上排为 `qwer`、下排为 `asdf`，四格布局使用 `asdf`
- `h`、`j`、`k`、`l` 以准星为起点向左、下、上、右移动。点按移动 1 个逻辑像素。长按连发后每 15 毫秒移动一次，步长逐渐增至 6 个逻辑像素
- `u`、`i`、`o` 分别触发左键、右键和中键
- `Backspace` 回到上一级选区并将准星移到该选区中心
- `Escape` 取消
- 不截图、不做 OCR、不做 OpenCV 区域检测
- 不提供 tile、floating、split 等额外选择模式

覆盖层用两种半透明颜色区分相邻格子。字母随格子缩放，格子变小时移到选区外。选区外的桌面保持可见。

启动时由合成器决定显示器。微调可以跨格，范围限于当前显示器。点击时先关闭覆盖层，再把指针移到准星位置并发送鼠标按下和释放事件。已在单屏 Hyprland 中用 GTK 窗口确认三种点击。

## 构建检查

```bash
cargo test
cargo run -- --print-keymap
cargo run
cargo build --locked --release
```

默认构建启用 Wayland 后端。用 `cargo test --no-default-features` 可以只构建和测试核心逻辑。启动时可以指定配置文件。

release profile 使用三级优化、Thin LTO、单代码生成单元、`panic = "abort"` 和符号剥离。

```bash
cargo run -- --config ~/.config/binger/config.toml
```

当 `home_row_keys` 自定义的选区键或点击键与微调键冲突时，配置优先。使用 `--print-keymap` 查看可用的微调键。

## Hyprland 启动绑定

后端完成后可以使用下面的绑定保留现有的启动和关闭行为：

```ini
bind = SUPER, F, exec, pgrep -x binger >/dev/null && pkill -x binger || binger
```

## 设计边界

实现依赖 `wlr-layer-shell` 创建全屏 overlay，依赖 `wl_keyboard` 和 `xkbcommon` 接收并解析当前键盘焦点，依赖 `wlr-virtual-pointer` 发出绝对指针移动和鼠标按钮事件。Wayland 输出信息只用于确定 overlay 的逻辑尺寸，不参与界面识别。

详细的后端拆分和里程碑见 [`docs/architecture.md`](docs/architecture.md) 与 [`docs/milestones.md`](docs/milestones.md)。

发布流程见 [`docs/releasing.md`](docs/releasing.md)。

## License

MIT. See [`LICENSE`](LICENSE).

