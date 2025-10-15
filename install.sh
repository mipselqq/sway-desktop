#!/bin/bash

set -e

cleanup() {
    rm -rf /tmp/yay
}

trap cleanup INT

USER=$SUDO_USER
HOME_DIR=/home/$USER
SRC_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" &> /dev/null && pwd)
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

CAT_DE="sway waybar kitty rofi copyq qt6ct gammastep grim slurp xdg-desktop-portal
       xdg-desktop-portal xdg-desktop-portal-gtk xdg-desktop-portal-wlr"

CAT_APPS="telegram-desktop discord throne-bin apidog-bin
	 
CAT_THEMING="qt6ct breeze libadwaita"
CAT_UTILS="wl-clipboard fzf fastfetch htop"
CAT_SHELL="fish fzf"
CAT_BUILD="base-devel brightnessctl git"
CAT_AUTH="libsecret"
CAT_DEV="git docker docker-compose rustup nodejs npm neovim vim ripgrep visual-studio-code-bin"
CAT_FONTS="noto-fonts noto-fonts-cjk noto-fonts-extra ttf-liberation ttf-dejavu ttf-roboto"
CAT_DURABILITY="snapper"
CAT_VIRT="virt-manager qemu-desktop libvirt edk2-vmf dnsmasq iptables-nft"
CAT_ARCHIEVES="tar 7zip unzip"
CAT_MEDIA="pipwire pipewire-pulse aimp vlc"
CAT_COMPAT="xorg-xwayland"

CAT_ALL="$CAT_DE $CAT_THEMING $CAT_UTILS $CAT_SHELL $CAT_BUILD $CAT_AUTH $CAT_DEV $CAT_FONTS\
	$CAT_DURABILITY $CAT_APPS $CAT_VIRT $CAT_ARCHIEVES $CAT_MEDIA $CAT_COMPAT"

echo "INFO: installing packages"
update_mirrorlist
ensure_yay_installed
sudo -u "$USER" yay -Suy --noconfirm
sudo -u "$USER" yay -Su --noconfirm --needed $CAT_ALL

mkdir_ln_fsn() {
    local src_file_path=$1
    local dest_file_path=$2
    local dest_dir_path=$(dirname -- $dest_file_path)

    if [[ "$dest_dir_path" == "$HOME_DIR"* ]]; then
        sudo -u "$USER" mkdir -p "$dest_dir_path"
	sudo -u "$USER" ln -fsn "$src_file_path" "$dest_file_path"
    else
	mkdir -p -- "$dest_dir_path"
	ln -fsn -- "$src_file_path" "$dest_file_path"
    fi
}

echo "INFO: linking configs"
mkdir_ln_fsn "$SRC_DIR/sway" "$HOME_DIR/.config/sway"
mkdir_ln_fsn "$SRC_DIR/waybar" "$HOME_DIR/.config/waybar"
mkdir_ln_fsn "$SRC_DIR/rofi" "$HOME_DIR/.config/rofi"
mkdir_ln_fsn "$SRC_DIR/shells/fish" "$HOME_DIR/.config/fish"
mkdir_ln_fsn "$SRC_DIR/desktops/google-chrome.desktop" "$HOME_DIR/.local/share/applications/google-chrome.desktop"
mkdir_ln_fsn "$SRC_DIR/desktops/sway/sway.desktop" "$HOME_DIR/.local/share/wayland-sessions/sway.desktop"
mkdir_ln_fsn "$SRC_DIR/units/clashd.service" "/etc/systemd/system/clashd.service"
mkdir_ln_fsn "$SRC_DIR/etc/fonts.conf" "/etc/fonts/fonts.conf"
echo "INFO: enabling services"
systemctl enable --now clashd

echo "INFO: done. Note you should install drivers ."

