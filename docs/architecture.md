# Architecture

## Runtime flow

1. 连接 Wayland display 并枚举 compositor、shm、seat、output、layer shell 和 virtual pointer globals。
2. 创建覆盖整个目标 output 的 overlay layer surface。
3. 请求键盘焦点，并把 surface 的 input region 设为空，让鼠标事件继续传递给下面的应用。
4. 在设置了 `HYPRLAND_INSTANCE_SIGNATURE` 的 Hyprland 会话中订阅 compositor 事件。其他 layer surface 关闭后，重新提交一次键盘交互状态，以恢复截图选择器退出时可能丢失的键盘焦点。
5. 将当前区域初始化为 output 的逻辑矩形。
6. 为当前选区绘制双色格子和子区域标签。八格布局上排显示 `qwer`，下排显示 `asdf`。
7. 区域选择和回退会将点击目标置于选区中心。`hjkl` 在显示器逻辑坐标内微调点击目标。长按沿用 Wayland 的键盘重复启动时机，启动后每 15 毫秒更新一次位置。指针和准星使用同一个目标坐标。
8. 收到点击键（默认为 `u`、`i`、`o`）后，先销毁覆盖层并同步 Wayland 连接，再移动指针到记录的目标位置并分别发送鼠标按下和释放帧。
9. 收到 `Escape` 后销毁 surface 并退出，不发送指针事件。

## Wayland objects

必需对象如下。

- `wl_compositor` 和 `wl_shm` 用于创建和绘制 surface
- `wl_seat` 和 `wl_keyboard` 用于接收键盘输入
- `zwlr_layer_shell_v1` 用于 overlay surface 和键盘交互
- `zwlr_virtual_pointer_manager_v1` 用于移动指针和点击
- `zxdg_output_manager_v1` 在 layer-shell 未给出尺寸时提供逻辑尺寸

不引入 `wlr-screencopy`。项目不读取屏幕内容，也不依赖 OpenCV。

## Rust module boundary

`bisect` 管理选区、撤销历史和逻辑坐标下的点击目标。`input` 将按键映射到选区、点击或微调。Wayland adapter 负责处理键盘事件和移动虚拟指针。渲染器读取选区和点击目标来绘制网格和准星。

## Output policy

覆盖层交由合成器选择显示器。客户端通过 `wl_surface.enter` 获得覆盖层所在的输出，并用 virtual-pointer v2 把指针绑定到该输出。多屏环境要求协议支持输出绑定，以免将点击落在其他屏幕上。

## Rendering policy

`wl_shm` 软件缓冲区按输出缩放因子绘制，指针定位使用逻辑坐标。渲染器使用系统字体，按格子大小调整字母尺寸，在小选区周围布置标签。微调时复用网格和字形，只向合成器标记新旧准星所在区域。

