#!/bin/sh

USER=lord
HOME_DIR=/home/$USER
SRC_DIR=$(pwd)

pacman -S sway kitty wmenu copyq qt6ct gammastep brightnessctl grim slurp libadwaita xdg-desktop-portal xdg-desktop-portal-gtk xdg-desktop-portal-wlr wl-clipboard waybar libsecret breeze rofi htop fastfetch noto-fonts noto-fonts-cjk noto-fonts-extra ttf-liberation ttf-dejavu ttf-roboto snapper --noconfirm --needed

if [ ! -f "/bin/yay" ]; then
    sudo pacman -S --needed git base-devel
    git clone https://aur.archlinux.org/yay.git
    cd yay
    makepkg -si
    rm . -rf
fi

rm -rf "$HOME_DIR/.config/sway"
ln -s "$SRC_DIR/sway" "$HOME_DIR/.config/sway"

rm -rf "$HOME_DIR/.config/waybar"
ln -s "$SRC_DIR/waybar" "$HOME_DIR/.config/waybar"

rm -rf "$HOME_DIR/.config/rofi"
ln -s "$SRC_DIR/rofi" "$HOME_DIR/.config/rofi"

rm -f "/usr/share/applications/google-chrome.desktop"
ln -s "$SRC_DIR/google-chrome.desktop" "/usr/share/applications/google-chrome.desktop"

rm -f /usr/share/wayland-sessions/sway.desktop
ln -s "$SRC_DIR/sway/sway.desktop" /usr/share/wayland-sessions/sway.desktop

