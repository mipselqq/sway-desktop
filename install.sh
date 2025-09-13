#!/bin/sh

USER=lord
HOME_DIR=/home/$USER
SRC_DIR=$(pwd)

pacman -S sway kitty wmenu copyq qt6ct gammastep brightnessctl grim slurp xdg-desktop-portal xdg-desktop-portal-wlr wl-clipboard waybar libsecret breeze --noconfirm --needed

rm -rf "$HOME_DIR/.config/sway"
ln -s "$SRC_DIR/sway" "$HOME_DIR/.config/sway"

rm -rf "$HOME_DIR/.config/waybar"
ln -s "$SRC_DIR/waybar" "$HOME_DIR/.config/waybar"

rm -f "/usr/share/applications/google-chrome.desktop"
ln -s "$SRC_DIR/google-chrome.desktop" "/usr/share/applications/google-chrome.desktop"

rm -f /usr/share/wayland-sessions/sway.desktop
ln -s "$SRC_DIR/sway/sway.desktop" /usr/share/wayland-sessions/sway.desktop

