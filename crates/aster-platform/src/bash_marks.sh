if [[ ${BASH_SOURCE[0]} == "${ASTER_BASH_RC-}" ]]; then
    PROMPT_COMMAND=${ASTER_BASH_PREVIOUS_PROMPT-}
    unset ASTER_BASH_RC ASTER_BASH_PREVIOUS_PROMPT
    [[ -n $PROMPT_COMMAND ]] || unset PROMPT_COMMAND
else
    [[ ! -r "$HOME/.bashrc" ]] || source "$HOME/.bashrc"
fi

__aster_command_status() {
    local status=$?
    printf '\033]133;D;%s\007' "$status"
    return "$status"
}

__aster_prepare_prompt() {
    local status=$?
    local start='\[\e]133;A\a\]' end='\[\e]133;B\a\]'
    PS1=${PS1#"$start"}
    PS1=${PS1%"$end"}
    PS1=$start$PS1$end
    return "$status"
}

PS0=$'\033]133;C\007'${PS0-}
if (( BASH_VERSINFO[0] > 5 || BASH_VERSINFO[0] == 5 && BASH_VERSINFO[1] >= 1 )); then
    PROMPT_COMMAND=(__aster_command_status "${PROMPT_COMMAND[@]}" __aster_prepare_prompt)
else
    PROMPT_COMMAND="__aster_command_status; ${PROMPT_COMMAND:+$PROMPT_COMMAND; }__aster_prepare_prompt"
fi
