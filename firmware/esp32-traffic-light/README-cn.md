# ESP32-C3 红绿灯固件

该固件把 Agent Activity Hub 的全局状态显示在三颗 LED 上，适用于
GFlash6/minic ESP32-C3 SuperMini 红绿灯板。默认接线为公共阳极 `GPIO7`、
绿灯 `GPIO10`、黄灯 `GPIO9`、红灯 `GPIO8`，三个灯均为低电平点亮。

上述硬件接线定义参考 [GFlash6/minic](https://github.com/GFlash6/minic)
项目（参考提交：[`a0149d7`](https://github.com/GFlash6/minic/commit/a0149d7ff86c5b50b6216743b4b267d93176f157)）。
感谢原作者 [GFlash6](https://github.com/GFlash6) 公开项目和接线信息。本目录的
固件不是上游固件副本；Agent Activity Hub 的逐行 JSON 协议、USB/BLE 接收和灯效同步
由本仓库实现并维护。

其他接线可通过 `LED_COMMON_ANODE_PIN`、`LED_GREEN_PIN`、`LED_YELLOW_PIN`、
`LED_RED_PIN` 和 `LED_ACTIVE_LOW` 编译宏修改；没有公共阳极控制脚时，将
`LED_COMMON_ANODE_PIN` 设为 `-1`。

## 刷写

1. 安装 VS Code 的 PlatformIO 插件，或安装 PlatformIO CLI。
2. 用支持数据传输的 USB 线连接 ESP32-C3。
3. 在本目录运行 `pio run -t upload`。
4. 打开 Agent Activity Hub 的“设置 > ESP32 设备”，刷新端口并连接带 ESP32 标记的端口。

固件同时广播名为 `Agent Activity Light` 的 BLE 设备，采用 Nordic UART Service UUID。
USB 和 BLE 接收相同的逐行 JSON 协议；当前桌面应用内置 USB 串口连接，BLE 接口可供
移动端或其他控制器直接使用。

## 协议

每条消息以换行符结束，协议版本为 `1`：

```json
{"type":"state","protocol":1,"status":"working","leds":"100","blink":false,"period":500,"brightness":100}
```

`leds` 的三位顺序固定为绿、黄、红；`period` 是闪烁单个亮/灭阶段的毫秒数。
