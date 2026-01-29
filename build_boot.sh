#!/bin/bash
# F1C100S 可启动镜像构建脚本
# 编译 -> 转 bin -> 添加 eGON.BT0 头 -> 可选烧录/运行

set -e

# 默认值
TARGET="armv5te-none-eabi"
EXAMPLE_NAME=""
OUTPUT_DIR="target/boot"
DO_FLASH=false
DO_FEL=false

usage() {
    echo "Usage: $0 [OPTIONS] <example_name>"
    echo ""
    echo "Options:"
    echo "  -f, --flash            Flash to SPI flash via sunxi-fel"
    echo "  -r, --run              Run via FEL mode (write + execute)"
    echo "  -o, --output DIR       Output directory (default: target/boot)"
    echo "  -h, --help             Show this help"
    echo ""
    echo "Examples:"
    echo "  $0 blinky              # Build only"
    echo "  $0 -f blinky           # Build and flash to SPI"
    echo "  $0 -r blinky           # Build and run via FEL"
    exit 1
}

# 解析参数
while [[ $# -gt 0 ]]; do
    case $1 in
        -f|--flash)
            DO_FLASH=true
            shift
            ;;
        -r|--run)
            DO_FEL=true
            shift
            ;;
        -o|--output)
            OUTPUT_DIR="$2"
            shift 2
            ;;
        -h|--help)
            usage
            ;;
        -*)
            echo "Unknown option: $1"
            usage
            ;;
        *)
            EXAMPLE_NAME="$1"
            shift
            ;;
    esac
done

if [ -z "$EXAMPLE_NAME" ]; then
    echo "Error: example_name is required"
    usage
fi

# 设置路径
ELF_PATH="target/${TARGET}/release/examples/${EXAMPLE_NAME}"

mkdir -p "$OUTPUT_DIR"

RAW_BIN="${OUTPUT_DIR}/${EXAMPLE_NAME}.bin"
BOOT_BIN="${OUTPUT_DIR}/${EXAMPLE_NAME}_boot.bin"

echo "=== Building example: ${EXAMPLE_NAME} ==="
echo "Target: ${TARGET}"

# 1. 编译
echo ""
echo "[1/3] Compiling..."
cargo +nightly build --release --example "${EXAMPLE_NAME}"

# 2. 转换为 bin
echo "[2/3] Converting to binary..."

# 检测 objcopy 工具
if command -v arm-none-eabi-objcopy &> /dev/null; then
    OBJCOPY="arm-none-eabi-objcopy"
elif command -v llvm-objcopy &> /dev/null; then
    OBJCOPY="llvm-objcopy"
elif command -v rust-objcopy &> /dev/null; then
    OBJCOPY="rust-objcopy"
else
    echo "Error: No objcopy found. Install arm-none-eabi-gcc, llvm, or cargo-binutils"
    exit 1
fi

$OBJCOPY -O binary "${ELF_PATH}" "${RAW_BIN}"
echo "  Created: ${RAW_BIN}"

# 3. 添加 eGON.BT0 头
echo "[3/3] Creating bootable image with eGON.BT0 header..."

create_boot_image() {
    local input="$1"
    local output="$2"
    
    # eGON.BT0 header 常量
    local HEADER_SIZE=32
    local CODE_START_OFFSET=48  # 0x30
    
    # 读取输入文件
    local code_size=$(stat -c%s "$input" 2>/dev/null || stat -f%z "$input")
    
    # 计算大小
    local padding_size=$((CODE_START_OFFSET - HEADER_SIZE))
    local total_size=$((CODE_START_OFFSET + code_size))
    local aligned_size=$(( (total_size + 511) & ~511 ))
    local tail_padding=$((aligned_size - total_size))
    
    # 创建临时文件
    local tmp_header=$(mktemp)
    local tmp_image=$(mktemp)
    
    # 构建 header (32 bytes)
    # 跳转指令: b +0x30 = 0xEA00000A
    printf '\x0a\x00\x00\xea' > "$tmp_header"
    # Magic: "eGON.BT0"
    printf 'eGON.BT0' >> "$tmp_header"
    # Checksum placeholder: 0x5F0A6C39
    printf '\x39\x6c\x0a\x5f' >> "$tmp_header"
    # Length (little-endian)
    printf "$(printf '\\x%02x\\x%02x\\x%02x\\x%02x' \
        $((aligned_size & 0xff)) \
        $(((aligned_size >> 8) & 0xff)) \
        $(((aligned_size >> 16) & 0xff)) \
        $(((aligned_size >> 24) & 0xff)))" >> "$tmp_header"
    # pub_head_size = 32
    printf '\x20\x00\x00\x00' >> "$tmp_header"
    # 填充到 32 字节
    dd if=/dev/zero bs=1 count=8 2>/dev/null >> "$tmp_header"
    
    # 构建镜像: header + padding + code + tail_padding
    cat "$tmp_header" > "$tmp_image"
    # padding (0xff)
    dd if=/dev/zero bs=1 count=$padding_size 2>/dev/null | tr '\0' '\377' >> "$tmp_image"
    # code
    cat "$input" >> "$tmp_image"
    # tail padding
    if [ $tail_padding -gt 0 ]; then
        dd if=/dev/zero bs=1 count=$tail_padding 2>/dev/null >> "$tmp_image"
    fi
    
    # 计算校验和
    local checksum=0
    local hex_dump=$(xxd -p -c 4 "$tmp_image")
    while read -r word; do
        if [ ${#word} -eq 8 ]; then
            # 转换 little-endian
            local le_word="${word:6:2}${word:4:2}${word:2:2}${word:0:2}"
            local val=$((16#$le_word))
            checksum=$(( (checksum + val) & 0xFFFFFFFF ))
        fi
    done <<< "$hex_dump"
    
    # 写入校验和到偏移 12
    local cs_bytes=$(printf '\\x%02x\\x%02x\\x%02x\\x%02x' \
        $((checksum & 0xff)) \
        $(((checksum >> 8) & 0xff)) \
        $(((checksum >> 16) & 0xff)) \
        $(((checksum >> 24) & 0xff)))
    
    # 使用 dd 写入校验和
    printf "$cs_bytes" | dd of="$tmp_image" bs=1 seek=12 count=4 conv=notrunc 2>/dev/null
    
    # 输出最终文件
    cp "$tmp_image" "$output"
    
    # 清理
    rm -f "$tmp_header" "$tmp_image"
    
    echo "  Code size: ${code_size} bytes"
    echo "  Total size: ${aligned_size} bytes (aligned to 512)"
    echo "  Code starts at: 0x30"
    printf "  Checksum: 0x%08X\n" $checksum
}

create_boot_image "${RAW_BIN}" "${BOOT_BIN}"

echo ""
echo "=== Build Complete ==="
echo "ELF:      ${ELF_PATH}"
echo "Binary:   ${RAW_BIN}"
echo "Bootable: ${BOOT_BIN}"

# 4. 烧录或运行
if [ "$DO_FLASH" = true ]; then
    echo ""
    echo "=== Flashing to SPI Flash ==="
    sunxi-fel spiflash-write 0 "${BOOT_BIN}"
    echo "Flash complete!"
fi

if [ "$DO_FEL" = true ]; then
    echo ""
    echo "=== Running via FEL mode ==="
    sunxi-fel write 0 "${RAW_BIN}"
    sunxi-fel exe 0
    echo "Execution started!"
fi
