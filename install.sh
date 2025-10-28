#!/bin/bash

set -e

cleanup() {
    rm -rf /tmp/yay
}

trap cleanup INT

USER=$SUDO_USER
HD=/home/$USER
SD=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" &> /dev/null && pwd)
COUNTRY_CODE="ru"

quit_if_not_sudo() {
    if [ "$(id -u)" -ne 0 ]; then
        echo "ERR: This script must be run as sudo!" >&2
	exit 1
    fi

    if [ -z "$USER" ]; then
        echo "ERR: This script must be run as sudo, not as root!" >&2
	exit 1
    fi
}
quit_if_not_sudo

ensure_yay_installed() {
    if ! command -v yay &> /dev/null; then
	sudo -u "$USER" bash -c '
	cd /tmp
        git clone https://aur.archlinux.org/yay.git
        cd yay
        makepkg -si
	'
	rm -rf /tmp/yay
    fi
}

update_mirrorlist() {
    echo "INFO: finding fastest mirrors for $COUNTRY_CODE"
    reflector --fastest 3 --country $COUNTRY_CODE --download-timeout 2\
	      --connection-timeout 2 --threads 3\
	      --save /etc/pacman.d/mirrorlist
}

CAT_DE="sway eww kitty rofi copyq qt6ct gammastep geoclue grim slurp wayfreeze xdg-desktop-portal
       xdg-desktop-portal xdg-desktop-portal-gtk xdg-desktop-portal-wlr"

CAT_APPS="qbittorrent firefox telegram-desktop discord apidog-bin"
	 
CAT_THEMING="qt6ct nwg-look breeze breeze-gtk"
CAT_UTILS="wl-clipboard fzf fastfetch htop"
CAT_SHELL="fish fzf"
CAT_BUILD="base-devel brightnessctl git"
CAT_AUTH="libsecret"
CAT_DEV="git docker docker-compose openssh rustup nodejs npm neovim vim ripgrep visual-studio-code-bin tokei bat"
CAT_FONTS="fontconfig freetype2 noto-fonts noto-fonts-cjk noto-fonts-extra ttf-liberation ttf-dejavu ttf-roboto ttf-fira-code"
CAT_DURABILITY="snapper"
CAT_VIRT="virt-manager qemu-desktop libvirt edk2-vmf dnsmasq"
CAT_ARCHIEVES="tar 7zip unzip"
CAT_MEDIA="pipwire pipewire-pulse aimp vlc pavucontrol"
CAT_COMPAT="xorg-xwayland"
CAT_ANTIHUILO="zapret throne-bin"
CAT_ALL="$CAT_DE $CAT_THEMING $CAT_UTILS $CAT_SHELL $CAT_BUILD $CAT_AUTH $CAT_DEV $CAT_FONTS\
	$CAT_DURABILITY $CAT_APPS $CAT_VIRT $CAT_ARCHIEVES $CAT_MEDIA $CAT_COMPAT $CAT_ANTIHUILO"

echo "INFO: installing packages"
update_mirrorlist
ensure_yay_installed

echo "INFO: removing conflicting packages"
pacman -R --noconfirm kddockwidgets-qt6 2>/dev/null || true

sudo -u "$USER" yay -Suy --noconfirm
sudo -u "$USER" yay -Su --noconfirm --needed $CAT_ALL
rustup default stable

mkdir_ln_fsn() {
    local src_file_path=$1
    local dest_file_path=$2
    local dest_dir_path=$(dirname -- $dest_file_path)

    if [[ "$dest_dir_path" == "$HD"* ]]; then
        sudo -u "$USER" mkdir -p "$dest_dir_path"
	sudo -u "$USER" ln -fsn "$src_file_path" "$dest_file_path"
    else
	mkdir -p -- "$dest_dir_path"
	ln -fsn -- "$src_file_path" "$dest_file_path"
    fi
}

echo "INFO: building stuff"
cargo build --release --manifest-path "$SD/eww/polling-server/Cargo.toml"

echo "INFO: linking configs"

conf_map=(
    "$SD/sway:$HD/.config/sway"
    "$SD/eww:$HD/.config/eww"
    "$SD/rofi:$HD/.config/rofi"
    "$SD/shells/fish:$HD/.config/fish"
    "$SD/etc/fonts.conf:/etc/fonts/fonts.conf"
    "$SD/etc/oomd.conf:/etc/systemd/oomd.conf"
    "$SD/desktops/sway.desktop:/usr/share/wayland-sessions/sway.desktop"
)

for conf in "${conf_map[@]}"; do
    src="${conf%:*}"
    dest="${conf#*:}"
    mkdir_ln_fsn "$src" "$dest"
done

echo "INFO: enabling services"
systemctl enable ly
systemctl enable --now systemd-oomd

echo "INFO: switching shell"
chsh root -s /bin/fish
chsh lord -s /bin/fish

echo "INFO: done. Note you should install drivers ."
