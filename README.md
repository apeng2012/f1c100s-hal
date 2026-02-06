# f1c100s-hal

Rust HAL (Hardware Abstraction Layer) crate for Allwinner F1C100S/F1C200S ARM9 microcontrollers.

> **Note**
> 本项目正在开发中，从 [ch32-hal](https://github.com/ch32-rs/ch32-hal) 移植而来，适配 Allwinner F1C100S/F1C200S 系列芯片。
> 欢迎反馈问题和贡献代码。

本 HAL 基于 [Embassy](https://github.com/embassy-rs/embassy) 框架设计，支持异步驱动。

## 芯片特性

F1C100S/F1C200S 是全志科技推出的低成本 ARM9 处理器：

- ARM926EJ-S 内核，五级流水线
- 内置 32MB DDR1 (F1C100S) / 64MB DDR1 (F1C200S)
- 支持 H.264/MPEG 视频解码，最高 720p@30fps
- 内置音频编解码器，支持耳机输出
- LCD RGB/i8080 接口，TV CVBS 输出
- 丰富外设：USB OTG、UART×3、SPI×2、TWI×3、PWM×2 等

## 外设支持状态

| 外设 | 状态 | 说明 |
|------|------|------|
| GPIO | ✅ | PA(4), PB(4), PC(4), PD(22), PE(13), PF(6) |
| CCU | ❌ | 时钟控制单元 |
| UART | ❌ | 3路串口 |
| SPI | ❌ | 2路 SPI |
| TWI (I2C) | ❌ | 3路 I2C |
| Timer | ❌ | 3路定时器 |
| PWM | ❌ | 2路 PWM 输出 |
| DMA | ❌ | 普通 DMA 和专用 DMA |
| ADC | ❌ | KEYADC (6位) / TP (12位触摸屏) |
| USB OTG | ❌ | USB 2.0 OTG |
| SD/MMC | ❌ | SD/MMC 卡接口 |
| Audio Codec | ❌ | 音频编解码器 |
| Display | ❌ | LCD/TV 输出 |
| CSI | ❌ | 摄像头接口 |
| IR | ❌ | 红外遥控 |

- ✅ 已完成
- ❌ 待实现

## 快速开始

### 环境准备

1. 安装 Rust nightly 工具链：
```bash
rustup install nightly
rustup component add rust-src --toolchain nightly
```

2. 安装 objcopy 工具（任选其一）：
```bash
# 方式1: ARM 工具链
sudo apt install gcc-arm-none-eabi

# 方式2: LLVM
sudo apt install llvm

# 方式3: cargo-binutils
cargo install cargo-binutils
rustup component add llvm-tools-preview
```

3. 安装 sunxi-fel（用于烧录/调试）：
```bash
sudo apt install sunxi-tools
```

### 编译示例

```bash
# 仅编译
./build_boot.sh blinky

# 编译并通过 FEL 模式运行
./build_boot.sh -r blinky

# 编译并烧录到 SPI Flash
./build_boot.sh -f blinky
```

编译产物：
- `target/boot/blinky.bin` - 原始二进制
- `target/boot/blinky_boot.bin` - 带 eGON.BT0 头的可启动镜像

### 示例代码

```rust
#![no_std]
#![no_main]

use arm9_rt::entry;
use hal::gpio::{DriveStrength, Level, Output};
use {f1c100s_hal as hal, panic_halt as _};

fn delay(count: u32) {
    for _ in 0..count {
        unsafe { core::arch::asm!("nop") };
    }
}

#[entry]
fn main() -> ! {
    let p = hal::init(Default::default());
    let mut led = Output::new(p.PE5, Level::Low, DriveStrength::default());

    loop {
        led.toggle();
        delay(100_000);
    }
}
```

## FEL 模式

F1C100S 支持 FEL 模式进行开发调试：

1. 将芯片置于 FEL 模式（通常是 SD 卡槽为空时上电）
2. 通过 USB 连接电脑
3. 使用 `sunxi-fel` 工具下载运行代码

```bash
# 检测设备
sunxi-fel ver

# 运行程序
./build_boot.sh -r blinky
```

## 内存布局

```
0x00000000 - 0x00007FFF : 内置 32KB SRAM
0x80000000 - 0x81FFFFFF : DDR1 (32MB, F1C100S)
0x80000000 - 0x83FFFFFF : DDR1 (64MB, F1C200S)
```

当前 HAL 使用内置 SRAM 运行，DDR 需要先调用 `dram::init()` 初始化 DRAM 控制器。

默认芯片型号为 F1C200S (64MB DDR1)，如需 F1C100S (32MB DDR1)：
```toml
[dependencies]
f1c100s-hal = { ..., default-features = false, features = ["time-driver-avs0", "debug-uart0", "f1c100s"] }
```

## 依赖项目

- [f1c100s-pac](https://github.com/apeng2012/f1c100s-pac) - 外设访问 crate
- [arm9](https://github.com/apeng2012/arm9) - ARM9 运行时支持

## License

MIT OR Apache-2.0
