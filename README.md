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

### 编译与下载

项目提供统一的构建脚本 `build_boot.sh`，支持两种启动模式：

#### SPL 模式（默认，推荐）

SPL（`f1c100s-spl_uart0.bin`）先初始化时钟、SDRAM、MMU，主程序运行在 SDRAM 中，可用空间充足（代码 32MB + 数据 32MB）。

```bash
# 仅编译
./build_boot.sh blinky

# 编译并通过 FEL 模式运行（开发调试推荐）
./build_boot.sh -r blinky

# 编译并烧录到 SPI Flash
./build_boot.sh -f blinky
```

编译产物：
- `target/boot/blinky_spl.bin` — 主程序二进制（加载到 SDRAM 0x80008000）
- `target/boot/blinky_flash.bin` — SPI Flash 完整镜像（SPL + uImage）

#### Direct 模式（-d）

程序直接运行在 32KB 内置 SRAM 中，带 eGON.BT0 启动头。空间受限（24KB 代码 + 8KB 数据），适用于极简场景。不支持 FEL 运行。

```bash
# 仅编译
./build_boot.sh -d blinky

# 编译并烧录到 SPI Flash
./build_boot.sh -d -f blinky
```

编译产物：
- `target/boot/blinky.bin` — 原始二进制
- `target/boot/blinky_boot.bin` — 带 eGON.BT0 头的可启动镜像

> **注意：** `-d` 和 `-r` 不能同时使用，Direct 模式下 FEL 运行不可用。

### FEL 模式

F1C100S 支持 FEL 模式进行开发调试：

1. 将芯片置于 FEL 模式（通常是 SD 卡槽为空时上电）
2. 通过 USB 连接电脑
3. 使用 `sunxi-fel` 工具下载运行代码

```bash
# 检测设备
sunxi-fel ver

# 编译并运行（SPL 模式）
./build_boot.sh -r blinky
```

FEL 模式下的执行流程：
1. `sunxi-fel spl` 将 SPL 写入 SRAM 并执行
2. SPL 初始化时钟、DRAM、MMU 后返回 FEL
3. `sunxi-fel write` 将主程序写入 SDRAM
4. `sunxi-fel exe` 跳转执行主程序

### 示例代码

```rust
#![no_std]
#![no_main]

use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use f1c100s_hal as hal;
use hal::gpio::{AnyPin, Level, Output};
use hal::{println, Peri};

#[embassy_executor::main(entry = "arm9_rt::entry")]
async fn main(_spawner: Spawner) -> ! {
    let p = hal::init(Default::default());
    let mut led = Output::new(p.PE5.into(), Level::Low, Default::default());

    loop {
        led.toggle();
        println!("[blink] toggle");
        Timer::after(Duration::from_millis(500)).await;
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    println!("PANIC: {:?}", info);
    loop {}
}
```

## 内存布局

### SPL 模式（默认）

```
0x00000000 - 0x00007FFF : 32KB SRAM (向量表拷贝至此)
0x80000000 - 0x80007FFF : SPL 页表区域 (TTB, 由 SPL 管理)
0x80008000 - 0x81FFFFFF : SDRAM 代码区 (FLASH, ~32MB)
0x82000000 - 0x83FFFFFF : SDRAM 数据区 (RAM, 32MB)
```

### Direct 模式

```
0x00000000 - 0x00005FFF : 24KB SRAM 代码区 (FLASH)
0x00006000 - 0x00007FFF : 8KB SRAM 数据区 (RAM)
```

默认芯片型号为 F1C200S (64MB DDR1)，如需 F1C100S (32MB DDR1)：
```toml
[dependencies]
f1c100s-hal = { ..., default-features = false, features = ["time-driver-avs0", "debug-uart0", "f1c100s"] }
```

## Cargo Features

| Feature | 默认 | 说明 |
|---------|------|------|
| `f1c200s` | ✅ | F1C200S 64MB DDR1 |
| `f1c100s` | | F1C100S 32MB DDR1 |
| `spl` | | SPL 启动模式，程序运行在 SDRAM（由 build_boot.sh 自动启用） |
| `debug-uart0` | ✅ | UART0 调试输出 (PE1=TX, PE0=RX) |
| `debug-uart1` | | UART1 调试输出 (PA3=TX, PA2=RX) |
| `debug-uart2` | | UART2 调试输出 (PE7=TX, PE8=RX) |
| `time-driver-avs0` | ✅ | AVS Counter 0 作为时间驱动 |
| `defmt` | | defmt 日志支持 |

## 依赖项目

- [f1c100s-pac](https://github.com/apeng2012/f1c100s-pac) — 外设访问 crate
- [arm9](https://github.com/apeng2012/arm9) — ARM9 运行时支持

## License

MIT OR Apache-2.0
