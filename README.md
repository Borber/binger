# binger

`binger` 是一个面向 wlroots Wayland 合成器的键盘鼠标选择器。它只保留当前工作流中需要的固定路径。

- 在全屏透明覆盖层上进行二分选择
- `asdfqwer` 选择区域。八格布局上排为 `qwer`、下排为 `asdf`，四格布局使用 `asdf`
- `h`、`j`、`k`、`l` 以准星为起点向左、下、上、右移动。点按移动 1 个逻辑像素。长按连发后按配置的间隔移动，步长逐渐增加
- 可以同时按住两个微调键进行斜向移动，例如 `h+j` 向左下移动
- `u`、`i`、`o` 分别触发左键、右键和中键
- `Enter`、`Space` 和数字键盘回车也触发左键
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

默认构建启用 Wayland 后端。用 `cargo test --no-default-features` 可以只构建和测试核心逻辑。启动时会自动读取 `$XDG_CONFIG_HOME/binger/config.toml`，未设置 `XDG_CONFIG_HOME` 时使用 `~/.config/binger/config.toml`。文件不存在时使用内置默认值，`--config PATH` 可以指定其他文件并覆盖自动路径。

release profile 使用三级优化、Thin LTO、单代码生成单元、`panic = "abort"` 和符号剥离。

```bash
cargo run -- --config ~/.config/binger/config.toml
```

`nudge_keys` 按左、下、上、右的顺序配置微调键。使用 `--print-keymap` 查看可用的微调键。

如果 `nudge_keys` 与 `home_row_keys` 重复，选区或点击键优先使用，重复的微调键不会生效。

`nudge_step` 控制单次微调的逻辑像素数，`nudge_interval_ms` 控制长按移动间隔，`nudge_acceleration_interval_ms` 控制加速周期，`nudge_max_step` 控制最大加速步长。

## Hyprland 启动绑定

在 Linux 上运行 `binger` 打开覆盖层，再运行一次关闭当前 Wayland 显示会话中的覆盖层。也可以用 `Escape` 关闭。

```ini
bind = SUPER, F, exec, binger
```

使用 Lua 配置的 Hyprland 可以为 `binger` 单独关闭覆盖层动画，让首帧直接显示。[Hyprland Layer Rules](https://wiki.hypr.land/Configuring/Core/Rules/Layer-Rules/)

```lua
hl.bind("SUPER + F", hl.dsp.exec_cmd("binger"))
hl.layer_rule({ match = { namespace = "binger" }, no_anim = true })
```

## 设计边界

实现依赖 `wlr-layer-shell` 创建全屏 overlay，依赖 `wl_keyboard` 和 `xkbcommon` 接收并解析当前键盘焦点，依赖 `wlr-virtual-pointer` 发出绝对指针移动和鼠标按钮事件。Wayland 输出信息只用于确定 overlay 的逻辑尺寸，不参与界面识别。

详细的后端拆分和里程碑见 [`docs/architecture.md`](docs/architecture.md) 与 [`docs/milestones.md`](docs/milestones.md)。

发布流程见 [`docs/releasing.md`](docs/releasing.md)。

## License

MIT. See [`LICENSE`](LICENSE).

