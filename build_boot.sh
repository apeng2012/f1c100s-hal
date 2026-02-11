#!/bin/bash
# F1C100S 构建脚本
#
# 默认 SPL 模式 (推荐):
#   SPL 初始化时钟、SDRAM、MMU，主程序运行在 SDRAM 中，空间充足
#   FEL:   sunxi-fel spl 加载 SPL → 写入主程序到 SDRAM → 执行
#   Flash: SPL(前32KB) + uImage(0x8000+) 写入 SPI Flash
#
# Direct 模式 (-d):
#   程序直接运行在 32KB SRAM 中，带 eGON.BT0 头，仅支持烧录到 SPI Flash
#   适用于极简场景，空间受限 (24KB代码 + 8KB数据)

set -e

TARGET="armv5te-none-eabi"
EXAMPLE_NAME=""
OUTPUT_DIR="target/boot"
SPL_BIN="f1c100s-spl_uart0.bin"
DIRECT_MODE=false
DO_FEL=false
DO_FLASH=false

usage() {
    echo "Usage: $0 [OPTIONS] <example_name>"
    echo ""
    echo "Options:"
    echo "  -r, --run              Run via FEL mode (SPL mode only)"
    echo "  -f, --flash            Flash to SPI flash"
    echo "  -d, --direct           Direct mode (no SPL, runs in 32KB SRAM)"
    echo "  -s, --spl FILE         SPL binary (default: f1c100s-spl_uart0.bin)"
    echo "  -o, --output DIR       Output directory (default: target/boot)"
    echo "  -h, --help             Show this help"
    echo ""
    echo "Examples:"
    echo "  $0 blinky              # Build (SPL mode)"
    echo "  $0 -r blinky           # Build and run via FEL (SPL mode)"
    echo "  $0 -f blinky           # Build and flash to SPI (SPL mode)"
    echo "  $0 -d blinky           # Build (direct mode, SRAM only)"
    echo "  $0 -d -f blinky        # Build and flash (direct mode)"
    exit 1
}

while [[ $# -gt 0 ]]; do
    case $1 in
        -r|--run)    DO_FEL=true; shift ;;
        -f|--flash)  DO_FLASH=true; shift ;;
        -d|--direct) DIRECT_MODE=true; shift ;;
        -s|--spl)    SPL_BIN="$2"; shift 2 ;;
        -o|--output) OUTPUT_DIR="$2"; shift 2 ;;
        -h|--help)   usage ;;
        -*)          echo "Error: Unknown option: $1"; usage ;;
        *)           EXAMPLE_NAME="$1"; shift ;;
    esac
done

if [ -z "$EXAMPLE_NAME" ]; then
    echo "Error: example_name is required"
    usage
fi

if [ "$DIRECT_MODE" = true ] && [ "$DO_FEL" = true ]; then
    echo "Error: -d (direct) and -r (FEL run) cannot be used together."
    echo "       Direct mode only supports flash (-f). FEL run requires SPL mode."
    exit 1
fi

# 检测 objcopy
detect_objcopy() {
    if command -v arm-none-eabi-objcopy &> /dev/null; then
        echo "arm-none-eabi-objcopy"
    elif command -v llvm-objcopy &> /dev/null; then
        echo "llvm-objcopy"
    elif command -v rust-objcopy &> /dev/null; then
        echo "rust-objcopy"
    else
        echo "Error: No objcopy found. Install arm-none-eabi-gcc, llvm, or cargo-binutils" >&2
        exit 1
    fi
}

OBJCOPY=$(detect_objcopy)
ELF_PATH="target/${TARGET}/release/examples/${EXAMPLE_NAME}"
mkdir -p "$OUTPUT_DIR"

# ============================================================================
# Direct 模式: 程序运行在 SRAM，带 eGON.BT0 头
# ============================================================================
if [ "$DIRECT_MODE" = true ]; then
    RAW_BIN="${OUTPUT_DIR}/${EXAMPLE_NAME}.bin"
    BOOT_BIN="${OUTPUT_DIR}/${EXAMPLE_NAME}_boot.bin"

    echo "=== Building (direct mode): ${EXAMPLE_NAME} ==="

    echo "[1/3] Compiling..."
    cargo +nightly build --release --example "${EXAMPLE_NAME}"

    echo "[2/3] Converting to binary..."
    $OBJCOPY -O binary "${ELF_PATH}" "${RAW_BIN}"

    echo "[3/3] Creating bootable image with eGON.BT0 header..."
    python3 mkboot.py "${RAW_BIN}" "${BOOT_BIN}"

    echo ""
    echo "=== Build Complete (direct) ==="
    echo "Binary:   ${RAW_BIN}"
    echo "Bootable: ${BOOT_BIN}"

    if [ "$DO_FLASH" = true ]; then
        echo ""
        echo "=== Flashing to SPI Flash ==="
        sunxi-fel spiflash-write 0 "${BOOT_BIN}"
        echo "Flash complete!"
    fi
    exit 0
fi

# ============================================================================
# SPL 模式 (默认): SPL 初始化 SDRAM，主程序运行在 SDRAM
# ============================================================================
if [ ! -f "$SPL_BIN" ]; then
    echo "Error: SPL binary not found: $SPL_BIN"
    exit 1
fi

RAW_BIN="${OUTPUT_DIR}/${EXAMPLE_NAME}_spl.bin"

echo "=== Building (SPL mode): ${EXAMPLE_NAME} ==="

echo "[1/2] Compiling with spl feature..."
cargo +nightly build --release --example "${EXAMPLE_NAME}" --features spl

echo "[2/2] Converting to binary..."
$OBJCOPY -O binary "${ELF_PATH}" "${RAW_BIN}"
echo "  Created: ${RAW_BIN} ($(stat -c%s "$RAW_BIN" 2>/dev/null || stat -f%z "$RAW_BIN") bytes)"

echo ""
echo "=== Build Complete (SPL) ==="
echo "Binary: ${RAW_BIN}"
echo "SPL:    ${SPL_BIN}"

# FEL 模式运行
if [ "$DO_FEL" = true ]; then
    echo ""
    echo "=== Running via FEL ==="
    sunxi-fel spl "${SPL_BIN}" \
              write 0x80008000 "${RAW_BIN}" \
              exe 0x80008000
    echo "Execution started!"
fi

# SPI Flash 烧录
if [ "$DO_FLASH" = true ]; then
    echo ""
    echo "=== Flashing to SPI Flash ==="

    UIMAGE="${OUTPUT_DIR}/${EXAMPLE_NAME}_uimage.bin"
    FLASH_IMG="${OUTPUT_DIR}/${EXAMPLE_NAME}_flash.bin"

    # 生成 uImage header + 程序数据
    python3 - "$RAW_BIN" "$UIMAGE" << 'PYEOF'
import struct, sys, zlib

bin_file = sys.argv[1]
out_file = sys.argv[2]

with open(bin_file, 'rb') as f:
    data = f.read()

IH_MAGIC = 0x27051956
load_addr = 0x80008000
entry_addr = 0x80008000
dcrc = zlib.crc32(data) & 0xFFFFFFFF
name = b'f1c100s-spl-app\x00' + b'\x00' * 16

header = struct.pack('>IIIIIIIBBBB32s',
    IH_MAGIC, 0, 0, len(data), load_addr, entry_addr, dcrc,
    5, 2, 2, 0, name)

hcrc = zlib.crc32(header) & 0xFFFFFFFF
header = header[:4] + struct.pack('>I', hcrc) + header[8:]

with open(out_file, 'wb') as f:
    f.write(header)
    f.write(data)

print(f"  uImage created, data size: {len(data)} bytes")
PYEOF

    # 合并: SPL (前 32KB) + uImage (0x8000+)
    python3 - "$SPL_BIN" "$UIMAGE" "$FLASH_IMG" << 'PYEOF'
import sys

spl_file = sys.argv[1]
uimage_file = sys.argv[2]
out_file = sys.argv[3]

with open(spl_file, 'rb') as f:
    spl = f.read()
with open(uimage_file, 'rb') as f:
    uimage = f.read()

spl_region = 0x8000
if len(spl) > spl_region:
    print(f"Error: SPL ({len(spl)} bytes) exceeds {spl_region} bytes!")
    sys.exit(1)

padded_spl = spl + b'\xff' * (spl_region - len(spl))

with open(out_file, 'wb') as f:
    f.write(padded_spl)
    f.write(uimage)

print(f"  Flash image: SPL({len(spl)}B) + uImage({len(uimage)}B) = {spl_region + len(uimage)}B total")
PYEOF

    sunxi-fel spiflash-write 0 "${FLASH_IMG}"
    echo "Flash complete!"
fi
