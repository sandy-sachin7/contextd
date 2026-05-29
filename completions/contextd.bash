_contextd() {
    local i cur prev opts cmd
    COMPREPLY=()
    cur="${COMP_WORDS[COMP_CWORD]}"
    prev="${COMP_WORDS[COMP_CWORD-1]}"
    cmd=""
    opts=""

    for i in ${COMP_WORDS[@]}; do
        case "${i}" in
            contextd) cmd="contextd" ;;
            daemon|mcp|setup|query) cmd+="__${i}" ;;
        esac
    done

    case "${cmd}" in
        contextd)
            opts="-c --config -h --help -V --version daemon mcp setup query"
            COMPREPLY=($(compgen -W "${opts}" -- "${cur}"))
            return 0
            ;;
        contextd__daemon)
            opts="-c --config -h --help"
            COMPREPLY=($(compgen -W "${opts}" -- "${cur}"))
            return 0
            ;;
        contextd__mcp)
            opts="-c --config -h --help"
            COMPREPLY=($(compgen -W "${opts}" -- "${cur}"))
            return 0
            ;;
        contextd__setup)
            opts="-c --config -h --help"
            COMPREPLY=($(compgen -W "${opts}" -- "${cur}"))
            return 0
            ;;
        contextd__query)
            opts="-l --limit -s --min-score -a --after -b --before -h --help <QUERY>"
            COMPREPLY=($(compgen -W "${opts}" -- "${cur}"))
            return 0
            ;;
    esac
}

complete -F _contextd contextd
