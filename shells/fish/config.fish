function set_bindings
    bind -M insert alt-c __fish_list_current_token
    bind -M default alt-c __fish_list_current_token
end

if status is-interactive
    set_bindings
end
