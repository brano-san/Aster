if [[ ${ASTER_ZDOTDIR_SET-} = 1 ]]; then
    ZDOTDIR=$ASTER_ZDOTDIR
else
    unset ZDOTDIR
fi
unset ASTER_ZDOTDIR_SET ASTER_ZDOTDIR
if [[ -r ${ZDOTDIR-$HOME}/.zshenv ]]; then
    source "${ZDOTDIR-$HOME}/.zshenv"
fi

if [[ -o interactive ]]; then
    autoload -Uz add-zsh-hook
    function __aster_preexec {
        printf '\033]133;C\007'
    }
    function __aster_precmd {
        local exit_code=$?
        printf '\033]133;D;%s\007\033]133;A\007' "$exit_code"
        local -r end=$'%{\033]133;B\007%}'
        local -r before_eol=$'%{\033]133;D;%?\007%}'
        PROMPT=${PROMPT%"$end"}$end
        PROMPT_EOL_MARK=${PROMPT_EOL_MARK-"%B%S%#%s%b"}
        PROMPT_EOL_MARK=$before_eol${PROMPT_EOL_MARK#"$before_eol"}
    }
    add-zsh-hook preexec __aster_preexec
    add-zsh-hook precmd __aster_precmd
fi
