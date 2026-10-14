# Twominal's shell integration for zsh (FISH-04, FISH-05).
#
# Twominal points ZDOTDIR at the folder this file is kept in, under the name .zshenv, so zsh
# reads it in place of the user's own .zshenv. It gives ZDOTDIR back, adds a hook that reports
# to Twominal before each prompt, and then reads the user's .zshenv. zsh goes on to read the
# user's other files from their own ZDOTDIR, as it always does. No file of the user's is
# created, changed or deleted.
#
# A report is ESC ] 2222 ; nonce ; key ; value BEL, its value written with every byte outside
# ! to ~, and every % and ;, as %XX. DESIGN.md §7 lists the reports.

# ZDOTDIR first, so that nothing else sees Twominal's folder in it: not the user's files, not a
# shell started from this one.
if (( ${+twominal_zdotdir} )); then
  ZDOTDIR=$twominal_zdotdir
else
  unset ZDOTDIR
fi
# Kept, but not exported: no command the user runs can read it.
typeset -g _twominal_nonce=${twominal_nonce-}
unset twominal_zdotdir twominal_nonce

if [[ -o interactive && -n $_twominal_nonce ]]; then
  # What each report last said, so that only a change is said again.
  typeset -gA _twominal_said

  # Sets REPLY to $1, encoded.
  _twominal_encode() {
    emulate -L zsh
    REPLY=$1
    [[ $REPLY == *[^!-~]* || $REPLY == *[%\;]* ]] || return 0
    # One byte at a time, whatever the user's locale.
    local LC_ALL=C byte encoded=
    local -i i
    for (( i = 1; i <= $#REPLY; i++ )); do
      byte=$REPLY[i]
      if [[ $byte == [[:graph:]] && $byte != [%\;] ]]; then
        encoded+=$byte
      else
        encoded+=%${(l:2::0:)$(( [##16] #byte ))}
      fi
    done
    REPLY=$encoded
  }

  # Sets REPLY to the names given, each encoded, separated by spaces.
  _twominal_names() {
    emulate -L zsh
    local -a names
    names=("$@")
    if (( ${#${(M)names:#*[^!-~]*}} || ${#${(M)names:#*[%\;]*}} )); then
      local name
      local -a encoded
      for name in $names; do
        _twominal_encode $name
        encoded+=($REPLY)
      done
      names=($encoded)
    fi
    REPLY=${(j: :)names}
  }

  # Writes the report $1, with the encoded value $2 when there is one, unless it says what it
  # said last time.
  _twominal_say() {
    emulate -L zsh
    if (( $# > 1 )); then
      (( ${+_twominal_said[$1]} )) && [[ $_twominal_said[$1] == $2 ]] && return 0
      _twominal_said[$1]=$2
      print -rn -- $'\e]2222;'"$_twominal_nonce;$1;$2"$'\a'
    else
      print -rn -- $'\e]2222;'"$_twominal_nonce;$1"$'\a'
    fi
  }

  # Before each prompt: what changed, then that the shell is ready for a command.
  _twominal_precmd() {
    local -i last=$?
    emulate -L zsh
    # Last from the next prompt on, so that "ready" follows whatever other hooks write.
    precmd_functions=(${precmd_functions:#_twominal_precmd} _twominal_precmd)

    _twominal_encode "$PWD"
    _twominal_say cwd "$REPLY"
    _twominal_encode "$PATH"
    _twominal_say path "$REPLY"
    _twominal_names ${(k)aliases} ${(k)galiases}
    _twominal_say aliases "$REPLY"
    _twominal_names ${(k)functions:#_twominal_*}
    _twominal_say functions "$REPLY"
    _twominal_say ready
    return last
  }

  typeset -ga precmd_functions
  precmd_functions+=(_twominal_precmd)
fi

# Last, so that nothing of the user's can change how the lines above are read.
if [[ -r ${ZDOTDIR:-$HOME}/.zshenv ]]; then
  source ${ZDOTDIR:-$HOME}/.zshenv
fi
