#!/bin/sh

USER=lord
HOME_DIR=/home/$USER
SRC_DIR=$(pwd)

pacman -S sway kitty wmenu copyq gammastep brightnessctl grim slurp wl-clipboard waybar libsecret breeze --noconfirm --needed

mkdir -p "$HOME_DIR/.config"
mkdir -p "$HOME_DIR/.local/share/applications"

rm -rf "$HOME_DIR/.config/sway"
ln -s "$SRC_DIR/sway" "$HOME_DIR/.config/sway"

rm -rf "$HOME_DIR/.config/waybar"
ln -s "$SRC_DIR/waybar" "$HOME_DIR/.config/waybar"

rm -f "$HOME_DIR/.local/share/applications/google-chrome.desktop"
ln -s "$SRC_DIR/google-chrome.desktop" "$HOME_DIR/.local/share/applications/google-chrome.desktop"

rm -f /usr/share/wayland-sessions/sway.desktop
ln -s "$SRC_DIR/sway/sway.desktop" /usr/share/wayland-sessions/sway.desktop

