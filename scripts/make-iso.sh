#!/bin/sh
# Builds a bootable GRUB (Multiboot2) ISO: build/huldra.iso
# Needs grub-mkrescue and xorriso (Debian/Ubuntu: grub-pc-bin grub-common xorriso mtools).
set -e
cd "$(dirname "$0")/.."

cargo build --release

mkdir -p build/iso/boot/grub
cp target/x86_64-unknown-none/release/huldra build/iso/boot/huldra
cat > build/iso/boot/grub/grub.cfg <<'EOF'
set timeout=0
set default=0

menuentry "Huldra" {
    multiboot2 /boot/huldra
    boot
}
EOF

grub-mkrescue -o build/huldra.iso build/iso
echo "ISO: build/huldra.iso  (run: qemu-system-x86_64 -cdrom build/huldra.iso -serial stdio)"
