function wifi
    nmcli device wifi rescan
    sleep 3
    nmtui
end
