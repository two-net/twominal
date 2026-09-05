# Twominal shell integration. This file only emits semantic terminal markers.
if [[ -n ${TWOMINAL_SHELL_INTEGRATION_ACTIVE:-} || ! -o interactive ]]; then
  return
fi

typeset -g TWOMINAL_SHELL_INTEGRATION_ACTIVE=1
typeset -gr _TWOMINAL_INTEGRATION_NONCE=${_TWOMINAL_BOOTSTRAP_NONCE:-${TWOMINAL_SHELL_INTEGRATION_NONCE:-}}
unset _TWOMINAL_BOOTSTRAP_NONCE TWOMINAL_SHELL_INTEGRATION_NONCE
typeset -g _TWOMINAL_COMMAND_RUNNING=0
typeset -gr _TWOMINAL_PROMPT_SUFFIX=$'%{\e]133;B;'"${_TWOMINAL_INTEGRATION_NONCE}"$'\a%}'

autoload -Uz add-zsh-hook

_twominal_hex_encode() {
  local LC_ALL=C input=$1 output='' byte index
  for (( index = 1; index <= ${#input}; index++ )); do
    printf -v byte '%02x' "'${input[index]}"
    output+=$byte
  done
  print -nr -- "$output"
}

_twominal_precmd() {
  local command_status=$?

  if (( _TWOMINAL_COMMAND_RUNNING )); then
    printf '\e]133;D;%d;%s\a' "$command_status" "$_TWOMINAL_INTEGRATION_NONCE"
  else
    printf '\e]133;D;;%s\a' "$_TWOMINAL_INTEGRATION_NONCE"
  fi
  _TWOMINAL_COMMAND_RUNNING=0
  printf '\e]133;A;%s\a' "$_TWOMINAL_INTEGRATION_NONCE"
  printf '\e]133;P;CwdHex=%s;%s\a' "$(_twominal_hex_encode "$PWD")" "$_TWOMINAL_INTEGRATION_NONCE"

  if [[ $PROMPT == *"$_TWOMINAL_PROMPT_SUFFIX" ]]; then
    PROMPT=${PROMPT%"$_TWOMINAL_PROMPT_SUFFIX"}
  fi
  PROMPT="${PROMPT}${_TWOMINAL_PROMPT_SUFFIX}"
}

_twominal_preexec() {
  _TWOMINAL_COMMAND_RUNNING=1
  printf '\e]133;C;%s\a' "$_TWOMINAL_INTEGRATION_NONCE"
}

add-zsh-hook precmd _twominal_precmd
add-zsh-hook preexec _twominal_preexec
