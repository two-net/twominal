# Twominal's shell integration for bash (FISH-04, FISH-05).
#
# Twominal starts bash with --rcfile naming this file, so bash reads it in place of ~/.bashrc.
# It reads the user's files as bash would have, then adds a hook that reports to Twominal
# before each prompt. bash ignores --rcfile in a login shell, so when Twominal wants a login
# shell it says so, and this file reads the login files in bash's own order. No file of the
# user's is created, changed or deleted.
#
# It uses nothing newer than bash 3.2, which macOS ships as /bin/bash.
#
# A report is ESC ] 2222 ; nonce ; key ; value BEL, its value written with every byte outside
# ! to ~, and every % and ;, as %XX. DESIGN.md §7 lists the reports.

# Kept, but not exported: no command the user runs can read them.
_twominal_nonce=${twominal_nonce-}
_twominal_login=${twominal_login-}
unset twominal_nonce twominal_login

# The functions come before the user's files, so that no alias of theirs can change them.

# Sets _twominal_encoded to $1, encoded.
_twominal_encode() {
  # One byte at a time, whatever the user's locale.
  local LC_ALL=C value=$1 encoded= byte
  local -i i code
  case $value in
    *[![:graph:]]* | *[%\;]*) ;;
    *)
      _twominal_encoded=$value
      return 0
      ;;
  esac
  for (( i = 0; i < ${#value}; i++ )); do
    byte=${value:i:1}
    case $byte in
      [%\;] | [![:graph:]])
        printf -v code '%d' "'$byte"
        printf -v byte '%%%02X' $(( code & 255 ))
        ;;
    esac
    encoded+=$byte
  done
  _twominal_encoded=$encoded
}

# Sets _twominal_encoded to the names in $1, one to a line, each encoded and separated by
# spaces.
_twominal_names() {
  local LC_ALL=C names=$1 name encoded= line=$'\n'
  case $names in
    *[!"$line"[:graph:]]* | *[%\;]*) ;;
    *)
      _twominal_encoded=${names//$line/ }
      return 0
      ;;
  esac
  names+=$line
  while [[ -n $names ]]; do
    name=${names%%"$line"*}
    names=${names#*"$line"}
    _twominal_encode "$name"
    encoded+=${encoded:+ }$_twominal_encoded
  done
  _twominal_encoded=$encoded
}

# Writes the report $1, with the encoded value $2 when there is one, unless it says what it
# said last time.
_twominal_say() {
  if (( $# > 1 )); then
    local said=_twominal_said_$1
    if [[ -n ${!said+set} && ${!said} == "$2" ]]; then
      return 0
    fi
    printf -v "$said" '%s' "$2"
    printf '\033]2222;%s;%s;%s\007' "$_twominal_nonce" "$1" "$2"
  else
    printf '\033]2222;%s;%s\007' "$_twominal_nonce" "$1"
  fi
}

# Before each prompt: what changed, then that the shell is ready for a command.
_twominal_prompt() {
  local last=$?
  _twominal_encode "$PWD"
  _twominal_say cwd "$_twominal_encoded"
  _twominal_encode "$PATH"
  _twominal_say path "$_twominal_encoded"
  _twominal_names "$(compgen -a)"
  _twominal_say aliases "$_twominal_encoded"
  _twominal_names "$(compgen -A function -X '_twominal_*')"
  _twominal_say functions "$_twominal_encoded"
  _twominal_say ready
  return $last
}

# The user's files, as bash would have read them.
if [[ -n $_twominal_login ]]; then
  if [[ -r /etc/profile ]]; then
    . /etc/profile
  fi
  if [[ -r ~/.bash_profile ]]; then
    . ~/.bash_profile
  elif [[ -r ~/.bash_login ]]; then
    . ~/.bash_login
  elif [[ -r ~/.profile ]]; then
    . ~/.profile
  fi
elif [[ -r ~/.bashrc ]]; then
  . ~/.bashrc
fi
unset _twominal_login

# Last, after whatever the user's files put in PROMPT_COMMAND. From bash 5.1 it may be an
# array.
if [[ -n $_twominal_nonce ]]; then
  if [[ $(declare -p PROMPT_COMMAND 2>/dev/null) == "declare -a"* ]]; then
    PROMPT_COMMAND+=(_twominal_prompt)
  else
    PROMPT_COMMAND=${PROMPT_COMMAND:+$PROMPT_COMMAND$'\n'}_twominal_prompt
  fi
fi
